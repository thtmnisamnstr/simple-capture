use sc_core::image::RgbaImage;
use sc_core::{Error, Result};
use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo, Pixmap, png_encoder};

use crate::raster::{check_layout, read_rgba};

/// PNG with an sRGB chunk and no ICC profile.
pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    check_layout(img)?;
    // Untagged so Skia writes no colour chunk; with_srgb_chunk adds the only one.
    let info = ImageInfo::new(
        (img.size.w as i32, img.size.h as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut pixels = img.data.clone();
    let pixmap = Pixmap::new(&info, &mut pixels, img.stride).ok_or_else(|| {
        Error::Invalid(format!(
            "encode_png: {}x{} pixel buffer rejected by Skia",
            img.size.w, img.size.h
        ))
    })?;
    let mut png = Vec::new();
    if !png_encoder::encode(&pixmap, &mut png, &png_encoder::Options::default()) {
        return Err(Error::Other(format!(
            "encode_png: Skia PNG encoder failed for {}x{}",
            img.size.w, img.size.h
        )));
    }
    with_srgb_chunk(&png)
}

/// PNG/JPEG through Skia, to premultiplied RGBA8 sRGB.
pub fn decode_image(bytes: &[u8]) -> Result<RgbaImage> {
    let image = Image::from_encoded(Data::new_copy(bytes)).ok_or_else(|| {
        Error::Invalid(format!(
            "decode_image: {} bytes are not an image this Skia build decodes (PNG, JPEG)",
            bytes.len()
        ))
    })?;
    read_rgba(&image)
}

const PNG_SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

pub(crate) fn with_srgb_chunk(png: &[u8]) -> Result<Vec<u8>> {
    let bad = |why: &str| Error::Other(format!("encode_png: malformed encoder output: {why}"));
    if png.len() < 8 || &png[..8] != PNG_SIG {
        return Err(bad("no PNG signature"));
    }
    let mut out = Vec::with_capacity(png.len() + 13);
    out.extend_from_slice(PNG_SIG);
    let mut pos = 8;
    let mut saw_ihdr = false;
    while pos < png.len() {
        if png.len() - pos < 12 {
            return Err(bad("truncated chunk header"));
        }
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        let end = pos
            .checked_add(12)
            .and_then(|p| p.checked_add(len))
            .filter(|&e| e <= png.len())
            .ok_or_else(|| bad("chunk runs past the end"))?;
        let kind = &png[pos + 4..pos + 8];
        match kind {
            b"iCCP" | b"sRGB" | b"gAMA" | b"cHRM" => {}
            _ => out.extend_from_slice(&png[pos..end]),
        }
        if kind == b"IHDR" {
            saw_ihdr = true;
            push_chunk(&mut out, b"sRGB", &[0]);
        }
        pos = end;
    }
    if !saw_ihdr {
        return Err(bad("no IHDR chunk"));
    }
    Ok(out)
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_png_iend() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn rejects_non_png() {
        assert!(with_srgb_chunk(b"not a png").is_err());
    }
}
