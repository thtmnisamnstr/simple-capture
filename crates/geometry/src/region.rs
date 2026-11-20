//! Point/pixel conversion and region editing. Stubs until wp-geometry.

use crate::{Handle, PointF, Ratio, RectPt, RectPx};

pub fn pt_to_px(_r: RectPt, _scale: f64) -> RectPx {
    RectPx::default()
}

/// Flips between bottom-left and top-left origins (Pitfall 3).
pub fn flip_y(r: RectPt, _container_h_pt: f64) -> RectPt {
    r
}

/// Snaps a region so its pixel size is even.
pub fn snap_region_even_px(r: RectPt, _scale: f64) -> RectPt {
    r
}

pub fn aspect_locked_resize(
    r: RectPt,
    _ratio: Option<Ratio>,
    _handle: Handle,
    _to: PointF,
    _bounds: RectPt,
) -> RectPt {
    r
}
