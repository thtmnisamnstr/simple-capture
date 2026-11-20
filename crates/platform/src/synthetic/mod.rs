//! Synthetic platform (plan §4.3, §6.2). PTS are scripted values on the scenario's host clock,
//! never the time a buffer happened to be delivered. Stubs until wp-platform-synth.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sc_core::ClockKind;
use sc_core::folder::LocalDateTime;
use sc_core::time::HostTime;
use sc_geometry::SizePx;

use crate::unsupported::Nothing;
use crate::{
    AudioReader, AudioSource, AudioWriterSpec, CameraSource, Capabilities, Decoder, Desktop,
    DeviceKind, DeviceStream, DisplayId, Error, FrameSink, Nv12View, OverlayWindows,
    PermissionStatus, Permissions, Platform, Result, ScreenSource, SourcePicker, SourceSpec,
    SystemAudioSource, TrackProbe, TrackWriter, VideoCodec, VideoReader, VideoWriterSpec,
    WriterFactory,
};

const STUB: &str = "sc_platform::synthetic: not implemented yet";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartFailure {
    #[default]
    Never,
    Always,
    WhileOtherStreamRuns,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenScript {
    pub size: SizePx,
    pub fps: u32,
    pub first_frame_latency_ms: u32,
    /// (at, dur) spans with no frames.
    pub idle: Vec<(i64, i64)>,
    pub idle_from_start: bool,
    pub start_fails: StartFailure,
    pub lost_after_ns: Option<i64>,
    pub trailing_after_new_stream: u32,
    /// Differs from `describe()` when set.
    pub first_frame_size: Option<SizePx>,
}

impl Default for ScreenScript {
    fn default() -> Self {
        ScreenScript {
            size: SizePx { w: 1280, h: 720 },
            fps: 30,
            first_frame_latency_ms: 30,
            idle: Vec::new(),
            idle_from_start: false,
            start_fails: StartFailure::Never,
            lost_after_ns: None,
            trailing_after_new_stream: 0,
            first_frame_size: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoScript {
    pub size: SizePx,
    pub fps: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioScript {
    pub rate: u32,
    pub channels: u16,
    pub buffer_frames: u32,
    pub beeps_ns: Vec<i64>,
    /// (at, dur)
    pub gaps: Vec<(i64, i64)>,
    /// (at, dur)
    pub overlaps: Vec<(i64, i64)>,
    pub clock_ppm: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiskScript {
    Constant(u64),
    Linear { start: u64, bytes_per_s: i64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScriptedEvent {
    WillSleep,
    DidWake,
    DeviceLost(DeviceKind),
    DevicesChanged(DeviceKind),
    OwnWindowsChanged,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    time_scale: f64,
    screens: Vec<(String, ScreenScript)>,
    camera: Option<VideoScript>,
    mic: Option<AudioScript>,
    system_audio: Option<AudioScript>,
    permissions: [PermissionStatus; 3],
    disk: DiskScript,
    events: Vec<(i64, ScriptedEvent)>,
}

impl Default for Scenario {
    fn default() -> Self {
        Scenario::new()
    }
}

impl Scenario {
    /// Time scale 1.0, permissions granted.
    pub fn new() -> Scenario {
        Scenario {
            time_scale: 1.0,
            screens: Vec::new(),
            camera: None,
            mic: None,
            system_audio: None,
            permissions: [PermissionStatus::Granted; 3],
            disk: DiskScript::Constant(u64::MAX / 2),
            events: Vec::new(),
        }
    }

    /// The host clock runs `s`× real time.
    pub fn time_scale(mut self, s: f64) -> Self {
        self.time_scale = s;
        self
    }

    /// Unknown names are created on demand.
    pub fn screen(mut self, name: &str, s: ScreenScript) -> Self {
        self.screens.push((name.to_string(), s));
        self
    }

    pub fn camera(mut self, c: Option<VideoScript>) -> Self {
        self.camera = c;
        self
    }

    pub fn mic(mut self, a: Option<AudioScript>) -> Self {
        self.mic = a;
        self
    }

    pub fn system_audio(mut self, a: Option<AudioScript>) -> Self {
        self.system_audio = a;
        self
    }

    pub fn permissions(
        mut self,
        screen: PermissionStatus,
        camera: PermissionStatus,
        mic: PermissionStatus,
    ) -> Self {
        self.permissions = [screen, camera, mic];
        self
    }

    pub fn disk(mut self, d: DiskScript) -> Self {
        self.disk = d;
        self
    }

    pub fn at(mut self, after_start_ns: i64, e: ScriptedEvent) -> Self {
        self.events.push((after_start_ns, e));
        self
    }
}

/// Frame identity encoded in 16×16 luma blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameCode {
    pub source_hash: u32,
    pub index: u32,
    pub pts_ms: u32,
}

/// Stub until wp-platform-synth.
pub fn source_hash(_name: &str) -> u32 {
    0
}

/// 16×16 luma blocks, mid-grey threshold; must survive HEVC at default bits_per_pixel.
/// Stub until wp-platform-synth.
pub fn read_code(_v: &Nv12View) -> Option<FrameCode> {
    None
}

/// `DisplayId("synthetic:NAME[:WxH]")`.
pub fn display_spec(name: &str, size: Option<SizePx>) -> SourceSpec {
    let id = match size {
        Some(s) => format!("synthetic:{name}:{}x{}", s.w, s.h),
        None => format!("synthetic:{name}"),
    };
    SourceSpec::Display {
        display: DisplayId(id),
    }
}

pub struct SyntheticPlatform {
    scenario: Scenario,
    nothing: Nothing,
    writers: Arc<dyn WriterFactory>,
    decoder: Arc<dyn Decoder>,
}

impl std::fmt::Debug for SyntheticPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyntheticPlatform")
            .field("scenario", &self.scenario)
            .finish_non_exhaustive()
    }
}

impl SyntheticPlatform {
    pub fn new(s: Scenario) -> Arc<SyntheticPlatform> {
        Self::with_writers(s, Arc::new(SctrkWriterFactory), Arc::new(SctrkDecoder))
    }

    pub fn with_writers(
        s: Scenario,
        w: Arc<dyn WriterFactory>,
        d: Arc<dyn Decoder>,
    ) -> Arc<SyntheticPlatform> {
        Arc::new(SyntheticPlatform {
            scenario: s,
            nothing: Nothing::new(STUB),
            writers: w,
            decoder: d,
        })
    }

    pub fn active_screen_streams(&self) -> usize {
        0
    }
}

impl Platform for SyntheticPlatform {
    fn name(&self) -> &'static str {
        "synthetic"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::none()
    }
    fn clock_kind(&self) -> ClockKind {
        ClockKind::ClockMonotonic
    }
    fn host_now(&self) -> HostTime {
        HostTime(0)
    }
    fn host_duration(&self, ns: i64) -> Duration {
        let real = (ns.max(0) as f64) / self.scenario.time_scale.max(f64::MIN_POSITIVE);
        Duration::from_nanos(real as u64)
    }
    fn local_now(&self) -> LocalDateTime {
        LocalDateTime {
            y: 2026,
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
        self.writers.clone()
    }
    fn decoder(&self) -> Arc<dyn Decoder> {
        self.decoder.clone()
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
        Err(Error::Unsupported(STUB))
    }
    fn free_disk_bytes(&self, _path: &Path) -> Result<u64> {
        Err(Error::Unsupported(STUB))
    }
    fn default_projects_dir(&self) -> Result<PathBuf> {
        Err(Error::Unsupported(STUB))
    }
    fn permission_host(&self) -> Option<String> {
        None
    }
}

/// Crash-tolerant `.sctrk` track files.
#[derive(Debug, Clone, Copy, Default)]
pub struct SctrkWriterFactory;

impl WriterFactory for SctrkWriterFactory {
    fn extension(&self) -> &'static str {
        "sctrk"
    }
    fn video(&self, _path: &Path, _spec: &VideoWriterSpec) -> Result<Box<dyn TrackWriter>> {
        Err(Error::Unsupported(STUB))
    }
    fn audio(&self, _path: &Path, _spec: &AudioWriterSpec) -> Result<Box<dyn TrackWriter>> {
        Err(Error::Unsupported(STUB))
    }
    fn encode_benchmark(
        &self,
        _size: SizePx,
        _fps: u32,
        _frames: u32,
        _codec: VideoCodec,
    ) -> Result<f64> {
        Err(Error::Unsupported(STUB))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SctrkDecoder;

impl Decoder for SctrkDecoder {
    fn open_video(&self, _path: &Path) -> Result<Box<dyn VideoReader>> {
        Err(Error::Unsupported(STUB))
    }
    fn open_audio(&self, _path: &Path, _rate: u32, _channels: u16) -> Result<Box<dyn AudioReader>> {
        Err(Error::Unsupported(STUB))
    }
    fn probe(&self, _path: &Path) -> Result<TrackProbe> {
        Err(Error::Unsupported(STUB))
    }
}
