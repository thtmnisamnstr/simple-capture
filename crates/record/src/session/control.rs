//! The control thread: start gate, switch controller (make-before-break, ADR-034 failure order),
//! source and system events, `project.json` rewrites, stop and finalize.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sc_core::folder::{ProjectLock, create_project_dir, rfc3339};
use sc_core::journal::{
    DeviceChange, DeviceKindJ, JournalEvent, JournalWriter, StopReason, SwitchNext,
};
use sc_core::project::{
    AudioInfo, AudioMixEntry, CameraKey, Clip, ColorInfo, DeviceInfo, Easing, Edl, EndReason,
    Extra, ProjectState, ProjectV1, ScreenSegment, Speed, StartReason, Track, TrackHealth,
    TrackKind, Via, VideoInfo, copy_background_asset,
};
use sc_core::sessionlog::{Level, SessionLog};
use sc_geometry::{Aspect, Ratio, SizePx};
use sc_platform::{
    AppRef, AudioWriterSpec, CameraLimit, DeviceKind, DeviceStream, FrameSink, PermissionKind,
    PermissionStatus, Platform, Resolution, ScreenStream, ScreenStreamConfig, SourceDescriptor,
    SourceEvent, SourceSpec, StreamTag, SystemAudioMode, SystemAudioStream, VideoCodec,
    VideoWriterSpec, WindowId,
};

use super::health;
use super::router::{self, Closed, Opened, Prepared, RouterCmd, RouterDone, RouterIn};
use super::shared::{Ctl, NOT_STARTED, Recorder, Registry, Shared};
use super::sink::{Feed, SessionSink, TrackIn};
use super::tracks::{self, TrackCtx, TrackResult};
use super::{Notice, RecordRequest, SessionObserver, SessionState, StopSummary};
use crate::testhooks::{HoldPoint, hold};
use crate::{Error, Result};

/// Matches `sc_capture_macos::av::DEFAULT_BITS_PER_PIXEL`.
const BITS_PER_PIXEL: f64 = 0.05;
const KEYFRAME_INTERVAL_S: f64 = 1.0;
const SCK_QUEUE_DEPTH: u32 = 6;
/// After the cut the old stream runs until its first frame at or past the cut (so every earlier
/// frame still in flight lands in the old segment), at most two frame intervals plus this.
const OLD_DRAIN_SLACK_NS: i64 = 10_000_000;

pub(crate) struct Started {
    pub sh: Arc<Shared>,
    pub thread: JoinHandle<()>,
}

struct Live {
    tag: StreamTag,
    stream: Box<dyn ScreenStream>,
}

#[derive(Clone)]
struct Cur {
    spec: SourceSpec,
    via: Via,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    First,
    Retry,
    Restore,
}

struct Switch {
    spec: SourceSpec,
    via: Via,
    desc: SourceDescriptor,
    size: SizePx,
    tag: StreamTag,
    deadline: Instant,
    stage: Stage,
    stream: Option<Box<dyn ScreenStream>>,
    error: String,
}

struct Cont {
    kind: TrackKind,
    device: Option<(DeviceKind, String, String)>,
    stream: Option<Box<dyn DeviceStream>>,
    tx: SyncSender<TrackIn>,
    join: JoinHandle<TrackResult>,
}

struct Control {
    sh: Arc<Shared>,
    req: RecordRequest,
    rx: Receiver<Ctl>,
    project: ProjectV1,
    lock: Option<ProjectLock>,
    sink: Arc<SessionSink>,
    router_tx: SyncSender<RouterIn>,
    router: Option<JoinHandle<()>>,
    ext: &'static str,
    next_tag: u64,
    next_seg: u32,
    screen: Option<Live>,
    cur: Option<Cur>,
    active: Option<(StreamTag, String)>,
    switch: Option<Switch>,
    queued: Option<(SourceSpec, Via)>,
    cont: Vec<Cont>,
    sys_stream: Option<Box<dyn SystemAudioStream>>,
    watch: Option<Box<dyn DeviceStream>>,
    health: Option<(Sender<()>, JoinHandle<()>)>,
    results: Vec<TrackResult>,
    finished: Option<std::result::Result<StopSummary, String>>,
    camera_keys: Vec<CameraKey>,
    pending_keys: Vec<CameraKey>,
    pending_since: Option<Instant>,
}

/// Camera keys reach the journal in batches: every append fsyncs, and a drag sends up to 30 keys
/// a second. A crash loses at most this much camera motion.
const KEY_FLUSH: Duration = Duration::from_millis(200);
/// Drags send keys about every 33 ms; a key after a longer pause is a jump, not motion.
const KEY_MOTION_GAP_NS: i64 = 100_000_000;

/// A key that follows the previous one within a drag keeps its easing (the motion is replayed);
/// one after a pause holds the old placement until it, then jumps.
fn key_easing(sent: Easing, gap_ns: i64) -> Easing {
    if sent == Easing::Linear && gap_ns > KEY_MOTION_GAP_NS {
        Easing::Step
    } else {
        sent
    }
}

fn check_permission(p: &dyn Platform, k: PermissionKind) -> Result<()> {
    match p.permissions().status(k) {
        PermissionStatus::Granted => Ok(()),
        _ => Err(Error::Platform(sc_platform::Error::PermissionDenied(k))),
    }
}

fn container_codec(ext: &str, video: bool) -> (String, String) {
    if ext == "sctrk" {
        ("sctrk".into(), "sctrk".into())
    } else if ext == "mkv" {
        ("matroska".into(), if video { "hevc_or_h264" } else { "aac" }.into())
    } else if video {
        ("mov_fragmented".into(), "hevc".into())
    } else {
        ("mov_fragmented".into(), "aac".into())
    }
}

fn empty_health() -> TrackHealth {
    TrackHealth {
        frames: 0,
        dropped: 0,
        writer_not_ready: 0,
        gaps: Vec::new(),
        extra: Extra::new(),
    }
}

