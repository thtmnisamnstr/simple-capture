//! The synthetic host clock, the one scheduler thread that delivers every scripted sample in
//! PTS order, and the source traits built on it.

use std::cmp::Reverse;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use sc_core::image::RgbaImage;
use sc_core::project::{RegionMethod, SourceDescriptor, SystemAudioMode};
use sc_core::time::HostTime;
use sc_geometry::{RectPt, SizePx};

use super::code::{FrameCode, background, pts_ms, source_hash, write_code};
use super::{
    AudioScript, DiskScript, Scenario, ScreenScript, ScriptedEvent, StartFailure, VideoScript,
};
use crate::{
    AppInfo, AppRef, AudioBuffer, AudioData, AudioSource, AudioTrackKind, CameraLimit,
    CameraSource, CaptureDevice, CpuNv12Frame, Desktop, DeviceKind, DeviceStream, DisplayInfo,
    Error, FrameBuffer, FrameSink, NativeFrame, PermissionKind, PermissionStatus, Permissions,
    Result, ScreenSource, ScreenStream, ScreenStreamConfig, SourceEvent, SourceList, SourceSpec,
    StreamTag, SystemAudioSource, SystemAudioStream, VideoFrame, WindowId, WindowInfo,
};

/// Host time of the synthetic clock when the platform is created, far from zero so that
/// subtracting offsets never goes negative.
pub const HOST_BASE_NS: i64 = 1_000_000_000_000;
pub const MIC_DEVICE_ID: &str = "synthetic-mic";
pub const CAMERA_DEVICE_ID: &str = "synthetic-camera";
pub const BEEP_NS: i64 = 50_000_000;
pub const BEEP_HZ: f64 = 1000.0;
pub const BEEP_AMPLITUDE: f32 = 0.5;
/// Name hashed into camera frame codes.
pub const CAMERA_SOURCE_NAME: &str = "camera";
/// Camera stream tags start here so they never collide with the caller's screen tags.
pub const CAMERA_TAG_BASE: u64 = 1 << 48;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) struct Clock {
    epoch: Instant,
    scale: f64,
}

impl Clock {
    fn new(scale: f64) -> Clock {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        Clock {
            epoch: Instant::now(),
            scale,
        }
    }

    pub(super) fn now(&self) -> i64 {
        HOST_BASE_NS + (self.epoch.elapsed().as_nanos() as f64 * self.scale) as i64
    }

    pub(super) fn real(&self, host_ns: i64) -> Duration {
        Duration::from_nanos((host_ns.max(0) as f64 / self.scale) as u64)
    }
}

enum Item {
    Screen(VideoFrame),
    Camera(VideoFrame),
    Audio(AudioTrackKind, AudioBuffer),
    Event(SourceEvent),
}

#[derive(Clone, Copy)]
enum Next {
    Frame(u64, i64),
    Lost(i64),
    Nothing,
}

struct ScreenGen {
    tag: StreamTag,
    hash: u32,
    start: i64,
    first_pts: i64,
    frame_ns: f64,
    size: SizePx,
    template: CpuNv12Frame,
    idle: Vec<(i64, i64)>,
    idle_from_start: bool,
    lost_after: Option<i64>,
    trailing_budget: u32,
    trailing_left: Option<u32>,
    delivered: u64,
    next: Next,
}

impl ScreenGen {
    fn pts(&self, k: u64) -> i64 {
        self.first_pts + (k as f64 * self.frame_ns).round() as i64
    }

    fn plan(&mut self, mut k: u64) {
        let lost_at = self.lost_after.map(|l| self.start.saturating_add(l));
        if self.idle_from_start {
            self.next = lost_at.map_or(Next::Nothing, Next::Lost);
            return;
        }
        loop {
            let pts = self.pts(k);
            if let Some(at) = lost_at
                && pts >= at
            {
                self.next = Next::Lost(at);
                return;
            }
            let off = pts - self.start;
            let Some(&(a, d)) = self
                .idle
                .iter()
                .find(|&&(a, d)| off >= a && off < a.saturating_add(d))
            else {
                self.next = Next::Frame(k, pts);
                return;
            };
            let end = self.start.saturating_add(a).saturating_add(d);
            if end >= i64::MAX / 2 {
                self.next = lost_at.map_or(Next::Nothing, Next::Lost);
                return;
            }
            let jump = ((end - self.first_pts) as f64 / self.frame_ns).ceil() as u64;
            k = jump.max(k + 1);
        }
    }

    fn due(&self) -> Option<i64> {
        if self.trailing_left == Some(0) {
            return None;
        }
        match self.next {
            Next::Frame(_, pts) => Some(pts),
            Next::Lost(at) => Some(at),
            Next::Nothing => None,
        }
    }

