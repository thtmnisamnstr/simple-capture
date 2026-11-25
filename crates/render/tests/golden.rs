//! GPU vs CPU golden tests. Without a wgpu adapter the GPU halves print `SKIPPED:` and pass,
//! unless `SC_REQUIRE_GPU=1`.

use std::sync::Arc;
use std::time::Instant;

use sc_core::image::RgbaImage;
use sc_geometry::{Aspect, Ratio, RectPx, SizePx, contain_fit, even_round, reference_canvas_size};
use sc_platform::{CpuNv12Frame, FrameBuffer};
use sc_render::{
    Backend, Compositor, DiffStats, FrameJob, OutputFormat, Redact, RenderedFrame, ScreenLayer,
    cpu, diff,
};

fn sz(w: u32, h: u32) -> SizePx {
    SizePx { w, h }
}

fn gpu() -> Option<Compositor> {
    let require = std::env::var("SC_REQUIRE_GPU").is_ok_and(|v| v == "1");
    let c = match Compositor::new(Backend::Auto) {
        Ok(c) => c,
        Err(e) if require => panic!("SC_REQUIRE_GPU=1 but Compositor::new failed: {e}"),
        Err(e) => {
            println!("SKIPPED: Compositor::new failed: {e}");
            return None;
        }
    };
    match c.adapter() {
        Some(a) => {
            println!("adapter: {a:?}");
            Some(c)
        }
        None if require => panic!("SC_REQUIRE_GPU=1 but no wgpu adapter"),
        None => {
            println!("SKIPPED: no wgpu adapter");
            None
        }
    }
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u8 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 56) as u8
    }
}

/// Gradients, a fine checkerboard, one-pixel lines, hard colour edges and noise, so the box
/// prefilter, the bilinear step and chroma handling are all exercised.
fn pattern(size: SizePx, full_range: bool, seed: u64) -> CpuNv12Frame {
    let (w, h) = (size.w as usize, size.h as usize);
    let mut rng = Lcg(seed);
    let mut y = vec![0u8; w * h];
    for r in 0..h {
        for c in 0..w {
            y[r * w + c] = match (c * 4 / w, r * 3 / h) {
                (0, _) => (16 + (c * 219) / w.max(1)) as u8,
                (1, _) => {
                    if (c / 2 + r / 2) % 2 == 0 {
                        30
                    } else {
                        220
                    }
                }
                (2, 0) => {
                    if c % 7 == 0 || r % 5 == 0 {
                        235
                    } else {
                        16
                    }
                }
                (2, _) => (16 + (r * 219) / h.max(1)) as u8,
                _ => rng.next(),
            };
        }
    }
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let mut uv = vec![0u8; 2 * cw * ch];
    for r in 0..ch {
        for c in 0..cw {
            let (u, v) = if c * 3 < cw {
                (
                    (64 + (c * 128) / cw) as u8,
                    (192 - (r * 128) / ch.max(1)) as u8,
                )
            } else if c * 3 < 2 * cw {
                if (c / 8 + r / 8) % 2 == 0 {
                    (40, 220)
                } else {
                    (220, 40)
                }
            } else {
                (rng.next(), rng.next())
            };
            uv[2 * (r * cw + c)] = u;
            uv[2 * (r * cw + c) + 1] = v;
        }
    }
    CpuNv12Frame {
        size,
        y,
        uv,
        full_range,
    }
}

fn background(size: SizePx) -> RgbaImage {
    let (w, h) = (size.w as usize, size.h as usize);
    let stride = w * 4 + 64;
    let mut data = vec![0u8; stride * h];
    for r in 0..h {
        for c in 0..w {
            let o = r * stride + c * 4;
            data[o] = (40 + c * 150 / w.max(1)) as u8;
            data[o + 1] = (30 + r * 100 / h.max(1)) as u8;
            data[o + 2] = 120;
            data[o + 3] = 255;
        }
    }
    RgbaImage { size, stride, data }
}

fn buffer(f: CpuNv12Frame) -> FrameBuffer {
    FrameBuffer(Arc::new(f))
}