fn video_info(size: SizePx, fps: u32) -> VideoInfo {
    let bt709 = || "bt709".to_string();
    VideoInfo {
        size_px: size,
        nominal_fps: fps,
        vfr: true,
        keyframe_interval_s: KEYFRAME_INTERVAL_S,
        color: ColorInfo {
            primaries: bt709(),
            transfer: bt709(),
            matrix: bt709(),
            range: "limited".into(),
            extra: Extra::new(),
        },
        extra: Extra::new(),
    }
}

fn resolution_text(r: Resolution) -> String {
    match r {
        Resolution::Native => "native".into(),
        Resolution::OneX => "1x".into(),
    }
}

fn device_kind_j(k: DeviceKind) -> DeviceKindJ {
    match k {
        DeviceKind::Camera => DeviceKindJ::Camera,
        DeviceKind::Mic => DeviceKindJ::Mic,
    }
}

struct DirGuard(Option<PathBuf>);

impl Drop for DirGuard {
    fn drop(&mut self) {
        if let Some(d) = self.0.take() {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

pub(crate) fn start(
    platform: Arc<dyn Platform>,
    req: RecordRequest,
    obs: Arc<dyn SessionObserver>,
) -> Result<Started> {
    let p = &*platform;
    check_permission(p, PermissionKind::Screen)?;
    if req.camera.is_some() {
        check_permission(p, PermissionKind::Camera)?;
    }
    if req.mic.is_some() {
        check_permission(p, PermissionKind::Microphone)?;
    }
    std::fs::create_dir_all(&req.projects_dir).map_err(|e| {
        Error::State(format!(
            "create projects folder {}: {e}",
            req.projects_dir.display()
        ))
    })?;
    let free = p.free_disk_bytes(&req.projects_dir)?;
    let need = req.tuning.disk_stop_bytes.saturating_mul(2);
    if free < need {
        return Err(Error::State(format!(
            "not enough free disk space in {}: {free} bytes free, {need} needed to start",
            req.projects_dir.display()
        )));
    }
    let local = p.local_now();
    let dir = create_project_dir(&req.projects_dir, &local, &req.title)?;
    let mut guard = DirGuard(Some(dir.clone()));
    let lock = ProjectLock::acquire(&dir)?;
    let mut project = ProjectV1::new_recording(
        sc_core::ids::new_id(),
        req.title.clone(),
        rfc3339(&local),
        env!("CARGO_PKG_VERSION").to_string(),
        p.clock_kind(),
    );
    {
        let r = &mut project.recording;
        r.start_host_ns = p.host_now().0;
        r.screen.fps_requested = req.fps;
        r.screen.resolution = req.resolution;
        r.screen.cursor = req.show_cursor;
        r.screen.click_highlight = req.click_highlight;
        r.system_audio = req.system_audio;
        r.permission_host = p.permission_host();
    }
    project.output.canvas = req.canvas.clone();
    if let Some(img) = &req.background_image {
        project.output.canvas.background = copy_background_asset(img, &dir)?;
    }
    project.save(&dir)?;
    let tracks_dir = dir.join("tracks");
    std::fs::create_dir_all(&tracks_dir)
        .map_err(|e| Error::State(format!("create {}: {e}", tracks_dir.display())))?;
    let journal = JournalWriter::create(&dir)?;
    let log = SessionLog::open(&dir)?;

    let (ctl_tx, rx) = mpsc::channel();
    let (router_tx, router_rx) = mpsc::sync_channel(req.tuning.screen_queue.max(1));
    let writers = p.writers();
    let ext = writers.extension();
    let sh = Arc::new(Shared {
        platform: platform.clone(),
        obs,
        dir: dir.clone(),
        rec: Recorder::new(journal, log, platform.clone()),
        registry: Registry::default(),
        state: Mutex::new(SessionState::Preparing),
        start_host: NOT_STARTED.into(),
        stop_host: i64::MAX.into(),
        tuning: req.tuning.clone(),
        ctl: ctl_tx,
        screen_stats: Mutex::new(None),
    });
    sh.set_state(SessionState::Preparing);
    let vspec = VideoWriterSpec {
        size: SizePx { w: 2, h: 2 },
        fps: req.fps.max(1),
        codec: VideoCodec::Hevc,
        keyframe_interval_s: KEYFRAME_INTERVAL_S,
        fragment_interval_s: req.tuning.fragment_interval_s,
        bits_per_pixel: BITS_PER_PIXEL,
    };
    let router = router::spawn(sh.clone(), writers, vspec, router_rx)
        .map_err(|e| Error::State(format!("spawn the screen writer thread: {e}")))?;
    let sink = SessionSink::new(sh.clone(), router_tx.clone());
    sh.rec
        .log(Level::Info, &format!("recording to {}", dir.display()));
    sh.rec.log(Level::Info, &config_line(&req));
    let mut c = Control {
        sh: sh.clone(),
        req,
        rx,
        project,
        lock: Some(lock),
        sink,
        router_tx,
        router: Some(router),
        ext,
        next_tag: 0,
        next_seg: 0,
        screen: None,
        cur: None,
        active: None,
        switch: None,
        queued: None,
        cont: Vec::new(),
        sys_stream: None,
        watch: None,
        health: None,
        results: Vec::new(),
        finished: None,
        camera_keys: Vec::new(),
        pending_keys: Vec::new(),
        pending_since: None,
    };
    if let Err(e) = c.begin() {
        sh.rec.log(Level::Error, &format!("start failed: {e}"));
        c.shutdown(sh.now());
        c.lock.take();
        return Err(e);
    }
    let thread = std::thread::Builder::new()
        .name("sc-record-control".into())
        .spawn(move || c.run())
        .map_err(|e| Error::State(format!("spawn the control thread: {e}")))?;
    guard.0 = None;
    Ok(Started { sh, thread })
}

impl Control {
    fn platform(&self) -> &dyn Platform {
        &*self.sh.platform
    }

    fn sink(&self) -> Arc<dyn FrameSink> {
        self.sink.clone()
    }

    fn cfg(&self) -> ScreenStreamConfig {
        ScreenStreamConfig {
            fps: self.req.fps,
            resolution: self.req.resolution,
            show_cursor: self.req.show_cursor,
            show_clicks: self.req.click_highlight,
            queue_depth: SCK_QUEUE_DEPTH,
            exclude: self.req.exclude.clone(),
        }
    }

    fn tag(&mut self) -> StreamTag {
        self.next_tag += 1;
        StreamTag(self.next_tag)
    }

    fn seg_names(&mut self) -> (String, String, String) {
        let n = self.next_seg;
        self.next_seg += 1;
        (
            format!("s{n}"),
            format!("screen-{n}"),
            format!("tracks/screen-{n:03}.{}", self.ext),
        )
    }

    fn router(&self, c: RouterCmd) -> bool {
        self.router_tx.send(RouterIn::Cmd(c)).is_ok()
    }

    fn router_rpc<T>(&self, make: impl FnOnce(Sender<T>) -> RouterCmd) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        if !self.router(make(tx)) {
            return None;
        }
        rx.recv().ok()
    }

    fn journal(&self, ev: JournalEvent) {
        self.sh.rec.journal(&ev);
    }

    fn log(&self, level: Level, msg: &str) {
        self.sh.rec.log(level, msg);
    }

    fn save(&self) {
        if let Err(e) = self.project.save(&self.sh.dir) {
            self.log(Level::Error, &format!("write project.json: {e}"));
        }
    }

    fn stop_screen(&self, s: Box<dyn ScreenStream>, what: &str) {
        if let Err(e) = s.stop() {
            self.log(Level::Warn, &format!("stop {what} screen stream: {e}"));
        }
    }

    fn begin(&mut self) -> Result<()> {
        let platform = self.sh.platform.clone();
        let sink = self.sink();
        match platform.watch_system(sink.clone()) {
            Ok(w) => self.watch = Some(w),
            Err(e) => self.log(Level::Warn, &format!("no system watcher: {e}")),
        }
        let cfg = self.cfg();
        let spec = self.req.source.clone();
        let (desc, size) = platform.screen().describe(&spec, &cfg)?;
        let tag = self.tag();
        let (seg, track_id, file) = self.seg_names();
        self.router(RouterCmd::Prepare(Box::new(Prepared {
            tag,
            seg,
            track_id,
            file,
            writer: None,
            size,
            via: self.req.via,
            reason: StartReason::RecordStart,
            source: desc,
            closes_old_with: EndReason::RecordStop,
        })));
        let stream = platform.screen().start(tag, &spec, &cfg, sink.clone())?;
        self.screen = Some(Live { tag, stream });
        self.cur = Some(Cur {
            spec,
            via: self.req.via,
        });
        let mut devices = Vec::new();
        if let Some(id) = self.req.camera.clone() {
            let name = platform
                .camera()
                .devices()
                .ok()
                .and_then(|d| d.into_iter().find(|d| d.id == id))
                .map_or_else(|| id.clone(), |d| d.name);
            let s = platform
                .camera()
                .start(&id, CameraLimit::default(), sink.clone())?;
            devices.push((TrackKind::Camera, DeviceKind::Camera, id, name, s));
        }
        if let Some(id) = self.req.mic.clone() {
            let name = platform
                .mic()
                .devices()
                .ok()
                .and_then(|d| d.into_iter().find(|d| d.id == id))
                .map_or_else(|| id.clone(), |d| d.name);
            let s = platform.mic().start(&id, sink.clone())?;
            devices.push((TrackKind::Mic, DeviceKind::Mic, id, name, s));
        }
        if self.req.system_audio != SystemAudioMode::Off {
            self.sys_stream = Some(platform.system_audio().start(
                self.req.system_audio,
                &self.req.exclude,
                sink.clone(),
            )?);
        }

        let want_cam = self.req.camera.is_some();
        let want_mic = self.req.mic.is_some();
        let want_sys = self.sys_stream.is_some();
        let timeout_ms = self.req.tuning.start_timeout_ms;
        let firsts = self
            .sink
            .wait_firsts(
                platform.host_duration(i64::from(timeout_ms) * 1_000_000),
                |f| {
                    f.screen
                        && (!want_cam || f.camera.is_some())
                        && (!want_mic || f.mic.is_some())
                        && (!want_sys || f.sys.is_some())
                },
            )
            .map_err(|f| {
                let mut missing = Vec::new();
                if !f.screen {
                    missing.push("screen");
                }
                if want_cam && f.camera.is_none() {
                    missing.push("camera");
                }
                if want_mic && f.mic.is_none() {
                    missing.push("mic");
                }
                if want_sys && f.sys.is_none() {
                    missing.push("system audio");
                }
                Error::Platform(sc_platform::Error::Timeout(format!(
                    "no first sample from {} within {timeout_ms} ms",
                    missing.join(", ")
                )))
            })?;

        let writers = platform.writers();
        for (kind, dkind, id, name, stream) in devices {
            let track_id = match kind {
                TrackKind::Camera => "camera",
                _ => "mic",
            };
            let file = format!("tracks/{track_id}.{}", self.ext);
            let path = self.sh.dir.join(&file);
            let format = stream.format();
            let (writer, track) = match kind {
                TrackKind::Camera => {
                    let size = firsts.camera.unwrap_or(SizePx { w: 2, h: 2 });
                    let spec = VideoWriterSpec {
                        size,
                        fps: 30,
                        codec: VideoCodec::Hevc,
                        keyframe_interval_s: KEYFRAME_INTERVAL_S,
                        fragment_interval_s: self.req.tuning.fragment_interval_s,
                        bits_per_pixel: BITS_PER_PIXEL,
                    };
                    (
                        writers.video(&path, &spec)?,
                        self.track(track_id, kind, &file, Some(video_info(size, 30)), None),
                    )
                }
                _ => {
                    let (rate, ch) = firsts.mic.unwrap_or((48_000, 1));
                    (
                        writers.audio(&path, &audio_spec(rate, ch, &self.req))?,
                        self.track(track_id, kind, &file, None, Some((rate, ch))),
                    )
                }
            };
            let mut track = track;
            track.device = Some(DeviceInfo {
                name: name.clone(),
                unique_id: id.clone(),
                model_id: None,
                format: Some(format),
                extra: Extra::new(),
            });
            self.project.recording.tracks.push(track);
            if kind == TrackKind::Camera
                && let Some(mut k) = self.req.camera_key.clone()
            {
                k.t_rec_ns = 0;
                self.camera_keys.push(k.clone());
                self.sh
                    .rec
                    .journal(&JournalEvent::CameraKeys { keys: vec![k] });
            }
            let c = self.spawn_track(track_id, kind, writer)?;
            self.cont.push(Cont {
                device: Some((dkind, id, name)),
                stream: Some(stream),
                ..c
            });
        }
        if want_sys {
            let file = format!("tracks/system_audio.{}", self.ext);
            let (rate, ch) = firsts.sys.unwrap_or((48_000, 2));
            let writer =
                writers.audio(&self.sh.dir.join(&file), &audio_spec(rate, ch, &self.req))?;
            let t = self.track(
                "system_audio",
                TrackKind::SystemAudio,
                &file,
                None,
                Some((rate, ch)),
            );
            self.project.recording.tracks.push(t);
            let c = self.spawn_track("system_audio", TrackKind::SystemAudio, writer)?;
            self.cont.push(c);
        }
        for t in &self.project.recording.tracks {
            if matches!(t.kind, TrackKind::Mic | TrackKind::SystemAudio) {
                self.project.audio_mix.insert(
                    t.id.clone(),
                    AudioMixEntry {
                        gain_db: 0.0,
                        muted: false,
                        extra: Extra::new(),
                    },
                );
            }
        }

        let start_host = platform.host_now().0;
        self.sh.start_host.store(start_host, Ordering::Release);
        if let Some(d) = self.req.duration_ns {
            self.sh
                .stop_host
                .store(start_host.saturating_add(d.max(1)), Ordering::Release);
        }
        self.project.recording.start_host_ns = start_host;
        self.journal(JournalEvent::Start {
            start_host_ns: start_host,
            clock: platform.clock_kind(),
            fps_requested: self.req.fps,
            resolution: resolution_text(self.req.resolution),
            system_audio: self.req.system_audio,
        });
        hold(HoldPoint::AfterStart);
        self.router(RouterCmd::Begin { start_host });
        self.save();
        let (htx, hrx) = mpsc::channel();
        let h = health::spawn(self.sh.clone(), hrx)
            .map_err(|e| Error::State(format!("spawn the health thread: {e}")))?;
        self.health = Some((htx, h));
        self.sh.set_state(SessionState::Recording);
        Ok(())
    }

    fn track(
        &self,
        id: &str,
        kind: TrackKind,
        file: &str,
        video: Option<sc_core::project::VideoInfo>,
        audio: Option<(u32, u16)>,
    ) -> Track {
        let (container, codec) = container_codec(self.ext, video.is_some());
        Track {
            id: id.to_string(),
            kind,
            segment_id: None,
            file: file.to_string(),
            container,
            codec,
            first_sample_offset_ns: 0,
            duration_ns: None,
            video,
            audio: audio.map(|(sample_rate, channels)| AudioInfo {
                sample_rate,
                channels,
                extra: Extra::new(),
            }),
            device: None,
            health: empty_health(),
            fragment_interval_s: self.req.tuning.fragment_interval_s,
            extra: Extra::new(),
        }
    }

    fn spawn_track(
        &self,
        id: &str,
        kind: TrackKind,
        writer: Box<dyn sc_platform::TrackWriter>,
    ) -> Result<Cont> {
        let feed = match kind {
            TrackKind::Camera => &self.sink.camera,
            TrackKind::Mic => &self.sink.mic,
            _ => &self.sink.sys,
        };
        let video = kind == TrackKind::Camera;
        let stats = self.sh.registry.add(id, video);
        let cap = if video {
            self.req.tuning.camera_queue
        } else {
            self.req.tuning.audio_queue
        };
        let (tx, rx) = mpsc::sync_channel(cap.max(1));
        let join = tracks::spawn(
            TrackCtx {
                sh: self.sh.clone(),
                track_id: id.to_string(),
                kind,
                writer,
                stats: stats.clone(),
            },
            rx,
        )
        .map_err(|e| Error::State(format!("spawn the {id} writer thread: {e}")))?;
        let _ = feed.set(Feed {
            tx: tx.clone(),
            stats,
        });
        Ok(Cont {
            kind,
            device: None,
            stream: None,
            tx,
            join,
        })
    }

    fn run(mut self) {
        loop {
            let wait = self
                .switch
                .as_ref()
                .map_or(Duration::from_millis(250), |s| {
                    s.deadline.saturating_duration_since(Instant::now())
                });
            match self.rx.recv_timeout(wait) {
                Ok(m) => {
                    if self.handle(m) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.flush_keys();
                    self.check_deadline();
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = self.finalize(StopReason::Quit);
                    return;
                }
            }
        }
    }

    fn live(&self) -> bool {
        self.finished.is_none()
    }

    fn handle(&mut self, m: Ctl) -> bool {
        match m {
            Ctl::Switch(spec, via) => {
                if self.live() {
                    self.request_switch(spec, via);
                }
            }
            Ctl::SetExclusions(list, reply) => {
                self.req.exclude = list;
                let _ = reply.send(self.update_exclusions());
            }
            Ctl::Stop(reason, reply) => {
                let _ = reply.send(self.finalize(reason));
                return true;
            }
            Ctl::Discard(reply) => {
                if self.live() {
                    let _ = self.finalize(StopReason::Quit);
                }
                let r = std::fs::remove_dir_all(&self.sh.dir)
                    .map_err(|e| Error::State(format!("remove {}: {e}", self.sh.dir.display())));
                let _ = reply.send(r);
                return true;
            }
            Ctl::Event(e) => {
                if self.live() {
                    self.on_event(e);
                }
            }
            Ctl::Cut(o) => {
                if self.live() {
                    self.on_cut(o);
                }
            }
            Ctl::OpenFailed { tag, error } => {
                if !self.live() {
                    return false;
                }
                if self.switch.as_ref().is_some_and(|s| s.tag == tag) {
                    if let Some(sw) = self.switch.as_mut()
                        && let Some(s) = sw.stream.take()
                    {
                        let _ = s.stop();
                    }
                    self.start_failed(error);
                } else if self.active.is_none() {
                    let _ = self.finalize(StopReason::Error);
                }
            }
            Ctl::CameraKey(k, sent) => {
                if self.live() {
                    self.on_camera_key(k, sent);
                }
            }
            Ctl::DiskStop(free) => {
                if self.live() {
                    self.log(
                        Level::Error,
                        &format!("stopping: {free} bytes of disk left"),
                    );
                    let _ = self.finalize(StopReason::Disk);
                }
            }
        }
        false
    }

    fn on_camera_key(&mut self, mut k: CameraKey, sent_host: i64) {
        if self.camera_keys.is_empty() {
            return;
        }
        let last = self.camera_keys.last().map_or(0, |l| l.t_rec_ns);
        k.t_rec_ns = (sent_host - self.sh.start_host()).max(last);
        k.easing = key_easing(k.easing, k.t_rec_ns - last);
        self.camera_keys.push(k.clone());
        self.pending_keys.push(k);
        if self
            .pending_since
            .get_or_insert_with(Instant::now)
            .elapsed()
            >= KEY_FLUSH
        {
            self.flush_keys();
        }
    }

    fn flush_keys(&mut self) {
        self.pending_since = None;
        if self.pending_keys.is_empty() {
            return;
        }
        let keys = std::mem::take(&mut self.pending_keys);
        self.sh.rec.journal(&JournalEvent::CameraKeys { keys });
    }

    fn update_exclusions(&self) -> Result<()> {
        let mut r = Ok(());
        let streams = self
            .screen
            .iter()
            .map(|l| &l.stream)
            .chain(self.switch.iter().filter_map(|s| s.stream.as_ref()));
        for s in streams {
            if let Err(e) = s.update_exclusions(&self.req.exclude) {
                self.log(Level::Warn, &format!("update exclusions: {e}"));
                r = Err(Error::Platform(e));
            }
        }
        r
    }

    fn app_for(&self, spec: &SourceSpec) -> Option<AppRef> {
        let lookup = |w: WindowId| self.platform().screen().window_app(w).ok().flatten();
        match spec {
            SourceSpec::Application { app, .. } => Some(app.clone()),
            SourceSpec::Window { window } => lookup(*window),
            SourceSpec::Picked(p) => match &p.descriptor {
                SourceDescriptor::Application {
                    app_pid: Some(pid),
                    app_bundle_id,
                    ..
                } => Some(AppRef {
                    pid: *pid,
                    bundle_id: app_bundle_id.clone(),
                }),
                SourceDescriptor::Window { window_id, .. } => lookup(WindowId(*window_id)),
                _ => None,
            },
            _ => None,
        }
    }

    fn follow_app(&self) {
        if self.req.system_audio != SystemAudioMode::App {
            return;
        }
        let Some(sys) = &self.sys_stream else { return };
        let app = self.cur.as_ref().and_then(|c| self.app_for(&c.spec));
        if let Err(e) = sys.follow_app(app.as_ref()) {
            self.log(Level::Warn, &format!("system audio follow_app: {e}"));
        }
    }

    fn apply_closed(&mut self, c: &Closed) {
        if let Some(s) = self
            .project
            .recording
            .screen
            .segments
            .iter_mut()
            .find(|s| s.id == c.seg)
        {
            s.end_rec_ns = Some(c.end_rec_ns);
            s.end_reason = Some(c.reason);
        }
    }

    fn on_cut(&mut self, o: Opened) {
        let settling = self.switch.as_ref().is_some_and(|s| s.tag == o.tag);
        let old_running = o
            .old_tag
            .is_some_and(|t| self.screen.as_ref().is_some_and(|l| l.tag == t));
        if settling
            && old_running
            && let Some(rx) = &o.old_passed
        {
            let frame_ns = 1_000_000_000 / i64::from(self.req.fps.max(1));
            let drain = self
                .platform()
                .host_duration(2 * frame_ns + OLD_DRAIN_SLACK_NS);
            let _ = rx.recv_timeout(drain);
        }
        let mut restored_from = None;
        if settling && let Some(mut sw) = self.switch.take() {
            if let Some(old) = self.screen.take() {
                self.stop_screen(old.stream, "old");
            }
            if let Some(stream) = sw.stream.take() {
                self.screen = Some(Live { tag: o.tag, stream });
            }
            if sw.stage == Stage::Restore {
                restored_from = Some(sw.error.clone());
            }
            self.cur = Some(Cur {
                spec: sw.spec,
                via: sw.via,
            });
        }
        if let Some(old_tag) = o.old_tag
            && let Some(Some(closed)) = self.router_rpc(|reply| RouterCmd::OldStopped {
                tag: old_tag,
                reply,
            })
        {
            self.apply_closed(&closed);
        }
        let seg = o.segment;
        self.active = Some((o.tag, seg.id.clone()));
        let first = self.project.recording.screen.segments.is_empty();
        if first && self.project.output.canvas.aspect == Aspect::Original {
            self.project.output.canvas.ratio = Ratio::of(SizePx {
                w: seg.content_px.w,
                h: seg.content_px.h,
            });
        }
        let mut t = self.track(
            &seg.track_id,
            TrackKind::Screen,
            &o.file,
            Some(video_info(seg.frame_px, self.req.fps)),
            None,
        );
        t.segment_id = Some(seg.id.clone());
        t.first_sample_offset_ns = seg.start_rec_ns;
        self.project.recording.tracks.push(t);
        self.project.recording.screen.segments.push(seg);
        self.save();
        self.follow_app();
        if settling {
            if let Some(error) = restored_from {
                self.sh.notice(Notice::SwitchFailed {
                    error,
                    restored: true,
                });
            }
            self.settled();
        }
    }

    fn settled(&mut self) {
        self.sh.set_state(SessionState::Recording);
        if let Some((spec, via)) = self.queued.take() {
            self.request_switch(spec, via);
        }
    }

    fn request_switch(&mut self, spec: SourceSpec, via: Via) {
        if self.switch.is_some() {
            self.queued = Some((spec, via));
            return;
        }
        let host = self.sh.now();
        let (desc, size) = match self.platform().screen().describe(&spec, &self.cfg()) {
            Ok(x) => x,
            Err(e) => {
                let error = format!("describe the new source: {e}");
                self.journal(JournalEvent::SwitchFailed {
                    host_ns: host,
                    error: error.clone(),
                    next: SwitchNext::KeepOld,
                });
                self.sh.notice(Notice::SwitchFailed {
                    error,
                    restored: self.screen.is_some(),
                });
                return;
            }
        };
        self.journal(JournalEvent::SwitchBegin {
            host_ns: host,
            to: desc.clone(),
            via,
        });
        hold(HoldPoint::AfterSwitchBegin);
        self.sh.set_state(SessionState::Switching);
        let mut sw = Switch {
            spec,
            via,
            desc,
            size,
            tag: StreamTag(0),
            deadline: Instant::now(),
            stage: Stage::First,
            stream: None,
            error: String::new(),
        };
        if let Err(e) = self.prepare(&mut sw) {
            let error = e.to_string();
            self.journal(JournalEvent::SwitchFailed {
                host_ns: self.sh.now(),
                error: error.clone(),
                next: SwitchNext::KeepOld,
            });
            self.sh.notice(Notice::SwitchFailed {
                error,
                restored: self.screen.is_some(),
            });
            self.settled();
            return;
        }
        self.switch = Some(sw);
        self.try_start();
    }

    fn prepare(&mut self, sw: &mut Switch) -> Result<()> {
        let (seg, track_id, file) = self.seg_names();
        self.journal(JournalEvent::SegmentPrepare {
            seg: seg.clone(),
            file: file.clone(),
            host_ns: self.sh.now(),
        });
        hold(HoldPoint::AfterSegmentPrepare);
        let spec = VideoWriterSpec {
            size: sw.size,
            fps: self.req.fps.max(1),
            codec: VideoCodec::Hevc,
            keyframe_interval_s: KEYFRAME_INTERVAL_S,
            fragment_interval_s: self.req.tuning.fragment_interval_s,
            bits_per_pixel: BITS_PER_PIXEL,
        };
        let writer = self
            .platform()
            .writers()
            .video(&self.sh.dir.join(&file), &spec)?;
        self.log(Level::Info, &format!("{seg}: writer created ({file})"));
        sw.tag = self.tag();
        self.router(RouterCmd::Prepare(Box::new(Prepared {
            tag: sw.tag,
            seg,
            track_id,
            file,
            writer: Some(writer),
            size: sw.size,
            via: sw.via,
            reason: StartReason::UserSwitch,
            source: sw.desc.clone(),
            closes_old_with: EndReason::UserSwitch,
        })));
        Ok(())
    }

    fn try_start(&mut self) {
        let cfg = self.cfg();
        let sink = self.sink();
        let timeout = self
            .platform()
            .host_duration(i64::from(self.req.tuning.switch_first_frame_timeout_ms) * 1_000_000);
        let Some(sw) = self.switch.as_mut() else {
            return;
        };
        match self
            .sh
            .platform
            .screen()
            .start(sw.tag, &sw.spec, &cfg, sink)
        {
            Ok(s) => {
                sw.stream = Some(s);
                sw.deadline = Instant::now() + timeout;
            }
            Err(e) => self.start_failed(e.to_string()),
        }
    }

    fn cancel_pending(&self, tag: StreamTag) -> bool {
        self.router_rpc(|reply| RouterCmd::Cancel { tag, reply })
            .unwrap_or(true)
    }

    fn start_failed(&mut self, error: String) {
        let Some(mut sw) = self.switch.take() else {
            return;
        };
        sw.error = error.clone();
        match sw.stage {
            Stage::First if self.screen.is_some() => {
                self.journal(JournalEvent::SwitchFailed {
                    host_ns: self.sh.now(),
                    error,
                    next: SwitchNext::RetryStopThenStart,
                });
                if let Some(old) = self.screen.take() {
                    self.stop_screen(old.stream, "old");
                }
                sw.stage = Stage::Retry;
                self.switch = Some(sw);
                self.try_start();
            }
            Stage::First | Stage::Retry => self.restore(sw, error),
            Stage::Restore => self.lose(sw, error),
        }
    }

    fn check_deadline(&mut self) {
        let due = self
            .switch
            .as_ref()
            .is_some_and(|s| s.stream.is_some() && Instant::now() >= s.deadline);
        if !due {
            return;
        }
        let Some(mut sw) = self.switch.take() else {
            return;
        };
        if !self.cancel_pending(sw.tag) {
            sw.deadline = Instant::now() + Duration::from_secs(3600);
            self.switch = Some(sw);
            return;
        }
        let error = format!(
            "no frame from the new source within {} ms",
            self.req.tuning.switch_first_frame_timeout_ms
        );
        self.abandon(sw, error);
    }

    fn abandon(&mut self, mut sw: Switch, error: String) {
        if let Some(s) = sw.stream.take() {
            self.stop_screen(s, "new");
        }
        match sw.stage {
            Stage::First if self.screen.is_some() => {
                self.journal(JournalEvent::SwitchFailed {
                    host_ns: self.sh.now(),
                    error: error.clone(),
                    next: SwitchNext::KeepOld,
                });
                self.sh.notice(Notice::SwitchFailed {
                    error,
                    restored: true,
                });
                self.settled();
            }
            Stage::First | Stage::Retry => self.restore(sw, error),
            Stage::Restore => self.lose(sw, error),
        }
    }

    fn restore(&mut self, mut sw: Switch, error: String) {
        self.journal(JournalEvent::SwitchFailed {
            host_ns: self.sh.now(),
            error: error.clone(),
            next: SwitchNext::RestoreOld,
        });
        self.cancel_pending(sw.tag);
        if let Some(s) = sw.stream.take() {
            self.stop_screen(s, "new");
        }
        let Some(cur) = self.cur.clone() else {
            return self.lose(sw, error);
        };
        let (desc, size) = match self.platform().screen().describe(&cur.spec, &self.cfg()) {
            Ok(x) => x,
            Err(e) => return self.lose(sw, format!("{error}; describe the old source: {e}")),
        };
        let mut nsw = Switch {
            spec: cur.spec,
            via: cur.via,
            desc,
            size,
            tag: StreamTag(0),
            deadline: Instant::now(),
            stage: Stage::Restore,
            stream: None,
            error,
        };
        if let Err(e) = self.prepare(&mut nsw) {
            let error = format!("{}; {e}", nsw.error);
            return self.lose(nsw, error);
        }
        self.switch = Some(nsw);
        self.try_start();
    }

    fn lose(&mut self, mut sw: Switch, error: String) {
        self.journal(JournalEvent::SwitchFailed {
            host_ns: self.sh.now(),
            error: error.clone(),
            next: SwitchNext::SourceLost,
        });
        self.cancel_pending(sw.tag);
        if let Some(s) = sw.stream.take() {
            self.stop_screen(s, "new");
        }
        if let Some(old) = self.screen.take() {
            self.stop_screen(old.stream, "old");
        }
        self.lose_active(&error);
        self.sh.notice(Notice::SwitchFailed {
            error,
            restored: false,
        });
        self.settled();
    }

    fn lose_active(&mut self, reason: &str) {
        self.cur = None;
        let Some((tag, seg)) = self.active.take() else {
            return;
        };
        let at_host = self.sh.now();
        self.journal(JournalEvent::SourceLost {
            host_ns: at_host,
            seg: seg.clone(),
            reason: reason.to_string(),
        });
        if let Some(Some(closed)) = self.router_rpc(|reply| RouterCmd::Lose {
            tag,
            at_host,
            reply,
        }) {
            self.apply_closed(&closed);
        }
        self.save();
        self.sh.notice(Notice::SourceLost { segment: seg });
    }

    fn on_event(&mut self, e: SourceEvent) {
        match e {
            SourceEvent::SourceLost { stream, ref reason } => {
                let reason = reason.clone();
                self.stream_lost(stream, reason, e)
            }
            SourceEvent::StreamStopped {
                stream,
                error: Some(ref reason),
            } => {
                let reason = reason.clone();
                self.stream_lost(stream, reason, e)
            }
            SourceEvent::StreamStopped { .. } => {}
            SourceEvent::SourceResized {
                stream,
                content_rect_pt,
                content_scale,
            } => {
                if let Some((tag, seg)) = &self.active
                    && *tag == stream
                {
                    self.journal(JournalEvent::SourceResized {
                        host_ns: self.sh.now(),
                        seg: seg.clone(),
                        content_rect_pt,
                        content_scale,
                    });
                }
            }
            SourceEvent::DeviceLost { kind, device_id } => self.device_lost(kind, &device_id),
            SourceEvent::DevicesChanged { kind } => {
                self.log(Level::Info, &format!("devices changed: {kind:?}"));
            }
            SourceEvent::OwnWindowsChanged => {
                let _ = self.update_exclusions();
            }
            SourceEvent::SystemWillSleep => {
                let _ = self.finalize(StopReason::Sleep);
            }
            SourceEvent::SystemDidWake => self.log(Level::Info, "system did wake"),
        }
    }

    fn stream_lost(&mut self, stream: StreamTag, reason: String, ev: SourceEvent) {
        if self.switch.as_ref().is_some_and(|s| s.tag == stream) {
            if !self.cancel_pending(stream) {
                self.sh.send(Ctl::Event(ev));
                return;
            }
            if let Some(sw) = self.switch.take() {
                self.abandon(sw, format!("new source lost: {reason}"));
            }
            return;
        }
        if self.screen.as_ref().is_some_and(|l| l.tag == stream) {
            if let Some(l) = self.screen.take() {
                self.stop_screen(l.stream, "lost");
            }
            if self.active.as_ref().is_some_and(|(t, _)| *t == stream) {
                self.lose_active(&reason);
            }
        }
    }

    fn device_lost(&mut self, kind: DeviceKind, device_id: &str) {
        let Some(i) = self.cont.iter().position(|c| {
            c.device
                .as_ref()
                .is_some_and(|(k, id, _)| *k == kind && id == device_id)
        }) else {
            return;
        };
        let mut c = self.cont.remove(i);
        let (_, id, name) = c
            .device
            .clone()
            .unwrap_or((kind, device_id.into(), device_id.into()));
        let track_id = match c.kind {
            TrackKind::Camera => "camera",
            _ => "mic",
        };
        self.journal(JournalEvent::Device {
            host_ns: self.sh.now(),
            track_id: Some(track_id.into()),
            kind: device_kind_j(kind),
            change: DeviceChange::Lost,
            name: name.clone(),
            unique_id: id,
        });
        if let Some(s) = c.stream.take() {
            let _ = s.stop();
        }
        self.finish_cont(c, None);
        self.sh.notice(Notice::DeviceLost { kind, name });
    }

    fn finish_cont(&mut self, c: Cont, end_host: Option<i64>) {
        let _ = c.tx.send(TrackIn::Finish { end_host });
        match c.join.join() {
            Ok(r) => self.results.push(r),
            Err(_) => self.log(
                Level::Error,
                &format!("{:?} writer thread panicked", c.kind),
            ),
        }
    }

    /// Stops every source and writer; returns the router's closing report.
    fn shutdown(&mut self, stop_host: i64) -> Option<RouterDone> {
        if let Some(mut sw) = self.switch.take()
            && let Some(s) = sw.stream.take()
        {
            self.stop_screen(s, "new");
        }
        if let Some(l) = self.screen.take() {
            self.stop_screen(l.stream, "current");
        }
        if let Some(s) = self.sys_stream.take()
            && let Err(e) = s.stop()
        {
            self.log(Level::Warn, &format!("stop system audio: {e}"));
        }
        for c in &mut self.cont {
            if let Some(s) = c.stream.take()
                && let Err(e) = s.stop()
            {
                self.sh
                    .rec
                    .log(Level::Warn, &format!("stop {:?}: {e}", c.kind));
            }
        }
        if let Some(w) = self.watch.take() {
            let _ = w.stop();
        }
        let done = self.router_rpc(|reply| RouterCmd::Stop { stop_host, reply });
        if let Some(j) = self.router.take() {
            let _ = j.join();
        }
        for c in std::mem::take(&mut self.cont) {
            self.finish_cont(c, Some(stop_host));
        }
        if let Some((tx, j)) = self.health.take() {
            drop(tx);
            let _ = j.join();
        }
        done
    }

    fn finalize(&mut self, reason: StopReason) -> Result<StopSummary> {
        if let Some(f) = &self.finished {
            return f.clone().map_err(Error::State);
        }
        self.sh.set_state(SessionState::Stopping);
        let now = self.sh.now();
        let stop_host = self
            .sh
            .stop_host
            .fetch_min(now, std::sync::atomic::Ordering::AcqRel)
            .min(now);
        // Observers treat Finalized/Failed as the last word, so the reason goes out first.
        match reason {
            StopReason::Sleep => self.sh.notice(Notice::StoppedForSleep),
            StopReason::Disk => self.sh.notice(Notice::StoppedForDisk),
            _ => {}
        }
        let start = self.sh.start_host();
        let done = self.shutdown(stop_host);
        if let Some(d) = &done {
            for c in &d.closed {
                self.apply_closed(c);
            }
            for (id, r) in &d.finished {
                if let Err(e) = r {
                    self.log(Level::Error, &format!("{id}: {e}"));
                }
            }
            if d.recreated > 0 {
                self.log(
                    Level::Info,
                    &format!(
                        "{} segment writer(s) recreated at the first frame's size",
                        d.recreated
                    ),
                );
            }
        }
        // A frame stamped just after `now()` can reach the router before the stop time is
        // published; the last segment then ends a fraction of a frame later. Move the stop
        // to that end so the segments still tile [0, duration).
        let stop_host = done
            .as_ref()
            .and_then(|d| d.closed.iter().map(|c| c.end_rec_ns).max())
            .map_or(stop_host, |end| stop_host.max(start + end));
        self.journal(JournalEvent::Stop {
            stop_host_ns: stop_host,
            reason,
        });
        let duration = (stop_host - start).max(0);
        self.fill_tracks(start, duration);
        let health = health::snapshot(&self.sh, stop_host, &|_, frames| {
            if duration > 0 {
                frames as f64 * 1e9 / duration as f64
            } else {
                0.0
            }
        });
        self.flush_keys();
        let keys = std::mem::take(&mut self.camera_keys);
        let p = &mut self.project;
        if !keys.is_empty() {
            super::set_camera_layout(p, keys);
        }
        p.state = ProjectState::Finalized;
        p.recording.stop_host_ns = Some(stop_host);
        p.edl = Edl {
            clips: vec![Clip {
                id: "c0".into(),
                src_in_rec_ns: 0,
                src_out_rec_ns: duration,
                speed: Speed { num: 1, den: 1 },
                muted: false,
                extra: Extra::new(),
            }],
            extra: Extra::new(),
        };
        let saved = self.project.save(&self.sh.dir);
        self.lock.take();
        let result = match saved {
            Ok(()) => {
                let s = StopSummary {
                    project_dir: self.sh.dir.clone(),
                    duration_ns: duration,
                    segments: self.project.recording.screen.segments.len(),
                    health,
                };
                self.log(
                    Level::Info,
                    &format!(
                        "finalized: {} ns, {} segment(s), reason {reason:?}",
                        duration, s.segments
                    ),
                );
                self.sh.set_state(SessionState::Finalized {
                    project_dir: self.sh.dir.clone(),
                });
                Ok(s)
            }
            Err(e) => {
                let error = format!("write the final project.json: {e}");
                self.log(Level::Error, &error);
                self.sh.set_state(SessionState::Failed {
                    error: error.clone(),
                });
                Err(error)
            }
        };
        self.finished = Some(result.clone());
        result.map_err(Error::State)
    }

    fn fill_tracks(&mut self, start: i64, duration: i64) {
        let segs: Vec<ScreenSegment> = self.project.recording.screen.segments.clone();
        for t in &mut self.project.recording.tracks {
            if t.kind == TrackKind::Screen {
                if let Some(s) = segs.iter().find(|s| Some(&s.id) == t.segment_id.as_ref()) {
                    t.duration_ns = Some(s.end_rec_ns.unwrap_or(duration) - s.start_rec_ns);
                }
                if let Some(st) = self.sh.registry.get(&t.id) {
                    let snap = st.snap(&t.id, 0.0);
                    t.health.frames = snap.frames;
                    t.health.dropped = snap.dropped;
                    t.health.writer_not_ready = snap.writer_not_ready;
                }
                continue;
            }
            let Some(r) = self.results.iter().find(|r| r.track_id == t.id) else {
                continue;
            };
            if let Some(first) = r.first_host {
                t.first_sample_offset_ns = first - start;
            }
            if let Some(Ok(f)) = &r.finished {
                t.duration_ns = f.duration_ns;
            }
            let snap = r.stats.snap(&t.id, 0.0);
            t.health.frames = snap.frames;
            t.health.dropped = snap.dropped;
            t.health.writer_not_ready = snap.writer_not_ready;
            t.health.gaps = r.gaps.clone();
        }
    }
}

fn audio_spec(rate: u32, channels: u16, req: &RecordRequest) -> AudioWriterSpec {
    AudioWriterSpec {
        sample_rate: rate,
        channels,
        bitrate: 96_000 * u32::from(channels.clamp(1, 2)),
        fragment_interval_s: req.tuning.fragment_interval_s,
    }
}

/// The settings a recording was asked for, once at start (the config log the M1 e2e checks).
pub(crate) fn config_line(req: &RecordRequest) -> String {
    let v = serde_json::json!({
        "source": format!("{:?}", req.source),
        "fps": req.fps,
        "resolution": req.resolution,
        "cursor": req.show_cursor,
        "click_highlight": req.click_highlight,
        "camera": req.camera,
        "mic": req.mic,
        "system_audio": req.system_audio,
        "aspect": req.canvas.aspect,
        "excluded_windows": req.exclude.len(),
    });
    format!("config {v}")
}
