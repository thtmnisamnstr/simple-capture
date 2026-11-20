//! ScreenCaptureKit sources, system audio, picker, permissions and AppKit glue.
//! Stubs until wp-mac-sck; every TCC-touching path must call `crate::tcc::guard` first.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_core::folder::LocalDateTime;
use sc_core::image::RgbaImage;
use sc_core::project::{SourceDescriptor, SystemAudioMode};
use sc_geometry::SizePx;
use sc_platform::{
    Capabilities, Desktop, DeviceStream, DisplayInfo, Error, FrameSink, OverlayRole,
    OverlayWindows, PermissionKind, PermissionStatus, Permissions, PickedSource, PickerRequest,
    Result, ScreenSource, ScreenStream, ScreenStreamConfig, SourceList, SourcePicker, SourceSpec,
    StreamTag, SystemAudioSource, SystemAudioStream, WindowId,
};

const STUB: &str = "sc_capture_macos::sck: not implemented yet";

fn stub<T>() -> Result<T> {
    Err(Error::Unsupported(STUB))
}

#[derive(Debug, Default)]
pub struct MacScreenSource {
    _priv: (),
}

impl MacScreenSource {
    pub fn new() -> MacScreenSource {
        MacScreenSource::default()
    }
}

impl ScreenSource for MacScreenSource {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        stub()
    }
    fn list(&self, _hide: &[WindowId]) -> Result<SourceList> {
        stub()
    }
    fn describe(
        &self,
        _spec: &SourceSpec,
        _cfg: &ScreenStreamConfig,
    ) -> Result<(SourceDescriptor, SizePx)> {
        stub()
    }
    fn start(
        &self,
        _tag: StreamTag,
        _spec: &SourceSpec,
        _cfg: &ScreenStreamConfig,
        _sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn ScreenStream>> {
        stub()
    }
    fn snapshot(&self, _spec: &SourceSpec, _max: SizePx) -> Result<RgbaImage> {
        stub()
    }
}

#[derive(Debug, Default)]
pub struct MacSystemAudio {
    _priv: (),
}

impl MacSystemAudio {
    pub fn new() -> MacSystemAudio {
        MacSystemAudio::default()
    }
}

impl SystemAudioSource for MacSystemAudio {
    fn start(
        &self,
        _mode: SystemAudioMode,
        _exclude: &[WindowId],
        _sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn SystemAudioStream>> {
        stub()
    }
}

#[derive(Debug, Default)]
pub struct MacPicker {
    _priv: (),
}

impl MacPicker {
    pub fn new() -> MacPicker {
        MacPicker::default()
    }
}

impl SourcePicker for MacPicker {
    fn present(
        &self,
        _req: PickerRequest,
        done: Box<dyn FnOnce(Result<Option<PickedSource>>) + Send>,
    ) {
        done(stub());
    }
    fn set_observer(&self, _on_pick: Option<Box<dyn Fn(PickedSource) + Send + Sync>>) {}
    fn set_active(&self, _active: bool) -> Result<()> {
        stub()
    }
}

#[derive(Debug, Default)]
pub struct MacPermissions {
    _priv: (),
}

impl MacPermissions {
    pub fn new() -> MacPermissions {
        MacPermissions::default()
    }
}

impl Permissions for MacPermissions {
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

#[derive(Debug, Default)]
pub struct MacOverlays {
    _priv: (),
}

impl MacOverlays {
    pub fn new() -> MacOverlays {
        MacOverlays::default()
    }
}

impl OverlayWindows for MacOverlays {
    unsafe fn configure(&self, _raw_window: *mut c_void, _role: OverlayRole) -> Result<WindowId> {
        stub()
    }
    fn registered(&self) -> Vec<WindowId> {
        Vec::new()
    }
}

#[derive(Debug, Default)]
pub struct MacDesktop {
    _priv: (),
}

impl MacDesktop {
    pub fn new() -> MacDesktop {
        MacDesktop::default()
    }
}

impl Desktop for MacDesktop {
    fn reveal(&self, _path: &Path) -> Result<()> {
        stub()
    }
    fn choose_image(&self, done: Box<dyn FnOnce(Option<PathBuf>) + Send>) {
        done(None);
    }
}

/// Sleep/wake and own-menu notifications.
pub fn watch_system(_sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
    stub()
}

pub fn free_disk_bytes(_path: &Path) -> Result<u64> {
    stub()
}

/// `~/Movies/Simple Capture`.
pub fn default_projects_dir() -> Result<PathBuf> {
    stub()
}

pub fn local_now() -> LocalDateTime {
    LocalDateTime::default()
}

pub fn capabilities() -> Capabilities {
    Capabilities::none()
}
