//! Synthetic platform (plan §4.3, §6.2): deterministic sources that behave like
//! ScreenCaptureKit where it matters, on a host clock that can run faster than real time.
//!
//! PTS are **scripted** values on the scenario's host clock, never the time a buffer happened
//! to be delivered. One scheduler thread per platform delivers every sample (screen, camera,
//! audio, system events) in PTS order when the host clock reaches it, so runs are reproducible.
//!
//! Time bases: the host clock starts at [`HOST_BASE_NS`] when the platform is created. Screen
//! `idle`, `lost_after_ns` and audio `beeps_ns`/`gaps`/`overlaps` are offsets from that
//! stream's `start()`. [`Scenario::at`] offsets are from the `watch_system` call. Disk scripts
//! run from platform creation.
//!
//! Source names: `display_spec(NAME, size)` gives `synthetic:NAME[:WxH]`; a region of NAME uses
//! the script `NAME/region` when present, else NAME's; `SourceSpec::Window` uses `window:ID`;
//! `SourceSpec::Application` uses `app:PID`. Unknown names get `ScreenScript::default()`.
//! Frame codes carry `source_hash(name)` of that key, the schedule index and `pts_ms`.
//!
//! # Examples
//!
//! **Time scale**: `host_now` and `host_duration` agree; 10 host seconds take 1 real second.
//! ```
//! use sc_platform::Platform;
//! use sc_platform::synthetic::{Scenario, SyntheticPlatform};
//! let p = SyntheticPlatform::new(Scenario::new().time_scale(10.0));
//! assert_eq!(p.host_duration(10_000_000_000), std::time::Duration::from_secs(1));
//! ```
//!
//! **Screens, on demand, coded frames**: start any `synthetic:NAME`; every frame carries a code.
//! ```
//! use std::sync::Arc;
//! use std::time::Duration;
//! use sc_platform::{NativeFrame, Platform, ScreenStreamConfig, Resolution, StreamTag};
//! use sc_platform::synthetic::{Collector, Scenario, SyntheticPlatform, display_spec, read_code,
//!     source_hash};
//! let p = SyntheticPlatform::new(Scenario::new().time_scale(20.0));
//! let cfg = ScreenStreamConfig { fps: 30, resolution: Resolution::Native, show_cursor: false,
//!     show_clicks: false, queue_depth: 6, exclude: vec![] };
//! let sink = Collector::new();
//! let spec = display_spec("anything", Some(sc_geometry::SizePx { w: 640, h: 360 }));
//! let s = p.screen().start(StreamTag(1), &spec, &cfg, sink.clone()).unwrap();
//! assert!(sink.wait_until(Duration::from_secs(5), |c| c.screen().len() >= 3));
//! s.stop().unwrap();
//! let f = &sink.screen()[0];
//! let mut code = None;
//! f.buffer.0.with_nv12(&mut |v| code = read_code(&v)).unwrap();
//! assert_eq!(code.unwrap().source_hash, source_hash("anything"));
//! ```
//!
//! **ScreenScript**: latency, idle spans, idle from start, start failures, loss, trailing frames,
//! a first frame whose size differs from `describe()`, and an application crop that moves.
//! ```
//! use sc_geometry::SizePx;
//! use sc_platform::synthetic::{Scenario, ScreenScript, StartFailure};
//! let s = Scenario::new()
//!     .screen("slow", ScreenScript { first_frame_latency_ms: 400, ..Default::default() })
//!     .screen("idle", ScreenScript { idle: vec![(1_000_000_000, 500_000_000)], ..Default::default() })
//!     .screen("frozen", ScreenScript { idle_from_start: true, ..Default::default() })
//!     .screen("broken", ScreenScript { start_fails: StartFailure::Always, ..Default::default() })
//!     .screen("shy", ScreenScript { start_fails: StartFailure::WhileOtherStreamRuns, ..Default::default() })
//!     .screen("lost", ScreenScript { lost_after_ns: Some(2_000_000_000), ..Default::default() })
//!     // after a newer stream's first frame, deliver 2 more frames, then go quiet
//!     .screen("old", ScreenScript { trailing_after_new_stream: 2, ..Default::default() })
//!     .screen("retina", ScreenScript { first_frame_size: Some(SizePx { w: 2560, h: 1440 }),
//!         ..Default::default() });
//! # let _ = s;
//! ```
//!
//! **Camera**: frames coded with `source_hash("camera")`, capped by `CameraLimit`.
//! ```
//! use sc_geometry::SizePx;
//! use sc_platform::synthetic::{Scenario, VideoScript, CAMERA_DEVICE_ID, SyntheticPlatform};
//! use sc_platform::Platform;
//! let p = SyntheticPlatform::new(Scenario::new()
//!     .camera(Some(VideoScript { size: SizePx { w: 1280, h: 720 }, fps: 30 })));
//! assert_eq!(p.camera().devices().unwrap()[0].id, CAMERA_DEVICE_ID);
//! ```
//!
//! **Audio**: mic and system audio with beeps, injected gaps and overlaps (`(at, dur)`), and a
//! device clock `clock_ppm` fast or slow; see [`audio_buffer_pts`] for the exact PTS.
//! ```
//! use sc_platform::synthetic::{AudioScript, Scenario, audio_buffer_pts};
//! let mic = AudioScript { rate: 48_000, channels: 1, buffer_frames: 480,
//!     beeps_ns: vec![1_000_000_000], gaps: vec![(500_000_000, 40_000_000)],
//!     overlaps: vec![(800_000_000, 20_000_000)], clock_ppm: 100.0 };
//! // 60 s of buffers at +100 ppm, minus the overlap, plus the gap: 6 ms + 20 ms late
//! let k = 6000;
//! assert_eq!(audio_buffer_pts(&mic, 0, k), 60_000_000_000 + 6_000_000 + 40_000_000 - 20_000_000);
//! let s = Scenario::new().mic(Some(mic.clone())).system_audio(Some(mic));
//! # let _ = s;
//! ```
//!
//! **Permissions**: `status` reports the script; `start` of a denied source fails fast.
//! ```
//! use sc_platform::{Error, PermissionKind, PermissionStatus as P, Platform};
//! use sc_platform::synthetic::{Collector, Scenario, SyntheticPlatform, display_spec};
//! let p = SyntheticPlatform::new(Scenario::new().permissions(P::Denied, P::Granted, P::NotDetermined));
//! assert_eq!(p.permissions().status(PermissionKind::Screen), P::Denied);
//! let cfg = sc_platform::ScreenStreamConfig { fps: 30, resolution: sc_platform::Resolution::Native,
//!     show_cursor: false, show_clicks: false, queue_depth: 6, exclude: vec![] };
//! let r = p.screen().start(sc_platform::StreamTag(1), &display_spec("a", None), &cfg, Collector::new());
//! assert!(matches!(r, Err(Error::PermissionDenied(PermissionKind::Screen))));
//! ```
//!
//! **Disk**: constant, or a linear fall in host time.
//! ```
//! use sc_platform::Platform;
//! use sc_platform::synthetic::{DiskScript, Scenario, SyntheticPlatform};
//! let p = SyntheticPlatform::new(Scenario::new()
//!     .disk(DiskScript::Linear { start: 1 << 30, bytes_per_s: -(1 << 20) }));
//! assert!(p.free_disk_bytes(std::path::Path::new("/")).unwrap() <= 1 << 30);
//! ```
//!
//! **Scripted events** through `watch_system`: sleep and wake, device lost (that device's
//! stream stops and it leaves `devices()` until `DevicesChanged`), own windows changed.
//! ```
//! use std::time::Duration;
//! use sc_platform::{DeviceKind, Platform, SourceEvent};
//! use sc_platform::synthetic::{Collector, Scenario, ScriptedEvent, SyntheticPlatform};
//! let p = SyntheticPlatform::new(Scenario::new().time_scale(10.0)
//!     .at(100_000_000, ScriptedEvent::DeviceLost(DeviceKind::Mic))
//!     .at(200_000_000, ScriptedEvent::WillSleep));
//! let sink = Collector::new();
//! let w = p.watch_system(sink.clone()).unwrap();
//! assert!(sink.wait_until(Duration::from_secs(5), |c| c.events().len() == 2));
//! assert_eq!(sink.events()[1], SourceEvent::SystemWillSleep);
//! w.stop().unwrap();
//! ```
//!
//! **Real writers**: `with_writers` keeps the synthetic sources and swaps the track files, e.g.
//! `SyntheticPlatform::with_writers(s, sc_capture_macos::writers(), sc_capture_macos::decoder())`.
//! ```
//! use std::sync::Arc;
//! use sc_platform::Platform;
//! use sc_platform::synthetic::{Scenario, SctrkDecoder, SctrkWriterFactory, SyntheticPlatform};
//! let p = SyntheticPlatform::with_writers(Scenario::new(), Arc::new(SctrkWriterFactory),
//!     Arc::new(SctrkDecoder));
//! assert_eq!(p.writers().extension(), "sctrk");
//! ```

