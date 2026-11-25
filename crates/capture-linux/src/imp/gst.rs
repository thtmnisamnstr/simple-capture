//! The screen track through GStreamer, in process (experimental): VA-API, NVENC or OpenH264 H.264 into
//! Matroska, the same container the FFmpeg helper writes, so reading, probing and recovery stay
//! with the helper. Audio tracks stay with the helper too. Video keeps the helper's constant-rate
//! rule: a frame lands in the slot nearest its time and the previous image fills skipped slots.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use sc_geometry::SizePx;
use sc_platform::{
    Append, AudioBuffer, AudioWriterSpec, CpuNv12Frame, Error, FinishedTrack, FrameBuffer, Result,
    TrackWriter, VideoCodec, VideoFrame, VideoWriterSpec, WriterFactory,
};
use sc_tracks::HelperWriters;
use sc_tracks::nv12::{even, pack_nv12};

const NS: f64 = 1e9;
const VIDEO_QUEUE: usize = 4;
/// The widest picture these H.264 encoders take; bigger tracks go to the helper.
const MAX_SIDE: u32 = 4096;
const QUEUED_FRAMES: u64 = 4;

/// Hardware first; x264enc (GPL) only when `SC_GST_ENCODER` names it, since the app keeps GPL
/// code in its helper process.
const ENCODERS: [&str; 4] = ["vah264enc", "vaapih264enc", "nvh264enc", "openh264enc"];

fn gst_error(what: &str, e: impl std::fmt::Display) -> Error {
    Error::Other(format!("GStreamer {what}: {e}"))
}

fn encoder_props(encoder: &str, kbps: u32, gop: u32) -> String {
    match encoder {
        "vah264enc" => format!("bitrate={kbps} key-int-max={gop} rate-control=cbr"),
        "vaapih264enc" => format!("bitrate={kbps} keyframe-period={gop} rate-control=cbr"),
        "nvh264enc" => format!("bitrate={kbps} gop-size={gop} preset=low-latency-hq"),
        "openh264enc" => format!("bitrate={} gop-size={gop} complexity=low", kbps * 1000),
        "x264enc" => {
            format!("bitrate={kbps} key-int-max={gop} speed-preset=veryfast tune=zerolatency")
        }
        _ => String::new(),
    }
}

/// A frame handed to GStreamer without copying it; gap fill pushes the same frame many times.
struct SharedBytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for SharedBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

fn frame_bytes(size: SizePx) -> u64 {
    u64::from(size.w) * u64::from(size.h) * 3 / 2
}

/// The output file is set on `sink` afterwards, so no path is ever parsed as pipeline syntax.
fn describe(encoder: &str, size: SizePx, fps: u32, kbps: u32, gop: u32) -> String {
    format!(
        "appsrc name=src format=time block=false max-bytes={} \
         caps=video/x-raw,format=NV12,width={},height={},framerate={fps}/1 \
         ! queue ! {encoder} {} ! h264parse ! video/x-h264,profile=(string){{high,main,constrained-baseline}} \
         ! matroskamux ! filesink name=sink",
        frame_bytes(size) * QUEUED_FRAMES,
        size.w,
        size.h,
        encoder_props(encoder, kbps, gop),
    )
}

pub fn usable_encoder() -> Option<&'static str> {
    if let Err(e) = gst::init() {
        tracing::warn!("GStreamer unavailable: {e}");
        return None;
    }
    let forced = std::env::var("SC_GST_ENCODER").ok();
    let candidates: Vec<&'static str> = match forced.as_deref() {
        Some("x264enc") => vec!["x264enc"],
        Some(name) => ENCODERS.iter().copied().filter(|e| *e == name).collect(),
        None => ENCODERS.to_vec(),
    };
    candidates.into_iter().find(|enc| {
        if gst::ElementFactory::find(enc).is_none() {
            return false;
        }
        let w = GstWriters {
            encoder: enc,
            helper: HelperWriters::new(
                sc_helpers::locate(sc_helpers::HelperKind::Ffmpeg)
                    .unwrap_or_else(|_| sc_helpers::bundled_path(sc_helpers::HelperKind::Ffmpeg)),
            ),
        };
        match w.encode_benchmark(SizePx { w: 256, h: 144 }, 30, 5, VideoCodec::Hevc) {
            Ok(_) => true,
            Err(e) => {
                tracing::info!("GStreamer {enc}: {e}");
                false
            }
        }
    })
}

#[derive(Debug, Clone)]
pub struct GstWriters {
    pub encoder: &'static str,
    pub helper: HelperWriters,
}

