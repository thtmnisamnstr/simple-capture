//! Canvas fit and size rules. Stubs until wp-geometry.

use crate::{Aspect, PointF, Ratio, RectPt, RectPx, SizeClass, SizePx};

/// `2·round(x/2)`, minimum 2.
pub fn even_round(_x: f64) -> u32 {
    2
}

/// Contain fit of `content` into `canvas`, both edges snapped (ARCHITECTURE §8).
pub fn contain_fit(_canvas: SizePx, _content: SizePx) -> RectPx {
    RectPx::default()
}

/// Source rect (px) for a cover fit of `image` into `dst` around `focus` (0..1).
pub fn cover_src_rect(_image: SizePx, _dst: SizePx, _focus: PointF) -> RectPt {
    RectPt::default()
}

pub fn natural_canvas(_ratio: Ratio, _content: SizePx) -> (f64, f64) {
    (0.0, 0.0)
}

pub fn reference_canvas_size(_aspect: Aspect, _ratio: Ratio, _segments: &[SizePx]) -> SizePx {
    SizePx::default()
}

pub fn preset_size(_class: SizeClass, _ratio: Ratio, _gif: bool) -> SizePx {
    SizePx::default()
}

/// True when the fitted content is under 25 % of the canvas width or height.
pub fn small_content(_canvas: SizePx, _content: SizePx) -> bool {
    false
}
