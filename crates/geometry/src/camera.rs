//! Camera overlay geometry (M2_PLAN §3.2): where the camera lands in the canvas and which part of
//! the camera frame fills it. Pure; mirrored in ui/src/lib/geometry.ts.

use crate::types::{PointF, Ratio, SizePx};

/// S/M/L: the shape's short side over the canvas short side.
pub const CAMERA_SIZE_PRESETS: [f64; 3] = [0.18, 0.26, 0.36];
pub const CAMERA_SIZE_MIN: f64 = 0.08;
pub const CAMERA_SIZE_MAX: f64 = 0.9;
/// Margin from the canvas edges for snapping and corner presets, over the canvas short side.
pub const CAMERA_MARGIN: f64 = 0.03;
pub const CAMERA_ZOOM_MAX: f64 = 4.0;

/// A rectangle with fractional coordinates, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The continuous part of a camera placement, in canvas-normalized units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    /// Shape centre over the canvas size, 0..1 on each axis.
    pub center: PointF,
    /// Shape short side over the canvas short side.
    pub size: f64,
    /// 1 = the camera frame just fills the shape; larger zooms in.
    pub zoom: f64,
    /// Where the crop sits inside the camera frame, −1..1 on each axis (0 = centred).
    pub pan: PointF,
    /// Corner radius over half the shape's short side, 0..1 (1 = circle or pill).
    pub radius: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraRects {
    /// Shape rectangle in canvas pixels.
    pub dst: RectF,
    /// Crop of the camera frame, in camera pixels, before mirroring.
    pub src: RectF,
    pub radius_px: f64,
}

/// Shape width and height for a short side `short` and shape aspect `aspect` (width:height).
pub fn shape_size(short: f64, aspect: Ratio) -> (f64, f64) {
    let a = aspect.as_f64();
    if a >= 1.0 {
        (short * a, short)
    } else {
        (short, short / a)
    }
}

/// Destination rectangle, camera crop and corner radius for `pose` on a `canvas` with a camera
/// frame of `camera` pixels. The shape always lies inside the canvas (a centre saved for another
/// frame shape is pulled in). The crop starts as the largest `aspect` rectangle inside the frame
/// (fill), shrinks by `zoom`, and moves by `pan` within the frame.
pub fn camera_rects(
    canvas: SizePx,
    camera: SizePx,
    aspect: Ratio,
    pose: &CameraPose,
) -> CameraRects {
    let (cw, ch) = (f64::from(canvas.w), f64::from(canvas.h));
    let short = cw.min(ch) * pose.size.clamp(CAMERA_SIZE_MIN, CAMERA_SIZE_MAX);
    let (w, h) = shape_size(short, aspect);
    let inside = |c: f64, len: f64, side: f64| {
        if len >= side {
            side / 2.0
        } else {
            (c * side).clamp(len / 2.0, side - len / 2.0)
        }
    };
    let dst = RectF {
        x: inside(pose.center.x, w, cw) - w / 2.0,
        y: inside(pose.center.y, h, ch) - h / 2.0,
        w,
        h,
    };
    let (fw, fh) = (f64::from(camera.w.max(1)), f64::from(camera.h.max(1)));
    let a = aspect.as_f64();
    let (fill_w, fill_h) = if fw / fh > a {
        (fh * a, fh)
    } else {
        (fw, fw / a)
    };
    let zoom = pose.zoom.clamp(1.0, CAMERA_ZOOM_MAX);
    let (sw, sh) = (fill_w / zoom, fill_h / zoom);
    let px = pose.pan.x.clamp(-1.0, 1.0);
    let py = pose.pan.y.clamp(-1.0, 1.0);
    let src = RectF {
        x: (fw - sw) / 2.0 * (1.0 + px),
        y: (fh - sh) / 2.0 * (1.0 + py),
        w: sw,
        h: sh,
    };
    CameraRects {
        dst,
        src,
        radius_px: pose.radius.clamp(0.0, 1.0) * w.min(h) / 2.0,
    }
}