impl WriterFactory for GstWriters {
    fn extension(&self) -> &'static str {
        "mkv"
    }

    fn video(&self, path: &Path, spec: &VideoWriterSpec) -> Result<Box<dyn TrackWriter>> {
        if spec.size.w > MAX_SIDE || spec.size.h > MAX_SIDE {
            return self.helper.video(path, spec);
        }
        Ok(Box::new(GstVideoWriter::start(self.encoder, path, spec)?))
    }

    fn audio(&self, path: &Path, spec: &AudioWriterSpec) -> Result<Box<dyn TrackWriter>> {
        self.helper.audio(path, spec)
    }

    fn encode_benchmark(
        &self,
        size: SizePx,
        fps: u32,
        frames: u32,
        codec: VideoCodec,
    ) -> Result<f64> {
        let dir = std::env::temp_dir().join(format!("sc-gst-bench-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("bench-{}.mkv", sc_core::ids::new_id()));
        let spec = VideoWriterSpec {
            size,
            fps,
            codec,
            keyframe_interval_s: 1.0,
            fragment_interval_s: 1.0,
            bits_per_pixel: 0.05,
        };
        let mut w = GstVideoWriter::start(self.encoder, &path, &spec)?;
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
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
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

struct Pipe {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
    max_bytes: u64,
}

impl Pipe {
    fn open(desc: &str, location: &Path, max_bytes: u64) -> Result<Pipe> {
        gst::init().map_err(|e| gst_error("init", e))?;
        let pipeline = gst::parse::launch(desc)
            .map_err(|e| gst_error("pipeline", e))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| Error::Other("GStreamer: not a pipeline".into()))?;
        let src = pipeline
            .by_name("src")
            .and_then(|e| e.downcast::<gst_app::AppSrc>().ok())
            .ok_or_else(|| Error::Other("GStreamer: no appsrc".into()))?;
        let sink = pipeline
            .by_name("sink")
            .filter(|e| e.has_property_with_type("location", gst::glib::Type::STRING))
            .ok_or_else(|| Error::Other("GStreamer: no filesink".into()))?;
        sink.set_property("location", location);
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| gst_error("start", e))?;
        Ok(Pipe {
            pipeline,
            src,
            max_bytes,
        })
    }

    fn bus_error(&self) -> Option<String> {
        let bus = self.pipeline.bus()?;
        let msg = bus.pop_filtered(&[gst::MessageType::Error])?;
        match msg.view() {
            gst::MessageView::Error(e) => Some(format!("{} ({:?})", e.error(), e.debug())),
            _ => None,
        }
    }

    /// Waits for room in the queue instead of blocking inside appsrc, so a pipeline that errors
    /// or stalls is noticed rather than hanging the writer.
    fn push(&self, bytes: &Arc<Vec<u8>>, pts_ns: u64, dur_ns: u64) -> Result<()> {
        let since = Instant::now();
        loop {
            if let Some(e) = self.bus_error() {
                return Err(Error::Other(format!("GStreamer: {e}")));
            }
            if self.src.current_level_bytes() < self.max_bytes {
                break;
            }
            if since.elapsed() > std::time::Duration::from_secs(10) {
                return Err(Error::Other(
                    "GStreamer: the encoder stopped taking frames".into(),
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let mut buf = gst::Buffer::from_slice(SharedBytes(bytes.clone()));
        {
            let b = buf
                .get_mut()
                .ok_or_else(|| Error::Other("GStreamer: shared buffer".into()))?;
            b.set_pts(gst::ClockTime::from_nseconds(pts_ns));
            b.set_duration(gst::ClockTime::from_nseconds(dur_ns));
        }
        self.src
            .push_buffer(buf)
            .map(|_| ())
            .map_err(|e| gst_error("push", e))
    }

    fn finish(self) -> Result<()> {
        let end = self.src.end_of_stream();
        let mut error = end.err().map(|e| format!("end of stream: {e}"));
        if let Some(bus) = self.pipeline.bus() {
            let wait = gst::ClockTime::from_seconds(30);
            match bus.timed_pop_filtered(wait, &[gst::MessageType::Eos, gst::MessageType::Error]) {
                Some(m) => {
                    if let gst::MessageView::Error(e) = m.view() {
                        error.get_or_insert(format!("{} ({:?})", e.error(), e.debug()));
                    }
                }
                None => {
                    error.get_or_insert("no end of stream after 30 s".into());
                }
            }
        }
        let _ = self.pipeline.set_state(gst::State::Null);
        match error {
            Some(e) => Err(Error::Other(format!("GStreamer: {e}"))),
            None => Ok(()),
        }
    }
}

pub struct GstVideoWriter {
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

impl GstVideoWriter {
    pub fn start(encoder: &str, path: &Path, spec: &VideoWriterSpec) -> Result<GstVideoWriter> {
        let size = even(spec.size);
        let fps = spec.fps.max(1);
        let gop = (f64::from(fps) * spec.keyframe_interval_s).round().max(1.0) as u32;
        let kbps =
            (spec.bits_per_pixel * 1.6 * f64::from(size.w) * f64::from(size.h) * f64::from(fps)
                / 1000.0)
                .clamp(500.0, 200_000.0) as u32;
        std::fs::File::create(path)?;
        let pipe = Pipe::open(
            &describe(encoder, size, fps, kbps, gop),
            path,
            frame_bytes(size) * QUEUED_FRAMES,
        )?;
        let (tx, rx) = sync_channel(VIDEO_QUEUE);
        let inflight = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(Mutex::new(None));
        let (i, f) = (inflight.clone(), failed.clone());
        let thread = std::thread::Builder::new()
            .name("track-video-gst".into())
            .spawn(move || video_loop(pipe, fps, &rx, &i, &f))?;
        Ok(GstVideoWriter {
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

fn video_loop(
    pipe: Pipe,
    fps: u32,
    rx: &Receiver<Msg>,
    inflight: &AtomicUsize,
    failed: &Mutex<Option<String>>,
) -> Outcome {
    let at = |slot: u64| (u128::from(slot) * 1_000_000_000 / u128::from(fps)) as u64;
    let dur = 1_000_000_000 / u64::from(fps);
    let mut next = 0u64;
    let mut last: Option<Arc<Vec<u8>>> = None;
    let mut error = None;
    let fill = |upto: u64, with: &Arc<Vec<u8>>, next: &mut u64| -> Result<()> {
        while *next < upto {
            pipe.push(with, at(*next), dur)?;
            *next += 1;
        }
        Ok(())
    };
    while let Ok(msg) = rx.recv() {
        let (r, end) = match msg {
            Msg::Frame { slot, bytes } => {
                inflight.fetch_sub(1, Ordering::AcqRel);
                let r = if slot < next {
                    Ok(())
                } else {
                    let pad = last.as_ref().unwrap_or(&bytes).clone();
                    fill(slot, &pad, &mut next).and_then(|()| fill(slot + 1, &bytes, &mut next))
                };
                last = Some(bytes);
                (r, false)
            }
            Msg::End { slot } => {
                let r = match &last {
                    Some(l) => fill(slot, l, &mut next),
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
    if let Err(e) = pipe.finish() {
        error.get_or_insert(e.to_string());
    }
    Outcome { slots: next, error }
}

impl TrackWriter for GstVideoWriter {
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

#[cfg(test)]
mod tests {
    use super::*;
    use sc_platform::Decoder;
    use sc_tracks::HelperDecoder;

    fn frame(size: SizePx, luma: u8) -> VideoFrame {
        VideoFrame {
            stream: sc_platform::StreamTag(0),
            pts: sc_platform::HostTime(0),
            size,
            content_rect_pt: None,
            content_scale: None,
            buffer: FrameBuffer(Arc::new(CpuNv12Frame::filled(size, luma, 128, 128, false))),
        }
    }

    /// CI installs x264enc only for this test; the app never picks it on its own.
    #[test]
    fn a_gstreamer_track_reads_back_through_the_helper() {
        if gst::init().is_err() || gst::ElementFactory::find("x264enc").is_none() {
            println!("SKIPPED: no GStreamer with x264enc here");
            return;
        }
        let dir = std::env::temp_dir().join(format!("sc-gst-{}", sc_core::ids::new_id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("screen.mkv");
        let size = SizePx { w: 640, h: 360 };
        let spec = VideoWriterSpec {
            size,
            fps: 30,
            codec: VideoCodec::Hevc,
            keyframe_interval_s: 1.0,
            fragment_interval_s: 1.0,
            bits_per_pixel: 0.1,
        };
        let mut w = GstVideoWriter::start("x264enc", &path, &spec).expect("writer");
        for i in 0..60i64 {
            if i == 40 || i == 41 {
                continue;
            }
            while !w.is_ready() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let f = frame(size, if i < 30 { 60 } else { 180 });
            w.append_video(&f, i * 1_000_000_000 / 30).expect("append");
        }
        let done = Box::new(w).finish(Some(2_000_000_000)).expect("finish");
        assert_eq!(done.duration_ns, Some(2_000_000_000));
        let ffmpeg = std::env::var("SC_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
        let d = HelperDecoder::new(PathBuf::from(ffmpeg));
        let probe = d.probe(&path).expect("probe");
        println!("gst probe: {probe:?}");
        assert!(probe.readable);
        assert_eq!(probe.size, Some(size));
        assert_eq!(probe.samples, 60, "skipped slots are filled");
        let mut r = d.open_video(&path).expect("reader");
        let mut n = 0;
        let mut lumas = Vec::new();
        while let Some(f) = r.next_frame().expect("frame") {
            f.frame
                .0
                .with_nv12(&mut |v| {
                    let s: u64 = v.y.iter().map(|&b| u64::from(b)).sum();
                    lumas.push(s as f64 / v.y.len() as f64);
                })
                .expect("nv12");
            n += 1;
        }
        assert_eq!(n, 60);
        assert!((lumas[5] - 60.0).abs() < 6.0, "{}", lumas[5]);
        assert!((lumas[50] - 180.0).abs() < 6.0, "{}", lumas[50]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
