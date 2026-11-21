//! Recorder-panel stills (ARCHITECTURE §9), served as `sc://still/<id>.png`.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use sc_core::image::RgbaImage;
use sc_core::project::Background;
use sc_geometry::{Aspect, Ratio, RectPx, SizePx, contain_fit, even_round};
use sc_platform::{CpuNv12Frame, FrameBuffer};
use sc_render::{FrameJob, OutputFormat, ScreenLayer};

pub const STILL_LONG_SIDE: u32 = 960;
const KEEP: usize = 8;

pub fn still_canvas(aspect: Aspect, content: Option<SizePx>) -> SizePx {
    let ratio = aspect
        .ratio()
        .or_else(|| content.filter(|c| c.w > 0 && c.h > 0).map(Ratio::of))
        .unwrap_or(Ratio::new(16, 9));
    let r = ratio.as_f64();
    let long = f64::from(STILL_LONG_SIDE);
    if r >= 1.0 {
        SizePx {
            w: even_round(long),
            h: even_round(long / r),
        }
    } else {
        SizePx {
            w: even_round(long * r),
            h: even_round(long),
        }
    }
}

/// Premultiplied sRGB RGBA → NV12, BT.709 limited range, in the integer maths of the renderer's
/// I420 output. Odd sizes are cropped to even.
pub fn rgba_to_nv12(img: &RgbaImage) -> CpuNv12Frame {
    let w = (img.size.w / 2 * 2).max(2);
    let h = (img.size.h / 2 * 2).max(2);
    let size = SizePx { w, h };
    let mut out = CpuNv12Frame::black(size, false);
    let (wu, hu) = (w as usize, h as usize);
    let (iw, ih) = (img.size.w.max(1) as usize, img.size.h.max(1) as usize);
    let uv_stride = out.uv_stride();
    let mut cols = Vec::with_capacity(wu);
    cols.extend((0..wu).map(|x| x.min(iw - 1) * 4));
    for cy in 0..hu / 2 {
        let rows = [
            (2 * cy).min(ih - 1) * img.stride,
            (2 * cy + 1).min(ih - 1) * img.stride,
        ];
        for cx in 0..wu / 2 {
            let mut sum = [0i32; 3];
            for (dy, row) in rows.iter().enumerate() {
                for dx in 0..2 {
                    let x = 2 * cx + dx;
                    let i = row + cols[x];
                    let (r, g, b) = match img.data.get(i..i + 3) {
                        Some(p) => (i32::from(p[0]), i32::from(p[1]), i32::from(p[2])),
                        None => (0, 0, 0),
                    };
                    let yv = (11_966 * r + 40_254 * g + 4_064 * b + 1_081_344) >> 16;
                    out.y[(2 * cy + dy) * wu + x] = yv.clamp(0, 255) as u8;
                    sum = [sum[0] + r, sum[1] + g, sum[2] + b];
                }
            }
            let u = (-6_596 * sum[0] - 22_189 * sum[1] + 28_785 * sum[2] + 33_685_504) >> 18;
            let v = (28_785 * sum[0] - 26_145 * sum[1] - 2_640 * sum[2] + 33_685_504) >> 18;
            let i = cy * uv_stride + cx * 2;
            out.uv[i] = u.clamp(0, 255) as u8;
            out.uv[i + 1] = v.clamp(0, 255) as u8;
        }
    }
    out
}

pub fn compose_png(
    background: &Background,
    canvas: SizePx,
    project_dir: &Path,
    main: Option<&FrameBuffer>,
    camera: Option<(&sc_core::camera::CameraPlacement, &FrameBuffer)>,
) -> Result<Vec<u8>, String> {
    let bg = sc_text::rasterize_background(background, canvas, project_dir)
        .map_err(|e| format!("still background: {e}"))?;
    let screen = main.map(|f| {
        let size = f.0.size();
        ScreenLayer {
            frame: f,
            src: RectPx {
                x: 0,
                y: 0,
                w: size.w,
                h: size.h,
            },
            dst: contain_fit(canvas, size),
        }
    });
    let camera = camera.map(|(p, f)| {
        let rects = sc_geometry::camera_rects(canvas, f.0.size(), p.shape.aspect(), &p.pose);
        sc_render::camera_layer(p, f, rects.dst)
    });
    let job = FrameJob {
        text: &[],
        background_key: None,
        camera,
        canvas,
        background: &bg,
        screen,
    };
    let out = sc_render::cpu::render_cpu(&job, OutputFormat::Rgba8)
        .map_err(|e| format!("still render: {e}"))?;
    let img = RgbaImage {
        size: out.size,
        stride: out.size.w as usize * 4,
        data: out.data,
    };
    sc_text::encode_png(&img).map_err(|e| format!("still png: {e}"))
}

