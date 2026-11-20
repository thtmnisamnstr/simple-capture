use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_core::ClockKind;
use sc_core::folder::LocalDateTime;
use sc_core::image::RgbaImage;
use sc_core::project::{SourceDescriptor, SystemAudioMode};
use sc_core::time::HostTime;
use sc_geometry::SizePx;

use crate::{
    AppRef, Append, AudioBuffer, AudioTrackKind, AudioWriterSpec, CameraLimit, Capabilities,
    CaptureDevice, DecodedFrame, DisplayInfo, FinishedTrack, OverlayRole, PermissionKind,
    PermissionStatus, PickedSource, PickerRequest, Result, ScreenStreamConfig, SourceEvent,
    SourceList, SourceSpec, StreamTag, TrackProbe, VideoCodec, VideoFrame, VideoWriterSpec,
    WindowId,
};

/// Called from capture queues; must not block.
pub trait FrameSink: Send + Sync {
    fn on_screen(&self, f: VideoFrame);
    fn on_camera(&self, f: VideoFrame);
    fn on_audio(&self, track: AudioTrackKind, a: AudioBuffer);
    fn on_event(&self, e: SourceEvent);
}

pub trait ScreenSource: Send + Sync {
    /// CoreGraphics/NSScreen only: never touches TCC.
    fn displays(&self) -> Result<Vec<DisplayInfo>>;
    /// Needs the grant. Only windows a person would record, never `hide` or this process's; apps
    /// are those owning a listed window.
    fn list(&self, hide: &[WindowId]) -> Result<SourceList>;
    /// The owner of on-screen window `id`, whether `list` shows it or not.
    fn window_app(&self, id: WindowId) -> Result<Option<AppRef>> {
        Ok(self
            .list(&[])?
            .windows
            .into_iter()
            .find(|w| w.id == id)
            .map(|w| w.app))
    }
    fn describe(
        &self,
        spec: &SourceSpec,
        cfg: &ScreenStreamConfig,
    ) -> Result<(SourceDescriptor, SizePx)>;
    /// Returns once started.
    fn start(
        &self,
        tag: StreamTag,
        spec: &SourceSpec,
        cfg: &ScreenStreamConfig,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn ScreenStream>>;
    fn snapshot(&self, spec: &SourceSpec, max: SizePx) -> Result<RgbaImage>;
}

pub trait ScreenStream: Send {
    fn tag(&self) -> StreamTag;
    /// Excludes `exclude` ∪ every window this process owns now (fetched with
    /// onScreenWindowsOnly = false); waits for completion. Picked streams: Err(Unsupported)
    /// unless the rebuilt filter is known to keep picker consent (plan §10 item 10).
    fn update_exclusions(&self, exclude: &[WindowId]) -> Result<()>;
    fn stop(self: Box<Self>) -> Result<()>;
}

pub trait SystemAudioSource: Send + Sync {
    fn start(
        &self,
        mode: SystemAudioMode,
        exclude: &[WindowId],
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn SystemAudioStream>>;
}

pub trait SystemAudioStream: Send {
    /// App mode: `None` = all.
    fn follow_app(&self, app: Option<&AppRef>) -> Result<()>;
    fn stop(self: Box<Self>) -> Result<()>;
}

pub trait DeviceStream: Send {
    fn format(&self) -> String;
    fn stop(self: Box<Self>) -> Result<()>;
}

pub trait AudioSource: Send + Sync {
    fn devices(&self) -> Result<Vec<CaptureDevice>>;
    fn start(&self, device_id: &str, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>>;
    /// Main-queue hop.
    fn show_mic_modes(&self) -> Result<()>;
}

pub trait CameraSource: Send + Sync {
    fn devices(&self) -> Result<Vec<CaptureDevice>>;
    fn start(
        &self,
        device_id: &str,
        limit: CameraLimit,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn DeviceStream>>;
}

pub trait SourcePicker: Send + Sync {
    fn present(
        &self,
        req: PickerRequest,
        done: Box<dyn FnOnce(Result<Option<PickedSource>>) + Send>,
    );
    /// Menu-bar re-picks.
    fn set_observer(&self, on_pick: Option<Box<dyn Fn(PickedSource) + Send + Sync>>);
    fn set_active(&self, active: bool) -> Result<()>;
}

pub trait TrackWriter: Send {
    fn append_video(&mut self, f: &VideoFrame, file_ns: i64) -> Result<Append>;
    /// `skip_frames` = trimmed overlap.
    fn append_audio(&mut self, a: &AudioBuffer, file_ns: i64, skip_frames: u32) -> Result<Append>;
    /// Same format as the last audio.
    fn append_silence(&mut self, file_ns: i64, frames: u32) -> Result<Append>;
    fn is_ready(&self) -> bool;
    fn finish(self: Box<Self>, end_file_ns: Option<i64>) -> Result<FinishedTrack>;
}

pub trait WriterFactory: Send + Sync {
    /// `"mov"` (macOS), `"sctrk"` (synthetic).
    fn extension(&self) -> &'static str;
    /// The file exists after this returns.
    fn video(&self, path: &Path, spec: &VideoWriterSpec) -> Result<Box<dyn TrackWriter>>;
    fn audio(&self, path: &Path, spec: &AudioWriterSpec) -> Result<Box<dyn TrackWriter>>;
    fn encode_benchmark(
        &self,
        size: SizePx,
        fps: u32,
        frames: u32,
        codec: VideoCodec,
    ) -> Result<f64>;
}

pub trait VideoReader: Send {
    fn size(&self) -> SizePx;
    fn next_frame(&mut self) -> Result<Option<DecodedFrame>>;
}

pub trait AudioReader: Send {
    fn first_sample_file_ns(&self) -> i64;
    fn read(&mut self, out: &mut [f32]) -> Result<usize>;
}

pub trait Decoder: Send + Sync {
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoReader>>;
    fn open_audio(&self, path: &Path, rate: u32, channels: u16) -> Result<Box<dyn AudioReader>>;
    fn probe(&self, path: &Path) -> Result<TrackProbe>;
}

pub trait Permissions: Send + Sync {
    /// NEVER prompts.
    fn status(&self, k: PermissionKind) -> PermissionStatus;
    /// Prompts. Only from a user click.
    fn request(&self, k: PermissionKind, done: Box<dyn FnOnce(PermissionStatus) + Send>);
    fn settings_url(&self, k: PermissionKind) -> Option<String>;
}

pub trait OverlayWindows: Send + Sync {
    /// Main thread. Swaps the NSWindow to our NSPanel subclass (non-activating), forces creation
    /// of its window device, sets level/collection behaviour per role, registers it for exclusion
    /// and returns its windowNumber; Err if windowNumber <= 0.
    ///
    /// # Safety
    /// `raw_window` must be a valid NSWindow pointer (or the OS equivalent), used on the main
    /// thread.
    unsafe fn configure(&self, raw_window: *mut c_void, role: OverlayRole) -> Result<WindowId>;
    fn registered(&self) -> Vec<WindowId>;
}

pub trait Desktop: Send + Sync {
    fn reveal(&self, path: &Path) -> Result<()>;
    /// PNG/JPEG.
    fn choose_image(&self, done: Box<dyn FnOnce(Option<PathBuf>) + Send>);
}

/// M3; kept for ADR-007.
pub trait PreviewSink: Send + Sync {}

pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;
    fn clock_kind(&self) -> ClockKind;
    fn host_now(&self) -> HostTime;
    /// Real wait for a host-clock span.
    fn host_duration(&self, ns: i64) -> std::time::Duration;
    fn local_now(&self) -> LocalDateTime;
    fn screen(&self) -> &dyn ScreenSource;
    fn system_audio(&self) -> &dyn SystemAudioSource;
    fn mic(&self) -> &dyn AudioSource;
    fn camera(&self) -> &dyn CameraSource;
    fn picker(&self) -> &dyn SourcePicker;
    fn writers(&self) -> Arc<dyn WriterFactory>;
    fn decoder(&self) -> Arc<dyn Decoder>;
    fn permissions(&self) -> &dyn Permissions;
    fn overlays(&self) -> &dyn OverlayWindows;
    fn desktop(&self) -> &dyn Desktop;
    /// Devices, sleep/wake, own menus.
    fn watch_system(&self, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>>;
    fn free_disk_bytes(&self, path: &Path) -> Result<u64>;
    /// Movies/Simple Capture.
    fn default_projects_dir(&self) -> Result<PathBuf>;
    /// `"com.simplecapture.dev"` when SC_DEV_HOST=1.
    fn permission_host(&self) -> Option<String>;
}
