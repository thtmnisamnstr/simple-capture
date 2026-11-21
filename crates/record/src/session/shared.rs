use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sc_core::journal::{JournalEvent, JournalWriter, StopReason};
use sc_core::project::Via;
use sc_core::sessionlog::{Level, SessionLog};
use sc_platform::{Platform, SourceEvent, SourceSpec, StreamTag, WindowId};
use serde_json::Value;

use super::router::Opened;
use super::{Notice, SessionObserver, SessionState, StopSummary, TrackHealthSnap, Tuning};
use crate::Result;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) const NOT_STARTED: i64 = i64::MAX;

pub(crate) enum Ctl {
    Switch(SourceSpec, Via),
    SetExclusions(Vec<WindowId>, Sender<Result<()>>),
    Stop(StopReason, Sender<Result<StopSummary>>),
    Discard(Sender<Result<()>>),
    Event(SourceEvent),
    Cut(Opened),
    OpenFailed {
        tag: StreamTag,
        error: String,
    },
    DiskStop(u64),
    /// The key and the host time it was sent at.
    CameraKey(sc_core::project::CameraKey, i64),
}

pub(crate) struct Recorder {
    inner: Mutex<(JournalWriter, SessionLog)>,
    platform: Arc<dyn Platform>,
}

fn describe(ev: &JournalEvent) -> String {
    match serde_json::to_value(ev) {
        Ok(Value::Object(mut m)) => {
            let name = m
                .remove("ev")
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            format!("{name} {}", Value::Object(m))
        }
        _ => format!("{ev:?}"),
    }
}

impl Recorder {
    pub(crate) fn new(j: JournalWriter, log: SessionLog, platform: Arc<dyn Platform>) -> Recorder {
        Recorder {
            inner: Mutex::new((j, log)),
            platform,
        }
    }

    /// Appends and fsyncs one journal line and mirrors it in session.log.
    pub(crate) fn journal(&self, ev: &JournalEvent) -> bool {
        let host = self.platform.host_now().0;
        let text = describe(ev);
        let mut g = lock(&self.inner);
        match g.0.append(ev) {
            Ok(()) => {
                let _ = g.1.line(Level::Info, &format!("{text} @{host}"));
                true
            }
            Err(e) => {
                let _ = g.1.line(
                    Level::Error,
                    &format!("journal write failed ({e}): {text} @{host}"),
                );
                false
            }
        }
    }

    pub(crate) fn log(&self, level: Level, msg: &str) {
        let host = self.platform.host_now().0;
        let _ = lock(&self.inner).1.line(level, &format!("{msg} @{host}"));
    }
}

#[derive(Default)]
pub(crate) struct TrackStats {
    pub frames: AtomicU64,
    pub dropped: AtomicU64,
    pub writer_not_ready: AtomicU64,
    pub gaps: AtomicU32,
    pub overlaps: AtomicU32,
    pub drift_ns: AtomicI64,
    pub overflow: AtomicU64,
}

pub(crate) fn bump(c: &AtomicU64) {
    c.fetch_add(1, Ordering::Relaxed);
}

impl TrackStats {
    pub(crate) fn snap(&self, track_id: &str, fps: f64) -> TrackHealthSnap {
        TrackHealthSnap {
            track_id: track_id.to_string(),
            frames: self.frames.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            writer_not_ready: self.writer_not_ready.load(Ordering::Relaxed),
            gaps: self.gaps.load(Ordering::Relaxed),
            overlaps: self.overlaps.load(Ordering::Relaxed),
            fps,
            drift_ns: self.drift_ns.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Registered {
    pub id: String,
    pub video: bool,
    pub stats: Arc<TrackStats>,
}

#[derive(Default)]
pub(crate) struct Registry {
    tracks: Mutex<Vec<Registered>>,
}

impl Registry {
    pub(crate) fn add(&self, id: &str, video: bool) -> Arc<TrackStats> {
        let stats = Arc::new(TrackStats::default());
        lock(&self.tracks).push(Registered {
            id: id.to_string(),
            video,
            stats: stats.clone(),
        });
        stats
    }

    pub(crate) fn all(&self) -> Vec<Registered> {
        lock(&self.tracks).clone()
    }

    pub(crate) fn get(&self, id: &str) -> Option<Arc<TrackStats>> {
        lock(&self.tracks)
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.stats.clone())
    }
}

pub(crate) struct Shared {
    pub platform: Arc<dyn Platform>,
    pub obs: Arc<dyn SessionObserver>,
    pub dir: PathBuf,
    pub rec: Recorder,
    pub registry: Registry,
    pub state: Mutex<SessionState>,
    pub start_host: AtomicI64,
    /// `i64::MAX` until the stop time is known: at the start for a request with a duration, else
    /// at the stop. Screen frames from then on are not recorded.
    pub stop_host: AtomicI64,
    pub tuning: Tuning,
    pub ctl: Sender<Ctl>,
    pub screen_stats: Mutex<Option<Arc<TrackStats>>>,
}

impl Shared {
    pub(crate) fn start_host(&self) -> i64 {
        self.start_host.load(Ordering::Acquire)
    }

    pub(crate) fn stop_host(&self) -> i64 {
        self.stop_host.load(Ordering::Acquire)
    }

    pub(crate) fn now(&self) -> i64 {
        self.platform.host_now().0
    }

    pub(crate) fn set_state(&self, s: SessionState) {
        self.rec.log(Level::Info, &format!("state {s:?}"));
        *lock(&self.state) = s.clone();
        self.obs.on_state(&s);
    }

    pub(crate) fn notice(&self, n: Notice) {
        self.rec.log(Level::Warn, &format!("notice {n:?}"));
        self.obs.on_notice(&n);
    }

    pub(crate) fn send(&self, c: Ctl) {
        let _ = self.ctl.send(c);
    }
}
