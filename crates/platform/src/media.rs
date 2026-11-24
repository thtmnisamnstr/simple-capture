use std::any::Any;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use sc_core::time::HostTime;
use sc_geometry::{RectPt, SizePx};

use crate::{Error, Result, StreamTag};

/// An OS frame (CVPixelBuffer on macOS) or a CPU frame, readable as NV12.
pub trait NativeFrame: Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    fn size(&self) -> SizePx;
    fn with_nv12(&self, f: &mut dyn FnMut(Nv12View<'_>)) -> Result<()>;

    /// The `CVPixelBufferRef` behind a macOS frame, valid while `self` lives, so the GPU can read
    /// its IOSurface without a copy.
    fn cv_pixel_buffer(&self) -> Option<std::ptr::NonNull<std::ffi::c_void>> {
        None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Nv12View<'a> {
    pub size: SizePx,
    pub y: &'a [u8],
    pub y_stride: usize,
    pub uv: &'a [u8],
    pub uv_stride: usize,
    pub full_range: bool,
}

/// Tightly packed NV12 in memory: `y` is `w × h` bytes; `uv` is `ceil(h/2)` rows of
/// [`CpuNv12Frame::uv_stride`] bytes (interleaved U, V at half resolution).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuNv12Frame {
    pub size: SizePx,
    pub y: Vec<u8>,
    pub uv: Vec<u8>,
    pub full_range: bool,
}

impl CpuNv12Frame {
    /// A frame filled with one colour (`y`, `u`, `v` code values).
    pub fn filled(size: SizePx, y: u8, u: u8, v: u8, full_range: bool) -> CpuNv12Frame {
        let uv_len = Self::uv_stride_for(size) * Self::uv_rows_for(size);
        let mut uv = vec![u; uv_len];
        for pair in uv.chunks_exact_mut(2) {
            pair[1] = v;
        }
        CpuNv12Frame {
            size,
            y: vec![y; size.w as usize * size.h as usize],
            uv,
            full_range,
        }
    }

    /// Black in the given range.
    pub fn black(size: SizePx, full_range: bool) -> CpuNv12Frame {
        Self::filled(size, if full_range { 0 } else { 16 }, 128, 128, full_range)
    }

    /// Bytes per UV row: the width rounded up to even.
    pub fn uv_stride(&self) -> usize {
        Self::uv_stride_for(self.size)
    }

    /// UV rows: the height rounded up, halved.
    pub fn uv_rows(&self) -> usize {
        Self::uv_rows_for(self.size)
    }

    pub fn uv_stride_for(size: SizePx) -> usize {
        (size.w as usize).div_ceil(2) * 2
    }

    pub fn uv_rows_for(size: SizePx) -> usize {
        (size.h as usize).div_ceil(2)
    }
}

impl NativeFrame for CpuNv12Frame {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn size(&self) -> SizePx {
        self.size
    }

    fn with_nv12(&self, f: &mut dyn FnMut(Nv12View<'_>)) -> Result<()> {
        let y_len = self.size.w as usize * self.size.h as usize;
        let uv_len = self.uv_stride() * self.uv_rows();
        if self.y.len() < y_len || self.uv.len() < uv_len {
            return Err(Error::Other(format!(
                "CpuNv12Frame {}x{}: planes too short (y {} < {y_len} or uv {} < {uv_len})",
                self.size.w,
                self.size.h,
                self.y.len(),
                self.uv.len()
            )));
        }
        f(Nv12View {
            size: self.size,
            y: &self.y,
            y_stride: self.size.w as usize,
            uv: &self.uv,
            uv_stride: self.uv_stride(),
            full_range: self.full_range,
        });
        Ok(())
    }
}

#[derive(Clone)]
pub struct FrameBuffer(pub Arc<dyn NativeFrame>);

impl fmt::Debug for FrameBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("FrameBuffer").field(&self.0.size()).finish()
    }
}

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub stream: StreamTag,
    pub pts: HostTime,
    pub size: SizePx,
    pub content_rect_pt: Option<RectPt>,
    pub content_scale: Option<f64>,
    pub buffer: FrameBuffer,
}

pub trait NativeAudio: Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    /// The samples as interleaved f32, when the platform can give them (level meters).
    fn to_f32(&self) -> Option<Vec<f32>> {
        None
    }
}

impl AudioData {
    pub fn samples(&self) -> Option<std::borrow::Cow<'_, [f32]>> {
        match self {
            AudioData::F32Interleaved(s) => Some(std::borrow::Cow::Borrowed(s)),
            AudioData::Native(n) => n.to_f32().map(std::borrow::Cow::Owned),
        }
    }
}

#[derive(Clone)]
pub enum AudioData {
    Native(Arc<dyn NativeAudio>),
    F32Interleaved(Arc<[f32]>),
}

impl fmt::Debug for AudioData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioData::Native(_) => f.write_str("AudioData::Native"),
            AudioData::F32Interleaved(s) => {
                write!(f, "AudioData::F32Interleaved({} samples)", s.len())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub pts: HostTime,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u32,
    pub data: AudioData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioTrackKind {
    Mic,
    SystemAudio,
    DeviceAudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    Hevc,
    H264,
}

/// Colour is always BT.709 limited.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoWriterSpec {
    pub size: SizePx,
    pub fps: u32,
    pub codec: VideoCodec,
    pub keyframe_interval_s: f64,
    pub fragment_interval_s: f64,
    pub bits_per_pixel: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioWriterSpec {
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate: u32,
    pub fragment_interval_s: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Append {
    Written,
    NotReady,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedTrack {
    pub path: PathBuf,
    pub duration_ns: Option<i64>,
    pub samples: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackProbe {
    pub readable: bool,
    pub duration_ns: Option<i64>,
    pub samples: u64,
    pub size: Option<SizePx>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub file_ns: i64,
    pub frame: FrameBuffer,
}
