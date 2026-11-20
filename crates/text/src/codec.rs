use sc_core::image::RgbaImage;
use sc_core::{Error, Result};

/// PNG with an sRGB chunk.
pub fn encode_png(_img: &RgbaImage) -> Result<Vec<u8>> {
    Err(Error::Unsupported("sc_text: not implemented yet"))
}

/// PNG/JPEG through Skia.
pub fn decode_image(_bytes: &[u8]) -> Result<RgbaImage> {
    Err(Error::Unsupported("sc_text: not implemented yet"))
}
