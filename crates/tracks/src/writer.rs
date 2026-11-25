//! Raw-track writers that pipe frames into the FFmpeg helper (Matroska, crash-tolerant).
//!
//! Video is written at a constant rate: a frame lands in the slot nearest its time and the
//! previous image fills skipped slots, so frame `k` of the file is at `k / fps` and readers need
//! no index. A helper outlives a killed app: it reads EOF on stdin and finishes the file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use sc_geometry::SizePx;
use sc_helpers::{HelperProcess, SpawnSpec};
use sc_platform::{
    Append, AudioBuffer, AudioData, AudioWriterSpec, CpuNv12Frame, Error, FinishedTrack,
    FrameBuffer, Result, TrackWriter, VideoCodec, VideoFrame, VideoWriterSpec, WriterFactory,
};

use crate::encoders;
use crate::nv12::{even, pack_nv12};

const VIDEO_QUEUE: usize = 4;
const AUDIO_QUEUE: usize = 256;
const NS: f64 = 1e9;

fn args_prefix() -> Vec<String> {
    ["-hide_banner", "-loglevel", "warning"]
        .map(String::from)
        .to_vec()
}

fn helper_error(what: &str, tail: &[String]) -> Error {
    Error::Other(format!("{what}; helper said: {}", tail.join(" | ")))
}

#[derive(Debug, Clone)]
pub struct HelperWriters {
    ffmpeg: PathBuf,
}

impl HelperWriters {
    pub fn new(ffmpeg: PathBuf) -> HelperWriters {
        HelperWriters { ffmpeg }
    }

    pub fn locate() -> Result<HelperWriters> {
        Ok(HelperWriters::new(sc_helpers::locate(
            sc_helpers::HelperKind::Ffmpeg,
        )?))
    }

    pub fn ffmpeg(&self) -> &Path {
        &self.ffmpeg
    }
}

