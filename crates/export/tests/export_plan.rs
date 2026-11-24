//! Plan and render without the helper (runs everywhere).

mod common;

use common::*;
use sc_export::ExportSize;
use sc_export::recipe::{FrameRenderer, QUICK_LONG_SIDE_CAP, default_out_path, quick_canvas};
use sc_geometry::{Aspect, RectPx, SizeClass, SizePx, contain_fit};
use sc_platform::synthetic::SctrkDecoder;
use sc_render::{Backend, OutputFormat};
use std::sync::Arc;

fn sz(w: u32, h: u32) -> SizePx {
    SizePx { w, h }
}

fn canvas_for(aspect: Aspect, segs: &[(u32, u32)], size: ExportSize) -> SizePx {
    let t = TempDir::new("plan");
    let mut p = new_project(t.path(), "P", 1000);
    p.aspect(aspect);
    for (i, &(w, h)) in segs.iter().enumerate() {
        let i = i as i64;
        let mut s = Seg::new(i * 100, i * 100 + 100, w, h);
        s.fps = 10;
        p.segment(s);
    }
    quick_canvas(&p.project, size)
}

#[test]
fn reference_canvas_with_long_side_cap() {
    let r = ExportSize::Reference;
    assert_eq!(
        canvas_for(Aspect::Original, &[(7680, 2160)], r),
        sz(3840, 1080)
    );
    assert_eq!(
        canvas_for(Aspect::R16x9, &[(7680, 2160)], r),
        sz(3840, 2160)
    );
    assert_eq!(
        canvas_for(Aspect::R9x16, &[(7680, 2160)], r),
        sz(2160, 3840)
    );
    assert_eq!(
        canvas_for(Aspect::Original, &[(1920, 1080)], r),
        sz(1920, 1080)
    );
    assert_eq!(canvas_for(Aspect::R4x3, &[(1920, 1080)], r), sz(1920, 1440));
    assert_eq!(canvas_for(Aspect::R1x1, &[(1920, 1080)], r), sz(1920, 1920));
    assert_eq!(
        canvas_for(Aspect::Original, &[(1001, 501)], r),
        sz(1000, 500)
    );
    assert_eq!(
        canvas_for(Aspect::Original, &[(1920, 1080), (1280, 720)], r),
        sz(1920, 1080)
    );
    for a in [
        Aspect::Original,
        Aspect::R16x9,
        Aspect::R4x3,
        Aspect::R1x1,
        Aspect::R9x16,
        Aspect::R4x5,
    ] {
        let c = canvas_for(a, &[(7680, 2160), (3024, 1964)], r);
        assert!(c.w.max(c.h) <= QUICK_LONG_SIDE_CAP, "{a:?} {c:?}");
        assert!(
            c.w.is_multiple_of(2) && c.h.is_multiple_of(2),
            "{a:?} {c:?}"
        );
    }
}

#[test]
fn size_classes_never_upscale() {
    let c = |a, class| canvas_for(a, &[(1920, 1080)], ExportSize::Class(class));
    assert_eq!(c(Aspect::R16x9, SizeClass::S2160), sz(1920, 1080));
    assert_eq!(c(Aspect::R16x9, SizeClass::S1440), sz(1920, 1080));
    assert_eq!(c(Aspect::R16x9, SizeClass::S1080), sz(1920, 1080));
    assert_eq!(c(Aspect::R16x9, SizeClass::S720), sz(1280, 720));
    assert_eq!(c(Aspect::R16x9, SizeClass::S480), sz(854, 480));
    assert_eq!(c(Aspect::R4x3, SizeClass::S1080), sz(1440, 1080));
    assert_eq!(c(Aspect::R9x16, SizeClass::S720), sz(720, 1280));
    assert_eq!(c(Aspect::R1x1, SizeClass::S480), sz(480, 480));
    assert_eq!(c(Aspect::Original, SizeClass::S720), sz(1280, 720));
    assert_eq!(
        canvas_for(
            Aspect::R16x9,
            &[(7680, 2160)],
            ExportSize::Class(SizeClass::S1440)
        ),
        sz(2560, 1440)
    );
}