mod code;
mod hub;
mod sctrk;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sc_core::ClockKind;
use sc_core::folder::LocalDateTime;
use sc_core::time::HostTime;
use sc_geometry::{RectPt, SizePx};

use crate::unsupported::Nothing;
use crate::{
    AppRef, AudioBuffer, AudioSource, AudioTrackKind, CameraSource, Capabilities, Decoder, Desktop,
    DeviceKind, DeviceStream, DisplayId, ExclusionKind, FrameSink, OverlayWindows,
    PermissionStatus, Permissions, Platform, Result, ScreenSource, SourceEvent, SourcePicker,
    SourceSpec, StreamTag, SystemAudioSource, VideoFrame, WindowId, WriterFactory,
};

pub use code::{
    CODE_BITS, CODE_BLOCK_PX, FrameCode, background, code_layout, coded_frame, pts_ms, read_code,
    source_hash, write_code,
};
pub use hub::{
    BEEP_AMPLITUDE, BEEP_HZ, BEEP_NS, CAMERA_DEVICE_ID, CAMERA_SOURCE_NAME, CAMERA_TAG_BASE,
    HOST_BASE_NS, MIC_DEVICE_ID, apply_camera_limit, audio_buffer_pts,
};
pub use sctrk::{SctrkDecoder, SctrkWriterFactory};

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
    /// Capped by `ScreenStreamConfig::fps` when that is lower.
    pub fps: u32,
    pub first_frame_latency_ms: u32,
    /// (at, dur) spans with no frames.
    pub idle: Vec<(i64, i64)>,
    /// No frame is ever delivered (a never-completing first frame).
    pub idle_from_start: bool,
    pub start_fails: StartFailure,
    /// `SourceLost` at this offset; no frames from then on.
    pub lost_after_ns: Option<i64>,
    /// Once a newer screen stream delivers its first frame (PTS `t1`), this stream delivers
    /// exactly this many more frames (all with PTS ≥ `t1`) and then none, so a router's
    /// trailing-frame count is deterministic.
    pub trailing_after_new_stream: u32,
    /// The size every delivered frame really has, when it differs from `describe()`.
    pub first_frame_size: Option<SizePx>,
    /// An application source's `crop_pt` from successive `describe()` calls (the last one
    /// repeats), as its windows move; empty: no crop.
    pub describe_crops: Vec<RectPt>,
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
            describe_crops: Vec::new(),
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
    /// The newest screen stream's content becomes this size (points at scale 1).
    Resize(SizePx),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    time_scale: f64,
    screens: Vec<(String, ScreenScript)>,
    camera: Option<VideoScript>,
    mic: Option<AudioScript>,
    system_audio: Option<AudioScript>,
    device_audio: Option<AudioScript>,
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
            device_audio: None,
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

    /// Sound the camera device sends with its pictures, like an iPhone's screen over USB.
    pub fn device_audio(mut self, a: Option<AudioScript>) -> Self {
        self.device_audio = a;
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

#[derive(Default)]
struct Collected {
    screen: Vec<VideoFrame>,
    camera: Vec<VideoFrame>,
    audio: Vec<(AudioTrackKind, AudioBuffer)>,
    events: Vec<SourceEvent>,
}

/// A `FrameSink` that keeps everything it receives, for tests.
#[derive(Default)]
pub struct Collector {
    got: Mutex<Collected>,
    cv: Condvar,
}

impl Collector {
    pub fn new() -> Arc<Collector> {
        Arc::new(Collector::default())
    }

    fn with<T>(&self, f: impl FnOnce(&mut Collected) -> T) -> T {
        let mut g = self.got.lock().unwrap_or_else(PoisonError::into_inner);
        let out = f(&mut g);
        self.cv.notify_all();
        out
    }

    pub fn screen(&self) -> Vec<VideoFrame> {
        self.with(|c| c.screen.clone())
    }

    pub fn camera(&self) -> Vec<VideoFrame> {
        self.with(|c| c.camera.clone())
    }

    pub fn audio(&self) -> Vec<(AudioTrackKind, AudioBuffer)> {
        self.with(|c| c.audio.clone())
    }

    pub fn events(&self) -> Vec<SourceEvent> {
        self.with(|c| c.events.clone())
    }

    /// Waits (real time) until `done(self)` holds; false on timeout.
    pub fn wait_until(&self, timeout: Duration, done: impl Fn(&Collector) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if done(self) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let g = self.got.lock().unwrap_or_else(PoisonError::into_inner);
            let wait = (deadline - now).min(Duration::from_millis(20));
            drop(self.cv.wait_timeout(g, wait));
        }
    }
}

