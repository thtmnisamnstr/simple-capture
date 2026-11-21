//! NV12 `CVPixelBuffer`s as wgpu textures without a copy (SPEC, "Frames into wgpu"): each plane of
//! the buffer's IOSurface becomes an `R8Uint` / `Rg8Uint` texture through a `CVMetalTextureCache`
//! on wgpu's Metal device, so the shaders read the same bytes an upload would.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBuffer,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetIOSurface, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetPlaneCount, CVPixelBufferGetWidthOfPlane,
};
use objc2_metal::{MTLPixelFormat, MTLTextureType};
use sc_geometry::SizePx;

const PIX_420V: u32 = u32::from_be_bytes(*b"420v");
const PIX_420F: u32 = u32::from_be_bytes(*b"420f");

pub(crate) struct TextureCache(CFRetained<CVMetalTextureCache>);

// SAFETY: a CVMetalTextureCache may be used from any thread; `Gpu` uses it from one at a time.
unsafe impl Send for TextureCache {}

struct KeepAlive(#[allow(dead_code)] CFRetained<CVMetalTexture>);

// SAFETY: only dropped (CFRelease), which is thread-safe.
unsafe impl Send for KeepAlive {}
// SAFETY: see Send.
unsafe impl Sync for KeepAlive {}

pub(crate) struct Planes {
    pub y: wgpu::Texture,
    pub uv: wgpu::Texture,
    pub full_range: bool,
}

impl TextureCache {
    pub(crate) fn new(device: &wgpu::Device) -> Option<TextureCache> {
        // SAFETY: the raw device is only borrowed to create the cache, which retains it.
        let hal = unsafe { device.as_hal::<wgpu_hal::api::Metal>() }?;
        let mut out: *mut CVMetalTextureCache = std::ptr::null_mut();
        // SAFETY: valid device; `out` receives a +1 reference on success.
        let rc = unsafe {
            CVMetalTextureCache::create(None, None, hal.raw_device(), None, NonNull::from(&mut out))
        };
        if rc != 0 {
            tracing::debug!("CVMetalTextureCacheCreate: {rc}");
            return None;
        }
        // SAFETY: success returned an owned, non-null cache.
        NonNull::new(out).map(|p| TextureCache(unsafe { CFRetained::from_raw(p) }))
    }

    /// The Y and CbCr planes of `pb` when it is IOSurface-backed NV12 (`420v`/`420f`) of `size`.
    ///
    /// # Safety
    /// `pb` must point to a live `CVPixelBuffer`.
    pub(crate) unsafe fn import(
        &self,
        device: &wgpu::Device,
        pb: NonNull<c_void>,
        size: SizePx,
    ) -> Option<Planes> {
        // SAFETY: the caller guarantees a live CVPixelBuffer.
        let pb: &CVPixelBuffer = unsafe { pb.cast().as_ref() };
        let fmt = CVPixelBufferGetPixelFormatType(pb);
        if (fmt != PIX_420V && fmt != PIX_420F)
            || CVPixelBufferGetPlaneCount(pb) != 2
            || CVPixelBufferGetIOSurface(Some(pb)).is_none()
        {
            return None;
        }
        let chroma = (size.w.div_ceil(2), size.h.div_ceil(2));
        let y = self.plane(device, pb, 0, (size.w, size.h), wgpu::TextureFormat::R8Uint)?;
        let uv = self.plane(device, pb, 1, chroma, wgpu::TextureFormat::Rg8Uint)?;
        Some(Planes {
            y,
            uv,
            full_range: fmt == PIX_420F,
        })
    }

    fn plane(
        &self,
        device: &wgpu::Device,
        pb: &CVPixelBuffer,
        plane: usize,
        (w, h): (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Option<wgpu::Texture> {
        let (pw, ph) = (
            CVPixelBufferGetWidthOfPlane(pb, plane),
            CVPixelBufferGetHeightOfPlane(pb, plane),
        );
        if (pw, ph) != (w as usize, h as usize) {
            return None;
        }
        let mtl_format = match format {
            wgpu::TextureFormat::R8Uint => MTLPixelFormat::R8Uint,
            _ => MTLPixelFormat::RG8Uint,
        };
        let mut out: *mut CVMetalTexture = std::ptr::null_mut();
        // SAFETY: valid cache and buffer; `out` receives a +1 reference on success.
        let rc = unsafe {
            CVMetalTextureCache::create_texture_from_image(
                None,
                &self.0,
                pb,
                None,
                mtl_format,
                pw,
                ph,
                plane,
                NonNull::from(&mut out),
            )
        };
        if rc != 0 {
            tracing::debug!("CVMetalTextureCacheCreateTextureFromImage plane {plane}: {rc}");
            return None;
        }
        // SAFETY: success returned an owned, non-null texture.
        let cv = unsafe { CFRetained::from_raw(NonNull::new(out)?) };
        let raw = CVMetalTextureGetTexture(&cv)?;
        let keep = KeepAlive(cv);
        // SAFETY: `raw` is a 2D, single-mip texture of `format` and this size; `keep` holds the
        // CVMetalTexture (and through it the pixel buffer) until wgpu drops the texture.
        let hal = unsafe {
            wgpu_hal::metal::Device::texture_from_raw(
                raw,
                format,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu_hal::CopyExtent {
                    width: w,
                    height: h,
                    depth: 1,
                },
                Some(Box::new(move || drop(keep))),
            )
        };
        // SAFETY: the descriptor matches the raw texture.
        Some(unsafe {
            device.create_texture_from_hal::<wgpu_hal::api::Metal>(
                hal,
                &wgpu::TextureDescriptor {
                    label: Some("sc_render iosurface plane"),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::TextureUses::RESOURCE,
            )
        })
    }

    pub(crate) fn flush(&self) {
        self.0.flush(0);
    }
}
