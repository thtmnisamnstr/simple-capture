use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use sc_geometry::SizePx;
use sc_platform::{AudioBuffer, AudioTrackKind, FrameSink, SourceEvent, VideoFrame};

use super::router::RouterIn;
use super::shared::{Ctl, Shared, TrackStats, bump, lock};

pub(crate) enum TrackIn {
    Video(VideoFrame),
    Audio(AudioBuffer),
    /// `end_host`: the recording's stop time; samples captured after it are cut from the file.
    Finish {
        end_host: Option<i64>,
    },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Firsts {
    pub screen: bool,
    pub camera: Option<SizePx>,
    pub mic: Option<(u32, u16)>,
    pub sys: Option<(u32, u16)>,
    pub dev: Option<(u32, u16)>,
}

pub(crate) struct Feed {
    pub tx: SyncSender<TrackIn>,
    pub stats: Arc<TrackStats>,
}

/// Capture callbacks only `try_send` here; nothing in this sink blocks.
pub(crate) struct SessionSink {
    sh: Arc<Shared>,
    router: SyncSender<RouterIn>,
    pub camera: OnceLock<Feed>,
    pub mic: OnceLock<Feed>,
    pub sys: OnceLock<Feed>,
    pub dev: OnceLock<Feed>,
    seen: [AtomicBool; 5],
    firsts: Mutex<Firsts>,
    cv: Condvar,
    levels: Mutex<[(std::time::Instant, f32); 2]>,
    held: Mutex<Option<VideoFrame>>,
}

impl SessionSink {
    fn level(&self, track: AudioTrackKind, a: &AudioBuffer) {
        let Some(samples) = a.data.samples() else {
            return;
        };
        let peak = samples.iter().fold(0f32, |m, v| m.max(v.abs()));
        let i = usize::from(track != AudioTrackKind::Mic);
        let report = {
            let mut l = lock(&self.levels);
            l[i].1 = l[i].1.max(peak);
            if l[i].0.elapsed() < std::time::Duration::from_millis(66) {
                None
            } else {
                let p = l[i].1;
                l[i] = (std::time::Instant::now(), 0.0);
                Some(p)
            }
        };
        if let Some(p) = report {
            self.sh.obs.on_level(track, p);
        }
    }

    /// The screen sends frames only when it changes: the last one seen while paused is recorded
    /// at `now`, so an idle screen does not show the picture from before the pause.
    pub(crate) fn resume_held(&self, now: i64) {
        if let Some(mut f) = lock(&self.held).take() {
            f.pts = sc_platform::HostTime(now.max(f.pts.0));
            let _ = self.router.try_send(RouterIn::Frame(f));
        }
    }

    fn paused(&self) -> bool {
        self.sh.paused.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn new(sh: Arc<Shared>, router: SyncSender<RouterIn>) -> Arc<SessionSink> {
        Arc::new(SessionSink {
            sh,
            router,
            camera: OnceLock::new(),
            mic: OnceLock::new(),
            sys: OnceLock::new(),
            dev: OnceLock::new(),
            seen: Default::default(),
            firsts: Mutex::new(Firsts::default()),
            cv: Condvar::new(),
            held: Mutex::new(None),
            levels: Mutex::new([(std::time::Instant::now(), 0.0); 2]),
        })
    }

    fn first(&self, i: usize, f: impl FnOnce(&mut Firsts)) {
        if self.seen[i].load(Ordering::Relaxed) {
            return;
        }
        f(&mut lock(&self.firsts));
        self.seen[i].store(true, Ordering::Relaxed);
        self.cv.notify_all();
    }

    /// Real-time wait; `Err` carries what had arrived by the deadline.
    pub(crate) fn wait_firsts(
        &self,
        timeout: Duration,
        ready: impl Fn(&Firsts) -> bool,
    ) -> Result<Firsts, Firsts> {
        let deadline = Instant::now() + timeout;
        let mut g = lock(&self.firsts);
        loop {
            if ready(&g) {
                return Ok(g.clone());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(g.clone());
            }
            g = self
                .cv
                .wait_timeout(g, deadline - now)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }
}

fn forward(feed: &OnceLock<Feed>, item: TrackIn, audio: bool) {
    let Some(f) = feed.get() else { return };
    if let Err(TrySendError::Full(_)) = f.tx.try_send(item) {
        bump(&f.stats.dropped);
        if audio {
            bump(&f.stats.overflow);
        }
    }
}

impl FrameSink for SessionSink {
    fn on_screen(&self, f: VideoFrame) {
        self.first(0, |x| x.screen = true);
        if self.paused() {
            *lock(&self.held) = Some(f);
            return;
        }
        if let Err(TrySendError::Full(_)) = self.router.try_send(RouterIn::Frame(f))
            && let Some(s) = lock(&self.sh.screen_stats).as_ref()
        {
            bump(&s.dropped);
        }
    }

    fn on_camera(&self, f: VideoFrame) {
        let size = f.buffer.0.size();
        self.first(1, |x| x.camera = Some(size));
        if self.paused() {
            return;
        }
        forward(&self.camera, TrackIn::Video(f), false);
    }

    fn on_audio(&self, track: AudioTrackKind, a: AudioBuffer) {
        let fmt = (a.sample_rate, a.channels);
        if self.paused() {
            return;
        }
        self.level(track, &a);
        match track {
            AudioTrackKind::Mic => {
                self.first(2, |x| x.mic = Some(fmt));
                forward(&self.mic, TrackIn::Audio(a), true);
            }
            AudioTrackKind::SystemAudio => {
                self.first(3, |x| x.sys = Some(fmt));
                forward(&self.sys, TrackIn::Audio(a), true);
            }
            AudioTrackKind::DeviceAudio => {
                self.first(4, |x| x.dev = Some(fmt));
                forward(&self.dev, TrackIn::Audio(a), true);
            }
        }
    }

    fn on_event(&self, e: SourceEvent) {
        self.sh.send(Ctl::Event(e));
    }
}
