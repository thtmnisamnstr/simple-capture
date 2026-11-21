use std::any::Any;
use std::ptr::NonNull;

use objc2_core_foundation::CFRetained;
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use sc_geometry::SizePx;
use sc_platform::{Error, NativeAudio, NativeFrame, Nv12View, Result};

pub(crate) const PIX_420V: u32 = u32::from_be_bytes(*b"420v");
pub(crate) const PIX_420F: u32 = u32::from_be_bytes(*b"420f");

/// A retained SCK screen sample buffer.
pub struct ScFrame {
    sb: CFRetained<CMSampleBuffer>,
    size: SizePx,
}

// CMSampleBuffer is an immutable, thread-safe CF object once delivered.
unsafe impl Send for ScFrame {}
unsafe impl Sync for ScFrame {}

impl ScFrame {
    pub(crate) fn retain(sb: &CMSampleBuffer, size: SizePx) -> ScFrame {
        ScFrame {
            sb: unsafe { CFRetained::retain(NonNull::from(sb)) },
            size,
        }
    }

    pub fn sample_buffer(&self) -> &CMSampleBuffer {
        &self.sb
    }
}

impl NativeFrame for ScFrame {
    fn as_any(&self) -> &dyn Any {
        &self.sb
    }

    fn size(&self) -> SizePx {
        self.size
    }

    fn with_nv12(&self, f: &mut dyn FnMut(Nv12View<'_>)) -> Result<()> {
        let Some(ib) = (unsafe { self.sb.image_buffer() }) else {
            return Err(Error::Other("ScFrame: no image buffer".into()));
        };
        let fmt = CVPixelBufferGetPixelFormatType(&ib);
        if (fmt != PIX_420V && fmt != PIX_420F) || CVPixelBufferGetPlaneCount(&ib) != 2 {
            return Err(Error::Other(format!(
                "ScFrame: pixel format {} is not NV12",
                crate::util::fourcc(fmt)
            )));
        }
        let rc = unsafe { CVPixelBufferLockBaseAddress(&ib, CVPixelBufferLockFlags::ReadOnly) };
        if rc != 0 {
            return Err(Error::Os {
                code: i64::from(rc),
                message: "CVPixelBufferLockBaseAddress".into(),
            });
        }
        let y_ptr = CVPixelBufferGetBaseAddressOfPlane(&ib, 0) as *const u8;
        let uv_ptr = CVPixelBufferGetBaseAddressOfPlane(&ib, 1) as *const u8;
        let y_stride = CVPixelBufferGetBytesPerRowOfPlane(&ib, 0);
        let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(&ib, 1);
        let y_rows = CVPixelBufferGetHeightOfPlane(&ib, 0);
        let uv_rows = CVPixelBufferGetHeightOfPlane(&ib, 1);
        let out = if y_ptr.is_null() || uv_ptr.is_null() {
            Err(Error::Other("ScFrame: null plane".into()))
        } else {
            let y = unsafe { std::slice::from_raw_parts(y_ptr, y_stride * y_rows) };
            let uv = unsafe { std::slice::from_raw_parts(uv_ptr, uv_stride * uv_rows) };
            f(Nv12View {
                size: self.size,
                y,
                y_stride,
                uv,
                uv_stride,
                full_range: fmt == PIX_420F,
            });
            Ok(())
        };
        unsafe { CVPixelBufferUnlockBaseAddress(&ib, CVPixelBufferLockFlags::ReadOnly) };
        out
    }
}

/// A retained SCK audio sample buffer (Float32, 48 kHz, stereo, non-interleaved).
pub struct ScAudio {
    sb: CFRetained<CMSampleBuffer>,
}

unsafe impl Send for ScAudio {}
unsafe impl Sync for ScAudio {}

impl ScAudio {
    pub(crate) fn retain(sb: &CMSampleBuffer) -> ScAudio {
        ScAudio {
            sb: unsafe { CFRetained::retain(NonNull::from(sb)) },
        }
    }

    pub fn sample_buffer(&self) -> &CMSampleBuffer {
        &self.sb
    }
}

impl NativeAudio for ScAudio {
    fn as_any(&self) -> &dyn Any {
        &self.sb
    }
    fn to_f32(&self) -> Option<Vec<f32>> {
        crate::av::pcm::to_f32(&self.sb)
    }
}

/// The CMSampleBuffer behind a frame delivered by this crate.
pub fn sample_buffer_of(f: &dyn NativeFrame) -> Option<&CMSampleBuffer> {
    f.as_any()
        .downcast_ref::<CFRetained<CMSampleBuffer>>()
        .map(|r| &**r)
}

/// The CMSampleBuffer behind an audio buffer delivered by this crate.
pub fn audio_sample_buffer_of(a: &dyn NativeAudio) -> Option<&CMSampleBuffer> {
    a.as_any()
        .downcast_ref::<CFRetained<CMSampleBuffer>>()
        .map(|r| &**r)
}
