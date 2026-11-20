use sc_core::image::RgbaImage;
use sc_core::{Error, Result};
use sc_geometry::SizePx;
use skia_safe::{AlphaType, ColorSpace, ColorType, Image, ImageInfo, image::CachingHint};

/// Largest side accepted for a raster we allocate (canvas or decoded image).
pub(crate) const MAX_SIDE: u32 = 32_768;
/// Largest pixel count accepted (1 GiB of RGBA8).
const MAX_PIXELS: u64 = 1 << 28;

/// Premultiplied RGBA8 tagged sRGB: the layout of every `RgbaImage` this crate makes.
pub(crate) fn rgba_info(size: SizePx) -> Result<ImageInfo> {
    check_size(size)?;
    let (w, h) = (to_i32(size.w)?, to_i32(size.h)?);
    Ok(ImageInfo::new(
        (w, h),
        ColorType::RGBA8888,
        AlphaType::Premul,
        ColorSpace::new_srgb(),
    ))
}

pub(crate) fn check_size(size: SizePx) -> Result<()> {
    if size.w == 0 || size.h == 0 {
        return Err(Error::Invalid(format!(
            "image size {}x{} has a zero side",
            size.w, size.h
        )));
    }
    if size.w > MAX_SIDE || size.h > MAX_SIDE || u64::from(size.w) * u64::from(size.h) > MAX_PIXELS
    {
        return Err(Error::Invalid(format!(
            "image size {}x{} is too large (max side {MAX_SIDE}, max {MAX_PIXELS} pixels)",
            size.w, size.h
        )));
    }
    Ok(())
}

fn to_i32(v: u32) -> Result<i32> {
    i32::try_from(v).map_err(|_| Error::Invalid(format!("dimension {v} does not fit in i32")))
}

/// A zeroed, tightly packed image of `size`.
pub(crate) fn blank(size: SizePx) -> Result<RgbaImage> {
    check_size(size)?;
    let stride = size.w as usize * 4;
    Ok(RgbaImage {
        size,
        stride,
        data: vec![0; stride * size.h as usize],
    })
}

/// Checks that `img.data` and `img.stride` describe `img.size` RGBA8 pixels.
pub(crate) fn check_layout(img: &RgbaImage) -> Result<()> {
    check_size(img.size)?;
    let row = img.size.w as usize * 4;
    let need = img.stride * (img.size.h as usize - 1) + row;
    if img.stride < row || img.data.len() < need {
        return Err(Error::Invalid(format!(
            "RgbaImage {}x{}: stride {} / {} bytes too small (need stride >= {row}, {need} bytes)",
            img.size.w,
            img.size.h,
            img.stride,
            img.data.len()
        )));
    }
    Ok(())
}

/// Decodes (if lazy) and converts `image` to premultiplied RGBA8 sRGB.
pub(crate) fn read_rgba(image: &Image) -> Result<RgbaImage> {
    let (w, h) = (image.width(), image.height());
    let size = SizePx {
        w: u32::try_from(w).unwrap_or(0),
        h: u32::try_from(h).unwrap_or(0),
    };
    let mut out = blank(size)?;
    let info = rgba_info(size)?;
    if !image.read_pixels(
        &info,
        &mut out.data,
        out.stride,
        (0, 0),
        CachingHint::Disallow,
    ) {
        return Err(Error::Invalid(format!(
            "could not decode {w}x{h} image pixels"
        )));
    }
    Ok(out)
}
