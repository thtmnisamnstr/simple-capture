use std::path::PathBuf;
use std::sync::Arc;

use objc2_core_audio_types::AudioStreamBasicDescription;
use sc_core::time::HostTime;
use sc_geometry::SizePx;
use sc_platform::synthetic::{FrameCode, coded_frame, read_code};
use sc_platform::{
    Append, AudioBuffer, AudioData, AudioWriterSpec, Decoder, FrameBuffer, NativeFrame, StreamTag,
    TrackWriter, VideoCodec, VideoFrame, VideoWriterSpec,
};

use super::frames::{LPCM, MacAudioSample, MacPixelFrame};
use super::pcm::{
    FLAG_FLOAT, FLAG_NON_INTERLEAVED, FLAG_PACKED, FLAG_SIGNED_INT, PcmFormat, f32_bytes,
    make_sample,
};
use super::writer::{AudioWriter, VideoWriter, write_nv12};
use super::{MacDecoder, devices};
use crate::util::ns_to_cmtime;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sc-av-unit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

fn hw_ok<T>(r: sc_platform::Result<T>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) if std::env::var("SC_ALLOW_NO_HW_ENCODER").ok().as_deref() == Some("1") => {
            println!("SKIPPED: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

fn decoded_frames(path: &std::path::Path) -> usize {
    let mut r = MacDecoder::new()
        .open_audio(path, 48_000, 2)
        .expect("open_audio");
    let mut buf = vec![0f32; 8192];
    let mut n = 0;
    loop {
        let k = r.read(&mut buf).expect("read");
        if k == 0 {
            return n / 2;
        }
        n += k;
    }
}

fn native(fmt: &PcmFormat, bufs: Vec<Vec<u8>>, frames: u32) -> AudioBuffer {
    let sb = make_sample(fmt, bufs, frames, ns_to_cmtime(0)).expect("make_sample");
    AudioBuffer {
        pts: HostTime(0),
        sample_rate: fmt.rate() as u32,
        channels: fmt.asbd.mChannelsPerFrame as u16,
        frames,
        data: AudioData::Native(Arc::new(MacAudioSample::new(sb))),
    }
}

#[test]
fn native_non_interleaved_and_int16_audio_with_skip() {
    let dir = tmp("native");
    let spec = AudioWriterSpec {
        sample_rate: 48_000,
        channels: 2,
        bitrate: 160_000,
        fragment_interval_s: 1.0,
    };
    let planar = PcmFormat::from_asbd(AudioStreamBasicDescription {
        mSampleRate: 48_000.0,
        mFormatID: LPCM,
        mFormatFlags: FLAG_FLOAT | FLAG_PACKED | FLAG_NON_INTERLEAVED,
        mBytesPerPacket: 4,
        mFramesPerPacket: 1,
        mBytesPerFrame: 4,
        mChannelsPerFrame: 2,
        mBitsPerChannel: 32,
        mReserved: 0,
    })
    .expect("planar format");
    let path = dir.join("system.mov");
    let mut w = AudioWriter::create(&path, &spec).expect("writer");
    let frames = 960u32;
    let mut kept = 0u64;
    for k in 0..50u32 {
        let skip = if k == 10 { 240 } else { 0 };
        let plane: Vec<f32> = (0..frames)
            .map(|i| ((i as f32) * 0.05).sin() * 0.3)
            .collect();
        let a = native(&planar, vec![f32_bytes(&plane), f32_bytes(&plane)], frames);
        let at = (kept as f64 * 1e9 / 48_000.0).round() as i64
            - (f64::from(skip) * 1e9 / 48_000.0).round() as i64;
        assert_eq!(
            w.append_audio(&a, at, skip).expect("append native"),
            Append::Written
        );
        kept += u64::from(frames - skip);
    }
    let fin = Box::new(w).finish(None).expect("finish");
    assert_eq!(fin.samples, kept);
    let decoded = decoded_frames(&path) as u64;
    println!("planar f32 native: kept {kept}, decoded {decoded}");
    assert!(
        decoded.abs_diff(kept) <= 2,
        "decoded {decoded} vs kept {kept}"
    );

    let int16 = PcmFormat::from_asbd(AudioStreamBasicDescription {
        mSampleRate: 48_000.0,
        mFormatID: LPCM,
        mFormatFlags: FLAG_SIGNED_INT | FLAG_PACKED,
        mBytesPerPacket: 2,
        mFramesPerPacket: 1,
        mBytesPerFrame: 2,
        mChannelsPerFrame: 1,
        mBitsPerChannel: 16,
        mReserved: 0,
    })
    .expect("int16 format");
    let path = dir.join("mic.mov");
    let mut w = AudioWriter::create(&path, &spec).expect("writer");
    for k in 0..25i64 {
        let bytes: Vec<u8> = (0..1920i32)
            .flat_map(|i| (((i % 64) - 32) as i16 * 300).to_le_bytes())
            .collect();
        let a = native(&int16, vec![bytes], 1920);
        assert_eq!(
            w.append_audio(&a, k * 40_000_000, 0).expect("append int16"),
            Append::Written
        );
    }
    assert_eq!(
        w.append_silence(1_000_000_000, 4800).expect("silence"),
        Append::Written
    );
    let fin = Box::new(w).finish(None).expect("finish");
    assert_eq!(fin.samples, 25 * 1920 + 4800);
    assert_eq!(fin.duration_ns, Some(1_100_000_000));
    let decoded = decoded_frames(&path) as u64;
    println!(
        "int16 mono native + silence: decoded {decoded} of {}",
        fin.samples
    );
    assert!(decoded.abs_diff(fin.samples) <= 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pixel_buffer_frames_append_without_copy() {
    let dir = tmp("pixel");
    let size = SizePx { w: 480, h: 272 };
    let spec = VideoWriterSpec {
        size,
        fps: 30,
        codec: VideoCodec::H264,
        keyframe_interval_s: 1.0,
        fragment_interval_s: 1.0,
        bits_per_pixel: super::DEFAULT_BITS_PER_PIXEL,
    };
    let path = dir.join("camera.mov");
    let Some(mut w) = hw_ok(VideoWriter::create(&path, &spec)) else {
        return;
    };
    for i in 0..20u32 {
        let code = FrameCode {
            source_hash: 7,
            index: i,
            pts_ms: i,
        };
        let src = coded_frame(size, code, true);
        let pb = w.new_pixel_buffer().expect("pool buffer");
        let mut r = Ok(());
        src.with_nv12(&mut |v| r = write_nv12(&pb, &v))
            .expect("view");
        r.expect("write_nv12");
        let frame = MacPixelFrame::from_pixel_buffer(pb);
        let mut back = None;
        frame
            .with_nv12(&mut |v| back = read_code(&v))
            .expect("read back");
        assert_eq!(
            back,
            Some(code),
            "420v copy of a full-range frame keeps the code"
        );
        let f = VideoFrame {
            stream: StreamTag(1),
            pts: HostTime(0),
            size,
            content_rect_pt: None,
            content_scale: None,
            buffer: FrameBuffer(Arc::new(frame)),
        };
        let ns = i64::from(i) * 33_333_333;
        while w.append_video(&f, ns).expect("append") == Append::NotReady {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    let fin = Box::new(w).finish(Some(700_000_000)).expect("finish");
    assert_eq!(fin.samples, 20);
    let mut r = MacDecoder::new().open_video(&path).expect("open");
    let mut n = 0;
    while let Some(d) = r.next_frame().expect("decode") {
        let mut c = None;
        d.frame
            .0
            .with_nv12(&mut |v| c = read_code(&v))
            .expect("nv12");
        assert_eq!(c.map(|c| c.index), Some(n));
        n += 1;
    }
    assert_eq!(n, 20);
    let p = MacDecoder::new().probe(&path).expect("probe");
    assert_eq!(p.duration_ns, Some(700_000_000), "{p:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn finish_without_samples_and_wrong_kind() {
    let dir = tmp("empty");
    let spec = AudioWriterSpec {
        sample_rate: 48_000,
        channels: 1,
        bitrate: 96_000,
        fragment_interval_s: 1.0,
    };
    let w = AudioWriter::create(&dir.join("a.mov"), &spec).expect("writer");
    let fin = Box::new(w).finish(Some(5)).expect("finish empty");
    assert_eq!((fin.samples, fin.duration_ns), (0, Some(5)));
    let mut w = AudioWriter::create(&dir.join("b.mov"), &spec).expect("writer");
    let f = VideoFrame {
        stream: StreamTag(1),
        pts: HostTime(0),
        size: SizePx { w: 16, h: 16 },
        content_rect_pt: None,
        content_scale: None,
        buffer: FrameBuffer(Arc::new(coded_frame(
            SizePx { w: 16, h: 16 },
            FrameCode {
                source_hash: 1,
                index: 0,
                pts_ms: 0,
            },
            false,
        ))),
    };
    assert!(w.append_video(&f, 0).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn camera_and_mic_start_hit_the_tripwire_in_plain_tests() {
    if std::env::var("SC_LIVE").is_ok() || std::env::var("SC_DEV_HOST").is_ok() {
        println!("SKIPPED: SC_LIVE or SC_DEV_HOST is set");
        return;
    }
    let sink: Arc<dyn sc_platform::FrameSink> = Arc::new(NullSink);
    for kind in [
        sc_platform::DeviceKind::Camera,
        sc_platform::DeviceKind::Mic,
    ] {
        let e = devices::start(kind, "any", None, sink.clone())
            .err()
            .expect("tripwire must refuse");
        assert!(e.to_string().contains("TCC tripwire"), "{e}");
    }
}

struct NullSink;
impl sc_platform::FrameSink for NullSink {
    fn on_screen(&self, _f: VideoFrame) {}
    fn on_camera(&self, _f: VideoFrame) {}
    fn on_audio(&self, _t: sc_platform::AudioTrackKind, _a: AudioBuffer) {}
    fn on_event(&self, _e: sc_platform::SourceEvent) {}
}

/// Frames and audio from wp-mac-sck carry a bare `CFRetained<CMSampleBuffer>`; the writers take
/// them without a copy (video) and without re-wrapping (audio).
#[test]
fn sck_sample_buffers_append() {
    use std::ptr::NonNull;

    use objc2_core_media::{
        CMSampleBuffer, CMSampleTimingInfo, CMVideoFormatDescription,
        CMVideoFormatDescriptionCreateForImageBuffer,
    };

    let dir = tmp("sck");
    let size = SizePx { w: 320, h: 240 };
    let spec = VideoWriterSpec {
        size,
        fps: 30,
        codec: VideoCodec::H264,
        keyframe_interval_s: 1.0,
        fragment_interval_s: 1.0,
        bits_per_pixel: super::DEFAULT_BITS_PER_PIXEL,
    };
    let path = dir.join("screen.mov");
    let Some(mut w) = hw_ok(VideoWriter::create(&path, &spec)) else {
        return;
    };
    for i in 0..10u32 {
        let code = FrameCode {
            source_hash: 3,
            index: i,
            pts_ms: i,
        };
        let pb = w.new_pixel_buffer().expect("pool buffer");
        let mut r = Ok(());
        coded_frame(size, code, false)
            .with_nv12(&mut |v| r = write_nv12(&pb, &v))
            .expect("view");
        r.expect("write_nv12");
        let mut desc: *const CMVideoFormatDescription = std::ptr::null();
        // SAFETY: valid pixel buffer and out pointer.
        let rc = unsafe {
            CMVideoFormatDescriptionCreateForImageBuffer(None, &pb, NonNull::from(&mut desc))
        };
        assert_eq!(rc, 0, "format description");
        // SAFETY: created above with +1.
        let desc = unsafe {
            objc2_core_foundation::CFRetained::from_raw(NonNull::new(desc.cast_mut()).unwrap())
        };
        let mut timing = CMSampleTimingInfo {
            duration: ns_to_cmtime(33_333_333),
            presentationTimeStamp: ns_to_cmtime(i64::from(i) * 33_333_333),
            decodeTimeStamp: ns_to_cmtime(i64::from(i) * 33_333_333),
        };
        let mut sb: *mut CMSampleBuffer = std::ptr::null_mut();
        // SAFETY: valid buffers and out pointers.
        let rc = unsafe {
            CMSampleBuffer::create_ready_with_image_buffer(
                None,
                &pb,
                &desc,
                NonNull::from(&mut timing),
                NonNull::from(&mut sb),
            )
        };
        assert_eq!(rc, 0, "sample buffer");
        // SAFETY: created above with +1.
        let sb = unsafe { objc2_core_foundation::CFRetained::from_raw(NonNull::new(sb).unwrap()) };
        let f = VideoFrame {
            stream: StreamTag(1),
            pts: HostTime(0),
            size,
            content_rect_pt: None,
            content_scale: None,
            buffer: FrameBuffer(Arc::new(crate::sck::ScFrame::retain(&sb, size))),
        };
        assert!(crate::sck::sample_buffer_of(&*f.buffer.0).is_some());
        while w
            .append_video(&f, i64::from(i) * 33_333_333)
            .expect("append")
            == Append::NotReady
        {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    let fin = Box::new(w).finish(None).expect("finish video");
    assert_eq!(fin.samples, 10);
    let mut r = MacDecoder::new().open_video(&path).expect("open");
    let mut n = 0;
    while let Some(d) = r.next_frame().expect("decode") {
        let mut c = None;
        d.frame
            .0
            .with_nv12(&mut |v| c = read_code(&v))
            .expect("nv12");
        assert_eq!(c.map(|c| c.index), Some(n));
        n += 1;
    }
    assert_eq!(n, 10);

    let planar = PcmFormat::from_asbd(AudioStreamBasicDescription {
        mSampleRate: 48_000.0,
        mFormatID: LPCM,
        mFormatFlags: FLAG_FLOAT | FLAG_PACKED | FLAG_NON_INTERLEAVED,
        mBytesPerPacket: 4,
        mFramesPerPacket: 1,
        mBytesPerFrame: 4,
        mChannelsPerFrame: 2,
        mBitsPerChannel: 32,
        mReserved: 0,
    })
    .expect("planar format");
    let aspec = AudioWriterSpec {
        sample_rate: 48_000,
        channels: 2,
        bitrate: 160_000,
        fragment_interval_s: 1.0,
    };
    let apath = dir.join("system.mov");
    let mut w = AudioWriter::create(&apath, &aspec).expect("audio writer");
    for k in 0..20i64 {
        let plane = vec![0.1f32; 960];
        let sb = make_sample(
            &planar,
            vec![f32_bytes(&plane), f32_bytes(&plane)],
            960,
            ns_to_cmtime(0),
        )
        .expect("make_sample");
        let a = AudioBuffer {
            pts: HostTime(0),
            sample_rate: 48_000,
            channels: 2,
            frames: 960,
            data: AudioData::Native(Arc::new(crate::sck::ScAudio::retain(&sb))),
        };
        assert_eq!(
            w.append_audio(&a, k * 20_000_000, 0)
                .expect("append sck audio"),
            Append::Written
        );
    }
    let fin = Box::new(w).finish(None).expect("finish audio");
    assert_eq!(fin.samples, 20 * 960);
    let _ = std::fs::remove_dir_all(dir);
}
