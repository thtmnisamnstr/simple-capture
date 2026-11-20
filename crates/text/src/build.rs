#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildInfo {
    pub freetype: bool,
    pub skia_milestone: u32,
    pub jpeg_decode: bool,
    pub webp_decode: bool,
}

/// What the linked Skia build supports. Stub until wp-text.
pub fn build_info() -> BuildInfo {
    BuildInfo {
        freetype: false,
        skia_milestone: 0,
        jpeg_decode: false,
        webp_decode: false,
    }
}