#[derive(Default)]
pub struct StillCache {
    items: VecDeque<(String, Arc<Vec<u8>>)>,
}

impl StillCache {
    pub fn insert(&mut self, id: String, png: Vec<u8>) {
        self.items.retain(|(k, _)| *k != id);
        self.items.push_back((id, Arc::new(png)));
        while self.items.len() > KEEP {
            self.items.pop_front();
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<Vec<u8>>> {
        self.items
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, v)| v.clone())
    }
}

pub fn still_url(id: &str) -> String {
    if cfg!(windows) {
        format!("http://sc.localhost/still/{id}.png")
    } else {
        format!("sc://still/{id}.png")
    }
}

pub fn id_from_path(host: Option<&str>, path: &str) -> Option<String> {
    let path = path.trim_start_matches('/');
    let rest = match host {
        Some("still") => path,
        _ => path.strip_prefix("still/")?,
    };
    let id = rest.strip_suffix(".png")?;
    let ok = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::project::Rgba;

    #[test]
    fn canvas_sizes() {
        assert_eq!(still_canvas(Aspect::R16x9, None), SizePx { w: 960, h: 540 });
        assert_eq!(still_canvas(Aspect::R9x16, None), SizePx { w: 540, h: 960 });
        assert_eq!(
            still_canvas(Aspect::Original, Some(SizePx { w: 1000, h: 1000 })),
            SizePx { w: 960, h: 960 }
        );
        assert_eq!(
            still_canvas(Aspect::Original, None),
            SizePx { w: 960, h: 540 }
        );
    }

    #[test]
    fn nv12_of_grey_and_red() {
        let mut img = RgbaImage {
            size: SizePx { w: 5, h: 3 },
            stride: 20,
            data: vec![128; 60],
        };
        for p in img.data.chunks_exact_mut(4) {
            p[3] = 255;
        }
        let f = rgba_to_nv12(&img);
        assert_eq!(f.size, SizePx { w: 4, h: 2 });
        assert!(f.y.iter().all(|&v| (125..=127).contains(&v)), "{:?}", f.y);
        assert!(f.uv.iter().all(|&v| v == 128));
        for p in img.data.chunks_exact_mut(4) {
            p.copy_from_slice(&[255, 0, 0, 255]);
        }
        let f = rgba_to_nv12(&img);
        assert_eq!(f.y[0], 63);
        assert_eq!(f.uv[0], 102);
        assert_eq!(f.uv[1], 240);
    }

    #[test]
    fn composes_a_png() {
        let bg = Background::Solid {
            color: Rgba([0.0, 0.0, 1.0, 1.0]),
        };
        let snap = RgbaImage {
            size: SizePx { w: 64, h: 64 },
            stride: 256,
            data: vec![255; 64 * 64 * 4],
        };
        let canvas = still_canvas(Aspect::R16x9, None);
        let frame = FrameBuffer(Arc::new(rgba_to_nv12(&snap)));
        let png =
            compose_png(&bg, canvas, Path::new("/nonexistent"), Some(&frame), None).expect("png");
        let img = sc_text::decode_image(&png).expect("decode");
        assert_eq!(img.size, canvas);
        let at = |x: usize, y: usize| &img.data[y * img.stride + x * 4..y * img.stride + x * 4 + 4];
        assert!(
            at(5, 270)[2] > 200 && at(5, 270)[0] < 30,
            "edge is background"
        );
        assert!(at(480, 270)[0] > 200, "centre is the snapshot");
        let plain = compose_png(&bg, canvas, Path::new("/nonexistent"), None, None).expect("png");
        assert!(plain.starts_with(b"\x89PNG"));
    }

    #[test]
    fn cache_and_paths() {
        let mut c = StillCache::default();
        for i in 0..10 {
            c.insert(format!("s{i}"), vec![i]);
        }
        assert!(c.get("s0").is_none());
        assert_eq!(c.get("s9").as_deref(), Some(&vec![9]));
        assert_eq!(
            id_from_path(Some("still"), "/abc-1.png"),
            Some("abc-1".into())
        );
        assert_eq!(
            id_from_path(Some("localhost"), "/still/abc.png"),
            Some("abc".into())
        );
        assert_eq!(id_from_path(Some("still"), "/../x.png"), None);
        assert_eq!(id_from_path(Some("other"), "/abc.png"), None);
        assert!(still_url("x").ends_with("still/x.png"));
    }
}
