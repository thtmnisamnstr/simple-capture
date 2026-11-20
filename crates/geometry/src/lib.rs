//! Pure geometry shared by every crate (M1 plan §4.1, ARCHITECTURE §8 and §10).

mod camera;
mod fit;
mod region;
mod types;

pub use camera::{
    CAMERA_MARGIN, CAMERA_SIZE_MAX, CAMERA_SIZE_MIN, CAMERA_SIZE_PRESETS, CAMERA_ZOOM_MAX,
    CameraPose, CameraRects, Corner, RectF, camera_rects, clamp_center, corner_center, ease_in_out,
    lerp, lerp_pose, px_from_1080, shape_size, snap_center,
};
pub use fit::{
    contain_fit, cover_src_rect, even_round, natural_canvas, preset_size, reference_canvas_size,
    small_content,
};
pub use region::{app_bounds_pt, aspect_locked_resize, flip_y, pt_to_px, snap_region_even_px};
pub use types::{Aspect, Handle, PointF, Ratio, RectNorm, RectPt, RectPx, SizeClass, SizePx};
