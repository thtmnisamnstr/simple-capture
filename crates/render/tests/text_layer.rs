//! Text layer: GPU vs CPU parity for placed, scaled, rotated and faded premultiplied images, and
//! what the CPU reference draws. Without a wgpu adapter the GPU half prints `SKIPPED:` and passes,
//! unless `SC_REQUIRE_GPU=1`.

use sc_core::image::RgbaImage;
use sc_geometry::SizePx;
use sc_render::{Backend, Compositor, FrameJob, OutputFormat, TextLayer, cpu, diff};

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

fn solid(size: SizePx, rgba: [u8; 4]) -> RgbaImage {
    RgbaImage {
        size,
        stride: size.w as usize * 4,
        data: rgba.repeat((size.w * size.h) as usize),
    }
}

/// A premultiplied pattern with soft edges, holes and every alpha level.
fn pattern(size: SizePx) -> RgbaImage {
    let mut data = Vec::with_capacity((size.w * size.h * 4) as usize);
    for y in 0..size.h {
        for x in 0..size.w {
            let a = (x * 7 + y * 3) % 256;
            let r = (x * 255 / size.w.max(1)) * a / 255;
            let g = (y * 255 / size.h.max(1)) * a / 255;
            let b = ((x ^ y) & 255) * a / 255;
            data.extend_from_slice(&[r as u8, g as u8, b as u8, a as u8]);
        }
    }
    RgbaImage {
        size,
        stride: size.w as usize * 4,
        data,
    }
}

fn placed(
    img: &RgbaImage,
    cx: f32,
    cy: f32,
    scale: f32,
    deg: f32,
    opacity: f32,
    key: u64,
) -> TextLayer<'_> {
    let (s, c) = deg.to_radians().sin_cos();
    let (a, b) = (scale * c, scale * s);
    let (ax, ay) = (img.size.w as f32 / 2.0, img.size.h as f32 / 2.0);
    TextLayer {
        image: img,
        key,
        to_canvas: [a, b, -b, a, cx - a * ax + b * ay, cy - b * ax - a * ay],
        opacity,
    }
}

#[test]
fn cpu_draws_an_unrotated_image_exactly() {
    let canvas = SizePx { w: 64, h: 48 };
    let bg = solid(canvas, [10, 20, 30, 255]);
    let img = solid(SizePx { w: 8, h: 6 }, [200, 100, 50, 255]);
    let layers = [TextLayer {
        image: &img,
        key: 1,
        to_canvas: [1.0, 0.0, 0.0, 1.0, 10.0, 12.0],
        opacity: 1.0,
    }];
    let job = FrameJob {
        text: &layers,
        canvas,
        background: &bg,
        background_key: None,
        screen: None,
        camera: None,
    };
    let out = cpu::render_cpu(&job, OutputFormat::Rgba8).expect("cpu");
    let px = |x: usize, y: usize| out.data[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4].to_vec();
    assert_eq!(px(12, 14), vec![200, 100, 50, 255]);
    assert_eq!(px(9, 14), vec![10, 20, 30, 255]);
    assert_eq!(px(18, 14), vec![10, 20, 30, 255]);
    let half = [TextLayer {
        opacity: 0.5,
        ..layers[0].clone()
    }];
    let job = FrameJob { text: &half, ..job };
    let out = cpu::render_cpu(&job, OutputFormat::Rgba8).expect("cpu");
    let p = out.data[(14 * 64 + 12) * 4..(14 * 64 + 12) * 4 + 3].to_vec();
    assert_eq!(p, vec![105, 60, 40]);
}

#[test]
fn gpu_matches_cpu_for_placed_scaled_rotated_and_faded_text() {
    let Some(mut g) = gpu() else { return };
    let canvas = SizePx { w: 320, h: 180 };
    let pat = pattern(canvas);
    let bg = RgbaImage {
        data: pat
            .data
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        ..pat
    };
    let a = pattern(SizePx { w: 97, h: 41 });
    let b = pattern(SizePx { w: 60, h: 60 });
    let cases: Vec<Vec<TextLayer>> = vec![
        vec![placed(&a, 160.0, 90.0, 1.0, 0.0, 1.0, 1)],
        vec![placed(&a, 100.3, 60.7, 1.37, 0.0, 0.8, 2)],
        vec![placed(&a, 160.0, 90.0, 0.6, 15.0, 1.0, 3)],
        vec![placed(&b, 300.0, 170.0, 1.0, -90.0, 0.5, 4)],
        vec![
            placed(&a, 80.0, 50.0, 1.2, 33.0, 1.0, 5),
            placed(&b, 120.0, 80.0, 0.9, -12.0, 0.7, 6),
        ],
    ];
    for (i, layers) in cases.iter().enumerate() {
        for out in [OutputFormat::Rgba8, OutputFormat::I420Bt709Limited] {
            let job = FrameJob {
                text: layers,
                canvas,
                background: &bg,
                background_key: None,
                screen: None,
                camera: None,
            };
            let want = cpu::render_cpu(&job, out).expect("cpu");
            let got = g.render(&job, out).expect("gpu");
            let d = diff(&want, &got);
            assert!(d.max <= 2 && d.mean < 0.02, "case {i} {out:?}: {d:?}");
        }
    }
}
