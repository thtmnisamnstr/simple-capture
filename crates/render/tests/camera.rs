//! Camera layer: GPU vs CPU parity, and what the CPU reference draws. Without a wgpu adapter the
//! GPU half prints `SKIPPED:` and passes, unless `SC_REQUIRE_GPU=1`.

use std::sync::Arc;

use sc_core::image::RgbaImage;
use sc_geometry::{RectPx, SizePx};
use sc_platform::{CpuNv12Frame, FrameBuffer};
use sc_render::{Backend, CameraLayer, Compositor, FrameJob, OutputFormat, cpu, diff};

fn sz(w: u32, h: u32) -> SizePx {
    SizePx { w, h }
}

fn rect(x: i32, y: i32, w: u32, h: u32) -> RectPx {
    RectPx { x, y, w, h }
}

fn gpu() -> Option<Compositor> {
    let require = std::env::var("SC_REQUIRE_GPU").is_ok_and(|v| v == "1");
    match Compositor::new(Backend::Auto) {
        Ok(c) if c.adapter().is_some() => Some(c),
        Ok(_) if require => panic!("SC_REQUIRE_GPU=1 but no wgpu adapter"),
        Err(e) if require => panic!("SC_REQUIRE_GPU=1 but Compositor::new failed: {e}"),
        _ => {
            println!("SKIPPED: no wgpu adapter");
            None
        }
    }
}

/// Limited-range NV12 frame from a per-pixel (y, u, v) function.
fn frame(size: SizePx, f: impl Fn(u32, u32) -> (u8, u8, u8)) -> FrameBuffer {
    let (w, h) = (size.w as usize, size.h as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let mut y = vec![0u8; w * h];
    let mut uv = vec![0u8; cw * ch * 2];
    for r in 0..h {
        for c in 0..w {
            let (yy, u, v) = f(c as u32, r as u32);
            y[r * w + c] = yy;
            if r % 2 == 0 && c % 2 == 0 {
                uv[2 * ((r / 2) * cw + c / 2)] = u;
                uv[2 * ((r / 2) * cw + c / 2) + 1] = v;
            }
        }
    }
    FrameBuffer(Arc::new(CpuNv12Frame {
        size,
        y,
        uv,
        full_range: false,
    }))
}

fn noise(size: SizePx, seed: u32) -> FrameBuffer {
    frame(size, |x, y| {
        let n = x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ seed;
        (
            16 + (n % 220) as u8,
            64 + (n >> 8) as u8 % 128,
            64 + (n >> 16) as u8 % 128,
        )
    })
}

fn background(size: SizePx, alpha: u8) -> RgbaImage {
    let (w, h) = (size.w as usize, size.h as usize);
    let stride = w * 4;
    let mut data = vec![0u8; stride * h];
    if alpha > 0 {
        for r in 0..h {
            for c in 0..w {
                let o = r * stride + c * 4;
                data[o] = (40 + c * 150 / w) as u8;
                data[o + 1] = (30 + r * 100 / h) as u8;
                data[o + 2] = 120;
                data[o + 3] = alpha;
            }
        }
    }
    RgbaImage { size, stride, data }
}

fn px(f: &sc_render::RenderedFrame, x: u32, y: u32) -> [u8; 4] {
    let o = ((y * f.size.w + x) * 4) as usize;
    [f.data[o], f.data[o + 1], f.data[o + 2], f.data[o + 3]]
}

fn layer(frame: &FrameBuffer, dst: RectPx, radius: u32) -> CameraLayer<'_> {
    CameraLayer {
        frame,
        src: rect(0, 0, frame.0.size().w, frame.0.size().h),
        dst,
        radius_px: radius,
        mirror: false,
        border: None,
        shadow: 0,
    }
}