    fn produce(&mut self) -> Option<Item> {
        match self.next {
            Next::Frame(k, pts) => {
                let mut frame = self.template.clone();
                write_code(
                    &mut frame,
                    FrameCode {
                        source_hash: self.hash,
                        index: k as u32,
                        pts_ms: pts_ms(pts),
                    },
                );
                self.delivered += 1;
                if let Some(left) = &mut self.trailing_left {
                    *left = left.saturating_sub(1);
                }
                self.plan(k + 1);
                Some(Item::Screen(VideoFrame {
                    stream: self.tag,
                    pts: HostTime(pts),
                    size: self.size,
                    content_rect_pt: None,
                    content_scale: None,
                    buffer: FrameBuffer(Arc::new(frame)),
                }))
            }
            Next::Lost(_) => {
                self.next = Next::Nothing;
                Some(Item::Event(SourceEvent::SourceLost {
                    stream: self.tag,
                    reason: "synthetic: source lost (scripted)".into(),
                }))
            }
            Next::Nothing => None,
        }
    }
}

struct CameraGen {
    tag: StreamTag,
    hash: u32,
    start: i64,
    frame_ns: f64,
    size: SizePx,
    template: CpuNv12Frame,
    k: u64,
}

impl CameraGen {
    fn pts(&self) -> i64 {
        self.start + ((self.k + 1) as f64 * self.frame_ns).round() as i64
    }

    fn produce(&mut self) -> Item {
        let pts = self.pts();
        let mut frame = self.template.clone();
        write_code(
            &mut frame,
            FrameCode {
                source_hash: self.hash,
                index: self.k as u32,
                pts_ms: pts_ms(pts),
            },
        );
        self.k += 1;
        Item::Camera(VideoFrame {
            stream: self.tag,
            pts: HostTime(pts),
            size: self.size,
            content_rect_pt: None,
            content_scale: None,
            buffer: FrameBuffer(Arc::new(frame)),
        })
    }
}

/// PTS of audio buffer `k`: its nominal position skewed by `clock_ppm`, plus every gap and minus
/// every overlap whose `at` that nominal position has reached.
pub fn audio_buffer_pts(script: &AudioScript, start: i64, k: u64) -> i64 {
    let buf_ns = f64::from(script.buffer_frames) * 1e9 / f64::from(script.rate.max(1));
    let nominal = k as f64 * buf_ns;
    let skewed = nominal * (1.0 + script.clock_ppm * 1e-6);
    let reached = |&&(at, _): &&(i64, i64)| nominal >= at as f64;
    let gaps: i64 = script.gaps.iter().filter(reached).map(|&(_, d)| d).sum();
    let overlaps: i64 = script
        .overlaps
        .iter()
        .filter(reached)
        .map(|&(_, d)| d)
        .sum();
    start + skewed.round() as i64 + gaps - overlaps
}

struct AudioGen {
    track: AudioTrackKind,
    device: Option<DeviceKind>,
    script: AudioScript,
    start: i64,
    buf_ns: i64,
    k: u64,
}

impl AudioGen {
    fn pts(&self) -> i64 {
        audio_buffer_pts(&self.script, self.start, self.k)
    }

    fn produce(&mut self) -> Item {
        let s = &self.script;
        let pts = self.pts();
        let frames = s.buffer_frames as usize;
        let ch = usize::from(s.channels);
        let rate = f64::from(s.rate);
        let mut data = vec![0f32; frames * ch];
        let off0 = pts - self.start;
        let off_end = off0 + self.buf_ns;
        for &b in &s.beeps_ns {
            if b >= off_end || b + BEEP_NS <= off0 {
                continue;
            }
            for i in 0..frames {
                let t = off0 as f64 + i as f64 * 1e9 / rate - b as f64;
                if (0.0..BEEP_NS as f64).contains(&t) {
                    let v = BEEP_AMPLITUDE
                        * (2.0 * std::f64::consts::PI * BEEP_HZ * t / 1e9).sin() as f32;
                    data[i * ch..(i + 1) * ch].fill(v);
                }
            }
        }
        self.k += 1;
        Item::Audio(
            self.track,
            AudioBuffer {
                pts: HostTime(pts),
                sample_rate: s.rate,
                channels: s.channels,
                frames: s.buffer_frames,
                data: AudioData::F32Interleaved(data.into()),
            },
        )
    }
}

struct EventGen {
    base: i64,
    events: Vec<(i64, ScriptedEvent)>,
    i: usize,
}

