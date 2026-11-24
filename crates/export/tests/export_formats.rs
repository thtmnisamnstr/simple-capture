//! Every container, codec and encoder the export offers, through the bundled helpers, each
//! checked with ffprobe: codecs, size, frame count, duration, audio, colour tags; GIF frame
//! timing. Skipped when the helpers are not built, unless `SC_REQUIRE_HELPERS=1`.

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use common::*;
use sc_core::project::TrackKind;
use sc_export::plan::ExportRequest;
use sc_export::settings::{
    AudioCodec, Encoder, ExportSettings, Format, GifMode, Loop, SizeChoice, VideoCodec,
};
use sc_export::{ExportJob, ExportState};
use sc_geometry::Aspect;
use sc_platform::Platform;
use sc_platform::synthetic::{Scenario, SyntheticPlatform};
use serde_json::Value;

fn platform() -> Arc<dyn Platform> {
    SyntheticPlatform::new(Scenario::new())
}

fn probe(file: &Path) -> Value {
    let ffprobe = sc_helpers::locate(sc_helpers::HelperKind::Ffprobe).expect("ffprobe");
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-count_packets",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(file)
        .output()
        .expect("run ffprobe");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("ffprobe json")
}

fn stream<'a>(v: &'a Value, kind: &str) -> Option<&'a Value> {
    v["streams"]
        .as_array()
        .and_then(|s| s.iter().find(|x| x["codec_type"] == kind))
}

fn export(dir: &Path, st: ExportSettings) -> std::path::PathBuf {
    let obs = Arc::new(Obs::default());
    let job = ExportJob::start_with(
        platform(),
        ExportRequest {
            project_dir: dir.to_path_buf(),
            settings: st.clone(),
            out_path: None,
            canvas: None,
        },
        obs.clone(),
    )
    .unwrap_or_else(|e| panic!("{st:?}: {e}"));
    let path = job.wait().unwrap_or_else(|e| panic!("{st:?}: {e}"));
    assert_eq!(
        obs.last_state(),
        Some(ExportState::Done { path: path.clone() })
    );
    path
}

fn project(t: &TempDir) -> std::path::PathBuf {
    let mut p = new_project(t.path(), "Formats", 2_000);
    p.aspect(Aspect::R16x9);
    p.segment(Seg::new(0, 2_000, 640, 360));
    let tone: Vec<f32> = (0..96_000)
        .map(|i| 0.3 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin())
        .collect();
    p.audio("mic", TrackKind::Mic, 48_000, 1, 0, &tone);
    p.save()
}

fn want_v(v: VideoCodec) -> &'static str {
    match v {
        VideoCodec::H264 => "h264",
        VideoCodec::Hevc => "hevc",
        VideoCodec::Av1 => "av1",
        VideoCodec::Vp9 => "vp9",
        VideoCodec::Vp8 => "vp8",
        VideoCodec::Prores => "prores",
        VideoCodec::Gif => "gif",
        VideoCodec::Webp => "webp",
    }
}

fn want_a(a: AudioCodec) -> Option<&'static str> {
    match a {
        AudioCodec::Aac => Some("aac"),
        AudioCodec::Opus => Some("opus"),
        AudioCodec::Flac => Some("flac"),
        AudioCodec::Alac => Some("alac"),
        AudioCodec::Vorbis => Some("vorbis"),
        AudioCodec::Pcm => Some("pcm_s16le"),
        AudioCodec::None => None,
    }
}

