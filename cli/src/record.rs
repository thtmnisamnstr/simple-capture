//! `record`: one session on a worker thread; the main thread pumps the run loop (macOS native).
//! Stops at `--duration`, on SIGINT/SIGTERM, or on a `stop` line on stdin (EOF is ignored).

use std::io::BufRead;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use sc_core::project::{ProjectV1, Via};
use sc_geometry::Aspect;
use sc_platform::{AudioTrackKind, CaptureDevice, Platform, Resolution, SystemAudioMode, WindowId};
use sc_record::{
    HealthSnapshot, Notice, RecordRequest, Session, SessionObserver, SessionState, StopReason,
    Tuning,
};
use serde_json::{Value, json};

use crate::args::{PlatformChoice, RecordArgs};
use crate::commands::{CliError, CliResult, EXIT_ERROR, EXIT_OK, print_json};
use crate::host;
use crate::spec::{self, BgChoice, SpecText};

#[derive(Default)]
#[cfg_attr(not(all(target_os = "macos", debug_assertions)), allow(dead_code))]
struct Ctl {
    started: Mutex<Option<Instant>>,
    user_stop: AtomicBool,
}

#[cfg_attr(not(all(target_os = "macos", debug_assertions)), allow(dead_code))]
enum OwnMsg {
    Add(WindowId, mpsc::Sender<()>),
    Remove(WindowId),
}

struct Plan {
    args: RecordArgs,
    synthetic: bool,
    source: SpecText,
    switches: Vec<(f64, String, SpecText)>,
    background: Option<BgChoice>,
}

pub fn run(json: bool, choice: PlatformChoice, args: RecordArgs) -> CliResult {
    let source = spec::parse_spec(&args.source).map_err(CliError::usage)?;
    let mut switches = args
        .switches
        .iter()
        .map(|(t, s)| {
            if !(t.is_finite() && *t >= 0.0) {
                return Err(CliError::usage(format!("--switch {t}={s}: bad time")));
            }
            Ok((*t, s.clone(), spec::parse_spec(s).map_err(CliError::usage)?))
        })
        .collect::<Result<Vec<_>, _>>()?;
    switches.sort_by(|a, b| a.0.total_cmp(&b.0));
    let background = args
        .background
        .as_deref()
        .map(spec::parse_bg)
        .transpose()
        .map_err(CliError::usage)?;
    if let Some(d) = args.duration_s
        && !(d.is_finite() && d > 0.0)
    {
        return Err(CliError::usage("--duration must be positive"));
    }
    let own_windows_ok =
        cfg!(all(target_os = "macos", debug_assertions)) && choice == PlatformChoice::Native;
    if !args.own_test_windows.is_empty() && !own_windows_ok {
        return Err(CliError::usage(
            "--own-test-window needs a macOS debug build and --platform native",
        ));
    }

    let platform = host::platform(choice)?;
    host::install_signal_handlers();
    let ctl = Arc::new(Ctl::default());
    watch_stdin(ctl.clone());
    let (own_tx, own_rx) = mpsc::channel();
    let own = args.own_test_windows.clone();
    let events = args.events;
    let plan = Plan {
        args,
        synthetic: choice == PlatformChoice::Synthetic,
        source,
        switches,
        background,
    };
    let work = {
        let (platform, ctl) = (platform.clone(), ctl.clone());
        move || drive(platform, plan, ctl, own_rx, json)
    };
    let tick = own_windows_tick(platform, ctl, own_tx, own, events);
    host::run_main(host::needs_main_loop(choice), work, tick)
}

