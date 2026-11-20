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
    Finish,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Firsts {
    pub screen: bool,
    pub camera: Option<SizePx>,
    pub mic: Option<(u32, u16)>,
    pub sys: Option<(u32, u16)>,
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
    seen: [AtomicBool; 4],
    firsts: Mutex<Firsts>,
    cv: Condvar,
}

impl SessionSink {
    pub(crate) fn new(sh: Arc<Shared>, router: SyncSender<RouterIn>) -> Arc<SessionSink> {
        Arc::new(SessionSink {
            sh,
            router,
            camera: OnceLock::new(),
            mic: OnceLock::new(),
            sys: OnceLock::new(),
            seen: Default::default(),
            firsts: Mutex::new(Firsts::default()),
            cv: Condvar::new(),
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
        if let Err(TrySendError::Full(_)) = self.router.try_send(RouterIn::Frame(f))
            && let Some(s) = lock(&self.sh.screen_stats).as_ref()
        {
            bump(&s.dropped);
        }
    }

    fn on_camera(&self, f: VideoFrame) {
        let size = f.buffer.0.size();
        self.first(1, |x| x.camera = Some(size));
        forward(&self.camera, TrackIn::Video(f), false);
    }

    fn on_audio(&self, track: AudioTrackKind, a: AudioBuffer) {
        let fmt = (a.sample_rate, a.channels);
        match track {
            AudioTrackKind::Mic => {
                self.first(2, |x| x.mic = Some(fmt));
                forward(&self.mic, TrackIn::Audio(a), true);
            }
            AudioTrackKind::SystemAudio => {
                self.first(3, |x| x.sys = Some(fmt));
                forward(&self.sys, TrackIn::Audio(a), true);
            }
        }
    }

    fn on_event(&self, e: SourceEvent) {
        self.sh.send(Ctl::Event(e));
    }
}
