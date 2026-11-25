//! Full MP4 exports through the bundled FFmpeg, checked with ffprobe. Skipped when the helpers
//! are not built, unless `SC_REQUIRE_HELPERS=1`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use sc_core::project::TrackKind;
use sc_export::recipe::quick_canvas;
use sc_export::validate::{Expect, ffprobe_check};
use sc_export::{ExportJob, ExportSize, ExportState, QuickExportRequest};
use sc_geometry::{Aspect, RectPx, SizeClass, SizePx};
use sc_platform::Platform;
use sc_platform::synthetic::{Scenario, SyntheticPlatform};

fn platform() -> Arc<dyn Platform> {
    SyntheticPlatform::new(Scenario::new())
}

fn run_export(
    dir: &Path,
    out: Option<PathBuf>,
    size: ExportSize,
    fps: Option<u32>,
) -> (PathBuf, Arc<Obs>) {
    let obs = Arc::new(Obs::default());
    let job = ExportJob::start(
        platform(),
        QuickExportRequest {
            project_dir: dir.to_path_buf(),
            out_path: out,
            size,
            fps,
        },
        obs.clone(),
    )
    .expect("start export");
    let path = job.wait().expect("export");
    assert_eq!(
        obs.last_state(),
        Some(ExportState::Done { path: path.clone() })
    );
    let states = obs.states.lock().expect("states").clone();
    assert_eq!(
        states[..4],
        [
            ExportState::Preparing,
            ExportState::Rendering,
            ExportState::Finalizing,
            ExportState::Done { path: path.clone() }
        ]
    );
    (path, obs)
}

fn expect(size: SizePx, fps: u32, frames: u64, audio: bool) -> Expect {
    Expect {
        size,
        fps,
        duration_ns: (frames as i64 * 1_000_000_000) / i64::from(fps),
        audio,
        vcodec: "h264".into(),
    }
}

fn no_parts(dir: &Path) {
    for e in std::fs::read_dir(dir).expect("read dir").flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        assert!(!n.ends_with(".part"), "left behind: {n}");
    }
}

#[test]
fn every_aspect_and_background_kind() {
    if !helpers_available() {
        return;
    }
    for aspect in [
        Aspect::Original,
        Aspect::R16x9,
        Aspect::R4x3,
        Aspect::R1x1,
        Aspect::R9x16,
        Aspect::R4x5,
    ] {
        for kind in ["solid", "linear", "radial", "image"] {
            let t = TempDir::new("mp4-matrix");
            let mut p = new_project(t.path(), "M", 334);
            p.aspect(aspect).segment(Seg::new(0, 334, 320, 180));
            let bg = match kind {
                "solid" => solid(),
                "linear" => linear(),
                "radial" => radial(),
                _ => image()(&p.dir),
            };
            p.background(bg);
            let dir = p.save();
            let canvas = quick_canvas(&p.project, ExportSize::Reference);
            let (path, _) = run_export(&dir, None, ExportSize::Reference, None);
            assert_eq!(path, t.path().join("M.mp4"));
            let r = ffprobe_check(&path, &expect(canvas, 30, 11, false))
                .unwrap_or_else(|e| panic!("{aspect:?} {kind}: {e}"));
            assert_eq!(r.frames, Some(11));
            no_parts(t.path());
        }
    }
}

#[test]
fn every_size_class() {
    if !helpers_available() {
        return;
    }
    let t = TempDir::new("mp4-classes");
    let mut p = new_project(t.path(), "C", 200);
    p.aspect(Aspect::R16x9)
        .segment(Seg::new(0, 200, 1920, 1080));
    let dir = p.save();
    for (class, want) in [
        (SizeClass::S2160, SizePx { w: 1920, h: 1080 }),
        (SizeClass::S1440, SizePx { w: 1920, h: 1080 }),
        (SizeClass::S1080, SizePx { w: 1920, h: 1080 }),
        (SizeClass::S720, SizePx { w: 1280, h: 720 }),
        (SizeClass::S480, SizePx { w: 854, h: 480 }),
    ] {
        let out = t.path().join(format!("c-{class:?}.mp4"));
        let (path, _) = run_export(&dir, Some(out), ExportSize::Class(class), None);
        ffprobe_check(&path, &expect(want, 30, 6, false))
            .unwrap_or_else(|e| panic!("{class:?}: {e}"));
    }
}

