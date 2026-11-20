//! A platform where every call returns `Error::Unsupported`. Used by the Windows and Linux
//! placeholders (until M6) and by stubs while their real backends are built.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use sc_core::ClockKind;
use sc_core::folder::LocalDateTime;
use sc_core::image::RgbaImage;
use sc_core::project::{SourceDescriptor, SystemAudioMode};
use sc_core::time::HostTime;
use sc_geometry::SizePx;

use crate::{
    AudioReader, AudioSource, AudioWriterSpec, CameraLimit, CameraSource, Capabilities,
    CaptureDevice, Decoder, Desktop, DeviceStream, DisplayInfo, Error, FrameSink, OverlayRole,
    OverlayWindows, PermissionKind, PermissionStatus, Permissions, PickedSource, PickerRequest,
    Platform, Result, ScreenSource, ScreenStream, ScreenStreamConfig, SourceList, SourcePicker,
    SourceSpec, StreamTag, SystemAudioSource, SystemAudioStream, TrackProbe, TrackWriter,
    VideoCodec, VideoReader, VideoWriterSpec, WindowId, WriterFactory,
};

/// Implements every component trait; every fallible call returns `Unsupported(what)`.
#[derive(Debug, Clone, Copy)]
pub struct Nothing {
    pub what: &'static str,
}

impl Nothing {
    pub const fn new(what: &'static str) -> Nothing {
        Nothing { what }
    }

    fn err<T>(&self) -> Result<T> {
        Err(Error::Unsupported(self.what))
    }
}

impl ScreenSource for Nothing {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        self.err()
    }
    fn list(&self, _hide: &[WindowId]) -> Result<SourceList> {
        self.err()
    }
    fn describe(
        &self,
        _spec: &SourceSpec,
        _cfg: &ScreenStreamConfig,
    ) -> Result<(SourceDescriptor, SizePx)> {
        self.err()
    }
    fn start(
        &self,
        _tag: StreamTag,
        _spec: &SourceSpec,
        _cfg: &ScreenStreamConfig,
        _sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn ScreenStream>> {
        self.err()
    }
    fn snapshot(&self, _spec: &SourceSpec, _max: SizePx) -> Result<RgbaImage> {
        self.err()
    }
}

