//! Raw tracks through Media Foundation, in process (experimental): H.264 and AAC into fragmented MP4
//! through a sink writer (a hardware encoder when the system has one), read back with a source
//! reader. Video keeps the helper's constant-rate rule: a frame lands in the slot nearest its
//! time and the previous image fills skipped slots.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, Once};
use std::thread::JoinHandle;
use std::time::Instant;

use sc_geometry::SizePx;
use sc_platform::{
    Append, AudioBuffer, AudioData, AudioReader, AudioWriterSpec, CpuNv12Frame, DecodedFrame,
    Decoder, Error, FinishedTrack, FrameBuffer, Result, TrackProbe, TrackWriter, VideoCodec,
    VideoFrame, VideoReader, VideoWriterSpec, WriterFactory,
};
use sc_tracks::HelperWriters;
use sc_tracks::nv12::{even, pack_nv12};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Variant::VT_I8;
use windows::core::{GUID, HSTRING, Interface};

use super::wgc::os;

const NS: f64 = 1e9;
const HNS_PER_S: i64 = 10_000_000;
const VIDEO_QUEUE: usize = 4;
const AUDIO_QUEUE: usize = 256;
/// The widest picture H.264 encoders take; bigger tracks go to the helper (HEVC or x264).
const MAX_SIDE: u32 = 4096;

fn startup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        if let Err(e) = MFStartup((MF_SDK_VERSION << 16) | MF_API_VERSION, MFSTARTUP_FULL) {
            tracing::warn!("MFStartup: {e}");
        }
    });
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn pack(hi: u32, lo: u32) -> u64 {
    (u64::from(hi) << 32) | u64::from(lo)
}

fn unpack(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, v as u32)
}

fn attributes(n: u32) -> Result<IMFAttributes> {
    let mut a: Option<IMFAttributes> = None;
    unsafe { MFCreateAttributes(&mut a, n) }.map_err(os)?;
    a.ok_or_else(|| Error::Other("MFCreateAttributes".into()))
}

fn media_type(major: &GUID, sub: &GUID) -> Result<IMFMediaType> {
    unsafe {
        let t = MFCreateMediaType().map_err(os)?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, major).map_err(os)?;
        t.SetGUID(&MF_MT_SUBTYPE, sub).map_err(os)?;
        Ok(t)
    }
}

fn sample(bytes: &[u8], time_hns: i64, dur_hns: i64) -> Result<IMFSample> {
    unsafe {
        let buf = MFCreateMemoryBuffer(bytes.len() as u32).map_err(os)?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        buf.Lock(&mut ptr, None, None).map_err(os)?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        buf.Unlock().map_err(os)?;
        buf.SetCurrentLength(bytes.len() as u32).map_err(os)?;
        let s = MFCreateSample().map_err(os)?;
        s.AddBuffer(&buf).map_err(os)?;
        s.SetSampleTime(time_hns).map_err(os)?;
        s.SetSampleDuration(dur_hns).map_err(os)?;
        Ok(s)
    }
}

/// A sink writer with one stream, `fmp4` for crash tolerance.
struct Sink {
    writer: IMFSinkWriter,
    stream: u32,
}

impl Sink {
    fn open(
        path: &Path,
        output: &IMFMediaType,
        input: &IMFMediaType,
        params: Option<&IMFAttributes>,
    ) -> Result<Sink> {
        startup();
        unsafe {
            let attrs = attributes(2)?;
            attrs
                .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)
                .map_err(os)?;
            attrs
                .SetGUID(
                    &MF_TRANSCODE_CONTAINERTYPE,
                    &MFTranscodeContainerType_FMPEG4,
                )
                .map_err(os)?;
            let url = HSTRING::from(path.as_os_str());
            let writer = MFCreateSinkWriterFromURL(&url, None, &attrs).map_err(os)?;
            let stream = writer.AddStream(output).map_err(os)?;
            writer
                .SetInputMediaType(stream, input, params)
                .map_err(os)?;
            writer.BeginWriting().map_err(os)?;
            Ok(Sink { writer, stream })
        }
    }

    fn write(&self, bytes: &[u8], time_hns: i64, dur_hns: i64) -> Result<()> {
        let s = sample(bytes, time_hns, dur_hns)?;
        unsafe { self.writer.WriteSample(self.stream, &s) }.map_err(os)
    }

    fn finish(self) -> Result<()> {
        unsafe { self.writer.Finalize() }.map_err(os)
    }
}

fn video_types(
    size: SizePx,
    fps: u32,
    gop: u32,
    bitrate: u32,
) -> Result<(IMFMediaType, IMFMediaType)> {
    unsafe {
        let out = media_type(&MFMediaType_Video, &MFVideoFormat_H264)?;
        out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate).map_err(os)?;
        out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(os)?;
        out.SetUINT64(&MF_MT_FRAME_SIZE, pack(size.w, size.h))
            .map_err(os)?;
        out.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1)).map_err(os)?;
        out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))
            .map_err(os)?;
        out.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)
            .map_err(os)?;
        out.SetUINT32(&MF_MT_MAX_KEYFRAME_SPACING, gop)
            .map_err(os)?;
        let inp = media_type(&MFMediaType_Video, &MFVideoFormat_NV12)?;
        inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(os)?;
        inp.SetUINT64(&MF_MT_FRAME_SIZE, pack(size.w, size.h))
            .map_err(os)?;
        inp.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1)).map_err(os)?;
        inp.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))
            .map_err(os)?;
        inp.SetUINT32(&MF_MT_DEFAULT_STRIDE, size.w).map_err(os)?;
        Ok((out, inp))
    }
}

