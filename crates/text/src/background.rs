use std::collections::VecDeque;
use std::fmt;
use std::path::{Component, Path};
use std::sync::Arc;

use sc_core::image::RgbaImage;
use sc_core::project::{Background, Fit, GradientStop, Rgba};
use sc_core::{Error, Result};
use sc_geometry::{PointF, SizePx, cover_src_rect};
use skia_safe::canvas::SrcRectConstraint;
use skia_safe::gradient::{Colors, Gradient, Interpolation, shaders};
use skia_safe::{
    Canvas, Color4f, CubicResampler, Data, FilterMode, Image, MipmapMode, Paint, Point, Rect,
    SamplingOptions, TileMode,
};

use crate::raster::{blank, rgba_info};

/// LRU of rasterized backgrounds keyed by (stable hash of Background, w, h).
pub struct BackgroundCache {
    capacity: usize,
    entries: VecDeque<Entry>,
}

struct Entry {
    key: u64,
    size: SizePx,
    bg: Background,
    image: Arc<RgbaImage>,
}

impl fmt::Debug for BackgroundCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackgroundCache")
            .field("capacity", &self.capacity)
            .field("len", &self.entries.len())
            .finish()
    }
}

impl BackgroundCache {
    /// A capacity of 0 is treated as 1.
    pub fn new(capacity: usize) -> BackgroundCache {
        let capacity = capacity.max(1);
        BackgroundCache {
            capacity,
            entries: VecDeque::with_capacity(capacity),
        }
    }

    pub fn get(
        &mut self,
        bg: &Background,
        size: SizePx,
        project_dir: &Path,
    ) -> Result<Arc<RgbaImage>> {
        let key = background_hash(bg);
        let hit = self
            .entries
            .iter()
            .position(|e| e.key == key && e.size == size && e.bg == *bg);
        if let Some(i) = hit
            && let Some(entry) = self.entries.remove(i)
        {
            let image = Arc::clone(&entry.image);
            self.entries.push_front(entry);
            return Ok(image);
        }
        let image = Arc::new(rasterize_background(bg, size, project_dir)?);
        self.entries.push_front(Entry {
            key,
            size,
            bg: bg.clone(),
            image: Arc::clone(&image),
        });
        self.entries.truncate(self.capacity);
        Ok(image)
    }
}

/// Rasterizes `bg` to an opaque, premultiplied RGBA8 sRGB image of `size`. Image assets are
/// read relative to `project_dir`.
pub fn rasterize_background(
    bg: &Background,
    size: SizePx,
    project_dir: &Path,
) -> Result<RgbaImage> {
    let mut out = blank(size)?;
    let info = rgba_info(size)?;
    let stride = out.stride;
    let canvas = Canvas::from_raster_direct(&info, &mut out.data, Some(stride), None)
        .ok_or_else(|| Error::Other(format!("Skia rejected a {}x{} raster", size.w, size.h)))?;
    canvas.clear(Color4f::new(0.0, 0.0, 0.0, 1.0));
    let (w, h) = (size.w as f32, size.h as f32);
    match bg {
        Background::Solid { color } => {
            canvas.clear(opaque(color));
        }
        Background::Linear { stops, angle_deg } => {
            let (colors, pos) = stop_arrays(stops)?;
            let (p0, p1) = linear_endpoints(size, *angle_deg);
            let grad = Gradient::new(
                Colors::new(&colors, Some(&pos), TileMode::Clamp, None),
                Interpolation::default(),
            );
            let shader = shaders::linear_gradient((p0, p1), &grad, None)
                .ok_or_else(|| Error::Other("Skia could not build the linear gradient".into()))?;
            fill(&canvas, shader);
        }
        Background::Radial {
            stops,
            center_norm,
            radius_norm,
        } => {
            let (colors, pos) = stop_arrays(stops)?;
            let center = Point::new(
                finite_or(center_norm.x, 0.5) as f32 * w,
                finite_or(center_norm.y, 0.5) as f32 * h,
            );
            let radius =
                (finite_or(*radius_norm, 0.0) * f64::from(w).hypot(f64::from(h)) / 2.0) as f32;
            if radius > 0.0 {
                let grad = Gradient::new(
                    Colors::new(&colors, Some(&pos), TileMode::Clamp, None),
                    Interpolation::default(),
                );
                let shader =
                    shaders::radial_gradient((center, radius), &grad, None).ok_or_else(|| {
                        Error::Other("Skia could not build the radial gradient".into())
                    })?;
                fill(&canvas, shader);
            } else if let Some(last) = colors.last() {
                canvas.clear(*last);
            }
        }
        Background::Image {
            asset,
            fit,
            focus_norm,
            ..
        } => {
            let image = load_asset(project_dir, asset)?;
            draw_image(&canvas, &image, size, *fit, *focus_norm);
        }
    }
    drop(canvas);
    Ok(out)
}

