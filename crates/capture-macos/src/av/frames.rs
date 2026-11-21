use std::any::Any;
use std::ptr::NonNull;
use std::sync::Arc;

use objc2_core_audio_types::AudioStreamBasicDescription;
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    CMAudioFormatDescriptionGetStreamBasicDescription, CMFormatDescription, CMSampleBuffer,
};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use sc_core::time::HostTime;
use sc_geometry::SizePx;
use sc_platform::{AudioBuffer, AudioData, Error, NativeAudio, NativeFrame, Nv12View, Result};

use crate::util::{cmtime_to_ns, fourcc};

pub const PIX_420V: u32 = u32::from_be_bytes(*b"420v");
pub const PIX_420F: u32 = u32::from_be_bytes(*b"420f");
pub(crate) const LPCM: u32 = u32::from_be_bytes(*b"lpcm");

/// A retained `CVPixelBuffer` (ScreenCaptureKit, AVCaptureSession, AVAssetReader). Writers
/// append it without a copy; any other `NativeFrame` is copied through `with_nv12`.
pub struct MacPixelFrame {
    pb: CFRetained<CVPixelBuffer>,
    size: SizePx,
}

// SAFETY: CVPixelBuffers are atomically reference counted and only read (locked read-only)
// or handed to AVFoundation, which retains them.
unsafe impl Send for MacPixelFrame {}
// SAFETY: see Send.
unsafe impl Sync for MacPixelFrame {}

impl MacPixelFrame {
    pub fn from_pixel_buffer(pb: CFRetained<CVPixelBuffer>) -> MacPixelFrame {
        let size = SizePx {
            w: u32::try_from(CVPixelBufferGetWidth(&pb)).unwrap_or(0),
            h: u32::try_from(CVPixelBufferGetHeight(&pb)).unwrap_or(0),
        };
        MacPixelFrame { pb, size }
    }

    pub fn from_sample_buffer(sb: &CMSampleBuffer) -> Option<MacPixelFrame> {
        // SAFETY: valid sample buffer; the getter returns a +1 reference or None.
        let ib = unsafe { sb.image_buffer() }?;
        Some(Self::from_pixel_buffer(ib))
    }

    pub fn pixel_buffer(&self) -> &CVPixelBuffer {
        &self.pb
    }

    pub fn pixel_format(&self) -> u32 {
        CVPixelBufferGetPixelFormatType(&self.pb)
    }
}

impl NativeFrame for MacPixelFrame {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn size(&self) -> SizePx {
        self.size
    }

    fn cv_pixel_buffer(&self) -> Option<NonNull<std::ffi::c_void>> {
        Some(NonNull::from(&*self.pb).cast())
    }

    fn with_nv12(&self, f: &mut dyn FnMut(Nv12View<'_>)) -> Result<()> {
        let fmt = self.pixel_format();
        if (fmt != PIX_420V && fmt != PIX_420F) || CVPixelBufferGetPlaneCount(&self.pb) != 2 {
            return Err(Error::Other(format!(
                "MacPixelFrame: pixel format {} is not NV12 (420v/420f)",
                fourcc(fmt)
            )));
        }
        let pb: &CVPixelBuffer = &self.pb;
        // SAFETY: valid pixel buffer; unlocked below on every path.
        let rc = unsafe { CVPixelBufferLockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly) };
        if rc != 0 {
            return Err(Error::Os {
                code: i64::from(rc),
                message: "CVPixelBufferLockBaseAddress".into(),
            });
        }
        let y_ptr = CVPixelBufferGetBaseAddressOfPlane(pb, 0) as *const u8;
        let uv_ptr = CVPixelBufferGetBaseAddressOfPlane(pb, 1) as *const u8;
        let y_stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
        let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 1);
        let y_rows = CVPixelBufferGetHeightOfPlane(pb, 0);
        let uv_rows = CVPixelBufferGetHeightOfPlane(pb, 1);
        let res = if y_ptr.is_null() || uv_ptr.is_null() {
            Err(Error::Other("MacPixelFrame: null plane address".into()))
        } else {
            // SAFETY: while locked, each plane holds `rows * bytes_per_row` readable bytes.
            let (y, uv) = unsafe {
                (
                    std::slice::from_raw_parts(y_ptr, y_stride * y_rows),
                    std::slice::from_raw_parts(uv_ptr, uv_stride * uv_rows),
                )
            };
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
        // SAFETY: balances the lock above.
        unsafe { CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly) };
        res
    }
}

/// A retained LPCM `CMSampleBuffer` (mic, system audio), carried as `AudioData::Native`.
pub struct MacAudioSample {
    sb: CFRetained<CMSampleBuffer>,
}

// SAFETY: sample buffers with ready data are immutable and atomically reference counted.
unsafe impl Send for MacAudioSample {}
// SAFETY: see Send.
unsafe impl Sync for MacAudioSample {}

impl MacAudioSample {
    pub fn new(sb: CFRetained<CMSampleBuffer>) -> MacAudioSample {
        MacAudioSample { sb }
    }

    pub fn retain(sb: &CMSampleBuffer) -> MacAudioSample {
        // SAFETY: `sb` is a valid CF object.
        let sb = unsafe { CFRetained::retain(NonNull::from(sb)) };
        MacAudioSample { sb }
    }

    pub fn sample_buffer(&self) -> &CMSampleBuffer {
        &self.sb
    }

    /// `pts` is the buffer's own presentation time (host time for SCK and AVCapture). `None`
    /// unless it is LPCM.
    pub fn audio_buffer(sb: &CMSampleBuffer) -> Option<AudioBuffer> {
        let asbd = asbd_of(sb)?;
        if asbd.mFormatID != LPCM {
            return None;
        }
        // SAFETY: valid sample buffer.
        let (pts, n) = unsafe { (sb.presentation_time_stamp(), sb.num_samples()) };
        Some(AudioBuffer {
            pts: HostTime(cmtime_to_ns(pts)?),
            sample_rate: asbd.mSampleRate.round() as u32,
            channels: u16::try_from(asbd.mChannelsPerFrame).ok()?,
            frames: u32::try_from(n).ok()?,
            data: AudioData::Native(Arc::new(Self::retain(sb))),
        })
    }
}

impl NativeAudio for MacAudioSample {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) fn asbd_of_desc(fd: &CMFormatDescription) -> Option<AudioStreamBasicDescription> {
    // SAFETY: returns null for non-audio descriptions, else a pointer valid while `fd` lives.
    let p = unsafe { CMAudioFormatDescriptionGetStreamBasicDescription(fd) };
    // SAFETY: see above; the struct is copied out.
    unsafe { p.as_ref() }.copied()
}

pub(crate) fn asbd_of(sb: &CMSampleBuffer) -> Option<AudioStreamBasicDescription> {
    // SAFETY: valid sample buffer.
    let fd = unsafe { sb.format_description() }?;
    asbd_of_desc(&fd)
}