impl FrameSink for Collector {
    fn on_screen(&self, f: VideoFrame) {
        self.with(|c| c.screen.push(f));
    }
    fn on_camera(&self, f: VideoFrame) {
        self.with(|c| c.camera.push(f));
    }
    fn on_audio(&self, track: AudioTrackKind, a: AudioBuffer) {
        self.with(|c| c.audio.push((track, a)));
    }
    fn on_event(&self, e: SourceEvent) {
        self.with(|c| c.events.push(e));
    }
}

pub struct SyntheticPlatform {
    hub: hub::HubRef,
    nothing: Nothing,
    writers: Arc<dyn WriterFactory>,
    decoder: Arc<dyn Decoder>,
}

impl std::fmt::Debug for SyntheticPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyntheticPlatform").finish_non_exhaustive()
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
            hub: hub::HubRef(Arc::new(hub::Hub::new(s))),
            nothing: Nothing::new("synthetic: no system picker or overlay windows"),
            writers: w,
            decoder: d,
        })
    }

    /// Screen streams started and not yet stopped (or dropped).
    pub fn active_screen_streams(&self) -> usize {
        self.hub.0.active_screen_streams()
    }

    /// Every `SystemAudioStream::follow_app` argument, in call order.
    pub fn followed_apps(&self) -> Vec<Option<AppRef>> {
        self.hub.0.followed_apps()
    }

    /// Every `ScreenStream::update_exclusions` call, in order.
    pub fn exclusion_updates(&self) -> Vec<(StreamTag, Vec<WindowId>)> {
        self.hub.0.exclusion_updates()
    }
}

