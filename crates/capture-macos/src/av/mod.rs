//! AVFoundation devices, writers and readers. Camera and mic opens call `crate::tcc::guard`
//! first; writers, readers and VideoToolbox are not TCC-touching.

mod devices;
mod frames;
mod pcm;
mod reader;
#[cfg(test)]
mod tests;
mod writer;

use std::path::Path;
use std::sync::Arc;

use sc_geometry::SizePx;
use sc_platform::{
    AudioReader, AudioSource, AudioWriterSpec, CameraLimit, CameraSource, CaptureDevice, Decoder,
    DeviceKind, DeviceStream, FrameSink, Result, TrackProbe, TrackWriter, VideoCodec, VideoReader,
    VideoWriterSpec, WriterFactory,
};

pub use devices::CAMERA_TAG_BASE;
pub use frames::{MacAudioSample, MacPixelFrame, PIX_420F, PIX_420V};

/// Spike 3 `encbench` default (0.05 bits per pixel per frame).
pub const DEFAULT_BITS_PER_PIXEL: f64 = 0.05;

#[derive(Debug, Default)]
pub struct MacMic {
    _priv: (),
}

impl MacMic {
    pub fn new() -> MacMic {
        MacMic::default()
    }
}

impl AudioSource for MacMic {
    fn devices(&self) -> Result<Vec<CaptureDevice>> {
        devices::list(DeviceKind::Mic)
    }
    fn start(&self, device_id: &str, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
        devices::start(DeviceKind::Mic, device_id, None, sink)
    }
    fn show_mic_modes(&self) -> Result<()> {
        devices::show_mic_modes()
    }
}

#[derive(Debug, Default)]
pub struct MacCamera {
    _priv: (),
}

impl MacCamera {
    pub fn new() -> MacCamera {
        MacCamera::default()
    }
}

impl CameraSource for MacCamera {
    fn devices(&self) -> Result<Vec<CaptureDevice>> {
        devices::list(DeviceKind::Camera)
    }
    fn start(
        &self,
        device_id: &str,
        limit: CameraLimit,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn DeviceStream>> {
        devices::start(DeviceKind::Camera, device_id, Some(limit), sink)
    }
}

#[derive(Debug, Default)]
pub struct MacWriterFactory {
    _priv: (),
}

impl MacWriterFactory {
    pub fn new() -> MacWriterFactory {
        MacWriterFactory::default()
    }
}

impl WriterFactory for MacWriterFactory {
    fn extension(&self) -> &'static str {
        "mov"
    }
    fn video(&self, path: &Path, spec: &VideoWriterSpec) -> Result<Box<dyn TrackWriter>> {
        Ok(Box::new(writer::VideoWriter::create(path, spec)?))
    }
    fn audio(&self, path: &Path, spec: &AudioWriterSpec) -> Result<Box<dyn TrackWriter>> {
        Ok(Box::new(writer::AudioWriter::create(path, spec)?))
    }
    fn encode_benchmark(
        &self,
        size: SizePx,
        fps: u32,
        frames: u32,
        codec: VideoCodec,
    ) -> Result<f64> {
        writer::encode_benchmark(size, fps, frames, codec)
    }
}

#[derive(Debug, Default)]
pub struct MacDecoder {
    _priv: (),
}

impl MacDecoder {
    pub fn new() -> MacDecoder {
        MacDecoder::default()
    }
}

impl Decoder for MacDecoder {
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoReader>> {
        Ok(Box::new(reader::MacVideoReader::open(path)?))
    }
    fn open_audio(&self, path: &Path, rate: u32, channels: u16) -> Result<Box<dyn AudioReader>> {
        Ok(Box::new(reader::MacAudioReader::open(
            path, rate, channels,
        )?))
    }
    fn probe(&self, path: &Path) -> Result<TrackProbe> {
        reader::probe(path)
    }
}

/// Camera and mic hot-plug: `DevicesChanged` on connect/disconnect, plus `DeviceLost` when a
/// device this process is capturing from disconnects.
pub fn watch_devices(sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
    devices::watch(sink)
}