#[test]
fn every_offered_video_combination_validates() {
    if !helpers_available() {
        return;
    }
    let t = TempDir::new("formats");
    let dir = project(&t);
    let mut cases: Vec<(Format, VideoCodec, AudioCodec, Encoder)> = vec![
        (
            Format::Mp4,
            VideoCodec::H264,
            AudioCodec::Aac,
            Encoder::Best,
        ),
        (
            Format::Mp4,
            VideoCodec::Hevc,
            AudioCodec::Aac,
            Encoder::Best,
        ),
        (
            Format::Mp4,
            VideoCodec::Av1,
            AudioCodec::Opus,
            Encoder::Best,
        ),
        (
            Format::Mp4,
            VideoCodec::H264,
            AudioCodec::Flac,
            Encoder::Best,
        ),
        (
            Format::Mp4,
            VideoCodec::H264,
            AudioCodec::Alac,
            Encoder::Best,
        ),
        (
            Format::Mp4,
            VideoCodec::H264,
            AudioCodec::None,
            Encoder::Best,
        ),
        (
            Format::Webm,
            VideoCodec::Vp9,
            AudioCodec::Opus,
            Encoder::Best,
        ),
        (
            Format::Webm,
            VideoCodec::Av1,
            AudioCodec::Opus,
            Encoder::Best,
        ),
        (
            Format::Webm,
            VideoCodec::Vp8,
            AudioCodec::Vorbis,
            Encoder::Best,
        ),
        (
            Format::Mov,
            VideoCodec::Prores,
            AudioCodec::Pcm,
            Encoder::Best,
        ),
    ];
    if cfg!(target_os = "macos") && std::env::var("SC_ALLOW_NO_HW_ENCODER").as_deref() != Ok("1") {
        cases.push((
            Format::Mp4,
            VideoCodec::H264,
            AudioCodec::Aac,
            Encoder::Fast,
        ));
        cases.push((
            Format::Mp4,
            VideoCodec::Hevc,
            AudioCodec::Aac,
            Encoder::Fast,
        ));
        cases.push((
            Format::Mov,
            VideoCodec::Prores,
            AudioCodec::Pcm,
            Encoder::Fast,
        ));
    }
    for (format, video, audio, encoder) in cases {
        let st = ExportSettings {
            video,
            audio,
            encoder,
            size: SizeChoice::Class(360),
            ..ExportSettings::for_format(format)
        };
        let path = export(&dir, st);
        let label = format!("{format:?} {video:?} {audio:?} {encoder:?}");
        let v = probe(&path);
        let vs = stream(&v, "video").unwrap_or_else(|| panic!("{label}: no video"));
        assert_eq!(vs["codec_name"], want_v(video), "{label}");
        assert_eq!(
            (vs["width"].as_u64(), vs["height"].as_u64()),
            (Some(640), Some(360)),
            "{label}"
        );
        let packets: u64 = vs["nb_read_packets"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        assert_eq!(packets, 60, "{label}: frames");
        let dur: f64 = v["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        assert!((dur - 2.0).abs() <= 0.07, "{label}: duration {dur}");
        if video == VideoCodec::Hevc {
            assert_eq!(vs["codec_tag_string"], "hvc1", "{label}");
        }
        let space = vs["color_space"].as_str().unwrap_or("");
        let want_space = if video == VideoCodec::Vp8 {
            "smpte170m"
        } else {
            "bt709"
        };
        assert_eq!(space, want_space, "{label}");
        match want_a(audio) {
            Some(a) => assert_eq!(
                stream(&v, "audio").map(|s| s["codec_name"].clone()),
                Some(Value::from(a)),
                "{label}"
            ),
            None => assert!(stream(&v, "audio").is_none(), "{label}: unexpected audio"),
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        assert_eq!(
            ext,
            ExportSettings::for_format(format).extension(),
            "{label}"
        );
    }
}

fn gif_delays(file: &Path) -> Vec<u16> {
    let b = std::fs::read(file).expect("gif");
    let mut out = Vec::new();
    let mut i = 0;
    while i + 7 < b.len() {
        if b[i] == 0x21 && b[i + 1] == 0xF9 && b[i + 2] == 0x04 {
            out.push(u16::from_le_bytes([b[i + 4], b[i + 5]]));
            i += 8;
        } else {
            i += 1;
        }
    }
    out
}

#[test]
fn gifs_from_gifski_and_the_palette_play_at_the_chosen_rate() {
    if !helpers_available() || sc_helpers::locate(sc_helpers::HelperKind::Gifski).is_err() {
        return;
    }
    let t = TempDir::new("gifs");
    let dir = project(&t);
    for (mode, fps, size) in [
        (GifMode::Gifski, 15u32, None),
        (GifMode::Palette, 10, None),
        (GifMode::Gifski, 25, None),
        (GifMode::Gifski, 10, Some(240)),
    ] {
        let mut st = ExportSettings::for_format(Format::Gif);
        if let Some(c) = size {
            st.size = SizeChoice::Class(c);
        }
        st.gif.mode = mode;
        st.fps = fps;
        st.gif.repeat = Loop::Times(2);
        let path = export(&dir, st);
        let v = probe(&path);
        let vs = stream(&v, "video").expect("gif stream");
        assert_eq!(vs["codec_name"], "gif");
        let want = match size {
            Some(240) => (Some(426), Some(240)),
            _ => (Some(640), Some(360)),
        };
        assert_eq!(
            (vs["width"].as_u64(), vs["height"].as_u64()),
            want,
            "{mode:?} {size:?}: even, never upscaled"
        );
        assert!(stream(&v, "audio").is_none());
        let delays = gif_delays(&path);
        let total: u32 = delays.iter().map(|d| u32::from(*d)).sum();
        assert_eq!(
            delays.len() as u32,
            2 * fps,
            "{mode:?} {fps}: frames {delays:?}"
        );
        assert!(
            total.abs_diff(200) <= 2,
            "{mode:?} {fps}: {total} cs for 2 s"
        );
        assert!(
            delays.iter().all(|d| *d >= 2),
            "browsers slow down delays under 2 cs"
        );
    }
}

/// Chunk type and body of each RIFF chunk inside a WebP's top-level chunk.
fn riff_chunks(b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut i = 12;
    while i + 8 <= b.len() {
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = &b[i + 8..(i + 8 + len).min(b.len())];
        out.push(([b[i], b[i + 1], b[i + 2], b[i + 3]], body));
        i += 8 + len + (len & 1);
    }
    out
}

#[test]
fn animated_webp_has_every_frame_and_the_loop_count() {
    if !helpers_available() {
        return;
    }
    let t = TempDir::new("anim");
    let dir = project(&t);
    let mut st = ExportSettings::for_format(Format::Webp);
    st.fps = 10;
    st.gif.repeat = Loop::Times(3);
    let path = export(&dir, st);
    let b = std::fs::read(&path).expect("file");
    assert_eq!((&b[0..4], &b[8..12]), (&b"RIFF"[..], &b"WEBP"[..]));
    let c = riff_chunks(&b);
    let vp8x = c.iter().find(|(k, _)| k == b"VP8X").expect("VP8X").1;
    let w = 1 + u32::from_le_bytes([vp8x[4], vp8x[5], vp8x[6], 0]);
    let h = 1 + u32::from_le_bytes([vp8x[7], vp8x[8], vp8x[9], 0]);
    assert_eq!((w, h), (640, 360), "never upscaled");
    let anim = c.iter().find(|(k, _)| k == b"ANIM").expect("ANIM").1;
    assert_eq!(u16::from_le_bytes([anim[4], anim[5]]), 3, "plays");
    let played_ms: u32 = c
        .iter()
        .filter(|(k, _)| k == b"ANMF")
        .map(|(_, f)| u32::from_le_bytes([f[12], f[13], f[14], 0]))
        .sum();
    assert!(
        played_ms.abs_diff(2000) <= 100,
        "{played_ms} ms of frames for 2 s"
    );
}

#[test]
fn a_size_limit_keeps_the_file_under_it() {
    if !helpers_available() {
        return;
    }
    let t = TempDir::new("target-size");
    let dir = project(&t);
    for mb in [0.05f64, 0.4] {
        let st = ExportSettings {
            quality: sc_export::settings::Quality::TargetSize { mb },
            ..ExportSettings::for_format(Format::Mp4)
        };
        let path = export(&dir, st);
        let bytes = std::fs::metadata(&path).expect("file").len();
        assert!(bytes as f64 <= mb * 1e6, "{mb} MB limit: {bytes} bytes");
        let v = probe(&path);
        assert_eq!(stream(&v, "video").expect("video")["codec_name"], "h264");
        assert!(stream(&v, "audio").is_some(), "{mb} MB keeps the sound");
    }
}
