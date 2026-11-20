//! 1 Hz health monitor: journal `health`, observer snapshot, disk guard, `FramesDropping`.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sc_core::journal::{HealthEntry, JournalEvent};
use sc_core::sessionlog::Level;

use super::shared::{Ctl, Shared};
use super::{HealthSnapshot, Notice};

const WINDOW_TICKS: usize = 10;

pub(crate) fn snapshot(sh: &Shared, now: i64, fps: &dyn Fn(&str, u64) -> f64) -> HealthSnapshot {
    let free = sh.platform.free_disk_bytes(&sh.dir).unwrap_or(u64::MAX);
    HealthSnapshot {
        rec_ns: (now - sh.start_host()).max(0),
        tracks: sh
            .registry
            .all()
            .iter()
            .map(|r| {
                let frames = r.stats.frames.load(Ordering::Relaxed);
                r.stats.snap(&r.id, fps(&r.id, frames))
            })
            .collect(),
        free_disk_bytes: free,
    }
}

#[derive(Default)]
struct Monitor {
    prev: HashMap<String, (i64, u64)>,
    window: HashMap<String, VecDeque<(u64, u64)>>,
    notified: HashMap<String, Instant>,
    overflow: HashMap<String, u64>,
    disk_warned: bool,
    disk_stop_sent: bool,
}

pub(crate) fn spawn(sh: Arc<Shared>, stop: Receiver<()>) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("sc-record-health".into())
        .spawn(move || {
            let mut m = Monitor::default();
            loop {
                match stop.recv_timeout(sh.platform.host_duration(1_000_000_000)) {
                    Err(RecvTimeoutError::Timeout) => m.tick(&sh),
                    _ => return,
                }
            }
        })
}

impl Monitor {
    fn tick(&mut self, sh: &Shared) {
        let now = sh.now();
        let prev = self.prev.clone();
        let snap = snapshot(sh, now, &|id, frames| match prev.get(id) {
            Some(&(t, f)) if now > t => frames.saturating_sub(f) as f64 * 1e9 / (now - t) as f64,
            _ => 0.0,
        });
        sh.rec.journal(&JournalEvent::Health {
            host_ns: now,
            rec_ns: snap.rec_ns,
            tracks: snap
                .tracks
                .iter()
                .map(|t| HealthEntry {
                    track_id: t.track_id.clone(),
                    frames: t.frames,
                    dropped: t.dropped,
                    writer_not_ready: t.writer_not_ready,
                    gaps: t.gaps,
                })
                .collect(),
        });
        sh.obs.on_health(&snap);
        for t in &snap.tracks {
            self.prev.insert(t.track_id.clone(), (now, t.frames));
        }
        self.frames_dropping(sh);
        self.disk(sh, snap.free_disk_bytes);
    }

    fn frames_dropping(&mut self, sh: &Shared) {
        for r in sh.registry.all() {
            let overflow = r.stats.overflow.load(Ordering::Relaxed);
            let before = self.overflow.insert(r.id.clone(), overflow).unwrap_or(0);
            if overflow > before {
                sh.rec.log(
                    Level::Warn,
                    &format!(
                        "{}: queue full, {} buffer(s) dropped in the last second",
                        r.id,
                        overflow - before
                    ),
                );
            }
            if !r.video {
                continue;
            }
            let w = self.window.entry(r.id.clone()).or_default();
            w.push_back((
                r.stats.frames.load(Ordering::Relaxed),
                r.stats.dropped.load(Ordering::Relaxed),
            ));
            if w.len() > WINDOW_TICKS + 1 {
                w.pop_front();
            }
            let (Some(&(f0, d0)), Some(&(f1, d1))) = (w.front(), w.back()) else {
                continue;
            };
            let (df, dd) = (f1.saturating_sub(f0), d1.saturating_sub(d0));
            if dd == 0 {
                continue;
            }
            let pct = dd as f64 * 100.0 / (df + dd) as f64;
            let quiet = self
                .notified
                .get(&r.id)
                .is_some_and(|t| t.elapsed() < Duration::from_secs(10));
            if pct > sh.tuning.frames_dropping_pct && !quiet {
                self.notified.insert(r.id.clone(), Instant::now());
                sh.notice(Notice::FramesDropping {
                    track: r.id.clone(),
                    pct,
                });
            }
        }
    }

    fn disk(&mut self, sh: &Shared, free: u64) {
        if free < sh.tuning.disk_stop_bytes {
            if !self.disk_stop_sent {
                self.disk_stop_sent = true;
                sh.rec.log(
                    Level::Error,
                    &format!("free disk {free} bytes is below the stop threshold"),
                );
                sh.send(Ctl::DiskStop(free));
            }
        } else if free < sh.tuning.disk_warn_bytes && !self.disk_warned {
            self.disk_warned = true;
            sh.notice(Notice::DiskLow { bytes_left: free });
        }
    }
}