#[test]
fn gpu_matches_cpu_for_every_camera_option() {
    let Some(mut g) = gpu() else { return };
    let canvas = sz(1280, 720);
    let screen = noise(sz(1920, 1080), 7);
    let cam = noise(sz(1280, 720), 99);
    let opaque = background(canvas, 255);
    let clear = background(canvas, 0);
    let dst = rect(820, 380, 360, 300);
    let variants: Vec<(&str, CameraLayer)> = vec![
        ("circle", layer(&cam, rect(900, 420, 300, 300), 150)),
        ("rounded", layer(&cam, dst, 40)),
        ("square", layer(&cam, dst, 0)),
        (
            "mirror",
            CameraLayer {
                mirror: true,
                ..layer(&cam, dst, 60)
            },
        ),
        (
            "border",
            CameraLayer {
                border: Some((8, [250, 250, 250])),
                ..layer(&cam, dst, 60)
            },
        ),
        (
            "shadow",
            CameraLayer {
                shadow: 255,
                ..layer(&cam, dst, 60)
            },
        ),
        (
            "zoomed crop",
            CameraLayer {
                src: rect(400, 200, 320, 180),
                ..layer(&cam, dst, 30)
            },
        ),
        ("off the edge", layer(&cam, rect(1150, 640, 300, 300), 150)),
    ];
    for (name, cam_layer) in variants {
        for (bg_name, bg, with_screen) in [("canvas", &opaque, true), ("bubble", &clear, false)] {
            let job = FrameJob {
                canvas,
                background: bg,
                screen: with_screen.then(|| sc_render::ScreenLayer {
                    frame: &screen,
                    src: rect(0, 0, 1920, 1080),
                    dst: rect(0, 0, 1280, 720),
                }),
                camera: Some(cam_layer.clone()),
            };
            for out in [OutputFormat::Rgba8, OutputFormat::I420Bt709Limited] {
                let a = g.render(&job, out).expect("gpu");
                let b = cpu::render_cpu(&job, out).expect("cpu");
                let s = diff(&a, &b);
                println!(
                    "{name} on {bg_name} {out:?}: max {} mean {:.5}",
                    s.max, s.mean
                );
                assert!(
                    s.max <= 2,
                    "{name} on {bg_name} {out:?}: max diff {}",
                    s.max
                );
                assert!(
                    s.mean <= 0.01,
                    "{name} on {bg_name} {out:?}: mean diff {}",
                    s.mean
                );
            }
        }
    }
}

#[test]
fn circle_shows_the_camera_inside_and_the_background_outside() {
    let canvas = sz(400, 400);
    let red = frame(sz(64, 64), |_, _| (63, 102, 240));
    let bg = background(canvas, 0);
    let job = FrameJob {
        canvas,
        background: &bg,
        screen: None,
        camera: Some(layer(&red, rect(100, 100, 200, 200), 100)),
    };
    let f = cpu::render_cpu(&job, OutputFormat::Rgba8).expect("render");
    let centre = px(&f, 200, 200);
    assert!(
        centre[0] > 240 && centre[1] < 20 && centre[2] < 20 && centre[3] == 255,
        "{centre:?}"
    );
    assert_eq!(
        px(&f, 105, 105),
        [0, 0, 0, 0],
        "outside the circle, inside the rect"
    );
    assert_eq!(px(&f, 50, 50), [0, 0, 0, 0]);
    let edge = px(&f, 129, 129);
    assert!(
        edge[3] > 0 && edge[3] < 255,
        "anti-aliased diagonal edge {edge:?}"
    );
}

#[test]
fn mirror_flips_the_camera_horizontally() {
    let canvas = sz(200, 200);
    let left_dark = frame(sz(64, 64), |x, _| {
        if x < 32 {
            (16, 128, 128)
        } else {
            (235, 128, 128)
        }
    });
    let bg = background(canvas, 255);
    let draw = |mirror| {
        let job = FrameJob {
            canvas,
            background: &bg,
            screen: None,
            camera: Some(CameraLayer {
                mirror,
                ..layer(&left_dark, rect(0, 0, 200, 200), 0)
            }),
        };
        cpu::render_cpu(&job, OutputFormat::Rgba8).expect("render")
    };
    let (plain, mirrored) = (draw(false), draw(true));
    assert!(px(&plain, 20, 100)[0] < 10 && px(&plain, 180, 100)[0] > 245);
    assert!(px(&mirrored, 20, 100)[0] > 245 && px(&mirrored, 180, 100)[0] < 10);
}

#[test]
fn border_sits_inside_the_edge_and_shadow_falls_below() {
    let canvas = sz(400, 400);
    let black = frame(sz(64, 64), |_, _| (16, 128, 128));
    let bg = background(canvas, 255);
    let job = FrameJob {
        canvas,
        background: &bg,
        screen: None,
        camera: Some(CameraLayer {
            border: Some((10, [255, 255, 0])),
            shadow: 255,
            ..layer(&black, rect(100, 100, 200, 200), 0)
        }),
    };
    let f = cpu::render_cpu(&job, OutputFormat::Rgba8).expect("render");
    assert_eq!(
        &px(&f, 104, 200)[..3],
        &[255, 255, 0],
        "border inside the left edge"
    );
    assert_eq!(
        &px(&f, 200, 200)[..3],
        &[0, 0, 0],
        "camera inside the border"
    );
    let under = px(&f, 200, 303);
    let plain = px(&f, 200, 390);
    let bg_under = [
        bg.data[(303 * 400 + 200) * 4],
        bg.data[(303 * 400 + 200) * 4 + 1],
    ];
    assert!(
        under[0] < bg_under[0] && under[1] < bg_under[1],
        "shadow darkens below: {under:?}"
    );
    assert_eq!(
        plain[0],
        bg.data[(390 * 400 + 200) * 4],
        "far from the shape: untouched"
    );
}