impl Platform for SyntheticPlatform {
    fn name(&self) -> &'static str {
        "synthetic"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            system_picker: false,
            application_source: true,
            app_audio: true,
            show_clicks: false,
            mic_modes: false,
            switch_source: true,
            exclusion: ExclusionKind::PerWindow,
        }
    }
    fn clock_kind(&self) -> ClockKind {
        ClockKind::ClockMonotonic
    }
    fn host_now(&self) -> HostTime {
        HostTime(self.hub.0.clock.now())
    }
    fn host_duration(&self, ns: i64) -> Duration {
        self.hub.0.clock.real(ns)
    }
    /// 2026-01-01 00:00:00 UTC plus the host time elapsed, so successive projects get distinct
    /// folder names.
    fn local_now(&self) -> LocalDateTime {
        let secs = (self.hub.0.clock.now() - HOST_BASE_NS).max(0) / 1_000_000_000;
        LocalDateTime {
            y: 2026,
            mo: 1,
            d: 1 + (secs / 86_400).min(27) as u8,
            h: (secs / 3600 % 24) as u8,
            mi: (secs / 60 % 60) as u8,
            s: (secs % 60) as u8,
            offset_min: 0,
        }
    }
    fn screen(&self) -> &dyn ScreenSource {
        &self.hub
    }
    fn system_audio(&self) -> &dyn SystemAudioSource {
        &self.hub
    }
    fn mic(&self) -> &dyn AudioSource {
        &self.hub
    }
    fn camera(&self) -> &dyn CameraSource {
        &self.hub
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
        &self.hub
    }
    fn overlays(&self) -> &dyn OverlayWindows {
        &self.nothing
    }
    fn desktop(&self) -> &dyn Desktop {
        &self.hub
    }
    fn watch_system(&self, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
        self.hub.0.watch_system(sink)
    }
    fn free_disk_bytes(&self, _path: &Path) -> Result<u64> {
        Ok(self.hub.0.free_disk_bytes())
    }
    fn default_projects_dir(&self) -> Result<PathBuf> {
        Ok(std::env::temp_dir().join("simple-capture synthetic"))
    }
    fn permission_host(&self) -> Option<String> {
        None
    }
}