impl SystemAudioSource for Nothing {
    fn start(
        &self,
        _mode: SystemAudioMode,
        _exclude: &[WindowId],
        _sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn SystemAudioStream>> {
        self.err()
    }
}

impl AudioSource for Nothing {
    fn devices(&self) -> Result<Vec<CaptureDevice>> {
        self.err()
    }
    fn start(&self, _device_id: &str, _sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
        self.err()
    }
    fn show_mic_modes(&self) -> Result<()> {
        self.err()
    }
}

impl CameraSource for Nothing {
    fn devices(&self) -> Result<Vec<CaptureDevice>> {
        self.err()
    }
    fn start(
        &self,
        _device_id: &str,
        _limit: CameraLimit,
        _sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn DeviceStream>> {
        self.err()
    }
}

impl SourcePicker for Nothing {
    fn present(
        &self,
        _req: PickerRequest,
        done: Box<dyn FnOnce(Result<Option<PickedSource>>) + Send>,
    ) {
        done(self.err());
    }
    fn set_observer(&self, _on_pick: Option<Box<dyn Fn(PickedSource) + Send + Sync>>) {}
    fn set_active(&self, _active: bool) -> Result<()> {
        self.err()
    }
}

impl WriterFactory for Nothing {
    fn extension(&self) -> &'static str {
        "none"
    }
    fn video(&self, _path: &Path, _spec: &VideoWriterSpec) -> Result<Box<dyn TrackWriter>> {
        self.err()
    }
    fn audio(&self, _path: &Path, _spec: &AudioWriterSpec) -> Result<Box<dyn TrackWriter>> {
        self.err()
    }
    fn encode_benchmark(
        &self,
        _size: SizePx,
        _fps: u32,
        _frames: u32,
        _codec: VideoCodec,
    ) -> Result<f64> {
        self.err()
    }
}

impl Decoder for Nothing {
    fn open_video(&self, _path: &Path) -> Result<Box<dyn VideoReader>> {
        self.err()
    }
    fn open_audio(&self, _path: &Path, _rate: u32, _channels: u16) -> Result<Box<dyn AudioReader>> {
        self.err()
    }
    fn probe(&self, _path: &Path) -> Result<TrackProbe> {
        self.err()
    }
}

impl Permissions for Nothing {
    fn status(&self, _k: PermissionKind) -> PermissionStatus {
        PermissionStatus::Unknown
    }
    fn request(&self, _k: PermissionKind, done: Box<dyn FnOnce(PermissionStatus) + Send>) {
        done(PermissionStatus::Unknown);
    }
    fn settings_url(&self, _k: PermissionKind) -> Option<String> {
        None
    }
}

impl OverlayWindows for Nothing {
    unsafe fn configure(&self, _raw_window: *mut c_void, _role: OverlayRole) -> Result<WindowId> {
        self.err()
    }
    fn registered(&self) -> Vec<WindowId> {
        Vec::new()
    }
}

impl Desktop for Nothing {
    fn reveal(&self, _path: &Path) -> Result<()> {
        self.err()
    }
    fn choose_image(&self, done: Box<dyn FnOnce(Option<PathBuf>) + Send>) {
        done(None);
    }
}

fn process_epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// A whole platform of `Nothing`, with all-false capabilities.
#[derive(Debug, Clone)]
pub struct UnsupportedPlatform {
    name: &'static str,
    clock: ClockKind,
    nothing: Nothing,
    shared: Arc<Nothing>,
}

impl UnsupportedPlatform {
    pub fn new(name: &'static str, clock: ClockKind, what: &'static str) -> UnsupportedPlatform {
        UnsupportedPlatform {
            name,
            clock,
            nothing: Nothing::new(what),
            shared: Arc::new(Nothing::new(what)),
        }
    }
}

impl Platform for UnsupportedPlatform {
    fn name(&self) -> &'static str {
        self.name
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::none()
    }
    fn clock_kind(&self) -> ClockKind {
        self.clock
    }
    fn host_now(&self) -> HostTime {
        HostTime(i64::try_from(process_epoch().elapsed().as_nanos()).unwrap_or(i64::MAX))
    }
    fn host_duration(&self, ns: i64) -> Duration {
        Duration::from_nanos(u64::try_from(ns).unwrap_or(0))
    }
    fn local_now(&self) -> LocalDateTime {
        LocalDateTime {
            y: 1970,
            mo: 1,
            d: 1,
            ..LocalDateTime::default()
        }
    }
    fn screen(&self) -> &dyn ScreenSource {
        &self.nothing
    }
    fn system_audio(&self) -> &dyn SystemAudioSource {
        &self.nothing
    }
    fn mic(&self) -> &dyn AudioSource {
        &self.nothing
    }
    fn camera(&self) -> &dyn CameraSource {
        &self.nothing
    }
    fn picker(&self) -> &dyn SourcePicker {
        &self.nothing
    }
    fn writers(&self) -> Arc<dyn WriterFactory> {
        self.shared.clone()
    }
    fn decoder(&self) -> Arc<dyn Decoder> {
        self.shared.clone()
    }
    fn permissions(&self) -> &dyn Permissions {
        &self.nothing
    }
    fn overlays(&self) -> &dyn OverlayWindows {
        &self.nothing
    }
    fn desktop(&self) -> &dyn Desktop {
        &self.nothing
    }
    fn watch_system(&self, _sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
        Err(Error::Unsupported(self.nothing.what))
    }
    fn free_disk_bytes(&self, _path: &Path) -> Result<u64> {
        Err(Error::Unsupported(self.nothing.what))
    }
    fn default_projects_dir(&self) -> Result<PathBuf> {
        Err(Error::Unsupported(self.nothing.what))
    }
    fn permission_host(&self) -> Option<String> {
        None
    }
}
