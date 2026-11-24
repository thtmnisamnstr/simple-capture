mod camera;
mod desktop;
mod enumerate;
mod mf;
mod overlays;
mod sys;
mod wgc;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sc_capture_common::audio::{CpalMic, CpalSystemAudio, mic_devices};
use sc_capture_common::watch::Watcher;
use sc_core::image::RgbaImage;
use sc_core::project::SourceDescriptor;
use sc_geometry::SizePx;
use sc_platform::unsupported::Nothing;
use sc_platform::{
    AudioSource, CameraLimit, CameraSource, Capabilities, CaptureDevice, ClockKind, Decoder,
    Desktop, DeviceStream, DisplayInfo, ExclusionKind, FrameSink, HostTime, LocalDateTime,
    OverlayWindows, PermissionKind, PermissionStatus, Permissions, Platform, Result, ScreenSource,
    ScreenStream, ScreenStreamConfig, SourceList, SourcePicker, SourceSpec, StreamTag,
    SystemAudioSource, WindowId, WriterFactory,
};
use sc_tracks::{HelperDecoder, HelperWriters};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

struct Screen;

impl ScreenSource for Screen {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        enumerate::displays()
    }

    fn list(&self, hide: &[WindowId]) -> Result<SourceList> {
        enumerate::list(hide)
    }

    fn describe(
        &self,
        spec: &SourceSpec,
        cfg: &ScreenStreamConfig,
    ) -> Result<(SourceDescriptor, SizePx)> {
        let p = wgc::plan(spec, cfg)?;
        Ok((p.descriptor, p.out))
    }

    fn start(
        &self,
        tag: StreamTag,
        spec: &SourceSpec,
        cfg: &ScreenStreamConfig,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn ScreenStream>> {
        let p = wgc::plan(spec, cfg)?;
        Ok(Box::new(wgc::start(tag, p, cfg, sink)?))
    }

    fn snapshot(&self, spec: &SourceSpec, max: SizePx) -> Result<RgbaImage> {
        let cfg = ScreenStreamConfig {
            fps: 30,
            resolution: sc_platform::Resolution::Native,
            show_cursor: false,
            show_clicks: false,
            queue_depth: 3,
            exclude: Vec::new(),
        };
        wgc::snapshot(wgc::plan(spec, &cfg)?, max)
    }
}

struct Camera;

impl CameraSource for Camera {
    fn devices(&self) -> Result<Vec<CaptureDevice>> {
        camera::devices()
    }

    fn start(
        &self,
        device_id: &str,
        limit: CameraLimit,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn DeviceStream>> {
        Ok(Box::new(camera::start(device_id, limit, sink)?))
    }
}

struct Perms;

impl Permissions for Perms {
    fn status(&self, k: PermissionKind) -> PermissionStatus {
        sys::permission(k)
    }

    fn request(&self, k: PermissionKind, done: Box<dyn FnOnce(PermissionStatus) + Send>) {
        done(sys::permission(k));
    }

    fn settings_url(&self, k: PermissionKind) -> Option<String> {
        sys::settings_url(k)
    }
}

pub struct WindowsPlatform {
    screen: Screen,
    mic: CpalMic,
    system_audio: CpalSystemAudio,
    camera: Camera,
    picker: Nothing,
    perms: Perms,
    overlays: overlays::Overlays,
    desktop: desktop::WinDesktop,
    writers: Arc<dyn WriterFactory>,
    decoder: Arc<dyn Decoder>,
}

/// Media Foundation records in process (experimental); `SC_MEDIA=helper` or a system without it falls
/// back to the FFmpeg helper.
fn media_writers(ffmpeg: &Path) -> Arc<dyn WriterFactory> {
    let forced = std::env::var("SC_MEDIA").is_ok_and(|v| v.eq_ignore_ascii_case("helper"));
    let mf = mf::MfWriters {
        helper: HelperWriters::new(ffmpeg.to_path_buf()),
    };
    if !forced && mf.usable() {
        tracing::info!("tracks: Media Foundation (experimental)");
        return Arc::new(mf);
    }
    tracing::info!("tracks: FFmpeg helper");
    Arc::new(HelperWriters::new(ffmpeg.to_path_buf()))
}

pub fn platform() -> Arc<dyn Platform> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let ffmpeg = sc_helpers::locate(sc_helpers::HelperKind::Ffmpeg)
        .unwrap_or_else(|_| PathBuf::from("ffmpeg.exe"));
    Arc::new(WindowsPlatform {
        screen: Screen,
        mic: CpalMic {
            clock: sys::host_now,
        },
        system_audio: CpalSystemAudio {
            clock: sys::host_now,
        },
        camera: Camera,
        picker: Nothing::new("the system picker"),
        perms: Perms,
        overlays: overlays::Overlays::default(),
        desktop: desktop::WinDesktop,
        writers: media_writers(&ffmpeg),
        decoder: Arc::new(mf::MfDecoder::new(Arc::new(HelperDecoder::new(ffmpeg)))),
    })
}

impl Platform for WindowsPlatform {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            system_picker: false,
            application_source: true,
            app_audio: false,
            show_clicks: false,
            mic_modes: false,
            switch_source: true,
            exclusion: ExclusionKind::PerWindow,
        }
    }

    fn clock_kind(&self) -> ClockKind {
        ClockKind::Qpc
    }

    fn host_now(&self) -> HostTime {
        sys::host_now()
    }

    fn host_duration(&self, ns: i64) -> Duration {
        Duration::from_nanos(u64::try_from(ns).unwrap_or(0))
    }

    fn local_now(&self) -> LocalDateTime {
        sys::local_now()
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
        &self.perms
    }

    fn overlays(&self) -> &dyn OverlayWindows {
        &self.overlays
    }

    fn desktop(&self) -> &dyn Desktop {
        &self.desktop
    }

    fn watch_system(&self, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
        let ids = |v: Result<Vec<CaptureDevice>>| {
            v.map(|d| d.into_iter().map(|d| d.id).collect())
                .unwrap_or_default()
        };
        Ok(Box::new(Watcher::start(
            sink,
            Box::new(move || ids(mic_devices())),
            Box::new(move || ids(camera::devices())),
        )?))
    }

    fn free_disk_bytes(&self, path: &Path) -> Result<u64> {
        sys::free_disk_bytes(path)
    }

    fn default_projects_dir(&self) -> Result<PathBuf> {
        sys::default_projects_dir()
    }

    fn permission_host(&self) -> Option<String> {
        None
    }
}
