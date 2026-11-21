//! Defaults for new text items.

use sc_geometry::PointF;

use crate::project::{
    Extra, Rgba, TextAlign, TextAnim, TextFill, TextItem, TextStyle, TextTransform,
};

/// Length of a new text item (SPEC: 3 seconds).
pub const DEFAULT_TEXT_NS: i64 = 3_000_000_000;

impl Default for TextStyle {
    fn default() -> TextStyle {
        TextStyle {
            font_family: "Inter".into(),
            size_1080: 72.0,
            weight: 700,
            italic: false,
            uppercase: false,
            align: TextAlign::Center,
            line_height: 1.2,
            letter_spacing: 0.0,
            opacity: 1.0,
            fill: TextFill::Solid(Rgba([1.0, 1.0, 1.0, 1.0])),
            outline: None,
            shadow: None,
            glow: None,
            background: None,
            extra: Extra::new(),
        }
    }
}

impl Default for TextTransform {
    fn default() -> TextTransform {
        TextTransform {
            center_norm: PointF { x: 0.5, y: 0.5 },
            scale: 1.0,
            rotation_deg: 0.0,
            extra: Extra::new(),
        }
    }
}

impl Default for TextAnim {
    fn default() -> TextAnim {
        TextAnim {
            in_: None,
            out: None,
            emphasis: None,
            extra: Extra::new(),
        }
    }
}

impl TextItem {
    pub fn new(start_tl_ns: i64, end_tl_ns: i64, content: &str) -> TextItem {
        TextItem {
            id: crate::ids::new_id(),
            start_tl_ns,
            end_tl_ns,
            layer: 0,
            content: content.to_string(),
            style: TextStyle::default(),
            transform: TextTransform::default(),
            anim: TextAnim::default(),
            extra: Extra::new(),
        }
    }
}
