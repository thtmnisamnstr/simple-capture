//! Playback audio and the playback clock (ADR-025): the default output device plays the
//! timeline mix, and the samples it has played are the clock. Without a device, a wall clock.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use sc_core::edit::Doc;
use sc_core::project::ProjectV1;
use sc_platform::Decoder;

use crate::audio::{CHANNELS, TimelineAudio};

const AHEAD: Duration = Duration::from_millis(250);
const BLOCK_FRAMES: usize = 1024;

enum Cmd {
    Play(i64),
    Pause,
    SetDoc(Box<Doc>),
}

struct Clock {
    base_tl: i64,
    heard_frames: u64,
    heard_at: Option<Instant>,
    wall_start: Instant,
}

struct Shared {
    playing: AtomicBool,
    generation: AtomicU64,
    rate: AtomicU64,
    device: AtomicBool,
    latency_ns: AtomicI64,
    ring: Mutex<(u64, VecDeque<f32>)>,
    clock: Mutex<Clock>,
}

/// Plays the edited timeline's audio and keeps time.
pub struct Playback {
    tx: Sender<Cmd>,
    shared: Arc<Shared>,
}

impl Playback {
    pub fn start(
        project: Arc<ProjectV1>,
        dir: &Path,
        decoder: Arc<dyn Decoder>,
        doc: Doc,
    ) -> Playback {
        let shared = Arc::new(Shared {
            playing: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            rate: AtomicU64::new(48_000),
            device: AtomicBool::new(false),
            latency_ns: AtomicI64::new(0),
            ring: Mutex::new((0, VecDeque::new())),
            clock: Mutex::new(Clock {
                base_tl: 0,
                heard_frames: 0,
                heard_at: None,
                wall_start: Instant::now(),
            }),
        });
        let (tx, rx) = channel();
        let s = shared.clone();
        let dir = dir.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("sc-playback-audio".into())
            .spawn(move || run(rx, s, project, &dir, decoder, doc));
        if let Err(e) = spawned {
            tracing::warn!("playback audio thread: {e}");
        }
        Playback { tx, shared }
    }

    pub fn play(&self, from_tl: i64) {
        {
            let mut c = lock(&self.shared.clock);
            c.base_tl = from_tl;
            c.heard_frames = 0;
            c.heard_at = None;
            c.wall_start = Instant::now();
        }
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        self.shared.playing.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Cmd::Play(from_tl));
    }

    pub fn pause(&self) {
        self.shared.playing.store(false, Ordering::SeqCst);
        let _ = self.tx.send(Cmd::Pause);
    }

    pub fn set_doc(&self, doc: Doc) {
        let _ = self.tx.send(Cmd::SetDoc(Box::new(doc)));
    }

    pub fn playing(&self) -> bool {
        self.shared.playing.load(Ordering::SeqCst)
    }

    pub fn has_device(&self) -> bool {
        self.shared.device.load(Ordering::SeqCst)
    }

    /// The timeline time being heard now. Before the device has played anything it stays at
    /// the start, so the picture waits for the sound.
    pub fn now_tl(&self) -> i64 {
        now_tl(&self.shared)
    }
}

fn now_tl(s: &Shared) -> i64 {
    let c = lock(&s.clock);
    if !s.device.load(Ordering::SeqCst) {
        return c.base_tl + c.wall_start.elapsed().as_nanos() as i64;
    }
    let Some(at) = c.heard_at else {
        return c.base_tl;
    };
    let rate = s.rate.load(Ordering::Relaxed).max(1);
    let heard = (i128::from(c.heard_frames) * 1_000_000_000 / i128::from(rate)) as i64;
    let latency = s.latency_ns.load(Ordering::Relaxed);
    (c.base_tl + heard - latency + at.elapsed().as_nanos() as i64).max(c.base_tl)
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.shared.playing.store(false, Ordering::SeqCst);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn open_stream(shared: &Arc<Shared>) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let config = device
        .default_output_config()
        .map_err(|e| tracing::warn!("playback: output config: {e}"))
        .ok()?;
    let mut cfg: cpal::StreamConfig = config.into();
    let rate = u64::from(cfg.sample_rate);
    let channels = usize::from(cfg.channels).max(1);
    cfg.channels = channels as u16;
    shared.rate.store(rate, Ordering::SeqCst);
    let s = shared.clone();
    let stream = device
        .build_output_stream(
            cfg,
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                fill_device(&s, data, channels, info);
            },
            |e| tracing::warn!("playback stream: {e}"),
            None,
        )
        .map_err(|e| tracing::warn!("playback: output stream: {e}"))
        .ok()?;
    stream
        .play()
        .map_err(|e| tracing::warn!("playback: start stream: {e}"))
        .ok()?;
    Some(stream)
}

