//! SPEC definition of done: on a text-heavy clip, x264 "Best quality" scores a higher SSIM than
//! the hardware "Fast" encoder at the same file size. macOS only (VideoToolbox).

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use common::*;
use sc_core::project::{Anim, Extra, TextItem};
use sc_export::plan::ExportRequest;
use sc_export::recipe::FrameRenderer;
use sc_export::settings::{AudioCodec, Encoder, ExportSettings, Format, Quality, VideoCodec};
use sc_export::{ExportJob, ExportState};
use sc_geometry::{PointF, SizePx};
use sc_platform::synthetic::{Scenario, SctrkDecoder, SyntheticPlatform};
use sc_render::{Backend, OutputFormat};

const W: u32 = 1280;
const H: u32 = 720;
const FPS: u32 = 30;
const LEN_MS: i64 = 4_000;

const LINES: [&str; 10] = [
    "fn compose(frame: &Frame, layout: Layout) -> Canvas {",
    "    let rect = layout.fit(frame.size, canvas.size);",
    "    canvas.draw(frame, rect).with_corner_radius(12.0);",
    "    for text in timeline.text_at(t) { canvas.text(text); }",
    "    // 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghij",
    "    assert!(rect.w % 2 == 0 && rect.h % 2 == 0, \"even\");",
    "    let audio = mix(&tracks, gains, speed).fade_ms(5);",
    "    export(canvas, audio, Settings { crf: 22, .. })?;",
    "    Ok(canvas) // The quick brown fox jumps over it",
    "}",
];

fn text_heavy_project(t: &TempDir) -> std::path::PathBuf {
    let mut p = new_project(t.path(), "Quality", LEN_MS);
    p.segment(Seg::new(0, LEN_MS, W, H));
    for (i, line) in LINES.iter().enumerate() {
        let start = (i as i64) * 150;
        let mut item = TextItem::new(start * MS, LEN_MS * MS, line);
        item.style.size_1080 = 34.0;
        item.style.weight = 400;
        item.style.font_family = "Courier Prime".into();
        item.transform.center_norm = PointF {
            x: 0.5,
            y: 0.08 + i as f64 * 0.09,
        };
        item.anim.in_ = Some(Anim {
            kind: "typewriter".into(),
            duration_s: 1.5,
            easing: "linear".into(),
            extra: Extra::new(),
        });
        p.project.text.push(item);
    }
    p.save()
}

fn export(dir: &Path, st: ExportSettings) -> std::path::PathBuf {
    let obs = Arc::new(Obs::default());
    let platform: Arc<dyn sc_platform::Platform> = SyntheticPlatform::new(Scenario::new());
    let job = ExportJob::start_with(
        platform,
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

fn luma_frames(file: &Path) -> Vec<Vec<u8>> {
    let ffmpeg = sc_helpers::locate(sc_helpers::HelperKind::Ffmpeg).expect("ffmpeg");
    let out = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-f", "rawvideo", "-pix_fmt", "gray", "-"])
        .output()
        .expect("decode");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
        .as_chunks::<{ (W * H) as usize }>()
        .0
        .iter()
        .map(|f| f.to_vec())
        .collect()
}

fn ssim(a: &[u8], b: &[u8]) -> f64 {
    const C1: f64 = (0.01 * 255.0) * (0.01 * 255.0);
    const C2: f64 = (0.03 * 255.0) * (0.03 * 255.0);
    let (w, h) = (W as usize, H as usize);
    let mut total = 0.0;
    let mut n = 0usize;
    for y0 in (0..h - 8).step_by(4) {
        for x0 in (0..w - 8).step_by(4) {
            let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for y in y0..y0 + 8 {
                for x in x0..x0 + 8 {
                    let p = f64::from(a[y * w + x]);
                    let q = f64::from(b[y * w + x]);
                    sa += p;
                    sb += q;
                    saa += p * p;
                    sbb += q * q;
                    sab += p * q;
                }
            }
            let k = 64.0;
            let (ma, mb) = (sa / k, sb / k);
            let va = saa / k - ma * ma;
            let vb = sbb / k - mb * mb;
            let cov = sab / k - ma * mb;
            total += ((2.0 * ma * mb + C1) * (2.0 * cov + C2))
                / ((ma * ma + mb * mb + C1) * (va + vb + C2));
            n += 1;
        }
    }
    total / n as f64
}

fn settings(encoder: Encoder, kbps: u32) -> ExportSettings {
    ExportSettings {
        video: VideoCodec::H264,
        audio: AudioCodec::None,
        encoder,
        quality: Quality::Custom {
            crf: None,
            kbps: Some(kbps),
        },
        fps: FPS,
        ..ExportSettings::for_format(Format::Mp4)
    }
}

#[test]
fn x264_best_beats_hardware_fast_at_the_same_size_on_text() {
    if !cfg!(target_os = "macos")
        || !helpers_available()
        || std::env::var("SC_ALLOW_NO_HW_ENCODER").as_deref() == Ok("1")
    {
        return;
    }
    let t = TempDir::new("quality");
    let dir = text_heavy_project(&t);
    let project = sc_core::project::ProjectV1::load(&dir).expect("project");
    let mut reference = FrameRenderer::new(
        &project,
        &dir,
        Arc::new(SctrkDecoder),
        SizePx { w: W, h: H },
        Backend::CpuOnly,
        OutputFormat::Rgba8,
    )
    .expect("renderer");
    let samples: Vec<usize> = (0..(LEN_MS as usize * FPS as usize / 1000))
        .step_by(12)
        .collect();
    let refs: Vec<Vec<u8>> = samples
        .iter()
        .map(|i| {
            let f = reference
                .render(*i as i64 * 1_000_000_000 / i64::from(FPS))
                .expect("render")
                .0;
            f.data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| rgb_to_yuv709([p[0], p[1], p[2]]).0)
                .collect()
        })
        .collect();
    let score = |file: &Path| {
        let frames = luma_frames(file);
        samples
            .iter()
            .zip(&refs)
            .map(|(i, r)| ssim(&frames[*i], r))
            .sum::<f64>()
            / samples.len() as f64
    };

    let fast = export(&dir, settings(Encoder::Fast, 900));
    let fast_size = std::fs::metadata(&fast).expect("fast").len();
    let mut kbps = 900u32;
    let (best, best_size) = loop {
        let best = export(&dir, settings(Encoder::Best, kbps));
        let size = std::fs::metadata(&best).expect("best").len();
        if size as f64 <= fast_size as f64 * 1.05 || kbps < 300 {
            break (best, size);
        }
        kbps = (f64::from(kbps) * fast_size as f64 / size as f64 * 0.97) as u32;
        let _ = std::fs::remove_file(&best);
    };
    let (s_best, s_fast) = (score(&best), score(&fast));
    eprintln!("x264 {best_size} B SSIM {s_best:.4}; VideoToolbox {fast_size} B SSIM {s_fast:.4}");
    assert!(
        best_size as f64 <= fast_size as f64 * 1.05,
        "x264 {best_size} B vs VideoToolbox {fast_size} B"
    );
    assert!(
        s_best > s_fast,
        "x264 SSIM {s_best:.4} should beat VideoToolbox {s_fast:.4}"
    );
}
