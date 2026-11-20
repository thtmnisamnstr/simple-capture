use crate::RenderedFrame;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DiffStats {
    pub max: u8,
    pub mean: f64,
}

/// Per-byte difference. Stub until wp-render.
pub fn diff(_a: &RenderedFrame, _b: &RenderedFrame) -> DiffStats {
    DiffStats::default()
}