fn fill(canvas: &Canvas, shader: skia_safe::Shader) {
    let mut paint = Paint::default();
    paint.set_shader(shader);
    paint.set_dither(true);
    canvas.draw_paint(&paint);
}

fn finite_or(v: f64, default: f64) -> f64 {
    if v.is_finite() { v } else { default }
}

fn unit(v: f64) -> f32 {
    finite_or(v, 0.0).clamp(0.0, 1.0) as f32
}

fn opaque(c: &Rgba) -> Color4f {
    Color4f::new(unit(c.0[0]), unit(c.0[1]), unit(c.0[2]), 1.0)
}

/// Stops sorted by position (stable, so equal positions keep their order), positions clamped to
/// 0..1, colours made opaque.
fn stop_arrays(stops: &[GradientStop]) -> Result<(Vec<Color4f>, Vec<f32>)> {
    if !(2..=4).contains(&stops.len()) {
        return Err(Error::Invalid(format!(
            "gradient background needs 2-4 stops, got {}",
            stops.len()
        )));
    }
    let mut sorted: Vec<(f32, Color4f)> = stops
        .iter()
        .map(|s| (unit(s.at), opaque(&s.color)))
        .collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(sorted.into_iter().map(|(p, c)| (c, p)).unzip())
}

/// CSS `linear-gradient(<angle>deg)` geometry: 0° points up, angles turn clockwise, and the
/// gradient line through the centre is long enough that the corners in its direction get the
/// first and last stop exactly.
pub(crate) fn linear_endpoints(size: SizePx, angle_deg: f64) -> (Point, Point) {
    let a = finite_or(angle_deg, 180.0).to_radians();
    let (w, h) = (f64::from(size.w), f64::from(size.h));
    let (dx, dy) = (a.sin(), -a.cos());
    let half = (w * dx.abs() + h * dy.abs()) / 2.0;
    let (cx, cy) = (w / 2.0, h / 2.0);
    (
        Point::new((cx - dx * half) as f32, (cy - dy * half) as f32),
        Point::new((cx + dx * half) as f32, (cy + dy * half) as f32),
    )
}

fn load_asset(project_dir: &Path, asset: &str) -> Result<Image> {
    let rel = Path::new(asset);
    if asset.is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(Error::Invalid(format!(
            "background asset path {asset:?} must be relative to the project, without '..'"
        )));
    }
    let path = project_dir.join(rel);
    let bytes = std::fs::read(&path).map_err(|e| {
        Error::Io(std::io::Error::new(
            e.kind(),
            format!("read background image {}: {e}", path.display()),
        ))
    })?;
    Image::from_encoded(Data::new_copy(&bytes)).ok_or_else(|| {
        Error::Invalid(format!(
            "background image {} is not a PNG or JPEG this build decodes",
            path.display()
        ))
    })
}

fn draw_image(canvas: &Canvas, image: &Image, size: SizePx, fit: Fit, focus: PointF) {
    let img_size = SizePx {
        w: image.width().max(0) as u32,
        h: image.height().max(0) as u32,
    };
    let (w, h) = (size.w as f32, size.h as f32);
    let paint = Paint::default();
    match fit {
        Fit::Cover => {
            let src = cover_src_rect(img_size, size, focus);
            let src = Rect::from_xywh(src.x as f32, src.y as f32, src.w as f32, src.h as f32);
            let dst = Rect::from_wh(w, h);
            canvas.draw_image_rect_with_sampling_options(
                image,
                Some((&src, SrcRectConstraint::Strict)),
                dst,
                sampling(dst.width() / src.width()),
                &paint,
            );
        }
        Fit::Contain => {
            let (iw, ih) = (img_size.w as f32, img_size.h as f32);
            let s = (w / iw).min(h / ih);
            let (dw, dh) = (iw * s, ih * s);
            let dst = Rect::from_xywh((w - dw) / 2.0, (h - dh) / 2.0, dw, dh);
            canvas.draw_image_rect_with_sampling_options(image, None, dst, sampling(s), &paint);
        }
    }
}

