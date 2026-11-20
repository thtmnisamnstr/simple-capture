use std::any::Any;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use sc_core::time::HostTime;
use sc_geometry::{RectPt, SizePx};

use crate::{Result, StreamTag};

/// An OS frame (CVPixelBuffer on macOS) or a CPU frame, readable as NV12.
pub trait NativeFrame: Send + Sync + 'static {
    fn as_any(&self) -> &dyn Any;
    fn size(&self) -> SizePx;
    fn with_nv12(&self, f: &mut dyn FnMut(Nv12View<'_>)) -> Result<()>;
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

/// Tightly packed NV12 in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuNv12Frame {
    pub size: SizePx,
    pub y: Vec<u8>,
    pub uv: Vec<u8>,
    pub full_range: bool,
}

impl NativeFrame for CpuNv12Frame {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn size(&self) -> SizePx {
        self.size
    }

    fn with_nv12(&self, f: &mut dyn FnMut(Nv12View<'_>)) -> Result<()> {
        f(Nv12View {
            size: self.size,
            y: &self.y,
            y_stride: self.size.w as usize,
            uv: &self.uv,
            uv_stride: self.size.w as usize,
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