impl WriterFactory for HelperWriters {
    fn extension(&self) -> &'static str {
        "mkv"
    }

    fn video(&self, path: &Path, spec: &VideoWriterSpec) -> Result<Box<dyn TrackWriter>> {
        Ok(Box::new(VideoWriter::start(&self.ffmpeg, path, spec)?))
    }

    fn audio(&self, path: &Path, spec: &AudioWriterSpec) -> Result<Box<dyn TrackWriter>> {
        Ok(Box::new(AudioWriter::start(&self.ffmpeg, path, spec)?))
    }

    fn encode_benchmark(
        &self,
        size: SizePx,
        fps: u32,
        frames: u32,
        codec: VideoCodec,
    ) -> Result<f64> {
        let dir = std::env::temp_dir().join(format!("sc-bench-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("bench.mkv");
        let spec = VideoWriterSpec {
            size,
            fps,
            codec,
            keyframe_interval_s: 1.0,
            fragment_interval_s: 1.0,
            bits_per_pixel: 0.05,
        };
        let mut w = VideoWriter::start(&self.ffmpeg, &path, &spec)?;
        let size = even(size);
        let frame = FrameBuffer(Arc::new(CpuNv12Frame::filled(size, 120, 100, 150, false)));
        let t0 = Instant::now();
        for i in 0..frames {
            let f = VideoFrame {
                stream: sc_platform::StreamTag(0),
                pts: sc_platform::HostTime(0),
                size,
                content_rect_pt: None,
                content_scale: None,
                buffer: frame.clone(),
            };
            while !w.is_ready() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            w.append_video(&f, i64::from(i) * 1_000_000_000 / i64::from(fps.max(1)))?;
        }
        Box::new(w).finish(None)?;
        let secs = t0.elapsed().as_secs_f64();
        let _ = std::fs::remove_dir_all(&dir);
        Ok(f64::from(frames) / secs.max(1e-6))
    }
}

enum Msg {
    Frame { slot: u64, bytes: Arc<Vec<u8>> },
    End { slot: u64 },
}

struct Outcome {
    slots: u64,
    error: Option<String>,
}

pub struct VideoWriter {
    path: PathBuf,
    size: SizePx,
    fps: u32,
    tx: SyncSender<Msg>,
    inflight: Arc<AtomicUsize>,
    failed: Arc<Mutex<Option<String>>>,
    thread: Option<JoinHandle<Outcome>>,
    appended: u64,
    last_slot: Option<u64>,
}

impl VideoWriter {
    pub fn start(ffmpeg: &Path, path: &Path, spec: &VideoWriterSpec) -> Result<VideoWriter> {
        let size = even(spec.size);
        let fps = spec.fps.max(1);
        let encoder = encoders::pick(ffmpeg, spec.codec);
        let gop = (f64::from(fps) * spec.keyframe_interval_s).round().max(1.0) as u32;
        let mut args = args_prefix();
        args.extend(encoders::pre_input(encoder));
        args.extend([
            "-f".to_string(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "nv12".into(),
            "-s".into(),
            format!("{}x{}", size.w, size.h),
            "-r".into(),
            fps.to_string(),
            "-i".into(),
            "pipe:0".into(),
        ]);
        args.extend(encoders::output_args(
            encoder,
            size,
            fps,
            gop,
            spec.bits_per_pixel,
        ));
        args.extend(encoders::container_args());
        args.extend(["-y".into(), path.to_string_lossy().into_owned()]);
        std::fs::File::create(path)?;
        let frame_bytes = size.w as usize * size.h as usize * 3 / 2;
        let proc = HelperProcess::spawn(SpawnSpec {
            program: ffmpeg.to_path_buf(),
            args,
            frame_bytes,
            queue_frames: 2,
        })?;
        let (tx, rx) = sync_channel(VIDEO_QUEUE);
        let inflight = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(Mutex::new(None));
        let (i, f) = (inflight.clone(), failed.clone());
        let thread = std::thread::Builder::new()
            .name("track-video".into())
            .spawn(move || video_loop(proc, &rx, &i, &f))?;
        Ok(VideoWriter {
            path: path.to_path_buf(),
            size,
            fps,
            tx,
            inflight,
            failed,
            thread: Some(thread),
            appended: 0,
            last_slot: None,
        })
    }

    fn slot(&self, file_ns: i64) -> u64 {
        (file_ns.max(0) as f64 * f64::from(self.fps) / NS).round() as u64
    }

    fn check(&self) -> Result<()> {
        match self.failed.lock().ok().and_then(|f| f.clone()) {
            Some(e) => Err(Error::Other(e)),
            None => Ok(()),
        }
    }
}

fn fill(proc: &HelperProcess, upto: u64, with: &[u8], next: &mut u64) -> Result<()> {
    while *next < upto {
        proc.write_frame(with)?;
        *next += 1;
    }
    Ok(())
}

fn video_loop(
    proc: HelperProcess,
    rx: &Receiver<Msg>,
    inflight: &AtomicUsize,
    failed: &Mutex<Option<String>>,
) -> Outcome {
    let mut next = 0u64;
    let mut last: Option<Arc<Vec<u8>>> = None;
    let mut error = None;
    while let Ok(msg) = rx.recv() {
        let (r, end) = match msg {
            Msg::Frame { slot, bytes } => {
                inflight.fetch_sub(1, Ordering::AcqRel);
                let r = if slot < next {
                    Ok(())
                } else {
                    let pad = last.as_ref().unwrap_or(&bytes).clone();
                    fill(&proc, slot, &pad, &mut next)
                        .and_then(|()| fill(&proc, slot + 1, &bytes, &mut next))
                };
                last = Some(bytes);
                (r, false)
            }
            Msg::End { slot } => {
                let r = match &last {
                    Some(l) => fill(&proc, slot, l, &mut next),
                    None => Ok(()),
                };
                (r, true)
            }
        };
        if let Err(e) = r {
            let e = e.to_string();
            if let Ok(mut f) = failed.lock() {
                *f = Some(e.clone());
            }
            error = Some(e);
            break;
        }
        if end {
            break;
        }
    }
    match proc.finish() {
        Ok(rep) if rep.success() => {}
        Ok(rep) => {
            let e = helper_error(
                &format!("video helper exited with {:?}", rep.code),
                &rep.stderr_tail,
            );
            error.get_or_insert(e.to_string());
        }
        Err(e) => {
            error.get_or_insert(e.to_string());
        }
    }
    Outcome { slots: next, error }
}

impl TrackWriter for VideoWriter {
    fn append_video(&mut self, f: &VideoFrame, file_ns: i64) -> Result<Append> {
        self.check()?;
        if !self.is_ready() {
            return Ok(Append::NotReady);
        }
        let slot = self.slot(file_ns);
        let mut bytes = Vec::new();
        pack_nv12(f.buffer.0.as_ref(), self.size, &mut bytes)?;
        self.inflight.fetch_add(1, Ordering::AcqRel);
        match self.tx.try_send(Msg::Frame {
            slot,
            bytes: Arc::new(bytes),
        }) {
            Ok(()) => {
                self.appended += 1;
                self.last_slot = Some(self.last_slot.map_or(slot, |s| s.max(slot)));
                Ok(Append::Written)
            }
            Err(TrySendError::Full(_)) => {
                self.inflight.fetch_sub(1, Ordering::AcqRel);
                Ok(Append::NotReady)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.check()?;
                Err(Error::Other("video track writer stopped".into()))
            }
        }
    }

    fn append_audio(&mut self, _: &AudioBuffer, _: i64, _: u32) -> Result<Append> {
        Err(Error::Other("audio appended to a video track".into()))
    }

    fn append_silence(&mut self, _: i64, _: u32) -> Result<Append> {
        Err(Error::Other("silence appended to a video track".into()))
    }

    fn is_ready(&self) -> bool {
        self.inflight.load(Ordering::Acquire) < VIDEO_QUEUE
    }

    fn finish(mut self: Box<Self>, end_file_ns: Option<i64>) -> Result<FinishedTrack> {
        let end = end_file_ns
            .map(|ns| self.slot(ns))
            .unwrap_or_else(|| self.last_slot.map_or(0, |s| s + 1));
        let _ = self.tx.send(Msg::End { slot: end });
        let outcome = match self.thread.take() {
            Some(t) => t
                .join()
                .map_err(|_| Error::Other("video track thread panicked".into()))?,
            None => Outcome {
                slots: 0,
                error: None,
            },
        };
        if let Some(e) = outcome.error {
            return Err(Error::Other(e));
        }
        Ok(FinishedTrack {
            path: self.path.clone(),
            duration_ns: Some((outcome.slots as f64 * NS / f64::from(self.fps)) as i64),
            samples: self.appended,
        })
    }
}

pub struct AudioWriter {
    path: PathBuf,
    rate: u32,
    channels: u16,
    proc: Option<HelperProcess>,
    written: u64,
    scratch: Vec<u8>,
}

impl AudioWriter {
    pub fn start(ffmpeg: &Path, path: &Path, spec: &AudioWriterSpec) -> Result<AudioWriter> {
        let rate = spec.sample_rate.max(8_000);
        let channels = spec.channels.clamp(1, 2);
        let mut args = args_prefix();
        args.extend([
            "-f".to_string(),
            "f32le".into(),
            "-ar".into(),
            rate.to_string(),
            "-ac".into(),
            channels.to_string(),
            "-i".into(),
            "pipe:0".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            spec.bitrate.max(64_000).to_string(),
        ]);
        args.extend(encoders::container_args());
        args.extend(["-y".into(), path.to_string_lossy().into_owned()]);
        std::fs::File::create(path)?;
        let proc = HelperProcess::spawn(SpawnSpec {
            program: ffmpeg.to_path_buf(),
            args,
            frame_bytes: 0,
            queue_frames: AUDIO_QUEUE,
        })?;
        Ok(AudioWriter {
            path: path.to_path_buf(),
            rate,
            channels,
            proc: Some(proc),
            written: 0,
            scratch: Vec::new(),
        })
    }

    fn proc(&self) -> Result<&HelperProcess> {
        self.proc
            .as_ref()
            .ok_or_else(|| Error::Other("audio track already finished".into()))
    }

    fn frame_at(&self, file_ns: i64) -> i64 {
        (file_ns as f64 * f64::from(self.rate) / NS).round() as i64
    }

    fn write_silence(&mut self, frames: u64) -> Result<()> {
        const CHUNK: u64 = 4_800;
        let mut left = frames;
        while left > 0 {
            let n = left.min(CHUNK);
            self.scratch.clear();
            self.scratch
                .resize(n as usize * usize::from(self.channels) * 4, 0);
            self.proc()?.write_frame(&self.scratch)?;
            self.written += n;
            left -= n;
        }
        Ok(())
    }

    fn write_samples(&mut self, samples: &[f32], src_channels: u16) -> Result<()> {
        let src_ch = usize::from(src_channels.max(1));
        let out_ch = usize::from(self.channels);
        let frames = samples.len() / src_ch;
        let mut bytes = std::mem::take(&mut self.scratch);
        bytes.clear();
        bytes.reserve(frames * out_ch * 4);
        for f in samples.chunks_exact(src_ch) {
            for c in 0..out_ch {
                let v = if out_ch == 1 && src_ch > 1 {
                    f.iter().sum::<f32>() / src_ch as f32
                } else {
                    f[c.min(src_ch - 1)]
                };
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        let r = self.proc()?.write_frame(&bytes);
        self.scratch = bytes;
        r?;
        self.written += frames as u64;
        Ok(())
    }
}

impl TrackWriter for AudioWriter {
    fn append_video(&mut self, _: &VideoFrame, _: i64) -> Result<Append> {
        Err(Error::Other("video appended to an audio track".into()))
    }

    fn append_audio(&mut self, a: &AudioBuffer, file_ns: i64, skip_frames: u32) -> Result<Append> {
        if !self.is_ready() {
            return Ok(Append::NotReady);
        }
        let AudioData::F32Interleaved(samples) = &a.data else {
            return Err(Error::Unsupported("native audio buffers in helper tracks"));
        };
        if a.sample_rate != self.rate {
            return Err(Error::Other(format!(
                "audio at {} Hz into a {} Hz track",
                a.sample_rate, self.rate
            )));
        }
        let ch = usize::from(a.channels.max(1));
        let start = self.frame_at(file_ns) + i64::from(skip_frames);
        let expected = self.written as i64;
        let tolerance = i64::from(self.rate) / 500;
        let mut skip = skip_frames as usize;
        if start > expected + tolerance {
            self.write_silence((start - expected) as u64)?;
        } else if start < expected - tolerance {
            skip += (expected - start) as usize;
        }
        let total = samples.len() / ch;
        if skip < total {
            self.write_samples(&samples[skip * ch..total * ch], a.channels)?;
        }
        Ok(Append::Written)
    }

    fn append_silence(&mut self, file_ns: i64, frames: u32) -> Result<Append> {
        let end = self.frame_at(file_ns) + i64::from(frames);
        let written = self.written as i64;
        if end > written {
            self.write_silence((end - written) as u64)?;
        }
        Ok(Append::Written)
    }

    fn is_ready(&self) -> bool {
        self.proc.as_ref().is_some_and(HelperProcess::has_room)
    }

    fn finish(mut self: Box<Self>, end_file_ns: Option<i64>) -> Result<FinishedTrack> {
        if let Some(end) = end_file_ns {
            let end = self.frame_at(end);
            let written = self.written as i64;
            if end > written {
                self.write_silence((end - written) as u64)?;
            }
        }
        let proc = self
            .proc
            .take()
            .ok_or_else(|| Error::Other("audio track already finished".into()))?;
        let rep = proc.finish()?;
        if !rep.success() {
            return Err(helper_error(
                &format!("audio helper exited with {:?}", rep.code),
                &rep.stderr_tail,
            ));
        }
        Ok(FinishedTrack {
            path: self.path.clone(),
            duration_ns: Some((self.written as f64 * NS / f64::from(self.rate)) as i64),
            samples: self.written,
        })
    }
}