fn compare(
    gpu: &mut Compositor,
    job: &FrameJob,
    out: OutputFormat,
    label: &str,
) -> (DiffStats, RenderedFrame) {
    let software = gpu.adapter().is_some_and(|a| a.software);
    let g = gpu.render(job, out).expect("gpu render");
    let c = cpu::render_cpu(job, out).expect("cpu render");
    assert_eq!(g.size, job.canvas);
    assert_eq!(g.data.len(), c.data.len(), "{label}: packed length");
    let s = diff(&g, &c);
    println!("{label} {out:?}: max {} mean {:.5}", s.max, s.mean);
    let max = if software { 3 } else { 2 };
    assert!(s.max <= max, "{label} {out:?}: max diff {} > {max}", s.max);
    if !software {
        assert!(
            s.mean <= 0.01,
            "{label} {out:?}: mean diff {} > 0.01",
            s.mean
        );
    }
    (s, c)
}

const ASPECTS: [Aspect; 6] = [
    Aspect::Original,
    Aspect::R16x9,
    Aspect::R4x3,
    Aspect::R1x1,
    Aspect::R9x16,
    Aspect::R4x5,
];
const SCALES: [f64; 5] = [1.0, 0.5, 0.25, 0.140625, 2.0];

#[test]
fn gpu_matches_cpu_every_aspect_and_scale() {
    let Some(mut gpu) = gpu() else { return };
    let content = sz(1280, 800);
    let frame = buffer(pattern(sz(1300, 822), false, 7));
    let src = RectPx {
        x: 10,
        y: 12,
        w: content.w,
        h: content.h,
    };
    let mut worst = DiffStats::default();
    for aspect in ASPECTS {
        let ratio = aspect.ratio().unwrap_or(Ratio::of(content));
        let reference = reference_canvas_size(aspect, ratio, &[content]);
        for scale in SCALES {
            let canvas = sz(
                even_round(f64::from(reference.w) * scale),
                even_round(f64::from(reference.h) * scale),
            );
            let dst = contain_fit(canvas, content);
            let bg = background(canvas);
            let job = FrameJob {
                text: &[],
                background_key: None,
                camera: None,
                canvas,
                background: &bg,
                screen: Some(ScreenLayer {
                    frame: &frame,
                    src,
                    dst,
                    redact: &[],
                }),
            };
            let label = format!(
                "{} x{scale} {}x{} dst {dst:?}",
                aspect.as_str(),
                canvas.w,
                canvas.h
            );
            for out in [OutputFormat::Rgba8, OutputFormat::I420Bt709Limited] {
                let (s, _) = compare(&mut gpu, &job, out, &label);
                worst.max = worst.max.max(s.max);
                worst.mean = worst.mean.max(s.mean);
            }
        }
    }
    println!(
        "worst over all cases: max {} mean {:.5}",
        worst.max, worst.mean
    );
}

#[test]
fn worked_examples_7680x2160() {
    let content = sz(7680, 2160);
    assert_eq!(
        contain_fit(sz(1920, 1080), content),
        RectPx {
            x: 0,
            y: 270,
            w: 1920,
            h: 540
        }
    );
    assert_eq!(
        contain_fit(sz(1080, 1920), content),
        RectPx {
            x: 0,
            y: 808,
            w: 1080,
            h: 304
        }
    );
    let Some(mut gpu) = gpu() else { return };
    let frame = buffer(pattern(content, false, 11));
    for canvas in [sz(1920, 1080), sz(1080, 1920), sz(3840, 2160)] {
        let bg = background(canvas);
        let job = FrameJob {
            text: &[],
            background_key: None,
            camera: None,
            canvas,
            background: &bg,
            screen: Some(ScreenLayer {
                frame: &frame,
                src: RectPx {
                    x: 0,
                    y: 0,
                    w: content.w,
                    h: content.h,
                },
                dst: contain_fit(canvas, content),
                redact: &[],
            }),
        };
        for out in [OutputFormat::Rgba8, OutputFormat::I420Bt709Limited] {
            compare(
                &mut gpu,
                &job,
                out,
                &format!("7680x2160 → {}x{}", canvas.w, canvas.h),
            );
        }
    }
}