/// The documented encoder settings: a keyframe every `gop` frames and no B-frames, so seeks
/// decode little and stored times need no reordering.
fn video_params(gop: u32) -> Result<IMFAttributes> {
    let p = attributes(2)?;
    unsafe {
        p.SetUINT32(&CODECAPI_AVEncMPVGOPSize, gop).map_err(os)?;
        p.SetUINT32(&CODECAPI_AVEncMPVDefaultBPictureCount, 0)
            .map_err(os)?;
    }
    Ok(p)
}

fn audio_types(rate: u32, channels: u16, bitrate: u32) -> Result<(IMFMediaType, IMFMediaType)> {
    let ch = u32::from(channels);
    let bytes_per_s = match bitrate {
        0..=111_999 => 12_000,
        112_000..=143_999 => 16_000,
        144_000..=175_999 => 20_000,
        _ => 24_000,
    };
    unsafe {
        let out = media_type(&MFMediaType_Audio, &MFAudioFormat_AAC)?;
        out.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
            .map_err(os)?;
        out.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)
            .map_err(os)?;
        out.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch).map_err(os)?;
        out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, bytes_per_s)
            .map_err(os)?;
        out.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0).map_err(os)?;
        let inp = media_type(&MFMediaType_Audio, &MFAudioFormat_PCM)?;
        inp.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)
            .map_err(os)?;
        inp.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)
            .map_err(os)?;
        inp.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch).map_err(os)?;
        inp.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 2 * ch)
            .map_err(os)?;
        inp.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, rate * 2 * ch)
            .map_err(os)?;
        inp.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)
            .map_err(os)?;
        Ok((out, inp))
    }
}

/// H.264 and AAC whatever codec is asked for; pictures over [`MAX_SIDE`] and audio at rates the
/// AAC encoder refuses go to the helper, whose Matroska the decoder reads through its fallback.
#[derive(Debug, Clone)]
pub struct MfWriters {
    pub helper: HelperWriters,
}

impl MfWriters {
    /// Whether this system can write an H.264 track (Windows "N" editions without the Media
    /// Feature Pack cannot).
    pub fn usable(&self) -> bool {
        self.encode_benchmark(SizePx { w: 64, h: 64 }, 30, 3, VideoCodec::Hevc)
            .map_err(|e| tracing::warn!("Media Foundation writer unavailable: {e}"))
            .is_ok()
    }
}

