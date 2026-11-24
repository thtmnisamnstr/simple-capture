mod camera;
mod desktop;
mod gst;
mod mjpeg;
mod portal;
mod pw;
mod sys;

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sc_capture_common::audio::{CpalMic, CpalSystemAudio, mic_devices};
use sc_capture_common::watch::Watcher;
use sc_core::image::RgbaImage;
use sc_core::project::SourceDescriptor;
use sc_geometry::SizePx;
use sc_platform::{
    AudioSource, CameraLimit, CameraSource, Capabilities, CaptureDevice, ClockKind, Decoder,
    Desktop, DeviceStream, DisplayInfo, Error, ExclusionKind, FrameSink, HostTime, LocalDateTime,
    OverlayRole, OverlayWindows, PermissionKind, PermissionStatus, Permissions, Platform, Result,
    ScreenSource, ScreenStream, ScreenStreamConfig, SourceList, SourcePicker, SourceSpec,
    StreamTag, SystemAudioSource, WindowId, WriterFactory,
};
use sc_tracks::{HelperDecoder, HelperWriters};

struct Screen;

impl ScreenSource for Screen {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(Vec::new())
    }

    fn list(&self, _hide: &[WindowId]) -> Result<SourceList> {
        Ok(SourceList::default())
    }

    fn describe(
        &self,
        spec: &SourceSpec,
        _cfg: &ScreenStreamConfig,
    ) -> Result<(SourceDescriptor, SizePx)> {
        let c = portal::cast_of(spec)?;
        Ok((portal::descriptor(&c), c.size))
    }

    fn start(
        &self,
        tag: StreamTag,
        spec: &SourceSpec,
        cfg: &ScreenStreamConfig,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn ScreenStream>> {
        let c = portal::cast_of(spec)?;
        let fd = c.remote()?;
        Ok(Box::new(pw::start(
            Some(fd),
            c.node,
            tag,
            c.size,
            cfg.fps,
            sink,
        )?))
    }

    fn snapshot(&self, _spec: &SourceSpec, _max: SizePx) -> Result<RgbaImage> {
        Err(Error::Unsupported("snapshots of portal sources"))
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
    fn status(&self, _k: PermissionKind) -> PermissionStatus {
        PermissionStatus::Granted
    }

    fn request(&self, _k: PermissionKind, done: Box<dyn FnOnce(PermissionStatus) + Send>) {
        done(PermissionStatus::Granted);
    }

    fn settings_url(&self, _k: PermissionKind) -> Option<String> {
        None
    }
}

#[derive(Default)]
struct Overlays {
    next: AtomicU32,
    ids: Mutex<Vec<WindowId>>,
}

impl OverlayWindows for Overlays {
    unsafe fn configure(&self, _raw_window: *mut c_void, _role: OverlayRole) -> Result<WindowId> {
        let id = WindowId(0x7000_0000 + self.next.fetch_add(1, Ordering::Relaxed));
        self.ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(id);
        Ok(id)
    }

    fn registered(&self) -> Vec<WindowId> {
        self.ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

pub struct LinuxPlatform {
    screen: Screen,
    mic: CpalMic,
    system_audio: CpalSystemAudio,
    camera: Camera,
    picker: portal::PortalPicker,
    perms: Perms,
    overlays: Overlays,
    desktop: desktop::LinuxDesktop,
    writers: Arc<dyn WriterFactory>,
    decoder: Arc<dyn Decoder>,
}

/// A screen stream from any PipeWire video node on the user's daemon (tests and diagnostics).
pub fn node_stream(
    node: u32,
    tag: StreamTag,
    out: SizePx,
    fps: u32,
    sink: Arc<dyn FrameSink>,
) -> Result<Box<dyn ScreenStream>> {
    Ok(Box::new(pw::start(None, node, tag, out, fps, sink)?))
}

pub fn platform() -> Arc<dyn Platform> {
    let ffmpeg = sc_helpers::locate(sc_helpers::HelperKind::Ffmpeg)
        .unwrap_or_else(|_| PathBuf::from("ffmpeg"));
    Arc::new(LinuxPlatform {
        screen: Screen,
        mic: CpalMic {
            clock: sys::host_now,
        },
        system_audio: CpalSystemAudio {
            clock: sys::host_now,
        },
        camera: Camera,
        picker: portal::PortalPicker,
        perms: Perms,
        overlays: Overlays::default(),
        desktop: desktop::LinuxDesktop,
        writers: media_writers(&ffmpeg),
        decoder: Arc::new(HelperDecoder::new(ffmpeg)),
    })
}

/// The screen track through GStreamer when it has a hardware or OpenH264 encoder (experimental);
/// `SC_MEDIA=helper`, or no such encoder, records with the FFmpeg helper.
fn media_writers(ffmpeg: &Path) -> Arc<dyn WriterFactory> {
    let helper = HelperWriters::new(ffmpeg.to_path_buf());
    let forced = std::env::var("SC_MEDIA").is_ok_and(|v| v.eq_ignore_ascii_case("helper"));
    if !forced && let Some(encoder) = gst::usable_encoder() {
        tracing::info!(encoder, "tracks: GStreamer (experimental)");
        return Arc::new(gst::GstWriters { encoder, helper });
    }
    tracing::info!("tracks: FFmpeg helper");
    Arc::new(helper)
}

impl Platform for LinuxPlatform {
    fn name(&self) -> &'static str {
        "linux"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            system_picker: true,
            application_source: false,
            app_audio: false,
            show_clicks: false,
            mic_modes: false,
            switch_source: true,
            exclusion: ExclusionKind::AutoHide,
        }
    }

    fn clock_kind(&self) -> ClockKind {
        ClockKind::ClockMonotonic
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
        Ok(sys::videos_dir().join("Simple Capture"))
    }

    fn permission_host(&self) -> Option<String> {
        None
    }
}