#[test]
fn full_range_input_matches() {
    let Some(mut gpu) = gpu() else { return };
    let frame = buffer(pattern(sz(641, 361), true, 3));
    let canvas = sz(480, 270);
    let bg = background(canvas);
    let job = FrameJob {
        text: &[],
        background_key: None,
        camera: None,
        canvas,
        background: &bg,
        screen: Some(ScreenLayer {
            frame: &frame,
            src: RectPx {
                x: 0,
                y: 0,
                w: 641,
                h: 361,
            },
            dst: contain_fit(canvas, sz(641, 361)),
            redact: &[],
        }),
    };
    compare(&mut gpu, &job, OutputFormat::Rgba8, "full range 641x361");
}

#[test]
fn background_only_is_copied_exactly() {
    let canvas = sz(322, 180);
    let bg = background(canvas);
    let job = FrameJob {
        text: &[],
        background_key: None,
        camera: None,
        canvas,
        background: &bg,
        screen: None,
    };
    let c = cpu::render_cpu(&job, OutputFormat::Rgba8).expect("cpu");
    for r in 0..canvas.h as usize {
        let row = canvas.w as usize * 4;
        assert_eq!(
            &c.data[r * row..(r + 1) * row],
            &bg.data[r * bg.stride..r * bg.stride + row]
        );
    }
    let Some(mut gpu) = gpu() else { return };
    let g = gpu.render(&job, OutputFormat::Rgba8).expect("gpu");
    assert_eq!(diff(&g, &c).max, 0);
}

/// In-gamut NV12 whose chroma is smooth, rendered 1:1 to I420.
fn round_trip(r: &mut dyn FnMut(&FrameJob) -> RenderedFrame, label: &str) {
    let size = sz(320, 180);
    let (w, h) = (size.w as usize, size.h as usize);
    let (cw, ch) = (w / 2, h / 2);
    let mut rng = Lcg(5);
    let y: Vec<u8> = (0..w * h).map(|_| 60 + rng.next() % 121).collect();
    let mut uv = vec![0u8; 2 * cw * ch];
    for i in 0..cw * ch {
        uv[2 * i] = 108 + rng.next() % 41;
        uv[2 * i + 1] = 108 + rng.next() % 41;
    }
    let src = CpuNv12Frame {
        size,
        y: y.clone(),
        uv: uv.clone(),
        full_range: false,
    };
    let frame = buffer(src);
    let bg = background(size);
    let job = FrameJob {
        text: &[],
        background_key: None,
        camera: None,
        canvas: size,
        background: &bg,
        screen: Some(ScreenLayer {
            frame: &frame,
            src: RectPx {
                x: 0,
                y: 0,
                w: size.w,
                h: size.h,
            },
            dst: RectPx {
                x: 0,
                y: 0,
                w: size.w,
                h: size.h,
            },
            redact: &[],
        }),
    };
    let out = r(&job);
    assert_eq!(out.data.len(), w * h * 3 / 2);
    let (oy, rest) = out.data.split_at(w * h);
    let (ou, ov) = rest.split_at(cw * ch);
    let max_y = oy
        .iter()
        .zip(&y)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    let max_u = (0..cw * ch)
        .map(|i| ou[i].abs_diff(uv[2 * i]))
        .max()
        .unwrap_or(0);
    let max_v = (0..cw * ch)
        .map(|i| ov[i].abs_diff(uv[2 * i + 1]))
        .max()
        .unwrap_or(0);
    println!("{label} YUV→RGB→YUV max diff: Y {max_y} U {max_u} V {max_v}");
    assert!(
        max_y <= 2 && max_u <= 2 && max_v <= 2,
        "{label}: round trip off by more than 2"
    );
}

#[test]
fn yuv_rgb_yuv_round_trip_cpu() {
    round_trip(
        &mut |job| cpu::render_cpu(job, OutputFormat::I420Bt709Limited).expect("cpu"),
        "cpu",
    );
}

#[test]
fn yuv_rgb_yuv_round_trip_gpu() {
    let Some(mut gpu) = gpu() else { return };
    round_trip(
        &mut |job| {
            gpu.render(job, OutputFormat::I420Bt709Limited)
                .expect("gpu")
        },
        "gpu",
    );
}