impl EventGen {
    fn produce(&mut self) -> Item {
        let e = self.events[self.i].1.clone();
        self.i += 1;
        Item::Event(match e {
            ScriptedEvent::WillSleep => SourceEvent::SystemWillSleep,
            ScriptedEvent::DidWake => SourceEvent::SystemDidWake,
            ScriptedEvent::DeviceLost(kind) => SourceEvent::DeviceLost {
                kind,
                device_id: device_id(kind).into(),
            },
            ScriptedEvent::DevicesChanged(kind) => SourceEvent::DevicesChanged { kind },
            ScriptedEvent::OwnWindowsChanged => SourceEvent::OwnWindowsChanged,
            ScriptedEvent::Resize(size) => SourceEvent::SourceResized {
                stream: StreamTag(0),
                content_rect_pt: Some(RectPt {
                    x: 0.0,
                    y: 0.0,
                    w: f64::from(size.w),
                    h: f64::from(size.h),
                }),
                content_scale: Some(1.0),
            },
        })
    }
}

fn device_id(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Camera => CAMERA_DEVICE_ID,
        DeviceKind::Mic => MIC_DEVICE_ID,
    }
}

enum Gen {
    Screen(Box<ScreenGen>),
    Camera(Box<CameraGen>),
    Audio(AudioGen),
    Events(EventGen),
}

impl Gen {
    fn due(&self) -> Option<i64> {
        match self {
            Gen::Screen(g) => g.due(),
            Gen::Camera(g) => Some(g.pts()),
            Gen::Audio(g) => Some(g.pts() + g.buf_ns),
            Gen::Events(g) => g.events.get(g.i).map(|(at, _)| g.base.saturating_add(*at)),
        }
    }

    fn produce(&mut self) -> Option<Item> {
        match self {
            Gen::Screen(g) => g.produce(),
            Gen::Camera(g) => Some(g.produce()),
            Gen::Audio(g) => Some(g.produce()),
            Gen::Events(g) => Some(g.produce()),
        }
    }

    fn device(&self) -> Option<DeviceKind> {
        match self {
            Gen::Camera(_) => Some(DeviceKind::Camera),
            Gen::Audio(g) => g.device,
            _ => None,
        }
    }
}

struct Emitter {
    id: u64,
    sink: Arc<dyn FrameSink>,
    source: Gen,
}

#[derive(Default)]
struct State {
    emitters: Vec<Emitter>,
    next_id: u64,
    next_camera_tag: u64,
    thread_running: bool,
    scheduler: Option<ThreadId>,
    on_demand: Vec<(String, ScreenScript)>,
    lost_camera: bool,
    lost_mic: bool,
    followed_apps: Vec<Option<AppRef>>,
    exclusion_updates: Vec<(StreamTag, Vec<WindowId>)>,
    describes: Vec<(String, usize)>,
}

pub(super) struct Hub {
    pub(super) clock: Clock,
    scenario: Scenario,
    state: Mutex<State>,
    cv: Condvar,
    deliver: Mutex<()>,
}

struct Handle {
    hub: Arc<Hub>,
    id: u64,
    stopped: bool,
}

impl Handle {
    fn stop(mut self) {
        self.stopped = true;
        self.hub.stop_emitter(self.id);
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.stopped {
            self.hub.remove_emitter(self.id);
        }
    }
}

struct Resolved {
    key: String,
    script: ScreenScript,
    size: SizePx,
    descriptor: SourceDescriptor,
}

fn parse_display_id(id: &str) -> Result<(String, Option<SizePx>)> {
    let rest = id.strip_prefix("synthetic:").ok_or_else(|| {
        Error::NotFound(format!(
            "synthetic: display id {id:?} is not synthetic:NAME[:WxH]"
        ))
    })?;
    if let Some((name, dims)) = rest.rsplit_once(':')
        && let Some((w, h)) = dims.split_once('x')
        && let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>())
    {
        return Ok((name.to_string(), Some(SizePx { w, h })));
    }
    Ok((rest.to_string(), None))
}

fn is_display_key(key: &str) -> bool {
    !key.starts_with("window:") && !key.starts_with("app:") && !key.contains('/')
}

impl Hub {
    pub(super) fn new(scenario: Scenario) -> Hub {
        Hub {
            clock: Clock::new(scenario.time_scale),
            scenario,
            state: Mutex::new(State {
                next_camera_tag: CAMERA_TAG_BASE,
                ..State::default()
            }),
            cv: Condvar::new(),
            deliver: Mutex::new(()),
        }
    }

    pub(super) fn active_screen_streams(&self) -> usize {
        lock(&self.state)
            .emitters
            .iter()
            .filter(|e| matches!(e.source, Gen::Screen(_)))
            .count()
    }

    pub(super) fn followed_apps(&self) -> Vec<Option<AppRef>> {
        lock(&self.state).followed_apps.clone()
    }

    pub(super) fn exclusion_updates(&self) -> Vec<(StreamTag, Vec<WindowId>)> {
        lock(&self.state).exclusion_updates.clone()
    }