fn watch_stdin(ctl: Arc<Ctl>) {
    let spawned = std::thread::Builder::new()
        .name("sc-cli-stdin".to_string())
        .spawn(move || {
            for line in std::io::stdin().lock().lines() {
                match line {
                    Ok(l) if l.trim() == "stop" => {
                        ctl.user_stop.store(true, Ordering::SeqCst);
                        return;
                    }
                    Ok(_) => {}
                    Err(_) => return,
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("no stdin watcher ({e}); stop with a signal or --duration");
    }
}

pub fn pick_device<'a>(devs: &'a [CaptureDevice], want: &str) -> Option<&'a CaptureDevice> {
    if want == "default" {
        return devs.iter().find(|d| d.is_default).or_else(|| devs.first());
    }
    let lower = want.to_lowercase();
    devs.iter()
        .find(|d| d.id == want)
        .or_else(|| devs.iter().find(|d| d.name == want))
        .or_else(|| devs.iter().find(|d| d.name.to_lowercase().contains(&lower)))
}

fn device(
    list: impl FnOnce() -> sc_platform::Result<Vec<CaptureDevice>>,
    want: &str,
    flag: &str,
) -> Result<Option<String>, CliError> {
    if want == "none" {
        return Ok(None);
    }
    let devs = list().map_err(|e| CliError::from(e).context(flag))?;
    match pick_device(&devs, want) {
        Some(d) => Ok(Some(d.id.clone())),
        None => Err(CliError::error(format!(
            "{flag} {want:?}: no such device (have: {})",
            devs.iter()
                .map(|d| format!("{:?}", d.name))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn build_request(platform: &dyn Platform, plan: &Plan) -> Result<RecordRequest, CliError> {
    let a = &plan.args;
    let projects_dir = match &a.projects_dir {
        Some(d) => d.clone(),
        None => platform.default_projects_dir()?,
    };
    std::fs::create_dir_all(&projects_dir).map_err(|e| {
        CliError::error(format!(
            "create projects folder {}: {e}",
            projects_dir.display()
        ))
    })?;
    let source = spec::resolve(platform, &plan.source, plan.synthetic)
        .map_err(|e| CliError::from(e).context("--source"))?;
    let aspect = Aspect::parse(&a.aspect)
        .ok_or_else(|| CliError::usage(format!("--aspect {:?}", a.aspect)))?;
    let mut canvas = ProjectV1::new_recording(
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        platform.clock_kind(),
    )
    .output
    .canvas;
    canvas.aspect = aspect;
    if let Some(r) = aspect.ratio() {
        canvas.ratio = r;
    }
    let mut background_image: Option<PathBuf> = None;
    match &plan.background {
        Some(BgChoice::Canvas(bg)) => canvas.background = bg.clone(),
        Some(BgChoice::Image(p)) => {
            background_image = Some(std::fs::canonicalize(p).map_err(|e| {
                CliError::error(format!("--background image {}: {e}", p.display()))
            })?);
        }
        None => {}
    }
    Ok(RecordRequest {
        projects_dir,
        title: a.title.clone().unwrap_or_else(|| "Recording".to_string()),
        source,
        via: Via::OwnChooser,
        fps: a.fps,
        resolution: if a.resolution == "1x" {
            Resolution::OneX
        } else {
            Resolution::Native
        },
        show_cursor: a.cursor,
        click_highlight: a.clicks,
        camera: device(|| platform.camera().devices(), &a.camera, "--camera")?,
        mic: device(|| platform.mic().devices(), &a.mic, "--mic")?,
        system_audio: match a.system_audio.as_str() {
            "all" => SystemAudioMode::All,
            "app" => SystemAudioMode::App,
            _ => SystemAudioMode::Off,
        },
        canvas,
        background_image,
        exclude: Vec::new(),
        tuning: Tuning::default(),
    })
}

fn reason_text(r: StopReason) -> &'static str {
    match r {
        StopReason::User => "user",
        StopReason::Duration => "duration",
        StopReason::Signal => "signal",
        StopReason::Quit => "quit",
        StopReason::Disk => "disk",
        StopReason::Sleep => "sleep",
        StopReason::Error => "error",
    }
}

fn drive(
    platform: Arc<dyn Platform>,
    plan: Plan,
    ctl: Arc<Ctl>,
    own_rx: mpsc::Receiver<OwnMsg>,
    json: bool,
) -> CliResult {
    let events = plan.args.events;
    let req = build_request(&*platform, &plan)?;
    let session = Session::start(platform.clone(), req, Arc::new(Observer { events }))
        .map_err(|e| CliError::from(e).context("record"))?;
    let t0 = Instant::now();
    if let Ok(mut s) = ctl.started.lock() {
        *s = Some(t0);
    }
    if events {
        print_json(&json!({
            "ev": "started",
            "project_dir": session.project_dir(),
            "host_ns": platform.host_now().0,
        }));
    }
    let mut pending = plan.switches.into_iter().peekable();
    let mut excluded: Vec<WindowId> = Vec::new();
    let mut failed = false;
    let mut self_finalized: Option<PathBuf> = None;
    let reason = loop {
        if host::signalled() {
            break StopReason::Signal;
        }
        if ctl.user_stop.load(Ordering::SeqCst) {
            break StopReason::User;
        }
        let elapsed = t0.elapsed().as_secs_f64();
        if plan.args.duration_s.is_some_and(|d| elapsed >= d) {
            break StopReason::Duration;
        }
        match session.state() {
            SessionState::Failed { .. } => {
                failed = true;
                break StopReason::Error;
            }
            SessionState::Finalized { project_dir } => {
                self_finalized = Some(project_dir);
                break StopReason::Error;
            }
            _ => {}
        }
        while let Some((_, text, sp)) = pending.next_if(|(t, _, _)| *t <= elapsed) {
            switch(&*platform, &session, &text, &sp, plan.synthetic, events);
        }
        while let Ok(msg) = own_rx.try_recv() {
            let ack = match msg {
                OwnMsg::Add(id, ack) => {
                    excluded.push(id);
                    Some(ack)
                }
                OwnMsg::Remove(id) => {
                    excluded.retain(|w| *w != id);
                    None
                }
            };
            if let Err(e) = session.set_exclusions(excluded.clone()) {
                eprintln!("simple-capture record: set_exclusions: {e}");
            }
            if let Some(ack) = ack {
                let _ = ack.send(());
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if events {
        print_json(&json!({
            "ev": "stop_request",
            "reason": reason_text(reason),
            "host_ns": platform.host_now().0,
        }));
    }
    let project_dir = session.project_dir().to_path_buf();
    let summary = match (session.stop(reason), self_finalized) {
        (Ok(s), _) => s,
        (Err(e), Some(dir)) => {
            tracing::info!("stop after the session finalized itself: {e}");
            if json {
                print_json(&json!({ "ev": "stopped", "project_dir": dir, "reason": "session" }));
            } else {
                println!("{}", dir.display());
            }
            return Ok(EXIT_OK);
        }
        (Err(e), None) => {
            return Err(CliError::from(e).context(&format!("stop {}", project_dir.display())));
        }
    };
    if json {
        print_json(&json!({
            "ev": "stopped",
            "project_dir": summary.project_dir,
            "reason": reason_text(reason),
            "duration_ns": summary.duration_ns,
            "segments": summary.segments,
        }));
    } else {
        println!("{}", summary.project_dir.display());
    }
    Ok(if failed { EXIT_ERROR } else { EXIT_OK })
}

fn switch(
    platform: &dyn Platform,
    session: &Session,
    text: &str,
    sp: &SpecText,
    synthetic: bool,
    events: bool,
) {
    let host_ns = platform.host_now().0;
    let result = spec::resolve(platform, sp, synthetic)
        .map_err(|e| CliError::from(e).msg)
        .and_then(|spec| {
            session
                .switch_source(spec, Via::OwnChooser)
                .map_err(|e| e.to_string())
        });
    match result {
        Ok(()) if events => print_json(&json!({
            "ev": "switch_request", "spec": text, "host_ns": host_ns,
        })),
        Ok(()) => {}
        Err(e) => {
            eprintln!("simple-capture record: --switch {text}: {e}");
            if events {
                print_json(&json!({
                    "ev": "switch_error", "spec": text, "host_ns": host_ns, "error": e,
                }));
            }
        }
    }
}

struct Observer {
    events: bool,
}

fn state_json(s: &SessionState) -> Value {
    match s {
        SessionState::Preparing => json!({ "ev": "state", "state": "preparing" }),
        SessionState::Recording => json!({ "ev": "state", "state": "recording" }),
        SessionState::Switching => json!({ "ev": "state", "state": "switching" }),
        SessionState::Stopping => json!({ "ev": "state", "state": "stopping" }),
        SessionState::Finalized { project_dir } => {
            json!({ "ev": "state", "state": "finalized", "project_dir": project_dir })
        }
        SessionState::Failed { error } => {
            json!({ "ev": "state", "state": "failed", "error": error })
        }
    }
}

fn track_kind(k: AudioTrackKind) -> &'static str {
    match k {
        AudioTrackKind::Mic => "mic",
        AudioTrackKind::SystemAudio => "system_audio",
    }
}

pub fn notice_json(n: &Notice) -> Value {
    let body = match n {
        Notice::SwitchFailed { error, restored } => {
            json!({ "notice": "switch_failed", "error": error, "restored": restored })
        }
        Notice::SourceLost { segment } => json!({ "notice": "source_lost", "segment": segment }),
        Notice::DeviceLost { kind, name } => {
            json!({ "notice": "device_lost", "kind": kind, "name": name })
        }
        Notice::DiskLow { bytes_left } => json!({ "notice": "disk_low", "bytes_left": bytes_left }),
        Notice::AudioGap { track, dur_ns } => {
            json!({ "notice": "audio_gap", "track": track_kind(*track), "dur_ns": dur_ns })
        }
        Notice::FramesDropping { track, pct } => {
            json!({ "notice": "frames_dropping", "track": track, "pct": pct })
        }
        Notice::StoppedForSleep => json!({ "notice": "stopped_for_sleep" }),
        Notice::StoppedForDisk => json!({ "notice": "stopped_for_disk" }),
    };
    let mut v = json!({ "ev": "notice" });
    if let (Some(o), Value::Object(b)) = (v.as_object_mut(), body) {
        o.extend(b);
    }
    v
}

fn health_json(h: &HealthSnapshot) -> Value {
    let tracks: Vec<Value> = h
        .tracks
        .iter()
        .map(|t| {
            json!({
                "track_id": t.track_id, "frames": t.frames, "dropped": t.dropped,
                "writer_not_ready": t.writer_not_ready, "gaps": t.gaps, "overlaps": t.overlaps,
                "fps": t.fps, "drift_ns": t.drift_ns,
            })
        })
        .collect();
    json!({
        "ev": "health", "rec_ns": h.rec_ns, "free_disk_bytes": h.free_disk_bytes, "tracks": tracks,
    })
}

impl SessionObserver for Observer {
    fn on_state(&self, s: &SessionState) {
        if self.events {
            print_json(&state_json(s));
        } else if let SessionState::Failed { error } = s {
            eprintln!("simple-capture record: session failed: {error}");
        }
    }

    fn on_health(&self, h: &HealthSnapshot) {
        if self.events {
            print_json(&health_json(h));
        }
    }

    fn on_notice(&self, n: &Notice) {
        if self.events {
            print_json(&notice_json(n));
        } else {
            eprintln!("simple-capture record: {n:?}");
        }
    }
}

#[cfg(all(target_os = "macos", debug_assertions))]
fn own_windows_tick(
    platform: Arc<dyn Platform>,
    ctl: Arc<Ctl>,
    tx: mpsc::Sender<OwnMsg>,
    plan: Vec<(f64, f64)>,
    events: bool,
) -> impl FnMut() {
    use crate::ownwin::OwnWindow;

    enum Slot {
        Pending { at: f64, dur: f64 },
        Shown { win: OwnWindow, until: f64 },
        Done,
    }
    struct Sched {
        slots: Vec<Slot>,
    }
    impl Drop for Sched {
        fn drop(&mut self) {
            for s in self.slots.drain(..) {
                if let Slot::Shown { win, .. } = s {
                    win.close();
                }
            }
        }
    }
    let mut sched = Sched {
        slots: plan
            .into_iter()
            .map(|(at, dur)| Slot::Pending { at, dur })
            .collect(),
    };
    move || {
        let Some(t0) = ctl.started.lock().ok().and_then(|s| *s) else {
            return;
        };
        let now = t0.elapsed().as_secs_f64();
        for (i, slot) in sched.slots.iter_mut().enumerate() {
            match std::mem::replace(slot, Slot::Done) {
                Slot::Pending { at, dur } if now >= at => {
                    let title = format!("SC-OWN-TEST-{i}");
                    match OwnWindow::create(&*platform, &title) {
                        Ok(win) => {
                            let (ack_tx, ack_rx) = mpsc::channel();
                            if tx.send(OwnMsg::Add(win.id, ack_tx)).is_ok() {
                                let _ = sc_capture_macos::mainloop::pump_recv(
                                    &ack_rx,
                                    Duration::from_secs(3),
                                );
                            }
                            win.reveal();
                            if events {
                                let (x, y, w, h) = crate::ownwin::FRAME_PT;
                                print_json(&json!({
                                    "ev": "own_window", "state": "shown", "title": title,
                                    "window_id": win.id.0, "at_s": now,
                                    "cocoa_frame_pt": [x, y, w, h],
                                }));
                            }
                            *slot = Slot::Shown {
                                win,
                                until: at + dur,
                            };
                        }
                        Err(e) => eprintln!("simple-capture record: --own-test-window: {e}"),
                    }
                }
                Slot::Shown { win, until } if now >= until => {
                    let id = win.id;
                    win.close();
                    let _ = tx.send(OwnMsg::Remove(id));
                    if events {
                        print_json(&json!({
                            "ev": "own_window", "state": "hidden", "window_id": id.0, "at_s": now,
                        }));
                    }
                }
                other => *slot = other,
            }
        }
    }
}

#[cfg(not(all(target_os = "macos", debug_assertions)))]
fn own_windows_tick(
    _platform: Arc<dyn Platform>,
    _ctl: Arc<Ctl>,
    _tx: mpsc::Sender<OwnMsg>,
    _plan: Vec<(f64, f64)>,
    _events: bool,
) -> impl FnMut() {
    || {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, name: &str, default: bool) -> CaptureDevice {
        CaptureDevice {
            id: id.into(),
            name: name.into(),
            model_id: None,
            is_default: default,
            kind_hint: None,
        }
    }

    #[test]
    fn device_choice() {
        let devs = [
            dev("a1", "FaceTime HD Camera", false),
            dev("b2", "Studio Display Camera", true),
        ];
        let id = |w: &str| pick_device(&devs, w).map(|d| d.id.as_str());
        assert_eq!(id("default"), Some("b2"));
        assert_eq!(id("a1"), Some("a1"));
        assert_eq!(id("FaceTime HD Camera"), Some("a1"));
        assert_eq!(id("studio"), Some("b2"));
        assert_eq!(id("nope"), None);
        assert!(pick_device(&[], "default").is_none());
        assert_eq!(
            pick_device(&devs[..1], "default").map(|d| d.id.as_str()),
            Some("a1")
        );
    }

    #[test]
    fn notices_are_tagged() {
        let v = notice_json(&Notice::AudioGap {
            track: AudioTrackKind::SystemAudio,
            dur_ns: 5,
        });
        assert_eq!(v["ev"], "notice");
        assert_eq!(v["notice"], "audio_gap");
        assert_eq!(v["track"], "system_audio");
    }
}