/// Every pixel outside `dst` is background and every pixel inside is screen.
fn check_letterbox(
    out: &RenderedFrame,
    bg: &RgbaImage,
    dst: RectPx,
    screen_rgb: [u8; 3],
    label: &str,
) {
    let (w, h) = (out.size.w as i32, out.size.h as i32);
    for y in 0..h {
        for x in 0..w {
            let o = ((y * w + x) * 4) as usize;
            let px = &out.data[o..o + 4];
            let inside =
                x >= dst.x && x < dst.x + dst.w as i32 && y >= dst.y && y < dst.y + dst.h as i32;
            if inside {
                assert_eq!(
                    &px[..3],
                    &screen_rgb,
                    "{label}: ({x},{y}) inside dst {dst:?}"
                );
                assert_eq!(px[3], 255);
            } else {
                let b = y as usize * bg.stride + x as usize * 4;
                assert_eq!(
                    px,
                    &bg.data[b..b + 4],
                    "{label}: ({x},{y}) outside dst {dst:?}"
                );
            }
        }
    }
}

fn letterbox_cases() -> Vec<(SizePx, SizePx)> {
    vec![
        (sz(640, 360), sz(1000, 300)),
        (sz(1280, 720), sz(300, 200)),
        (sz(1080, 1920), sz(7680, 2160)),
        (sz(500, 500), sz(333, 777)),
    ]
}

fn letterbox(r: &mut dyn FnMut(&FrameJob) -> RenderedFrame, label: &str) {
    for (canvas, content) in letterbox_cases() {
        let frame = buffer(CpuNv12Frame::filled(content, 140, 90, 170, false));
        let bg = background(canvas);
        let dst = contain_fit(canvas, content);
        let job = FrameJob {
            text: &[],
            background_key: None,
            camera: None,
            canvas,
            background: &bg,
            screen: Some(ScreenLayer {
                frame: &frame,
                src: RectPx {
                    x: 0,
                    y: 0,
                    w: content.w,
                    h: content.h,
                },
                dst,
                redact: &[],
            }),
        };
        let one = FrameJob {
            text: &[],
            background_key: None,
            camera: None,
            canvas: sz(2, 2),
            background: &background(sz(2, 2)),
            screen: Some(ScreenLayer {
                frame: &frame,
                src: RectPx {
                    x: 0,
                    y: 0,
                    w: 2,
                    h: 2,
                },
                dst: RectPx {
                    x: 0,
                    y: 0,
                    w: 2,
                    h: 2,
                },
                redact: &[],
            }),
        };
        let reference = cpu::render_cpu(&one, OutputFormat::Rgba8).expect("cpu");
        let rgb = [reference.data[0], reference.data[1], reference.data[2]];
        let out = r(&job);
        check_letterbox(
            &out,
            &bg,
            dst,
            rgb,
            &format!("{label} {content:?} in {canvas:?}"),
        );
    }
}

#[test]
fn letterbox_edges_exactly_at_dst_cpu() {
    letterbox(
        &mut |job| cpu::render_cpu(job, OutputFormat::Rgba8).expect("cpu"),
        "cpu",
    );
}

#[test]
fn letterbox_edges_exactly_at_dst_gpu() {
    let Some(mut gpu) = gpu() else { return };
    letterbox(
        &mut |job| gpu.render(job, OutputFormat::Rgba8).expect("gpu"),
        "gpu",
    );
}

#[test]
fn cpu_only_backend_renders_the_reference() {
    let mut c = Compositor::new(Backend::CpuOnly).expect("cpu only");
    assert!(c.adapter().is_none());
    let canvas = sz(320, 180);
    let frame = buffer(pattern(sz(640, 400), false, 1));
    let bg = background(canvas);
    let job = FrameJob {
        text: &[],
        background_key: None,
        camera: None,
        canvas,
        background: &bg,
        screen: Some(ScreenLayer {
            frame: &frame,
            src: RectPx {
                x: 0,
                y: 0,
                w: 640,
                h: 400,
            },
            dst: contain_fit(canvas, sz(640, 400)),
            redact: &[],
        }),
    };
    let a = c
        .render(&job, OutputFormat::I420Bt709Limited)
        .expect("render");
    let b = cpu::render_cpu(&job, OutputFormat::I420Bt709Limited).expect("cpu");
    assert_eq!(a, b);
}

