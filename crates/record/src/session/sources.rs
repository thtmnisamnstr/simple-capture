//! The recording's main picture: a screen source from the platform, or a camera or a plugged-in
//! device's screen recorded as the main picture, whose frames the router takes like screen frames.

use std::sync::Arc;

use sc_core::project::SourceDescriptor;
use sc_geometry::SizePx;
use sc_platform::{
    AudioBuffer, AudioTrackKind, CameraLimit, DeviceKind, DeviceStream, FrameSink, Platform,
    Result, ScreenStream, ScreenStreamConfig, SourceEvent, SourceSpec, StreamTag, VideoFrame,
    WindowId,
};

/// The size writers start with; the router recreates them at the first frame's real size.
const CAMERA_GUESS: SizePx = SizePx { w: 1920, h: 1080 };

pub(crate) fn describe(
    p: &dyn Platform,
    spec: &SourceSpec,
    cfg: &ScreenStreamConfig,
) -> Result<(SourceDescriptor, SizePx)> {
    match spec {
        SourceSpec::Camera { device } | SourceSpec::Device { device } => {
            let name = p
                .camera()
                .devices()
                .ok()
                .and_then(|d| d.into_iter().find(|d| &d.id == device))
                .map(|d| d.name);
            Ok((
                SourceDescriptor::Camera {
                    device_id: device.clone(),
                    device_name: name,
                },
                CAMERA_GUESS,
            ))
        }
        _ => p.screen().describe(spec, cfg),
    }
}

pub(crate) fn start(
    p: &dyn Platform,
    tag: StreamTag,
    spec: &SourceSpec,
    cfg: &ScreenStreamConfig,
    sink: Arc<dyn FrameSink>,
) -> Result<Box<dyn ScreenStream>> {
    match spec {
        SourceSpec::Camera { device } => {
            let adapter = Arc::new(CameraAsScreen {
                inner: sink,
                tag,
                device: device.clone(),
            });
            let stream = p.camera().start(device, CameraLimit::default(), adapter)?;
            Ok(Box::new(CameraStream {
                tag,
                inner: Some(stream),
            }))
        }
        SourceSpec::Device { device } => {
            let adapter = Arc::new(CameraAsScreen {
                inner: sink,
                tag,
                device: device.clone(),
            });
            let stream = p
                .device_screens()
                .start(device, CameraLimit::SCREEN, adapter)?;
            Ok(Box::new(CameraStream {
                tag,
                inner: Some(stream),
            }))
        }
        _ => p.screen().start(tag, spec, cfg, sink),
    }
}

struct CameraAsScreen {
    inner: Arc<dyn FrameSink>,
    tag: StreamTag,
    device: String,
}

impl FrameSink for CameraAsScreen {
    fn on_screen(&self, _: VideoFrame) {}

    fn on_camera(&self, f: VideoFrame) {
        self.inner.on_screen(VideoFrame {
            stream: self.tag,
            ..f
        });
    }

    fn on_audio(&self, _: AudioTrackKind, _: AudioBuffer) {}

    fn on_event(&self, e: SourceEvent) {
        match e {
            SourceEvent::DeviceLost {
                kind: DeviceKind::Camera,
                ref device_id,
            } if *device_id == self.device => self.inner.on_event(SourceEvent::SourceLost {
                stream: self.tag,
                reason: format!("camera {device_id} disconnected"),
            }),
            other => self.inner.on_event(other),
        }
    }
}

struct CameraStream {
    tag: StreamTag,
    inner: Option<Box<dyn DeviceStream>>,
}

impl ScreenStream for CameraStream {
    fn tag(&self) -> StreamTag {
        self.tag
    }

    fn update_exclusions(&self, _exclude: &[WindowId]) -> Result<()> {
        Ok(())
    }

    fn stop(mut self: Box<Self>) -> Result<()> {
        match self.inner.take() {
            Some(s) => s.stop(),
            None => Ok(()),
        }
    }
}
