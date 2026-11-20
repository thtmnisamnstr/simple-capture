//! AVFoundation devices, writers and readers. Stubs until wp-mac-av; camera and mic opens must
//! call `crate::tcc::guard` first (writers, readers and VideoToolbox are not TCC-touching).

use std::path::Path;
use std::sync::Arc;

use sc_geometry::SizePx;
use sc_platform::{
    AudioReader, AudioSource, AudioWriterSpec, CameraLimit, CameraSource, CaptureDevice, Decoder,
    DeviceStream, Error, FrameSink, Result, TrackProbe, TrackWriter, VideoCodec, VideoReader,
    VideoWriterSpec, WriterFactory,
};

const STUB: &str = "sc_capture_macos::av: not implemented yet";

fn stub<T>() -> Result<T> {
    Err(Error::Unsupported(STUB))
}

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
        stub()
    }
    fn start(&self, _device_id: &str, _sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
        stub()
    }
    fn show_mic_modes(&self) -> Result<()> {
        stub()
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
        stub()
    }
    fn start(
        &self,
        _device_id: &str,
        _limit: CameraLimit,
        _sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn DeviceStream>> {
        stub()
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
    fn video(&self, _path: &Path, _spec: &VideoWriterSpec) -> Result<Box<dyn TrackWriter>> {
        stub()
    }
    fn audio(&self, _path: &Path, _spec: &AudioWriterSpec) -> Result<Box<dyn TrackWriter>> {
        stub()
    }
    fn encode_benchmark(
        &self,
        _size: SizePx,
        _fps: u32,
        _frames: u32,
        _codec: VideoCodec,
    ) -> Result<f64> {
        stub()
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
    fn open_video(&self, _path: &Path) -> Result<Box<dyn VideoReader>> {
        stub()
    }
    fn open_audio(&self, _path: &Path, _rate: u32, _channels: u16) -> Result<Box<dyn AudioReader>> {
        stub()
    }
    fn probe(&self, _path: &Path) -> Result<TrackProbe> {
        stub()
    }
}

/// Camera and mic hot-plug notifications.
pub fn watch_devices(_sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
    stub()
}
