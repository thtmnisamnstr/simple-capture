//! Export speed (ignored by default):
//! `cargo test --locked -p simple-capture-export --test export_bench -- --ignored --nocapture`.

mod common;

use std::sync::Arc;
use std::time::Instant;

use common::*;
use sc_decode::cfr_frame_times;
use sc_export::recipe::{FrameRenderer, quick_canvas, x264_quick};
use sc_export::validate::{Expect, ffprobe_check};
use sc_export::{ExportJob, ExportSize, QuickExportRequest};
use sc_geometry::{Aspect, SizePx};
use sc_helpers::y4m::{self, Y4mHeader};
use sc_helpers::{HelperKind, HelperProcess, SpawnSpec};
use sc_platform::synthetic::{Scenario, SctrkDecoder, SyntheticPlatform};
use sc_render::{Backend, Compositor, OutputFormat};

fn adapter() -> String {
    Compositor::new(Backend::Auto)
        .ok()
        .and_then(|c| c.adapter())
        .map_or("CPU".into(), |a| format!("{} ({})", a.name, a.backend))
}

fn bench_job(w: u32, h: u32, secs: i64) {
    let t = TempDir::new("bench");
    let mut p = new_project(t.path(), "Bench", secs * 1000);
    p.aspect(Aspect::R16x9)
        .segment(Seg::new(0, secs * 1000, w, h));
    let dir = p.save();
    let canvas = quick_canvas(&p.project, ExportSize::Reference);
    let obs = Arc::new(Obs::default());
    let t0 = Instant::now();
    let job = ExportJob::start(
        SyntheticPlatform::new(Scenario::new()),
        QuickExportRequest {
            project_dir: dir,
            out_path: None,
            size: ExportSize::Reference,
            fps: None,
        },
        obs,
    )
    .expect("start");
    let path = job.wait().expect("export");
    let el = t0.elapsed().as_secs_f64();
    let frames = (secs * 30) as u64;
    ffprobe_check(
        &path,
        &Expect {
            size: canvas,
            fps: 30,
            duration_ns: secs * 1_000_000_000,
            audio: false,
            vcodec: "h264".into(),
        },
    )
    .expect("probe");
    eprintln!(
        "BENCH quick export {}x{} {secs} s @30: {el:.2} s wall, {:.1} fps, {:.2}x realtime, renderer {}",
        canvas.w,
        canvas.h,
        frames as f64 / el,
        secs as f64 / el,
        adapter()
    );
}

#[test]
#[ignore = "bench"]
fn bench_1080p_30s() {
    if helpers_available() {
        bench_job(1920, 1080, 30);
    }
}

#[test]
#[ignore = "bench"]
fn bench_2160p_30s() {
    if helpers_available() {
        bench_job(3840, 2160, 30);
    }
}

#[test]
#[ignore = "bench"]
fn bench_7680x2160_x264() {
    if !helpers_available() {
        return;
    }
    let secs = 10i64;
    let t = TempDir::new("bench-wide");
    let mut p = new_project(t.path(), "Wide", secs * 1000);
    p.segment(Seg::new(0, secs * 1000, 7680, 2160));
    let dir = p.save();
    let canvas = SizePx { w: 7680, h: 2160 };
    let mut r = FrameRenderer::new(
        &p.project,
        &dir,
        Arc::new(SctrkDecoder),
        canvas,
        Backend::Auto,
        OutputFormat::I420Bt709Limited,
    )
    .expect("renderer");
    let out = t.path().join("wide.mp4");
    let helper = HelperProcess::spawn(SpawnSpec {
        program: sc_helpers::locate(HelperKind::Ffmpeg).expect("ffmpeg"),
        args: x264_quick(canvas, 30, false, &out),
        frame_bytes: y4m::FRAME.len() + y4m::i420_frame_bytes(canvas),
        queue_frames: 3,
    })
    .expect("spawn");
    let t0 = Instant::now();
    helper
        .write_bytes(
            y4m::header(&Y4mHeader {
                size: canvas,
                fps_num: 30,
                fps_den: 1,
            })
            .as_bytes(),
        )
        .expect("header");
    let mut n = 0u64;
    for t in cfr_frame_times(secs * 1_000_000_000, 30) {
        let (f, _) = r.render(t).expect("render");
        helper
            .write_frame_parts(&[y4m::FRAME, &f.data])
            .expect("write");
        n += 1;
    }
    let rep = helper.finish().expect("finish");
    assert!(rep.success(), "{:?}", rep.stderr_tail);
    let el = t0.elapsed().as_secs_f64();
    eprintln!(
        "BENCH x264 medium CRF22 7680x2160 {secs} s @30: {el:.2} s wall, {:.1} fps, {:.2}x realtime, renderer {}",
        n as f64 / el,
        secs as f64 / el,
        adapter()
    );
}