#[test]
fn default_name_skips_taken_files() {
    let t = TempDir::new("plan-name");
    let mut p = new_project(t.path(), "Demo", 1000);
    p.segment(Seg::new(0, 1000, 320, 180));
    let dir = p.save();
    assert_eq!(
        default_out_path(&dir).expect("name"),
        t.path().join("Demo.mp4")
    );
    std::fs::write(t.path().join("Demo.mp4"), b"x").expect("write");
    std::fs::write(t.path().join("Demo 2.mp4.part"), b"x").expect("write");
    assert_eq!(
        default_out_path(&dir).expect("name"),
        t.path().join("Demo 3.mp4")
    );
}

fn px(d: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [d[i], d[i + 1], d[i + 2], d[i + 3]]
}

#[test]
fn render_every_aspect_and_background() {
    let backend = if require_gpu() {
        Backend::Auto
    } else {
        Backend::CpuOnly
    };
    for aspect in [
        Aspect::Original,
        Aspect::R16x9,
        Aspect::R4x3,
        Aspect::R1x1,
        Aspect::R9x16,
        Aspect::R4x5,
    ] {
        for kind in ["solid", "linear", "radial", "image"] {
            let t = TempDir::new("render");
            let mut p = new_project(t.path(), "R", 200);
            p.aspect(aspect).segment(Seg::new(0, 200, 320, 180));
            let bg = match kind {
                "solid" => solid(),
                "linear" => linear(),
                "radial" => radial(),
                _ => image()(&p.dir),
            };
            p.background(bg);
            let dir = p.save();
            let canvas = quick_canvas(&p.project, ExportSize::Reference);
            let mut r = FrameRenderer::new(
                &p.project,
                &dir,
                Arc::new(SctrkDecoder),
                canvas,
                backend,
                OutputFormat::Rgba8,
            )
            .expect("renderer");
            if require_gpu() {
                assert!(
                    r.compositor().adapter().is_some(),
                    "SC_REQUIRE_GPU=1: no adapter"
                );
            }
            let (f, dst) = r.render(50 * MS).expect("render");
            assert_eq!(f.size, canvas);
            assert_eq!(f.data.len(), (canvas.w * canvas.h * 4) as usize);
            let dst = dst.expect("screen drawn");
            assert_eq!(dst, contain_fit(canvas, sz(320, 180)), "{aspect:?} {kind}");
            if kind == "solid" && (dst.x > 0 || dst.y > 0) {
                let corner = px(&f.data, canvas.w, 0, 0);
                for (c, want) in corner[..3].iter().zip([51u8, 102, 153]) {
                    assert!(c.abs_diff(want) <= 2, "{aspect:?} corner {corner:?}");
                }
            }
            let mid = px(
                &f.data,
                canvas.w,
                (dst.x + dst.w as i32 / 2) as u32,
                (dst.y + dst.h as i32 - 2) as u32,
            );
            assert_eq!(mid[3], 255);
            let mut r = FrameRenderer::new(
                &p.project,
                &dir,
                Arc::new(SctrkDecoder),
                canvas,
                backend,
                OutputFormat::I420Bt709Limited,
            )
            .expect("renderer");
            let (f, _) = r.render(0).expect("render i420");
            assert_eq!(f.data.len(), sc_helpers::y4m::i420_frame_bytes(canvas));
        }
    }
}

#[test]
fn segments_of_different_sizes_and_a_gap() {
    let t = TempDir::new("segs");
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
    let dir = p.save();
    let canvas = quick_canvas(&p.project, ExportSize::Reference);
    assert_eq!(canvas, sz(1280, 720));
    let mut r = FrameRenderer::new(
        &p.project,
        &dir,
        Arc::new(SctrkDecoder),
        canvas,
        Backend::CpuOnly,
        OutputFormat::I420Bt709Limited,
    )
    .expect("renderer");
    let fit = |w, h| Some(contain_fit(canvas, sz(w, h)));
    for (ms, want) in [
        (0, fit(640, 360)),
        (399, fit(640, 360)),
        (400, fit(1280, 640)),
        (699, fit(1280, 640)),
        (750, fit(1280, 640)),
        (899, fit(1280, 640)),
        (900, fit(480, 640)),
        (1199, fit(480, 640)),
    ] {
        let (_, dst) = r.render(ms * MS).expect("render");
        assert_eq!(dst, want, "t = {ms} ms");
    }
}