    pub(super) fn free_disk_bytes(&self) -> u64 {
        match self.scenario.disk {
            DiskScript::Constant(b) => b,
            DiskScript::Linear { start, bytes_per_s } => {
                let secs = (self.clock.now() - HOST_BASE_NS) as f64 / 1e9;
                (start as f64 + bytes_per_s as f64 * secs).max(0.0) as u64
            }
        }
    }

    fn status(&self, k: PermissionKind) -> PermissionStatus {
        let [screen, camera, mic] = self.scenario.permissions;
        match k {
            PermissionKind::Screen => screen,
            PermissionKind::Camera => camera,
            PermissionKind::Microphone => mic,
        }
    }

    fn permission(&self, k: PermissionKind) -> Result<()> {
        if self.status(k) == PermissionStatus::Granted {
            Ok(())
        } else {
            Err(Error::PermissionDenied(k))
        }
    }

    fn next_crop(&self, key: &str, crops: &[RectPt]) -> Option<RectPt> {
        let mut st = lock(&self.state);
        let n = match st.describes.iter_mut().find(|(k, _)| k == key) {
            Some((_, n)) => {
                *n += 1;
                *n
            }
            None => {
                st.describes.push((key.to_string(), 0));
                0
            }
        };
        crops.get(n.min(crops.len().saturating_sub(1))).copied()
    }

    fn script_for(&self, key: &str, create: bool) -> Option<ScreenScript> {
        if let Some((_, s)) = self.scenario.screens.iter().find(|(n, _)| n == key) {
            return Some(s.clone());
        }
        let mut st = lock(&self.state);
        if let Some((_, s)) = st.on_demand.iter().find(|(n, _)| n == key) {
            return Some(s.clone());
        }
        if !create {
            return None;
        }
        st.on_demand
            .push((key.to_string(), ScreenScript::default()));
        Some(ScreenScript::default())
    }

    fn resolve(&self, spec: &SourceSpec) -> Result<Resolved> {
        match spec {
            SourceSpec::Display { display } => {
                let (name, size) = parse_display_id(&display.0)?;
                let script = self.script_for(&name, true).unwrap_or_default();
                let size = size.unwrap_or(script.size);
                Ok(Resolved {
                    descriptor: SourceDescriptor::Display {
                        display_id: display.0.clone(),
                        display_name: Some(name.clone()),
                        scale: 1.0,
                        frame_pt: Some(RectPt {
                            x: 0.0,
                            y: 0.0,
                            w: f64::from(size.w),
                            h: f64::from(size.h),
                        }),
                    },
                    key: name,
                    script,
                    size,
                })
            }
            SourceSpec::Region { display, region_pt } => {
                let (name, _) = parse_display_id(&display.0)?;
                let key = format!("{name}/region");
                let script = match self.script_for(&key, false) {
                    Some(s) => s,
                    None => self.script_for(&name, true).unwrap_or_default(),
                };
                let size = SizePx {
                    w: region_pt.w.round().max(1.0) as u32,
                    h: region_pt.h.round().max(1.0) as u32,
                };
                Ok(Resolved {
                    descriptor: SourceDescriptor::Region {
                        display_id: display.0.clone(),
                        scale: 1.0,
                        region_pt: *region_pt,
                        method: RegionMethod::SourceRect,
                    },
                    key,
                    script,
                    size,
                })
            }
            SourceSpec::Window { window } => {
                let key = format!("window:{}", window.0);
                let script = self.script_for(&key, true).unwrap_or_default();
                Ok(Resolved {
                    descriptor: SourceDescriptor::Window {
                        window_id: window.0,
                        app_bundle_id: None,
                        app_name: Some("Synthetic".into()),
                        window_title: Some(key.clone()),
                        scale: 1.0,
                    },
                    size: script.size,
                    key,
                    script,
                })
            }
            SourceSpec::Application { display, app } => {
                let key = format!("app:{}", app.pid);
                let script = self.script_for(&key, true).unwrap_or_default();
                Ok(Resolved {
                    descriptor: SourceDescriptor::Application {
                        display_id: display.0.clone(),
                        app_bundle_id: app.bundle_id.clone(),
                        app_pid: Some(app.pid),
                        app_name: Some(key.clone()),
                        scale: 1.0,
                        crop_pt: None,
                    },
                    size: script.size,
                    key,
                    script,
                })
            }
            SourceSpec::Picked(_) => Err(Error::Unsupported(
                "synthetic: picked sources (the synthetic platform has no picker)",
            )),
            SourceSpec::Camera { .. } | SourceSpec::Device { .. } => Err(Error::Unsupported(
                "camera sources go through the camera, not the screen",
            )),
        }
    }