#[test]
fn segments_gap_audio_and_fps_override() {
    if !helpers_available() {
        return;
    }
    let t = TempDir::new("mp4-segs");
    let mut p = new_project(t.path(), "Segs", 1200);
    p.aspect(Aspect::R16x9);
    p.segment(Seg::new(0, 400, 640, 360));
    let mut s1 = Seg::new(400, 700, 1280, 720);
    s1.content = Some(RectPx {
        x: 0,
        y: 40,
        w: 1280,
        h: 640,
    });
    p.segment(s1);
    p.segment(Seg::new(900, 1200, 480, 640));
    let tone: Vec<f32> = (0..48_000)
        .map(|i| 0.3 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin())
        .collect();
    p.audio("mic", TrackKind::Mic, 48_000, 1, 50, &tone);
    p.audio(
        "sys",
        TrackKind::SystemAudio,
        44_100,
        2,
        0,
        &vec![0.1; 2 * 44_100],
    );
    let dir = p.save();
    for fps in [30u32, 25, 24] {
        let out = t.path().join(format!("segs-{fps}.mp4"));
        let (path, obs) = run_export(&dir, Some(out), ExportSize::Reference, Some(fps));
        let frames = (1200 * u64::from(fps)).div_ceil(1000);
        let r = ffprobe_check(
            &path,
            &expect(SizePx { w: 1280, h: 720 }, fps, frames, true),
        )
        .unwrap_or_else(|e| panic!("{fps} fps: {e}"));
        assert_eq!(r.frames, Some(frames));
        assert_eq!(r.acodec.as_deref(), Some("aac"));
        let gap = (r.audio_duration_ns.expect("audio duration") - r.duration_ns).abs();
        eprintln!(
            "{fps} fps: video {} ns, audio-video {gap} ns",
            r.duration_ns
        );
        let last = obs
            .progress
            .lock()
            .expect("progress")
            .last()
            .cloned()
            .expect("progress");
        assert_eq!((last.frames_done, last.frames_total), (frames, frames));
    }
    no_parts(t.path());
}

#[test]
fn second_export_gets_a_new_name() {
    if !helpers_available() {
        return;
    }
    let t = TempDir::new("mp4-names");
    let mut p = new_project(t.path(), "Twice", 100);
    p.segment(Seg::new(0, 100, 64, 36));
    let dir = p.save();
    let (a, _) = run_export(&dir, None, ExportSize::Reference, None);
    let (b, _) = run_export(&dir, None, ExportSize::Reference, None);
    assert_eq!(a, t.path().join("Twice.mp4"));
    assert_eq!(b, t.path().join("Twice 2.mp4"));
    let obs = Arc::new(Obs::default());
    let err = ExportJob::start(
        platform(),
        QuickExportRequest {
            project_dir: dir,
            out_path: Some(a.clone()),
            size: ExportSize::Reference,
            fps: None,
        },
        obs,
    )
    .expect_err("existing output is never overwritten");
    assert!(err.to_string().contains("overwrite"), "{err}");
}

fn ffmpeg_running_for(path: &Path) -> bool {
    let out = Command::new("/bin/ps")
        .args(["-axo", "command"])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.contains("ffmpeg") && l.contains(&*path.to_string_lossy()))
}