/// Mipmapped trilinear when shrinking (cubic aliases there), Mitchell cubic when enlarging.
fn sampling(scale: f32) -> SamplingOptions {
    if scale < 1.0 {
        SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear)
    } else {
        SamplingOptions::from(CubicResampler::mitchell())
    }
}

/// FNV-1a 64 over a fixed encoding of the fields that affect pixels (not `extra`); stable across
/// processes and Rust versions, unlike `DefaultHasher`.
pub(crate) fn background_hash(bg: &Background) -> u64 {
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    match bg {
        Background::Solid { color } => {
            h.byte(1);
            h.rgba(color);
        }
        Background::Linear { stops, angle_deg } => {
            h.byte(2);
            h.stops(stops);
            h.f64(*angle_deg);
        }
        Background::Radial {
            stops,
            center_norm,
            radius_norm,
        } => {
            h.byte(3);
            h.stops(stops);
            h.f64(center_norm.x);
            h.f64(center_norm.y);
            h.f64(*radius_norm);
        }
        Background::Image {
            asset,
            sha256,
            fit,
            focus_norm,
        } => {
            h.byte(4);
            h.str(asset);
            h.str(sha256);
            h.byte(match fit {
                Fit::Contain => 0,
                Fit::Cover => 1,
            });
            h.f64(focus_norm.x);
            h.f64(focus_norm.y);
        }
    }
    h.0
}

struct Fnv(u64);

impl Fnv {
    fn byte(&mut self, b: u8) {
        self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    fn bytes(&mut self, bs: &[u8]) {
        for &b in bs {
            self.byte(b);
        }
    }
    fn f64(&mut self, v: f64) {
        self.bytes(&v.to_bits().to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.bytes(&(s.len() as u64).to_le_bytes());
        self.bytes(s.as_bytes());
    }
    fn rgba(&mut self, c: &Rgba) {
        for v in c.0 {
            self.f64(v);
        }
    }
    fn stops(&mut self, stops: &[GradientStop]) {
        self.bytes(&(stops.len() as u64).to_le_bytes());
        for s in stops {
            self.f64(s.at);
            self.rgba(&s.color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sz(w: u32, h: u32) -> SizePx {
        SizePx { w, h }
    }

    fn close(p: Point, x: f32, y: f32) -> bool {
        (p.x - x).abs() < 1e-3 && (p.y - y).abs() < 1e-3
    }

    #[test]
    fn linear_endpoints_follow_css_angles() {
        let s = sz(1920, 1080);
        let (a, b) = linear_endpoints(s, 180.0);
        assert!(
            close(a, 960.0, 0.0) && close(b, 960.0, 1080.0),
            "{a:?} {b:?}"
        );
        let (a, b) = linear_endpoints(s, 90.0);
        assert!(
            close(a, 0.0, 540.0) && close(b, 1920.0, 540.0),
            "{a:?} {b:?}"
        );
        let (a, b) = linear_endpoints(s, 0.0);
        assert!(
            close(a, 960.0, 1080.0) && close(b, 960.0, 0.0),
            "{a:?} {b:?}"
        );
    }

    #[test]
    fn linear_endpoints_reach_the_corners_at_diagonals() {
        let s = sz(1000, 1000);
        let (a, b) = linear_endpoints(s, 135.0);
        assert!(
            close(a, 0.0, 0.0) && close(b, 1000.0, 1000.0),
            "{a:?} {b:?}"
        );
    }

    #[test]
    fn hash_is_stable_and_sees_alpha() {
        let a = Background::Solid {
            color: Rgba([0.1, 0.2, 0.3, 1.0]),
        };
        let b = Background::Solid {
            color: Rgba([0.1, 0.2, 0.3, 0.5]),
        };
        assert_eq!(background_hash(&a), background_hash(&a.clone()));
        assert_ne!(background_hash(&a), background_hash(&b));
    }
}
