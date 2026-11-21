//! Helper tracks written and read back with the real helper: constant-rate slots from irregular
//! frames, seeking, audio gaps, and a file cut short.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_geometry::SizePx;
use sc_platform::synthetic::{FrameCode, coded_frame, read_code};
use sc_platform::{
    AudioBuffer, AudioData, AudioWriterSpec, Decoder, FrameBuffer, HostTime, StreamTag, VideoCodec,
    VideoFrame, VideoWriterSpec, WriterFactory,
};
use sc_tracks::{HelperDecoder, HelperWriters};

const MS: i64 = 1_000_000;
const SIZE: SizePx = SizePx { w: 1280, h: 720 };

/// The bundled helper, else an `ffmpeg` on PATH (CI's Linux runner).
fn ffmpeg() -> Option<PathBuf> {
    let on_path = || {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|d| d.join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX)))
                .find(|p| p.is_file())
        })
    };
    match sc_helpers::locate(sc_helpers::HelperKind::Ffmpeg) {
        Ok(p) => Some(p),
        Err(_) if on_path().is_some() => on_path(),
        Err(e) => {
            assert!(
                std::env::var("SC_REQUIRE_HELPERS").as_deref() != Ok("1"),
                "{e}"
            );
            None
        }
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sc-tracks-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("tmp");
    d
}

fn frame(index: u32) -> VideoFrame {
    VideoFrame {
        stream: StreamTag(0),
        pts: HostTime(0),
        size: SIZE,
        content_rect_pt: None,
        content_scale: None,
        buffer: FrameBuffer(Arc::new(coded_frame(
            SIZE,
            FrameCode {
                source_hash: 7,
                index,
                pts_ms: 0,
            },
            false,
        ))),
    }
}

fn code(f: &sc_platform::DecodedFrame) -> Option<u32> {
    let mut out = None;
    f.frame
        .0
        .with_nv12(&mut |v| out = read_code(&v).map(|c| c.index))
        .expect("nv12");
    out
}

const TIMES_MS: [i64; 8] = [0, 33, 100, 400, 433, 1_000, 1_500, 1_966];

fn write_video(dir: &Path, w: &HelperWriters) -> PathBuf {
    let path = dir.join("screen.mkv");
    let mut t = w
        .video(
            &path,
            &VideoWriterSpec {
                size: SIZE,
                fps: 30,
                codec: VideoCodec::Hevc,
                keyframe_interval_s: 1.0,
                fragment_interval_s: 1.0,
                bits_per_pixel: 0.1,
            },
        )
        .expect("writer");
    assert!(path.exists(), "the file exists once the writer starts");
    for (i, ms) in TIMES_MS.iter().enumerate() {
        while !t.is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        t.append_video(&frame(i as u32), ms * MS).expect("append");
    }
    let fin = t.finish(Some(2_000 * MS)).expect("finish");
    assert_eq!(fin.samples, TIMES_MS.len() as u64);
    assert_eq!(fin.duration_ns, Some(2_000 * MS));
    path
}

fn expected_index(slot: u64) -> u32 {
    let t = slot as f64 / 30.0 * 1000.0;
    TIMES_MS
        .iter()
        .rposition(|ms| (*ms as f64 / 1000.0 * 30.0).round() as u64 <= slot)
        .map(|i| i as u32)
        .unwrap_or_else(|| panic!("no frame before {t} ms"))
}