/// Border width in canvas pixels for a width given in 1080 units (ADR-033).
pub fn px_from_1080(canvas: SizePx, width_1080: f64) -> f64 {
    width_1080 * f64::from(canvas.w.min(canvas.h)) / 1080.0
}

/// Linear interpolation, `f` in 0..1.
pub fn lerp(a: f64, b: f64, f: f64) -> f64 {
    a + (b - a) * f
}

/// Ease-in-out (smoothstep), `f` in 0..1.
pub fn ease_in_out(f: f64) -> f64 {
    let f = f.clamp(0.0, 1.0);
    f * f * (3.0 - 2.0 * f)
}

pub fn lerp_pose(a: &CameraPose, b: &CameraPose, f: f64) -> CameraPose {
    let p = |a: PointF, b: PointF| PointF {
        x: lerp(a.x, b.x, f),
        y: lerp(a.y, b.y, f),
    };
    CameraPose {
        center: p(a.center, b.center),
        size: lerp(a.size, b.size, f),
        zoom: lerp(a.zoom, b.zoom, f),
        pan: p(a.pan, b.pan),
        radius: lerp(a.radius, b.radius, f),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Centre (normalized) of a shape of `aspect` and `size` sitting in `corner` with the standard
/// margin.
pub fn corner_center(canvas: SizePx, aspect: Ratio, size: f64, corner: Corner) -> PointF {
    let (cw, ch) = (f64::from(canvas.w), f64::from(canvas.h));
    let short = cw.min(ch);
    let (w, h) = shape_size(short * size, aspect);
    let m = short * CAMERA_MARGIN;
    let (left, top) = (m + w / 2.0, m + h / 2.0);
    let (right, bottom) = (cw - m - w / 2.0, ch - m - h / 2.0);
    let (x, y) = match corner {
        Corner::TopLeft => (left, top),
        Corner::TopRight => (right, top),
        Corner::BottomLeft => (left, bottom),
        Corner::BottomRight => (right, bottom),
    };
    PointF {
        x: x / cw,
        y: y / ch,
    }
}

/// Keeps the whole shape inside the canvas.
pub fn clamp_center(canvas: SizePx, aspect: Ratio, size: f64, center: PointF) -> PointF {
    let (cw, ch) = (f64::from(canvas.w), f64::from(canvas.h));
    let (w, h) = shape_size(cw.min(ch) * size, aspect);
    let fx = (w / 2.0 / cw).min(0.5);
    let fy = (h / 2.0 / ch).min(0.5);
    PointF {
        x: center.x.clamp(fx, 1.0 - fx),
        y: center.y.clamp(fy, 1.0 - fy),
    }
}

/// Snaps the centre to the margin lines at the edges, and to the canvas centre lines, when within
/// `threshold_px` canvas pixels; then clamps inside the canvas.
pub fn snap_center(
    canvas: SizePx,
    aspect: Ratio,
    size: f64,
    center: PointF,
    threshold_px: f64,
) -> PointF {
    let (cw, ch) = (f64::from(canvas.w), f64::from(canvas.h));
    let short = cw.min(ch);
    let (w, h) = shape_size(short * size, aspect);
    let m = short * CAMERA_MARGIN;
    let snap = |v: f64, targets: [f64; 3]| {
        targets
            .into_iter()
            .map(|t| (t, (t - v).abs()))
            .filter(|(_, d)| *d <= threshold_px)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(v, |(t, _)| t)
    };
    let x = snap(center.x * cw, [m + w / 2.0, cw / 2.0, cw - m - w / 2.0]);
    let y = snap(center.y * ch, [m + h / 2.0, ch / 2.0, ch - m - h / 2.0]);
    clamp_center(
        canvas,
        aspect,
        size,
        PointF {
            x: x / cw,
            y: y / ch,
        },
    )
}