    fn add_emitter(self: &Arc<Self>, sink: Arc<dyn FrameSink>, source: Gen) -> Result<Handle> {
        let mut st = lock(&self.state);
        st.next_id += 1;
        let id = st.next_id;
        st.emitters.push(Emitter { id, sink, source });
        if !st.thread_running {
            let hub = Arc::clone(self);
            let spawned = std::thread::Builder::new()
                .name("sc-synthetic".into())
                .spawn(move || hub.run());
            if let Err(e) = spawned {
                st.emitters.retain(|e| e.id != id);
                return Err(Error::Other(format!(
                    "synthetic: cannot spawn the scheduler thread: {e}"
                )));
            }
            st.thread_running = true;
        }
        self.cv.notify_all();
        Ok(Handle {
            hub: Arc::clone(self),
            id,
            stopped: false,
        })
    }

    fn remove_emitter(&self, id: u64) {
        let mut st = lock(&self.state);
        st.emitters.retain(|e| e.id != id);
        self.cv.notify_all();
    }

    /// Also waits out a delivery in flight, so nothing of the emitter arrives after this returns
    /// (except when a sink stops its own stream from the scheduler thread).
    fn stop_emitter(&self, id: u64) {
        let on_scheduler = {
            let mut st = lock(&self.state);
            st.emitters.retain(|e| e.id != id);
            self.cv.notify_all();
            st.scheduler == Some(std::thread::current().id())
        };
        if !on_scheduler {
            drop(lock(&self.deliver));
        }
    }

    fn run(self: Arc<Self>) {
        let mut st = lock(&self.state);
        st.scheduler = Some(std::thread::current().id());
        loop {
            if st.emitters.is_empty() {
                st.thread_running = false;
                st.scheduler = None;
                return;
            }
            let best = st
                .emitters
                .iter()
                .enumerate()
                .filter_map(|(i, e)| e.source.due().map(|d| (d, Reverse(e.id), i)))
                .min();
            let Some((due, _, i)) = best else {
                st = self.cv.wait(st).unwrap_or_else(PoisonError::into_inner);
                continue;
            };
            let now = self.clock.now();
            if due > now {
                st = self
                    .cv
                    .wait_timeout(st, self.clock.real(due - now))
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
                continue;
            }
            let id = st.emitters[i].id;
            let sink = Arc::clone(&st.emitters[i].sink);
            let Some(mut item) = st.emitters[i].source.produce() else {
                continue;
            };
            if let Item::Event(SourceEvent::SourceResized { stream, .. }) = &mut item
                && let Some(tag) = st.emitters.iter().rev().find_map(|e| match &e.source {
                    Gen::Screen(g) => Some(g.tag),
                    _ => None,
                })
            {
                *stream = tag;
            }
            self.after_produce(&mut st, i, id, &item);
            let guard = lock(&self.deliver);
            drop(st);
            match item {
                Item::Screen(f) => sink.on_screen(f),
                Item::Camera(f) => sink.on_camera(f),
                Item::Audio(k, a) => sink.on_audio(k, a),
                Item::Event(e) => sink.on_event(e),
            }
            drop(guard);
            st = lock(&self.state);
        }
    }

    fn after_produce(&self, st: &mut State, i: usize, id: u64, item: &Item) {
        match item {
            Item::Screen(_) => {
                let first = matches!(&st.emitters[i].source, Gen::Screen(g) if g.delivered == 1);
                if first {
                    for e in st.emitters.iter_mut().filter(|e| e.id < id) {
                        if let Gen::Screen(g) = &mut e.source
                            && g.trailing_left.is_none()
                        {
                            g.trailing_left = Some(g.trailing_budget);
                        }
                    }
                }
            }
            Item::Event(SourceEvent::DeviceLost { kind, .. }) => {
                match kind {
                    DeviceKind::Camera => st.lost_camera = true,
                    DeviceKind::Mic => st.lost_mic = true,
                }
                st.emitters.retain(|e| e.source.device() != Some(*kind));
            }
            Item::Event(SourceEvent::DevicesChanged { kind }) => match kind {
                DeviceKind::Camera => st.lost_camera = false,
                DeviceKind::Mic => st.lost_mic = false,
            },
            _ => {}
        }
    }