fn fill_device(s: &Shared, data: &mut [f32], channels: usize, info: &cpal::OutputCallbackInfo) {
    let ts = info.timestamp();
    if let Some(d) = ts.playback.checked_duration_since(ts.callback) {
        s.latency_ns.store(d.as_nanos() as i64, Ordering::Relaxed);
    }
    let frames = data.len() / channels;
    let mut n = 0;
    if s.playing.load(Ordering::Relaxed)
        && let Ok(mut ring) = s.ring.try_lock()
    {
        let generation = s.generation.load(Ordering::Relaxed);
        if ring.0 == generation {
            let q = &mut ring.1;
            n = frames.min(q.len() / usize::from(CHANNELS));
            for f in 0..n {
                let (l, r) = (q.pop_front().unwrap_or(0.0), q.pop_front().unwrap_or(0.0));
                let out = &mut data[f * channels..(f + 1) * channels];
                if channels == 1 {
                    out[0] = 0.5 * (l + r);
                } else {
                    out[0] = l;
                    out[1] = r;
                    out[2..].fill(0.0);
                }
            }
            if n > 0
                && let Ok(mut c) = s.clock.try_lock()
            {
                if c.heard_at.is_none() {
                    c.heard_frames = 0;
                } else {
                    c.heard_frames += n as u64;
                }
                c.heard_at = Some(Instant::now());
            }
        }
    }
    data[n * channels..].fill(0.0);
}

fn run(
    rx: Receiver<Cmd>,
    shared: Arc<Shared>,
    project: Arc<ProjectV1>,
    dir: &Path,
    decoder: Arc<dyn Decoder>,
    doc: Doc,
) {
    let stream = open_stream(&shared);
    shared.device.store(stream.is_some(), Ordering::SeqCst);
    let rate = shared.rate.load(Ordering::SeqCst) as u32;
    let mut audio = TimelineAudio::new(project, dir, decoder, doc, rate);
    let mut block = vec![0f32; BLOCK_FRAMES * usize::from(CHANNELS)];
    let ahead = (AHEAD.as_secs_f64() * f64::from(rate)) as usize * usize::from(CHANNELS);
    loop {
        let wait = if shared.playing.load(Ordering::SeqCst) {
            Duration::from_millis(5)
        } else {
            Duration::from_millis(200)
        };
        match rx.recv_timeout(wait) {
            Ok(Cmd::Play(from)) => {
                audio.seek(from);
                let mut ring = lock(&shared.ring);
                ring.0 = shared.generation.load(Ordering::SeqCst);
                ring.1.clear();
            }
            Ok(Cmd::Pause) => lock(&shared.ring).1.clear(),
            Ok(Cmd::SetDoc(d)) => {
                audio.set_doc(*d);
                if shared.playing.load(Ordering::SeqCst) {
                    let now = now_tl(&shared);
                    audio.seek(now);
                    let mut c = lock(&shared.clock);
                    c.base_tl = now;
                    c.heard_frames = 0;
                    c.heard_at = None;
                    c.wall_start = Instant::now();
                    drop(c);
                    let generation = shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
                    let mut ring = lock(&shared.ring);
                    ring.0 = generation;
                    ring.1.clear();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if stream.is_none() || !shared.playing.load(Ordering::SeqCst) {
            continue;
        }
        loop {
            let queued = lock(&shared.ring).1.len();
            if queued >= ahead {
                break;
            }
            if let Err(e) = audio.read(&mut block) {
                tracing::warn!("playback audio: {e}");
                block.fill(0.0);
            }
            lock(&shared.ring).1.extend(block.iter().copied());
        }
    }
    drop(stream);
}