#[test]
fn invalid_jobs_are_errors() {
    let bg = background(sz(10, 10));
    let odd = FrameJob {
        text: &[],
        background_key: None,
        camera: None,
        canvas: sz(11, 10),
        background: &bg,
        screen: None,
    };
    assert!(
        cpu::render_cpu(&odd, OutputFormat::Rgba8).is_err(),
        "background size mismatch"
    );
    let bg = background(sz(11, 10));
    let odd = FrameJob {
        text: &[],
        background_key: None,
        camera: None,
        canvas: sz(11, 10),
        background: &bg,
        screen: None,
    };
    assert!(cpu::render_cpu(&odd, OutputFormat::Rgba8).is_ok());
    assert!(
        cpu::render_cpu(&odd, OutputFormat::I420Bt709Limited).is_err(),
        "odd I420"
    );
    if let Some(mut gpu) = gpu() {
        assert!(gpu.render(&odd, OutputFormat::I420Bt709Limited).is_err());
        assert!(gpu.render(&odd, OutputFormat::Rgba8).is_ok());
    }
}

/// `cargo test --release -p simple-capture-render --test golden -- --ignored --nocapture bench`
#[test]
#[ignore = "benchmark"]
fn bench_render_readback() {
    let Some(mut gpu) = gpu() else { return };
    let cases = [
        ("1080p 1:1", sz(1920, 1080), sz(1920, 1080)),
        ("1080p from 3840x2160", sz(1920, 1080), sz(3840, 2160)),
        ("2160p 1:1", sz(3840, 2160), sz(3840, 2160)),
        ("2160p from 7680x2160", sz(3840, 2160), sz(7680, 2160)),
    ];
    for (label, canvas, content) in cases {
        let frame = buffer(pattern(content, false, 9));
        let bg = background(canvas);
        let job = FrameJob {
            text: &[],
            background_key: None,
            camera: None,
            canvas,
            background: &bg,
            screen: Some(ScreenLayer {
                frame: &frame,
                src: RectPx {
                    x: 0,
                    y: 0,
                    w: content.w,
                    h: content.h,
                },
                dst: contain_fit(canvas, content),
                redact: &[],
            }),
        };
        for out in [OutputFormat::Rgba8, OutputFormat::I420Bt709Limited] {
            for _ in 0..5 {
                gpu.render(&job, out).expect("warm");
            }
            let mut ms: Vec<f64> = (0..30)
                .map(|_| {
                    let t = Instant::now();
                    gpu.render(&job, out).expect("render");
                    t.elapsed().as_secs_f64() * 1e3
                })
                .collect();
            ms.sort_by(f64::total_cmp);
            let t = Instant::now();
            cpu::render_cpu(&job, out).expect("cpu");
            let cpu_ms = t.elapsed().as_secs_f64() * 1e3;
            println!(
                "BENCH {label} {out:?}: gpu render+readback median {:.2} ms (min {:.2}, max {:.2}); cpu reference {:.1} ms",
                ms[15], ms[0], ms[29], cpu_ms
            );
        }
    }
}