    pub(super) fn watch_system(
        self: &Arc<Self>,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn DeviceStream>> {
        let mut events = self.scenario.events.clone();
        events.sort_by_key(|(at, _)| *at);
        let source = Gen::Events(EventGen {
            base: self.clock.now(),
            events,
            i: 0,
        });
        Ok(Box::new(SynthDeviceStream {
            handle: self.add_emitter(sink, source)?,
            format: "synthetic system events".into(),
        }))
    }

    fn displays_list(&self) -> Vec<DisplayInfo> {
        let mut keys: Vec<(String, ScreenScript)> = self
            .scenario
            .screens
            .iter()
            .filter(|(k, _)| is_display_key(k))
            .cloned()
            .collect();
        for (k, s) in &lock(&self.state).on_demand {
            if is_display_key(k) && !keys.iter().any(|(n, _)| n == k) {
                keys.push((k.clone(), s.clone()));
            }
        }
        let mut x = 0.0;
        keys.into_iter()
            .enumerate()
            .map(|(i, (key, s))| {
                let info = DisplayInfo {
                    id: crate::DisplayId(format!("synthetic:{key}")),
                    name: key,
                    frame_pt: RectPt {
                        x,
                        y: 0.0,
                        w: f64::from(s.size.w),
                        h: f64::from(s.size.h),
                    },
                    scale: 1.0,
                    size_px: s.size,
                    is_main: i == 0,
                };
                x += f64::from(s.size.w);
                info
            })
            .collect()
    }

    fn start_audio(
        self: &Arc<Self>,
        track: AudioTrackKind,
        device: Option<DeviceKind>,
        script: &AudioScript,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Handle> {
        if script.rate == 0 || script.channels == 0 || script.buffer_frames == 0 {
            return Err(Error::Other(format!(
                "synthetic: audio script needs rate, channels and buffer_frames > 0 ({script:?})"
            )));
        }
        let source = Gen::Audio(AudioGen {
            track,
            device,
            buf_ns: (f64::from(script.buffer_frames) * 1e9 / f64::from(script.rate)).round() as i64,
            script: script.clone(),
            start: self.clock.now(),
            k: 0,
        });
        self.add_emitter(sink, source)
    }
}

struct SynthScreenStream {
    tag: StreamTag,
    handle: Handle,
}

impl ScreenStream for SynthScreenStream {
    fn tag(&self) -> StreamTag {
        self.tag
    }

    fn update_exclusions(&self, exclude: &[WindowId]) -> Result<()> {
        lock(&self.handle.hub.state)
            .exclusion_updates
            .push((self.tag, exclude.to_vec()));
        Ok(())
    }

    fn stop(self: Box<Self>) -> Result<()> {
        self.handle.stop();
        Ok(())
    }
}

struct SynthDeviceStream {
    handle: Handle,
    format: String,
}

impl DeviceStream for SynthDeviceStream {
    fn format(&self) -> String {
        self.format.clone()
    }

    fn stop(self: Box<Self>) -> Result<()> {
        self.handle.stop();
        Ok(())
    }
}

struct SynthSystemAudioStream {
    handle: Handle,
}

impl SystemAudioStream for SynthSystemAudioStream {
    fn follow_app(&self, app: Option<&AppRef>) -> Result<()> {
        lock(&self.handle.hub.state)
            .followed_apps
            .push(app.cloned());
        Ok(())
    }

    fn stop(self: Box<Self>) -> Result<()> {
        self.handle.stop();
        Ok(())
    }
}

/// Starting a stream needs `Arc<Hub>`, and `Platform` hands out plain references.
pub(super) struct HubRef(pub(super) Arc<Hub>);

impl ScreenSource for HubRef {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(self.0.displays_list())
    }

    fn list(&self, hide: &[WindowId]) -> Result<SourceList> {
        self.0.permission(PermissionKind::Screen)?;
        let displays = self.0.displays_list();
        let main = displays.first().map(|d| d.id.clone());
        let mut windows = Vec::new();
        let mut apps = Vec::new();
        for (key, s) in &self.0.scenario.screens {
            if let Some(id) = key
                .strip_prefix("window:")
                .and_then(|n| n.parse::<u32>().ok())
            {
                if hide.contains(&WindowId(id)) {
                    continue;
                }
                windows.push(WindowInfo {
                    id: WindowId(id),
                    title: key.clone(),
                    app_name: "Synthetic".into(),
                    app: AppRef {
                        pid: 0,
                        bundle_id: None,
                    },
                    frame_pt: RectPt {
                        x: 0.0,
                        y: 0.0,
                        w: f64::from(s.size.w),
                        h: f64::from(s.size.h),
                    },
                    display: main.clone(),
                    on_screen: true,
                    layer: 0,
                    z_index: windows.len() as u32,
                });
            } else if let Some(pid) = key.strip_prefix("app:").and_then(|n| n.parse::<i32>().ok()) {
                apps.push(AppInfo {
                    app: AppRef {
                        pid,
                        bundle_id: None,
                    },
                    name: key.clone(),
                });
            }
        }
        Ok(SourceList {
            displays,
            windows,
            apps,
        })
    }

    fn describe(
        &self,
        spec: &SourceSpec,
        _cfg: &ScreenStreamConfig,
    ) -> Result<(SourceDescriptor, SizePx)> {
        self.0.permission(PermissionKind::Screen)?;
        let mut r = self.0.resolve(spec)?;
        if let SourceDescriptor::Application { crop_pt, .. } = &mut r.descriptor {
            *crop_pt = self.0.next_crop(&r.key, &r.script.describe_crops);
        }
        Ok((r.descriptor, r.size))
    }

    fn start(
        &self,
        tag: StreamTag,
        spec: &SourceSpec,
        cfg: &ScreenStreamConfig,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn ScreenStream>> {
        let hub = &self.0;
        hub.permission(PermissionKind::Screen)?;
        let r = hub.resolve(spec)?;
        match r.script.start_fails {
            StartFailure::Never => {}
            StartFailure::Always => {
                return Err(Error::Os {
                    code: -3801,
                    message: format!("synthetic: {} fails to start (scripted)", r.key),
                });
            }
            StartFailure::WhileOtherStreamRuns => {
                if hub.active_screen_streams() > 0 {
                    return Err(Error::Busy(format!(
                        "synthetic: {} fails to start while another screen stream runs (scripted)",
                        r.key
                    )));
                }
            }
        }
        let s = &r.script;
        let fps = match (cfg.fps, s.fps.max(1)) {
            (0, f) => f,
            (c, f) => c.min(f),
        };
        let size = s.first_frame_size.unwrap_or(r.size);
        let hash = source_hash(&r.key);
        let start = hub.clock.now();
        let mut source = ScreenGen {
            tag,
            hash,
            start,
            first_pts: start + i64::from(s.first_frame_latency_ms) * 1_000_000,
            frame_ns: 1e9 / f64::from(fps),
            size,
            template: background(size, hash, false),
            idle: s.idle.clone(),
            idle_from_start: s.idle_from_start,
            lost_after: s.lost_after_ns,
            trailing_budget: s.trailing_after_new_stream,
            trailing_left: None,
            delivered: 0,
            next: Next::Nothing,
        };
        source.plan(0);
        let handle = hub.add_emitter(sink, Gen::Screen(Box::new(source)))?;
        Ok(Box::new(SynthScreenStream { tag, handle }))
    }

    fn snapshot(&self, spec: &SourceSpec, max: SizePx) -> Result<RgbaImage> {
        self.0.permission(PermissionKind::Screen)?;
        let r = self.0.resolve(spec)?;
        let size = r.script.first_frame_size.unwrap_or(r.size);
        let f = 1f64
            .min(f64::from(max.w) / f64::from(size.w.max(1)))
            .min(f64::from(max.h) / f64::from(size.h.max(1)));
        let out = SizePx {
            w: ((f64::from(size.w) * f).round() as u32).max(1),
            h: ((f64::from(size.h) * f).round() as u32).max(1),
        };
        let frame = background(size, source_hash(&r.key), false);
        let mut img = RgbaImage {
            size: out,
            stride: out.w as usize * 4,
            data: vec![0; out.w as usize * out.h as usize * 4],
        };
        frame.with_nv12(&mut |v| {
            for oy in 0..out.h as usize {
                let sy = oy * size.h as usize / out.h as usize;
                for ox in 0..out.w as usize {
                    let sx = ox * size.w as usize / out.w as usize;
                    let y = f64::from(v.y[sy * v.y_stride + sx]);
                    let uvi = (sy / 2) * v.uv_stride + (sx / 2) * 2;
                    let (u, vv) = (f64::from(v.uv[uvi]), f64::from(v.uv[uvi + 1]));
                    let rgb = bt709_limited_to_rgb(y, u, vv);
                    let o = oy * img.stride + ox * 4;
                    img.data[o..o + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
            }
        })?;
        Ok(img)
    }
}

fn bt709_limited_to_rgb(y: f64, u: f64, v: f64) -> [u8; 3] {
    let y = (y - 16.0) * 255.0 / 219.0;
    let u = (u - 128.0) * 255.0 / 224.0;
    let v = (v - 128.0) * 255.0 / 224.0;
    let c = |x: f64| x.round().clamp(0.0, 255.0) as u8;
    [
        c(y + 1.5748 * v),
        c(y - 0.1873 * u - 0.4681 * v),
        c(y + 1.8556 * u),
    ]
}

impl SystemAudioSource for HubRef {
    fn start(
        &self,
        mode: SystemAudioMode,
        _exclude: &[WindowId],
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn SystemAudioStream>> {
        if mode == SystemAudioMode::Off {
            return Err(Error::Other(
                "synthetic: system audio mode Off has no stream".into(),
            ));
        }
        self.0.permission(PermissionKind::Screen)?;
        let script =
            self.0.scenario.system_audio.as_ref().ok_or_else(|| {
                Error::NotFound("synthetic: the scenario has no system audio".into())
            })?;
        let handle = self
            .0
            .start_audio(AudioTrackKind::SystemAudio, None, script, sink)?;
        Ok(Box::new(SynthSystemAudioStream { handle }))
    }
}

impl AudioSource for HubRef {
    fn devices(&self) -> Result<Vec<CaptureDevice>> {
        let lost = lock(&self.0.state).lost_mic;
        Ok(match (&self.0.scenario.mic, lost) {
            (Some(_), false) => vec![CaptureDevice {
                id: MIC_DEVICE_ID.into(),
                name: "Synthetic Mic".into(),
                model_id: Some("synthetic".into()),
                is_default: true,
                kind_hint: Some("builtin".into()),
            }],
            _ => Vec::new(),
        })
    }

    fn start(&self, device_id: &str, sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
        self.0.permission(PermissionKind::Microphone)?;
        if !AudioSource::devices(self)?
            .iter()
            .any(|d| d.id == device_id)
        {
            return Err(Error::NotFound(format!("synthetic: no mic {device_id:?}")));
        }
        let script = self
            .0
            .scenario
            .mic
            .as_ref()
            .ok_or_else(|| Error::NotFound("synthetic: the scenario has no mic".into()))?;
        let handle =
            self.0
                .start_audio(AudioTrackKind::Mic, Some(DeviceKind::Mic), script, sink)?;
        Ok(Box::new(SynthDeviceStream {
            handle,
            format: format!(
                "synthetic f32 {} Hz {} ch, {} frames/buffer",
                script.rate, script.channels, script.buffer_frames
            ),
        }))
    }

    fn show_mic_modes(&self) -> Result<()> {
        Err(Error::Unsupported("synthetic: no mic modes"))
    }
}

/// The camera size scaled down (to even sizes) to fit `limit.max`, and its fps capped at
/// `limit.max_fps`.
pub fn apply_camera_limit(script: &VideoScript, limit: CameraLimit) -> (SizePx, u32) {
    let s = script.size;
    let f = 1f64
        .min(f64::from(limit.max.w) / f64::from(s.w.max(1)))
        .min(f64::from(limit.max.h) / f64::from(s.h.max(1)));
    let size = if f < 1.0 {
        SizePx {
            w: (((f64::from(s.w) * f) as u32) & !1).max(2),
            h: (((f64::from(s.h) * f) as u32) & !1).max(2),
        }
    } else {
        s
    };
    (size, script.fps.max(1).min(limit.max_fps.max(1)))
}

impl CameraSource for HubRef {
    fn devices(&self) -> Result<Vec<CaptureDevice>> {
        let lost = lock(&self.0.state).lost_camera;
        Ok(match (&self.0.scenario.camera, lost) {
            (Some(_), false) => vec![CaptureDevice {
                id: CAMERA_DEVICE_ID.into(),
                name: "Synthetic Camera".into(),
                model_id: Some("synthetic".into()),
                is_default: true,
                kind_hint: Some("builtin".into()),
            }],
            _ => Vec::new(),
        })
    }

    fn start(
        &self,
        device_id: &str,
        limit: CameraLimit,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn DeviceStream>> {
        self.0.permission(PermissionKind::Camera)?;
        if !CameraSource::devices(self)?
            .iter()
            .any(|d| d.id == device_id)
        {
            return Err(Error::NotFound(format!(
                "synthetic: no camera {device_id:?}"
            )));
        }
        let script = self
            .0
            .scenario
            .camera
            .as_ref()
            .ok_or_else(|| Error::NotFound("synthetic: the scenario has no camera".into()))?;
        let (size, fps) = apply_camera_limit(script, limit);
        let tag = {
            let mut st = lock(&self.0.state);
            st.next_camera_tag += 1;
            StreamTag(st.next_camera_tag)
        };
        let hash = source_hash(CAMERA_SOURCE_NAME);
        let source = Gen::Camera(Box::new(CameraGen {
            tag,
            hash,
            start: self.0.clock.now(),
            frame_ns: 1e9 / f64::from(fps),
            size,
            template: background(size, hash, false),
            k: 0,
        }));
        let handle = self.0.add_emitter(sink, source)?;
        Ok(Box::new(SynthDeviceStream {
            handle,
            format: format!("synthetic nv12 {}x{} @ {fps} fps", size.w, size.h),
        }))
    }
}

impl Permissions for HubRef {
    fn status(&self, k: PermissionKind) -> PermissionStatus {
        self.0.status(k)
    }

    fn request(&self, k: PermissionKind, done: Box<dyn FnOnce(PermissionStatus) + Send>) {
        done(self.0.status(k));
    }

    fn settings_url(&self, _k: PermissionKind) -> Option<String> {
        None
    }
}

impl Desktop for HubRef {
    fn reveal(&self, path: &Path) -> Result<()> {
        if path.exists() {
            Ok(())
        } else {
            Err(Error::NotFound(format!(
                "synthetic: reveal {}: no such path",
                path.display()
            )))
        }
    }

    fn choose_image(&self, done: Box<dyn FnOnce(Option<std::path::PathBuf>) + Send>) {
        done(None);
    }
}