impl WriterFactory for MfWriters {
    fn extension(&self) -> &'static str {
        "mp4"
    }

    fn video(&self, path: &Path, spec: &VideoWriterSpec) -> Result<Box<dyn TrackWriter>> {
        if spec.size.w > MAX_SIDE || spec.size.h > MAX_SIDE {
            return self.helper.video(path, spec);
        }
        Ok(Box::new(MfVideoWriter::start(path, spec)?))
    }

    fn audio(&self, path: &Path, spec: &AudioWriterSpec) -> Result<Box<dyn TrackWriter>> {
        if !matches!(spec.sample_rate, 44_100 | 48_000) {
            return self.helper.audio(path, spec);
        }
        Ok(Box::new(MfAudioWriter::start(path, spec)?))
    }

    fn encode_benchmark(
        &self,
        size: SizePx,
        fps: u32,
        frames: u32,
        codec: VideoCodec,
    ) -> Result<f64> {
        let dir = std::env::temp_dir().join(format!("sc-mf-bench-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("bench-{}.mp4", sc_core::ids::new_id()));
        let spec = VideoWriterSpec {
            size,
            fps,
            codec,
            keyframe_interval_s: 1.0,
            fragment_interval_s: 1.0,
            bits_per_pixel: 0.05,
        };
        let mut w = MfVideoWriter::start(&path, &spec)?;
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

enum VideoMsg {
    Frame { slot: u64, bytes: Arc<Vec<u8>> },
    End { slot: u64 },
}

struct Outcome {
    slots: u64,
    error: Option<String>,
}

pub struct MfVideoWriter {
    path: PathBuf,
    size: SizePx,
    fps: u32,
    tx: SyncSender<VideoMsg>,
    inflight: Arc<AtomicUsize>,
    failed: Arc<Mutex<Option<String>>>,
    thread: Option<JoinHandle<Outcome>>,
    appended: u64,
    last_slot: Option<u64>,
}

impl MfVideoWriter {
    pub fn start(path: &Path, spec: &VideoWriterSpec) -> Result<MfVideoWriter> {
        let size = even(spec.size);
        let fps = spec.fps.max(1);
        let gop = (f64::from(fps) * spec.keyframe_interval_s).round().max(1.0) as u32;
        let bitrate = (spec.bits_per_pixel * f64::from(size.w) * f64::from(size.h) * f64::from(fps))
            .clamp(500_000.0, 200_000_000.0) as u32;
        let (tx, rx) = sync_channel(VIDEO_QUEUE);
        let (opened_tx, opened_rx) = sync_channel(1);
        let inflight = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(Mutex::new(None));
        let (i, f, p) = (inflight.clone(), failed.clone(), path.to_path_buf());
        let thread = std::thread::Builder::new()
            .name("track-video-mf".into())
            .spawn(move || {
                let sink = video_types(size, fps, gop, bitrate)
                    .and_then(|(out, inp)| Sink::open(&p, &out, &inp, Some(&video_params(gop)?)));
                match sink {
                    Ok(sink) => {
                        let _ = opened_tx.send(Ok(()));
                        video_loop(sink, fps, &rx, &i, &f)
                    }
                    Err(e) => {
                        let _ = opened_tx.send(Err(e));
                        Outcome {
                            slots: 0,
                            error: None,
                        }
                    }
                }
            })?;
        opened_rx
            .recv()
            .map_err(|_| Error::Other("video writer thread ended early".into()))??;
        Ok(MfVideoWriter {
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
    sink: Sink,
    fps: u32,
    rx: &Receiver<VideoMsg>,
    inflight: &AtomicUsize,
    failed: &Mutex<Option<String>>,
) -> Outcome {
    let dur = HNS_PER_S / i64::from(fps);
    let at = |slot: u64| (i128::from(slot) * i128::from(HNS_PER_S) / i128::from(fps)) as i64;
    let mut next = 0u64;
    let mut last: Option<Arc<Vec<u8>>> = None;
    let mut error = None;
    let fill = |upto: u64, with: &[u8], next: &mut u64| -> Result<()> {
        while *next < upto {
            sink.write(with, at(*next), dur)?;
            *next += 1;
        }
        Ok(())
    };
    while let Ok(msg) = rx.recv() {
        let (r, end) = match msg {
            VideoMsg::Frame { slot, bytes } => {
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
            VideoMsg::End { slot } => {
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
    if let Err(e) = sink.finish() {
        error.get_or_insert(format!("finish video track: {e}"));
    }
    Outcome { slots: next, error }
}

impl TrackWriter for MfVideoWriter {
    fn append_video(&mut self, f: &VideoFrame, file_ns: i64) -> Result<Append> {
        self.check()?;
        if !self.is_ready() {
            return Ok(Append::NotReady);
        }
        let slot = self.slot(file_ns);
        let mut bytes = Vec::new();
        pack_nv12(f.buffer.0.as_ref(), self.size, &mut bytes)?;
        self.inflight.fetch_add(1, Ordering::AcqRel);
        match self.tx.try_send(VideoMsg::Frame {
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
        let _ = self.tx.send(VideoMsg::End { slot: end });
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

enum AudioMsg {
    Pcm {
        bytes: Vec<u8>,
        at: u64,
        frames: u64,
    },
    End,
}

pub struct MfAudioWriter {
    path: PathBuf,
    rate: u32,
    channels: u16,
    tx: SyncSender<AudioMsg>,
    inflight: Arc<AtomicUsize>,
    failed: Arc<Mutex<Option<String>>>,
    thread: Option<JoinHandle<Option<String>>>,
    written: u64,
}

impl MfAudioWriter {
    pub fn start(path: &Path, spec: &AudioWriterSpec) -> Result<MfAudioWriter> {
        let rate = if spec.sample_rate == 44_100 {
            44_100
        } else {
            48_000
        };
        if spec.sample_rate != rate {
            return Err(Error::Other(format!(
                "Media Foundation AAC takes 44.1 or 48 kHz, not {} Hz",
                spec.sample_rate
            )));
        }
        let channels = spec.channels.clamp(1, 2);
        let (tx, rx) = sync_channel::<AudioMsg>(AUDIO_QUEUE);
        let (opened_tx, opened_rx) = sync_channel(1);
        let inflight = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(Mutex::new(None));
        let (i, f, p, bitrate) = (
            inflight.clone(),
            failed.clone(),
            path.to_path_buf(),
            spec.bitrate,
        );
        let thread = std::thread::Builder::new()
            .name("track-audio-mf".into())
            .spawn(move || {
                let sink = match audio_types(rate, channels, bitrate)
                    .and_then(|(out, inp)| Sink::open(&p, &out, &inp, None))
                {
                    Ok(s) => {
                        let _ = opened_tx.send(Ok(()));
                        s
                    }
                    Err(e) => {
                        let _ = opened_tx.send(Err(e));
                        return None;
                    }
                };
                let mut error = None;
                while let Ok(msg) = rx.recv() {
                    let AudioMsg::Pcm { bytes, at, frames } = msg else {
                        break;
                    };
                    i.fetch_sub(1, Ordering::AcqRel);
                    let t = (i128::from(at) * i128::from(HNS_PER_S) / i128::from(rate)) as i64;
                    let d = (i128::from(frames) * i128::from(HNS_PER_S) / i128::from(rate)) as i64;
                    if let Err(e) = sink.write(&bytes, t, d) {
                        let e = e.to_string();
                        if let Ok(mut g) = f.lock() {
                            *g = Some(e.clone());
                        }
                        error = Some(e);
                        break;
                    }
                }
                if let Err(e) = sink.finish() {
                    error.get_or_insert(format!("finish audio track: {e}"));
                }
                error
            })?;
        opened_rx
            .recv()
            .map_err(|_| Error::Other("audio writer thread ended early".into()))??;
        Ok(MfAudioWriter {
            path: path.to_path_buf(),
            rate,
            channels,
            tx,
            inflight,
            failed,
            thread: Some(thread),
            written: 0,
        })
    }

    fn frame_at(&self, file_ns: i64) -> i64 {
        (file_ns as f64 * f64::from(self.rate) / NS).round() as i64
    }

    fn send(&mut self, bytes: Vec<u8>, frames: u64) -> Result<()> {
        if let Some(e) = self.failed.lock().ok().and_then(|f| f.clone()) {
            return Err(Error::Other(e));
        }
        self.inflight.fetch_add(1, Ordering::AcqRel);
        let at = self.written;
        self.tx
            .send(AudioMsg::Pcm { bytes, at, frames })
            .map_err(|_| Error::Other("audio track writer stopped".into()))?;
        self.written += frames;
        Ok(())
    }

    fn write_silence(&mut self, frames: u64) -> Result<()> {
        const CHUNK: u64 = 4_800;
        let mut left = frames;
        while left > 0 {
            let n = left.min(CHUNK);
            self.send(vec![0; n as usize * usize::from(self.channels) * 2], n)?;
            left -= n;
        }
        Ok(())
    }

    fn write_samples(&mut self, samples: &[f32], src_channels: u16) -> Result<()> {
        let src_ch = usize::from(src_channels.max(1));
        let out_ch = usize::from(self.channels);
        let frames = samples.len() / src_ch;
        let mut bytes = Vec::with_capacity(frames * out_ch * 2);
        for f in samples.chunks_exact(src_ch) {
            for c in 0..out_ch {
                let v = if out_ch == 1 && src_ch > 1 {
                    f.iter().sum::<f32>() / src_ch as f32
                } else {
                    f[c.min(src_ch - 1)]
                };
                let s = (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                bytes.extend_from_slice(&s.to_le_bytes());
            }
        }
        self.send(bytes, frames as u64)
    }
}

impl TrackWriter for MfAudioWriter {
    fn append_video(&mut self, _: &VideoFrame, _: i64) -> Result<Append> {
        Err(Error::Other("video appended to an audio track".into()))
    }

    fn append_audio(&mut self, a: &AudioBuffer, file_ns: i64, skip_frames: u32) -> Result<Append> {
        if !self.is_ready() {
            return Ok(Append::NotReady);
        }
        let AudioData::F32Interleaved(samples) = &a.data else {
            return Err(Error::Unsupported(
                "native audio buffers in Media Foundation tracks",
            ));
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
        self.inflight.load(Ordering::Acquire) < AUDIO_QUEUE / 2
    }

    fn finish(mut self: Box<Self>, end_file_ns: Option<i64>) -> Result<FinishedTrack> {
        if let Some(end) = end_file_ns {
            let end = self.frame_at(end);
            let written = self.written as i64;
            if end > written {
                self.write_silence((end - written) as u64)?;
            }
        }
        let _ = self.tx.send(AudioMsg::End);
        let outcome = match self.thread.take() {
            Some(t) => t
                .join()
                .map_err(|_| Error::Other("audio track thread panicked".into()))?,
            None => None,
        };
        if let Some(e) = outcome {
            return Err(Error::Other(e));
        }
        Ok(FinishedTrack {
            path: self.path.clone(),
            duration_ns: Some((self.written as f64 * NS / f64::from(self.rate)) as i64),
            samples: self.written,
        })
    }
}

/// Reads `.mp4` tracks with Media Foundation and everything else (helper-written `.mkv`) with
/// `fallback`, which also takes over when Media Foundation cannot open a file.
pub struct MfDecoder {
    fallback: Arc<dyn Decoder>,
}

impl MfDecoder {
    pub fn new(fallback: Arc<dyn Decoder>) -> MfDecoder {
        MfDecoder { fallback }
    }
}

fn is_mp4(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
}

impl Decoder for MfDecoder {
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoReader>> {
        self.open_video_at(path, 0)
    }

    fn open_video_at(&self, path: &Path, from_file_ns: i64) -> Result<Box<dyn VideoReader>> {
        if is_mp4(path) {
            match MfVideoReader::open(path, from_file_ns) {
                Ok(r) => return Ok(Box::new(r)),
                Err(e) => tracing::warn!(path = %path.display(), "Media Foundation video: {e}"),
            }
        }
        self.fallback.open_video_at(path, from_file_ns)
    }

    fn open_audio(&self, path: &Path, rate: u32, channels: u16) -> Result<Box<dyn AudioReader>> {
        self.open_audio_at(path, rate, channels, 0)
    }

    fn open_audio_at(
        &self,
        path: &Path,
        rate: u32,
        channels: u16,
        from_file_ns: i64,
    ) -> Result<Box<dyn AudioReader>> {
        if is_mp4(path) {
            match MfAudioReader::open(path, rate, channels, from_file_ns) {
                Ok(r) => return Ok(Box::new(r)),
                Err(e) => tracing::warn!(path = %path.display(), "Media Foundation audio: {e}"),
            }
        }
        self.fallback
            .open_audio_at(path, rate, channels, from_file_ns)
    }

    fn probe(&self, path: &Path) -> Result<TrackProbe> {
        if is_mp4(path) {
            match probe_mp4(path) {
                Ok(p) if p.readable => return Ok(p),
                Ok(_) => {}
                Err(e) => tracing::warn!(path = %path.display(), "Media Foundation probe: {e}"),
            }
        }
        self.fallback.probe(path)
    }
}

fn source_reader(path: &Path, video: bool) -> Result<IMFSourceReader> {
    startup();
    unsafe {
        let attrs = attributes(1)?;
        if video {
            attrs
                .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
                .map_err(os)?;
        }
        let url = HSTRING::from(path.as_os_str());
        MFCreateSourceReaderFromURL(&url, &attrs).map_err(os)
    }
}

fn seek(reader: &IMFSourceReader, file_ns: i64) -> Result<()> {
    if file_ns <= 0 {
        return Ok(());
    }
    let pos = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_I8,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    hVal: file_ns / 100,
                },
            }),
        },
    };
    unsafe { reader.SetCurrentPosition(&GUID::zeroed(), &pos) }.map_err(os)
}

struct Read {
    sample: Option<IMFSample>,
    time_hns: i64,
    end: bool,
    changed: bool,
}

fn read(reader: &IMFSourceReader, stream: u32) -> Result<Read> {
    let mut flags = 0u32;
    let mut time = 0i64;
    let mut sample: Option<IMFSample> = None;
    unsafe {
        reader
            .ReadSample(
                stream,
                0,
                None,
                Some(&mut flags),
                Some(&mut time),
                Some(&mut sample),
            )
            .map_err(os)?;
    }
    if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
        return Err(Error::Other("Media Foundation read error".into()));
    }
    Ok(Read {
        end: flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0,
        changed: flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0,
        sample,
        time_hns: time,
    })
}

fn locked<T>(sample: &IMFSample, f: impl FnOnce(&[u8]) -> T) -> Result<T> {
    unsafe {
        let buf = sample.ConvertToContiguousBuffer().map_err(os)?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len = 0u32;
        buf.Lock(&mut ptr, None, Some(&mut len)).map_err(os)?;
        let out = f(std::slice::from_raw_parts(ptr, len as usize));
        let _ = buf.Unlock();
        Ok(out)
    }
}

/// File times count from the first frame: the encoder's reorder delay shifts every stored
/// time by a frame or so, which would put video behind audio.
struct MfVideoReader {
    reader: IMFSourceReader,
    native: SizePx,
    size: SizePx,
    stride: Option<usize>,
    stream: u32,
    done: bool,
    origin_hns: i64,
    pending: Option<DecodedFrame>,
}

unsafe impl Send for MfVideoReader {}

impl MfVideoReader {
    fn open(path: &Path, from_file_ns: i64) -> Result<MfVideoReader> {
        let reader = source_reader(path, true)?;
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        unsafe {
            let native = reader.GetNativeMediaType(stream, 0).map_err(os)?;
            let (nw, nh) = visible_size(&native)?;
            let want = media_type(&MFMediaType_Video, &MFVideoFormat_NV12)?;
            want.SetUINT64(&MF_MT_FRAME_SIZE, pack(nw, nh))
                .map_err(os)?;
            reader
                .SetCurrentMediaType(stream, None, &want)
                .map_err(os)?;
            let cur = reader.GetCurrentMediaType(stream).map_err(os)?;
            let (w, h) = padding_free(visible_size(&cur)?, SizePx { w: nw, h: nh });
            let stride = cur
                .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                .ok()
                .map(|s| (s as i32).unsigned_abs() as usize)
                .filter(|s| *s >= w as usize);
            let mut r = MfVideoReader {
                reader,
                native: SizePx { w: nw, h: nh },
                size: SizePx { w, h },
                stride,
                stream,
                done: false,
                origin_hns: 0,
                pending: None,
            };
            let first = r.decode()?;
            if let Some((t, f)) = first {
                r.origin_hns = t;
                r.pending = Some(DecodedFrame {
                    file_ns: 0,
                    frame: f,
                });
                if from_file_ns > 0 {
                    r.seek_to(from_file_ns)?;
                }
            }
            Ok(r)
        }
    }
}

/// The picture's visible size: decoders report frames padded to whole 16-pixel blocks
/// (640×368 for 640×360) and put the real size in the display aperture.
fn visible_size(t: &IMFMediaType) -> Result<(u32, u32)> {
    let (w, h) = unsafe { unpack(t.GetUINT64(&MF_MT_FRAME_SIZE).map_err(os)?) };
    let mut area = [0u8; 16];
    let mut len = 0u32;
    if unsafe { t.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, Some(&mut len)) }.is_ok()
        && len == 16
    {
        let cx = i32::from_le_bytes([area[8], area[9], area[10], area[11]]);
        let cy = i32::from_le_bytes([area[12], area[13], area[14], area[15]]);
        if cx > 0 && cy > 0 && cx as u32 <= w && cy as u32 <= h {
            return Ok((cx as u32, cy as u32));
        }
    }
    Ok((w, h))
}

/// The file's own size when `got` only adds block padding to it.
fn padding_free(got: (u32, u32), native: SizePx) -> (u32, u32) {
    let pad = |g: u32, n: u32| g >= n && g < n + 16;
    if pad(got.0, native.w) && pad(got.1, native.h) {
        (native.w, native.h)
    } else {
        got
    }
}

/// The decoder's own row pitch when its buffer gives one, else the media type's stride.
fn nv12_of(sample: &IMFSample, size: SizePx, stride: Option<usize>) -> Result<CpuNv12Frame> {
    unsafe {
        let buf = sample.GetBufferByIndex(0).map_err(os)?;
        if let Ok(b2) = buf.cast::<IMF2DBuffer2>() {
            let mut scan0: *mut u8 = std::ptr::null_mut();
            let mut pitch = 0i32;
            let mut start: *mut u8 = std::ptr::null_mut();
            let mut len = 0u32;
            if b2
                .Lock2DSize(
                    MF2DBuffer_LockFlags_Read,
                    &mut scan0,
                    &mut pitch,
                    &mut start,
                    &mut len,
                )
                .is_ok()
            {
                let skip = scan0.offset_from(start).max(0) as usize;
                let frame = (pitch > 0 && skip < len as usize).then(|| {
                    let data = std::slice::from_raw_parts(scan0, len as usize - skip);
                    nv12_from(data, size, Some(pitch as usize))
                });
                let _ = b2.Unlock2D();
                if let Some(f) = frame {
                    return Ok(f);
                }
            }
        }
    }
    locked(sample, |d| nv12_from(d, size, stride))
}

/// Copies an NV12 picture out of a decoder buffer whose rows may be padded and whose planes
/// may be separated by extra rows.
fn nv12_from(data: &[u8], size: SizePx, stride: Option<usize>) -> CpuNv12Frame {
    let (w, h) = (size.w as usize, size.h as usize);
    let ch = h.div_ceil(2);
    let stride = stride.unwrap_or(data.len() / (h + ch)).max(w);
    let rows = data.len() / stride;
    let y_rows = if rows > h + ch { rows * 2 / 3 } else { h };
    CpuNv12Frame::from_padded(data, size, stride, y_rows)
}

impl MfVideoReader {
    fn decode(&mut self) -> Result<Option<(i64, FrameBuffer)>> {
        while !self.done {
            let r = read(&self.reader, self.stream)?;
            self.done = r.end;
            if r.changed {
                self.refresh_format()?;
            }
            if let Some(s) = r.sample {
                let frame = nv12_of(&s, self.size, self.stride)?;
                return Ok(Some((r.time_hns, FrameBuffer(Arc::new(frame)))));
            }
        }
        Ok(None)
    }

    fn refresh_format(&mut self) -> Result<()> {
        unsafe {
            let cur = self.reader.GetCurrentMediaType(self.stream).map_err(os)?;
            let (w, h) = padding_free(visible_size(&cur)?, self.native);
            self.size = SizePx { w, h };
            self.stride = cur
                .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                .ok()
                .map(|s| (s as i32).unsigned_abs() as usize)
                .filter(|s| *s >= w as usize);
        }
        Ok(())
    }

    /// Source readers do not promise exact seeks: when the first frame lands after `file_ns`,
    /// seek two seconds earlier once more, so callers get a frame at or before it.
    fn seek_to(&mut self, file_ns: i64) -> Result<()> {
        for back in [0, 2_000_000_000] {
            let target = (file_ns - back).max(0);
            seek(&self.reader, target + self.origin_hns * 100)?;
            self.done = false;
            if let Some((t, f)) = self.decode()? {
                let at = (t - self.origin_hns) * 100;
                self.pending = Some(DecodedFrame {
                    file_ns: at,
                    frame: f,
                });
                if at <= file_ns + 1_000_000 {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

impl VideoReader for MfVideoReader {
    fn size(&self) -> SizePx {
        self.size
    }

    fn next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        if let Some(p) = self.pending.take() {
            return Ok(Some(p));
        }
        Ok(self.decode()?.map(|(t, frame)| DecodedFrame {
            file_ns: (t - self.origin_hns) * 100,
            frame,
        }))
    }
}

struct MfAudioReader {
    reader: IMFSourceReader,
    stream: u32,
    channels: usize,
    first_ns: i64,
    pending: Vec<f32>,
    at: usize,
    done: bool,
}

unsafe impl Send for MfAudioReader {}

impl MfAudioReader {
    fn open(path: &Path, rate: u32, channels: u16, from_file_ns: i64) -> Result<MfAudioReader> {
        let reader = source_reader(path, false)?;
        let stream = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        unsafe {
            let want = media_type(&MFMediaType_Audio, &MFAudioFormat_Float)?;
            want.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, rate)
                .map_err(os)?;
            want.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(channels))
                .map_err(os)?;
            want.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 32)
                .map_err(os)?;
            reader
                .SetCurrentMediaType(stream, None, &want)
                .map_err(os)?;
            let cur = reader.GetCurrentMediaType(stream).map_err(os)?;
            let got_rate = cur.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).map_err(os)?;
            let got_ch = cur.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).map_err(os)?;
            if got_rate != rate || got_ch != u32::from(channels) {
                return Err(Error::Other(format!(
                    "Media Foundation gave {got_rate} Hz × {got_ch}, not {rate} Hz × {channels}"
                )));
            }
        }
        seek(&reader, from_file_ns)?;
        let mut r = MfAudioReader {
            reader,
            stream,
            channels: usize::from(channels.max(1)),
            first_ns: 0,
            pending: Vec::new(),
            at: 0,
            done: false,
        };
        r.first_ns = r.fill()?.unwrap_or(0);
        Ok(r)
    }

    fn fill(&mut self) -> Result<Option<i64>> {
        while !self.done {
            let r = read(&self.reader, self.stream)?;
            self.done = r.end;
            if let Some(s) = r.sample {
                self.pending = locked(&s, |d| {
                    d.chunks_exact(4)
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect()
                })?;
                self.at = 0;
                return Ok(Some(r.time_hns * 100));
            }
        }
        Ok(None)
    }
}

impl AudioReader for MfAudioReader {
    fn first_sample_file_ns(&self) -> i64 {
        self.first_ns
    }

    fn read(&mut self, out: &mut [f32]) -> Result<usize> {
        let mut n = 0;
        while n < out.len() {
            if self.at >= self.pending.len() && self.fill()?.is_none() {
                break;
            }
            let take = (out.len() - n).min(self.pending.len() - self.at);
            out[n..n + take].copy_from_slice(&self.pending[self.at..self.at + take]);
            n += take;
            self.at += take;
        }
        Ok(n - n % self.channels)
    }
}

fn probe_mp4(path: &Path) -> Result<TrackProbe> {
    let reader = source_reader(path, false)?;
    let video = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let audio = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
    let size = unsafe { reader.GetNativeMediaType(video, 0) }
        .ok()
        .and_then(|t| unsafe { t.GetUINT64(&MF_MT_FRAME_SIZE) }.ok())
        .map(|v| {
            let (w, h) = unpack(v);
            SizePx { w, h }
        });
    let stream = if size.is_some() { video } else { audio };
    unsafe {
        let all = MF_SOURCE_READER_ALL_STREAMS.0 as u32;
        let _ = reader.SetStreamSelection(all, false);
        reader.SetStreamSelection(stream, true).map_err(os)?;
    }
    let mut samples = 0u64;
    let mut end_hns = None::<i64>;
    let mut origin_hns = None::<i64>;
    loop {
        let mut flags = 0u32;
        let mut time = 0i64;
        let mut sample: Option<IMFSample> = None;
        let r = unsafe {
            reader.ReadSample(
                stream,
                0,
                None,
                Some(&mut flags),
                Some(&mut time),
                Some(&mut sample),
            )
        };
        if r.is_err() || flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
            break;
        }
        if let Some(s) = sample {
            samples += 1;
            let d = unsafe { s.GetSampleDuration() }.unwrap_or(0);
            end_hns = Some(end_hns.unwrap_or(0).max(time + d));
            origin_hns = Some(origin_hns.map_or(time, |o| o.min(time)));
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
    }
    let origin = if size.is_some() {
        origin_hns.unwrap_or(0)
    } else {
        0
    };
    let readable = samples > 0 && end_hns.is_some_and(|e| e > origin);
    Ok(TrackProbe {
        readable,
        duration_ns: end_hns.filter(|_| readable).map(|e| (e - origin) * 100),
        samples: if size.is_some() { samples } else { 0 },
        size,
        error: (!readable).then(|| "no samples".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_copy_skips_row_padding_and_plane_gaps() {
        let size = SizePx { w: 4, h: 2 };
        let stride = 8;
        let y_rows = 4;
        let mut data = vec![0u8; stride * (y_rows + 2)];
        for y in 0..2 {
            for x in 0..4 {
                data[y * stride + x] = (10 * y + x) as u8;
            }
        }
        for x in 0..4 {
            data[y_rows * stride + x] = 200 + x as u8;
        }
        let f = nv12_from(&data, size, Some(stride));
        assert_eq!(f.y, vec![0, 1, 2, 3, 10, 11, 12, 13]);
        assert_eq!(f.uv, vec![200, 201, 202, 203]);
        let tight = nv12_from(&data[..stride * 3], size, None);
        assert_eq!(tight.y, f.y);
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sc-mf-{name}-{}", sc_core::ids::new_id()));
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

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

    fn spec(size: SizePx) -> VideoWriterSpec {
        VideoWriterSpec {
            size,
            fps: 30,
            codec: VideoCodec::Hevc,
            keyframe_interval_s: 1.0,
            fragment_interval_s: 1.0,
            bits_per_pixel: 0.1,
        }
    }

    fn mean_luma(f: &DecodedFrame) -> f64 {
        let mut sum = 0u64;
        let mut n = 0u64;
        f.frame
            .0
            .with_nv12(&mut |v| {
                sum = v.y.iter().map(|&b| u64::from(b)).sum();
                n = v.y.len() as u64;
            })
            .expect("nv12");
        sum as f64 / n.max(1) as f64
    }

    #[test]
    fn video_round_trips_with_constant_rate_and_seeks_to_keyframes() {
        let dir = temp("video");
        let path = dir.join("screen.mp4");
        let size = SizePx { w: 640, h: 360 };
        let Ok(ffmpeg) = sc_helpers::locate(sc_helpers::HelperKind::Ffmpeg) else {
            eprintln!("SKIPPED: no ffmpeg helper");
            return;
        };
        let factory = MfWriters {
            helper: HelperWriters::new(ffmpeg),
        };
        let mut w = factory.video(&path, &spec(size)).expect("writer");
        for i in 0..60i64 {
            let f = frame(size, if i < 30 { 60 } else { 180 });
            while !w.is_ready() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            if i == 40 || i == 41 {
                continue;
            }
            assert_eq!(
                w.append_video(&f, i * 1_000_000_000 / 30).expect("append"),
                Append::Written
            );
        }
        let done = w.finish(Some(2_000_000_000)).expect("finish");
        assert_eq!(done.duration_ns, Some(2_000_000_000));

        let d = MfDecoder::new(Arc::new(sc_platform::unsupported::Nothing::new("helper")));
        let probe = d.probe(&path).expect("probe");
        println!("mf probe: {probe:?}");
        assert!(probe.readable);
        assert_eq!(probe.size, Some(size));
        assert_eq!(probe.samples, 60, "skipped slots are filled");
        let dur = probe.duration_ns.unwrap_or(0);
        assert!((1_960_000_000..=2_040_000_000).contains(&dur), "{dur}");

        let mut r = d.open_video(&path).expect("reader");
        assert_eq!(r.size(), size);
        let mut frames = Vec::new();
        while let Some(f) = r.next_frame().expect("frame") {
            frames.push(f);
        }
        assert_eq!(frames.len(), 60);
        assert!(
            (mean_luma(&frames[5]) - 60.0).abs() < 6.0,
            "{}",
            mean_luma(&frames[5])
        );
        assert!(
            (mean_luma(&frames[50]) - 180.0).abs() < 6.0,
            "{}",
            mean_luma(&frames[50])
        );
        assert!(
            (frames[45].file_ns - 1_500_000_000).abs() < 1_000_000,
            "{}",
            frames[45].file_ns
        );

        let mut s = d.open_video_at(&path, 1_500_000_000).expect("seek");
        let first = s.next_frame().expect("frame").expect("some");
        println!(
            "mf seek to 1.5 s starts at {} ms",
            first.file_ns / 1_000_000
        );
        assert!(
            (900_000_000..=1_500_000_000).contains(&first.file_ns),
            "a keyframe at or before 1.5 s, at most a second back: {}",
            first.file_ns
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn odd_widths_read_back_without_shearing() {
        let dir = temp("odd");
        let path = dir.join("window.mp4");
        let size = SizePx { w: 1366, h: 362 };
        let mut halves = CpuNv12Frame::filled(size, 50, 128, 128, false);
        for row in halves.y.chunks_exact_mut(size.w as usize) {
            row[size.w as usize / 2..].fill(200);
        }
        let buffer = FrameBuffer(Arc::new(halves));
        let mut w = MfVideoWriter::start(&path, &spec(size)).expect("writer");
        for i in 0..10i64 {
            while !w.is_ready() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let f = VideoFrame {
                buffer: buffer.clone(),
                ..frame(size, 0)
            };
            w.append_video(&f, i * 1_000_000_000 / 30).expect("append");
        }
        Box::new(w).finish(None).expect("finish");
        let d = MfDecoder::new(Arc::new(sc_platform::unsupported::Nothing::new("helper")));
        let mut r = d.open_video(&path).expect("reader");
        assert_eq!(r.size(), size);
        let f = r.next_frame().expect("frame").expect("some");
        f.frame
            .0
            .with_nv12(&mut |v| {
                for y in [10usize, 180, 350] {
                    let row = &v.y[y * v.y_stride..];
                    let (l, rt) = (row[300], row[1000]);
                    assert!(
                        (i32::from(l) - 50).abs() < 12 && (i32::from(rt) - 200).abs() < 12,
                        "row {y}: {l} and {rt}"
                    );
                }
            })
            .expect("nv12");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn audio_round_trips_and_fills_gaps_with_silence() {
        let dir = temp("audio");
        let path = dir.join("mic.mp4");
        let spec = AudioWriterSpec {
            sample_rate: 48_000,
            channels: 2,
            bitrate: 128_000,
            fragment_interval_s: 1.0,
        };
        let mut w = MfAudioWriter::start(&path, &spec).expect("writer");
        let tone = |start: usize, n: usize| -> Vec<f32> {
            (start..start + n)
                .flat_map(|i| {
                    let v = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
                    [v, v]
                })
                .collect()
        };
        let buf = |samples: Vec<f32>| AudioBuffer {
            pts: sc_platform::HostTime(0),
            sample_rate: 48_000,
            channels: 2,
            frames: (samples.len() / 2) as u32,
            data: AudioData::F32Interleaved(samples.into()),
        };
        w.append_audio(&buf(tone(0, 24_000)), 0, 0)
            .expect("first half second");
        w.append_audio(&buf(tone(48_000, 24_000)), 1_000_000_000, 0)
            .expect("after a gap");
        let done = Box::new(w).finish(Some(2_000_000_000)).expect("finish");
        assert_eq!(done.samples, 96_000);

        let d = MfDecoder::new(Arc::new(sc_platform::unsupported::Nothing::new("helper")));
        let mut r = d.open_audio(&path, 48_000, 2).expect("reader");
        let mut all = Vec::new();
        let mut chunk = vec![0f32; 9_600];
        loop {
            let n = r.read(&mut chunk).expect("read");
            if n == 0 {
                break;
            }
            all.extend_from_slice(&chunk[..n]);
        }
        let frames = all.len() / 2;
        println!(
            "mf audio: {frames} frames, first at {} ns",
            r.first_sample_file_ns()
        );
        assert!((94_000..=100_000).contains(&frames), "{frames}");
        let rms = |a: usize, b: usize| {
            let s = &all[a * 2..b * 2];
            (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
        };
        assert!(rms(6_000, 18_000) > 0.25, "tone: {}", rms(6_000, 18_000));
        assert!(rms(30_000, 42_000) < 0.02, "gap: {}", rms(30_000, 42_000));
        assert!(
            rms(54_000, 66_000) > 0.25,
            "tone again: {}",
            rms(54_000, 66_000)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_track_left_unfinished_reads_up_to_its_last_fragment() {
        let dir = temp("crash");
        let path = dir.join("screen.mp4");
        let size = SizePx { w: 320, h: 180 };
        let mut w = MfVideoWriter::start(&path, &spec(size)).expect("writer");
        for i in 0..75i64 {
            while !w.is_ready() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            w.append_video(&frame(size, 100), i * 1_000_000_000 / 30)
                .expect("append");
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        std::mem::forget(w);
        let copy = dir.join("copy.mp4");
        std::fs::copy(&path, &copy).expect("copy");
        let p = MfDecoder::new(Arc::new(sc_platform::unsupported::Nothing::new("helper")))
            .probe(&copy)
            .expect("probe");
        assert!(
            p.readable && p.samples >= 30,
            "an unfinished track keeps its completed fragments: {len} bytes, {p:?}"
        );
    }
}