#[test]
fn irregular_frames_land_in_constant_rate_slots_and_seek() {
    let Some(ff) = ffmpeg() else { return };
    let dir = tmp("video");
    let path = write_video(&dir, &HelperWriters::new(ff.clone()));
    let d = HelperDecoder::new(ff);
    let p = d.probe(&path).expect("probe");
    assert!(p.readable, "{p:?}");
    assert_eq!(p.size, Some(SIZE));
    assert_eq!(p.samples, 60);
    assert!(
        (p.duration_ns.unwrap_or(0) - 2_000 * MS).abs() <= 34 * MS,
        "{p:?}"
    );

    let mut r = d.open_video(&path).expect("open");
    let mut n = 0u64;
    while let Some(f) = r.next_frame().expect("frame") {
        assert_eq!(f.file_ns, (n as f64 * 1e9 / 30.0).round() as i64);
        assert_eq!(code(&f), Some(expected_index(n)), "slot {n}");
        n += 1;
    }
    assert_eq!(n, 60);

    for at_ms in [0i64, 420, 1_010, 1_990] {
        let mut r = d.open_video_at(&path, at_ms * MS).expect("seek");
        let f = r.next_frame().expect("frame").expect("a frame");
        let slot = (at_ms as f64 * 30.0 / 1000.0).floor() as u64;
        assert!(f.file_ns <= at_ms * MS, "{at_ms}: {}", f.file_ns);
        assert_eq!(code(&f), Some(expected_index(slot)), "seek {at_ms} ms");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_cut_short_still_reads_up_to_the_cut() {
    let Some(ff) = ffmpeg() else { return };
    let dir = tmp("cut");
    let path = write_video(&dir, &HelperWriters::new(ff.clone()));
    let bytes = std::fs::read(&path).expect("read");
    let cut = dir.join("cut.mkv");
    std::fs::write(&cut, &bytes[..bytes.len() * 3 / 5]).expect("write");
    let d = HelperDecoder::new(ff);
    let p = d.probe(&cut).expect("probe");
    assert!(p.readable, "{p:?}");
    let mut r = d.open_video(&cut).expect("open");
    let mut n = 0;
    while let Ok(Some(f)) = r.next_frame() {
        if code(&f).is_some() {
            n += 1;
        }
    }
    assert!(n >= 10, "decodable frames before the cut: {n}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn audio_keeps_its_timeline_across_gaps() {
    let Some(ff) = ffmpeg() else { return };
    let dir = tmp("audio");
    let path = dir.join("mic.mkv");
    let w = HelperWriters::new(ff.clone());
    let mut t = w
        .audio(
            &path,
            &AudioWriterSpec {
                sample_rate: 48_000,
                channels: 1,
                bitrate: 128_000,
                fragment_interval_s: 1.0,
            },
        )
        .expect("writer");
    let tone: Arc<[f32]> = (0..4_800)
        .map(|i| 0.5 * (i as f32 * 1_000.0 * std::f32::consts::TAU / 48_000.0).sin())
        .collect();
    let buf = AudioBuffer {
        pts: HostTime(0),
        sample_rate: 48_000,
        channels: 1,
        frames: 4_800,
        data: AudioData::F32Interleaved(tone),
    };
    for i in 0..10 {
        t.append_audio(&buf, i * 100 * MS, 0).expect("append");
    }
    for i in 15..25 {
        t.append_audio(&buf, i * 100 * MS, 0).expect("append");
    }
    let fin = t.finish(Some(2_500 * MS)).expect("finish");
    assert_eq!(fin.samples, 120_000);

    let d = HelperDecoder::new(ff);
    let mut r = d.open_audio(&path, 48_000, 1).expect("open");
    let mut all = Vec::new();
    let mut chunk = vec![0f32; 4_096];
    loop {
        let n = r.read(&mut chunk).expect("read");
        if n == 0 {
            break;
        }
        all.extend_from_slice(&chunk[..n]);
    }
    assert!(
        (all.len() as i64 - 120_000).abs() <= 2_048,
        "{} samples",
        all.len()
    );
    let rms =
        |a: usize, b: usize| (all[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt();
    assert!(rms(10_000, 40_000) > 0.3, "tone before the gap");
    assert!(rms(50_000, 70_000) < 0.01, "silence in the gap");
    assert!(rms(80_000, 110_000) > 0.3, "tone after the gap");

    let mut r = d.open_audio_at(&path, 48_000, 1, 2_000 * MS).expect("seek");
    assert_eq!(r.first_sample_file_ns(), 2_000 * MS);
    let n = r.read(&mut chunk).expect("read");
    assert!(n > 0);
    let _ = std::fs::remove_dir_all(&dir);
}
