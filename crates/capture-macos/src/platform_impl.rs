//! `MacPlatform`: delegates every call to `sck` and `av` (frozen wiring; the behaviour lives in
//! those modules).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sc_core::ClockKind;
use sc_core::folder::LocalDateTime;
use sc_core::time::HostTime;
use sc_platform::{
    AudioSource, CameraSource, Capabilities, Decoder, Desktop, DeviceStream, FrameSink,
    OverlayWindows, Permissions, Platform, Result, ScreenSource, SourcePicker, SystemAudioSource,
    WriterFactory,
};

use crate::{av, sck, util};

pub(crate) const DEV_HOST_BUNDLE_ID: &str = "com.simplecapture.dev";

pub(crate) struct MacPlatform {
    screen: sck::MacScreenSource,
    system_audio: sck::MacSystemAudio,
    picker: sck::MacPicker,
    permissions: sck::MacPermissions,
    overlays: sck::MacOverlays,
    desktop: sck::MacDesktop,
    mic: av::MacMic,
    camera: av::MacCamera,
    writers: Arc<av::MacWriterFactory>,
    decoder: Arc<av::MacDecoder>,
}

impl MacPlatform {
    pub(crate) fn new() -> MacPlatform {
        av::allow_screen_devices();
        MacPlatform {
            screen: sck::MacScreenSource::new(),
            system_audio: sck::MacSystemAudio::new(),
            picker: sck::MacPicker::new(),
            permissions: sck::MacPermissions::new(),
            overlays: sck::MacOverlays::new(),
            desktop: sck::MacDesktop::new(),
            mic: av::MacMic::new(),
            camera: av::MacCamera::new(),
            writers: Arc::new(av::MacWriterFactory::new()),
            decoder: Arc::new(av::MacDecoder::new()),
        }
    }
}

/// System watcher (sleep/wake, own menus) plus device watcher, stopped together.
struct Watchers(Box<dyn DeviceStream>, Box<dyn DeviceStream>);

impl DeviceStream for Watchers {
    fn format(&self) -> String {
        format!("{} + {}", self.0.format(), self.1.format())
    }

    fn stop(self: Box<Self>) -> Result<()> {
        let a = self.0.stop();
        let b = self.1.stop();
        a.and(b)
    }
}

impl Platform for MacPlatform {
    fn name(&self) -> &'static str {
        "macos"
    }
    fn capabilities(&self) -> Capabilities {
        sck::capabilities()
    }
    fn clock_kind(&self) -> ClockKind {
        ClockKind::MachHost
    }
    fn host_now(&self) -> HostTime {
        util::host_now()
    }
    fn host_duration(&self, ns: i64) -> Duration {
        Duration::from_nanos(u64::try_from(ns).unwrap_or(0))
    }
    fn local_now(&self) -> LocalDateTime {
        sck::local_now()
    }
    fn screen(&self) -> &dyn ScreenSource {
        &self.screen
    }
    fn system_audio(&self) -> &dyn SystemAudioSource {
        &self.system_audio
    }
    fn mic(&self) -> &dyn AudioSource {
        &self.mic
    }
    fn camera(&self) -> &dyn CameraSource {
        &self.camera
    }
    fn picker(&self) -> &dyn SourcePicker {
        &self.picker
    }
    fn writers(&self) -> Arc<dyn WriterFactory> {
        self.writers.clone()
    }
    fn decoder(&self) -> Arc<dyn Decoder> {
        self.decoder.clone()
    }
    fn permissions(&self) -> &dyn Permissions {
        &self.permissions
    }
    fn overlays(&self) -> &dyn OverlayWindows {
        &self.overlays
    }
    fn desktop(&self) -> &dyn Desktop {
        &self.desktop
    }
    fn watch_system(&self, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
        let system = sck::watch_system(sink.clone())?;
        match av::watch_devices(sink) {
            Ok(devices) => Ok(Box::new(Watchers(system, devices))),
            Err(e) => {
                let _ = system.stop();
                Err(e)
            }
        }
    }
    fn free_disk_bytes(&self, path: &Path) -> Result<u64> {
        sck::free_disk_bytes(path)
    }
    fn default_projects_dir(&self) -> Result<PathBuf> {
        sck::default_projects_dir()
    }
    fn permission_host(&self) -> Option<String> {
        (std::env::var("SC_DEV_HOST").ok().as_deref() == Some("1"))
            .then(|| DEV_HOST_BUNDLE_ID.to_string())
    }
}
