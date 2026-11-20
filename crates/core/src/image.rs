use sc_geometry::SizePx;

/// Premultiplied RGBA8, sRGB.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RgbaImage {
    pub size: SizePx,
    pub stride: usize,
    pub data: Vec<u8>,
}