/// The integer filter stays within 1 code value of an exact float box average when the ratio
/// is an integer (box only, bilinear lands on texel centres).
#[test]
fn integer_box_matches_float_average() {
    let content = sz(1024, 512);
    let f = pattern(content, false, 21);
    let (yp, uvp) = (f.y.clone(), f.uv.clone());
    let frame = buffer(f);
    let canvas = sz(256, 128);
    let bg = background(canvas);
    let job = FrameJob {
        text: &[],
        background_key: None,
        camera: None,
        canvas,
        background: &bg,
        screen: Some(ScreenLayer {
            frame: &frame,
            src: RectPx {
                x: 0,
                y: 0,
                w: content.w,
                h: content.h,
            },
            dst: RectPx {
                x: 0,
                y: 0,
                w: canvas.w,
                h: canvas.h,
            },
            redact: &[],
        }),
    };
    let out = cpu::render_cpu(&job, OutputFormat::Rgba8).expect("cpu");
    let w = content.w as usize;
    let rgb = |x: usize, y: usize| {
        let yy = f64::from(yp[y * w + x]) - 16.0;
        let ci = (y / 2) * w + (x / 2) * 2;
        let (u, v) = (f64::from(uvp[ci]) - 128.0, f64::from(uvp[ci + 1]) - 128.0);
        let l = 255.0 / 219.0 * yy;
        let k = 255.0 / 224.0;
        [
            l + k * 1.5748 * v,
            l - k * (0.1873 * u + 0.4681 * v),
            l + k * 1.8556 * u,
        ]
        .map(|c: f64| c.clamp(0.0, 255.0).round())
    };
    let mut max = 0.0f64;
    for oy in 0..canvas.h as usize {
        for ox in 0..canvas.w as usize {
            let mut s = [0.0; 3];
            for dy in 0..4 {
                for dx in 0..4 {
                    let p = rgb(ox * 4 + dx, oy * 4 + dy);
                    (0..3).for_each(|c| s[c] += p[c]);
                }
            }
            let o = (oy * canvas.w as usize + ox) * 4;
            for (got, sum) in out.data[o..o + 3].iter().zip(s) {
                max = max.max((f64::from(*got) - sum / 16.0).abs());
            }
        }
    }
    println!("integer filter vs float box average: max {max:.3}");
    assert!(
        max <= 1.5,
        "integer filter drifts {max} from the float average"
    );
}

#[test]
fn compositor_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Compositor>();
    assert!(Backend::parse("METAL") == Some(Backend::Metal));
    assert!(Backend::parse("cpu") == Some(Backend::CpuOnly));
    assert!(Backend::parse("opengl-ish").is_none());
}

#[test]
fn redaction_hides_the_boxes_the_same_on_gpu_and_cpu() {
    let frame = buffer(pattern(sz(1280, 720), false, 11));
    let canvas = sz(960, 540);
    let bg = background(canvas);
    let boxes = [
        Redact {
            rect: [0.1, 0.1, 0.4, 0.3],
            black: false,
        },
        Redact {
            rect: [0.35, 0.3, 0.3, 0.3],
            black: true,
        },
        Redact {
            rect: [0.9, 0.85, 0.5, 0.5],
            black: false,
        },
    ];
    let layer = |redact| ScreenLayer {
        frame: &frame,
        src: RectPx {
            x: 0,
            y: 0,
            w: 1280,
            h: 720,
        },
        dst: contain_fit(canvas, sz(1280, 720)),
        redact,
    };
    let job = |redact| FrameJob {
        text: &[],
        background_key: None,
        camera: None,
        canvas,
        background: &bg,
        screen: Some(layer(redact)),
    };
    let plain = cpu::render_cpu(&job(&[]), OutputFormat::Rgba8).expect("plain");
    let hidden = cpu::render_cpu(&job(&boxes), OutputFormat::Rgba8).expect("redacted");
    let at = |f: &RenderedFrame, x: u32, y: u32| {
        let i = ((y * canvas.w + x) * 4) as usize;
        [f.data[i], f.data[i + 1], f.data[i + 2]]
    };
    assert_eq!(at(&hidden, 480, 270), [0, 0, 0], "inside the black box");
    assert_eq!(at(&hidden, 20, 20), at(&plain, 20, 20), "outside every box");
    let row = |f: &RenderedFrame, y: u32| (100..380).map(|x| at(f, x, y)).collect::<Vec<_>>();
    let distinct = |v: Vec<[u8; 3]>| {
        let mut v = v;
        v.dedup();
        v.len()
    };
    assert!(
        distinct(row(&hidden, 80)) * 4 < distinct(row(&plain, 80)),
        "the mosaic flattens detail: {} vs {}",
        distinct(row(&hidden, 80)),
        distinct(row(&plain, 80))
    );
    if let Some(mut gpu) = gpu() {
        compare(&mut gpu, &job(&boxes), OutputFormat::Rgba8, "redacted");
        compare(
            &mut gpu,
            &job(&boxes),
            OutputFormat::I420Bt709Limited,
            "redacted",
        );
        compare(&mut gpu, &job(&[]), OutputFormat::Rgba8, "after redaction");
    }
}