#[test]
fn cancel_removes_part_and_helper() {
    if !helpers_available() {
        return;
    }
    let t = TempDir::new("mp4-cancel");
    let mut p = new_project(t.path(), "Cancel", 20_000);
    p.aspect(Aspect::R16x9)
        .segment(Seg::new(0, 20_000, 1920, 1080));
    p.audio("mic", TrackKind::Mic, 48_000, 1, 0, &vec![0.1; 48_000 * 20]);
    let dir = p.save();
    let obs = Arc::new(Obs::default());
    let job = ExportJob::start(
        platform(),
        QuickExportRequest {
            project_dir: dir,
            out_path: None,
            size: ExportSize::Reference,
            fps: None,
        },
        obs.clone(),
    )
    .expect("start");
    let part = t.path().join("Cancel.mp4.part");
    let t0 = Instant::now();
    while obs
        .progress
        .lock()
        .expect("progress")
        .last()
        .is_none_or(|p| p.frames_done < 5)
    {
        assert!(t0.elapsed() < Duration::from_secs(60), "no progress");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(part.exists());
    assert!(ffmpeg_running_for(&part));
    job.cancel();
    let err = job.wait().expect_err("canceled");
    assert!(err.to_string().contains("canceled"), "{err}");
    assert_eq!(obs.last_state(), Some(ExportState::Canceled));
    assert_eq!(t.names(), vec!["Cancel.sc".to_string()]);
    assert!(!ffmpeg_running_for(&part));
}

#[test]
fn colour_patches_survive_within_3() {
    if !helpers_available() {
        return;
    }
    const PATCHES: [[u8; 3]; 8] = [
        [200, 30, 30],
        [30, 200, 30],
        [30, 30, 200],
        [128, 128, 128],
        [235, 235, 235],
        [20, 20, 20],
        [220, 200, 40],
        [40, 180, 200],
    ];
    let t = TempDir::new("mp4-colour");
    let mut p = new_project(t.path(), "Colour", 200);
    p.aspect(Aspect::R1x1);
    let canvas = SizePx { w: 640, h: 640 };
    let bg = image_background(&p.dir, canvas, |x, _| PATCHES[(x / 80) as usize]);
    p.background(bg);
    let screen = SizePx { w: 640, h: 360 };
    let mut f = sc_platform::CpuNv12Frame::filled(screen, 16, 128, 128, false);
    let uv_stride = sc_platform::CpuNv12Frame::uv_stride_for(screen);
    for y in 0..360usize {
        for x in 0..640usize {
            let (yy, u, v) = rgb_to_yuv709(PATCHES[x / 80]);
            f.y[y * 640 + x] = yy;
            if y % 2 == 0 && x % 2 == 0 {
                f.uv[(y / 2) * uv_stride + x] = u;
                f.uv[(y / 2) * uv_stride + x + 1] = v;
            }
        }
    }
    let mut seg = Seg::new(0, 200, 640, 360);
    seg.frame = Some(f);
    p.segment(seg);
    let dir = p.save();
    assert_eq!(quick_canvas(&p.project, ExportSize::Reference), canvas);
    let (path, _) = run_export(&dir, None, ExportSize::Reference, None);
    ffprobe_check(&path, &expect(canvas, 30, 6, false)).expect("probe");

    let ffmpeg = sc_helpers::locate(sc_helpers::HelperKind::Ffmpeg).expect("ffmpeg");
    let out = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=in_color_matrix=bt709:in_range=tv:flags=accurate_rnd+full_chroma_int,format=rgb24",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .output()
        .expect("decode");
    assert_eq!(
        out.stdout.len(),
        640 * 640 * 3,
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rgb = out.stdout;
    let mut worst = 0u32;
    let mut detail = Vec::new();
    for (band, cy) in [("background", 70usize), ("screen", 320)] {
        for (i, want) in PATCHES.iter().enumerate() {
            let cx = i * 80 + 40;
            let mut acc = [0u64; 3];
            for y in cy - 8..cy + 8 {
                for x in cx - 8..cx + 8 {
                    for c in 0..3 {
                        acc[c] += u64::from(rgb[(y * 640 + x) * 3 + c]);
                    }
                }
            }
            let got = acc.map(|v| (v as f64 / 256.0).round() as i32);
            let err = (0..3)
                .map(|c| (got[c] - i32::from(want[c])).unsigned_abs())
                .max()
                .unwrap_or(0);
            worst = worst.max(err);
            detail.push(format!("{band}{i}:{want:?}->{got:?}"));
        }
    }
    eprintln!("colour patches: max |err| {worst}: {}", detail.join(" "));
    assert!(worst <= 3, "max |err| {worst} > 3: {}", detail.join(" "));
}
