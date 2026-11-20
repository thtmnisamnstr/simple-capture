//! `sc-record-synth`: records with the synthetic platform until it is killed, for the kill tests
//! (M1 plan §8.3 wp-kill-recovery-test).
//!
//! ```text
//! sc-record-synth --projects-dir DIR [--switch-at-ms N] [--hold-at POINT] [--audio] [--camera]
//!                 [--max-s S] [--slow-start-ms N]
//! ```
//!
//! Records `synthetic:a` (320×180) and, with `--switch-at-ms`, switches to `synthetic:b`
//! (480×270). Prints `STARTED <rec ns> <project dir>` once the session runs, with the recording
//! time at that moment, and `SWITCH` when it asks for the switch. Stops by itself after
//! `--max-s` (default 60) so a dead test never leaves it running. `--slow-start-ms N` waits
//! N ms more before `STARTED`, as a slow `Session::start` does when its journal and project
//! writes are slow (Windows CI).
//!
//! `--hold-at POINT` (feature `test-hooks`) sets `SC_TEST_HOLD_AT` only just before the switch
//! (`after_start`: before the start), so the hold lands on the switch's journal line and not on
//! the first segment's, which the router journals with the same points.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_core::journal::{JournalEvent, read_journal};
use sc_core::project::{ProjectV1, Via};
use sc_geometry::SizePx;
use sc_platform::synthetic::{
    AudioScript, CAMERA_DEVICE_ID, MIC_DEVICE_ID, Scenario, SyntheticPlatform, VideoScript,
    display_spec,
};
use sc_platform::{Platform, Resolution, SystemAudioMode};
use sc_record::{
    HealthSnapshot, Notice, RecordRequest, Session, SessionObserver, SessionState, StopReason,
    Tuning,
};

const USAGE: &str = "usage: sc-record-synth --projects-dir DIR [--switch-at-ms N] \
                     [--hold-at POINT] [--audio] [--camera] [--max-s S] [--slow-start-ms N]";
const POINTS: [&str; 4] = [
    "after_start",
    "after_switch_begin",
    "after_segment_prepare",
    "after_segment_open",
];

struct Args {
    projects_dir: PathBuf,
    switch_at_ms: Option<u64>,
    hold_at: Option<String>,
    audio: bool,
    camera: bool,
    max_s: u64,
    slow_start_ms: u64,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        projects_dir: PathBuf::new(),
        switch_at_ms: None,
        hold_at: None,
        audio: false,
        camera: false,
        max_s: 60,
        slow_start_ms: 0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--projects-dir" => a.projects_dir = PathBuf::from(value()?),
            "--switch-at-ms" => {
                a.switch_at_ms = Some(value()?.parse().map_err(|e| format!("{arg}: {e}"))?)
            }
            "--hold-at" => {
                let p = value()?;
                if !POINTS.contains(&p.as_str()) {
                    return Err(format!("--hold-at {p:?}: one of {POINTS:?}"));
                }
                a.hold_at = Some(p);
            }
            "--audio" => a.audio = true,
            "--camera" => a.camera = true,
            "--max-s" => a.max_s = value()?.parse().map_err(|e| format!("{arg}: {e}"))?,
            "--slow-start-ms" => {
                a.slow_start_ms = value()?.parse().map_err(|e| format!("{arg}: {e}"))?
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if a.projects_dir.as_os_str().is_empty() {
        return Err("--projects-dir is required".into());
    }
    if a.hold_at.as_deref().is_some_and(|p| p != "after_start") && a.switch_at_ms.is_none() {
        return Err("--hold-at at a switch point needs --switch-at-ms".into());
    }
    Ok(a)
}

struct Obs;

impl SessionObserver for Obs {
    fn on_state(&self, s: &SessionState) {
        if let SessionState::Failed { error } = s {
            eprintln!("sc-record-synth: session failed: {error}");
        }
    }
    fn on_health(&self, _: &HealthSnapshot) {}
    fn on_notice(&self, n: &Notice) {
        eprintln!("sc-record-synth: notice {n:?}");
    }
}

fn arm(point: &str) {
    // SAFETY: every other thread belongs to the session or the synthetic platform, which are
    // pure Rust and read the environment only through `std::env`, serialised with this write
    // by std's environment lock.
    unsafe { std::env::set_var("SC_TEST_HOLD_AT", point) };
}

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn find_in_journal<T>(
    dir: &Path,
    f: impl Fn(&JournalEvent) -> Option<T>,
) -> Result<Option<T>, String> {
    let j = read_journal(dir).map_err(|e| format!("read the journal: {e}"))?;
    Ok(j.events.iter().find_map(f))
}

/// The router opens s0 after `Session::start` returns. Switching before s0's first frame is on
/// disk would journal `switch_begin` ahead of s0's `segment_open`, or hold at s0's hold points.
fn wait_s0_written(dir: &Path) -> Result<(), String> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let file = find_in_journal(dir, |e| match e {
            JournalEvent::SegmentOpen { seg, file, .. } if seg == "s0" => Some(dir.join(file)),
            _ => None,
        })?;
        let bytes = file.and_then(|f| std::fs::read(f).ok()).unwrap_or_default();
        let header = bytes.iter().position(|b| *b == b'\n');
        if header.is_some_and(|h| bytes.len() > h + 1) {
            return Ok(());
        }
        if Instant::now() > end {
            return Err("s0 has no frame on disk after 10 s".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn size(w: u32, h: u32) -> SizePx {
    SizePx { w, h }
}

fn run(a: Args) -> Result<(), String> {
    let audio = AudioScript {
        rate: 48_000,
        channels: 1,
        buffer_frames: 480,
        beeps_ns: Vec::new(),
        gaps: Vec::new(),
        overlaps: Vec::new(),
        clock_ppm: 0.0,
    };
    let mut scenario = Scenario::new();
    if a.audio {
        scenario = scenario
            .mic(Some(audio.clone()))
            .system_audio(Some(AudioScript {
                channels: 2,
                ..audio
            }));
    }
    if a.camera {
        scenario = scenario.camera(Some(VideoScript {
            size: size(320, 180),
            fps: 15,
        }));
    }
    let platform: Arc<dyn Platform> = SyntheticPlatform::new(scenario);
    let canvas = ProjectV1::new_recording(
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        platform.clock_kind(),
    )
    .output
    .canvas;
    let req = RecordRequest {
        projects_dir: a.projects_dir.clone(),
        title: "Kill test".into(),
        source: display_spec("a", Some(size(320, 180))),
        via: Via::OwnChooser,
        fps: 30,
        resolution: Resolution::Native,
        show_cursor: true,
        click_highlight: false,
        camera: a.camera.then(|| CAMERA_DEVICE_ID.to_string()),
        mic: a.audio.then(|| MIC_DEVICE_ID.to_string()),
        system_audio: if a.audio {
            SystemAudioMode::All
        } else {
            SystemAudioMode::Off
        },
        canvas,
        background_image: None,
        exclude: Vec::new(),
        duration_ns: None,
        tuning: Tuning::default(),
    };
    if a.hold_at.as_deref() == Some("after_start") {
        arm("after_start");
    }
    let t0 = Instant::now();
    let session =
        Session::start(platform.clone(), req, Arc::new(Obs)).map_err(|e| format!("start: {e}"))?;
    std::thread::sleep(Duration::from_millis(a.slow_start_ms));
    let dir = session.project_dir();
    let start_host = find_in_journal(dir, |e| match e {
        JournalEvent::Start { start_host_ns, .. } => Some(*start_host_ns),
        _ => None,
    })?
    .ok_or("the journal has no start event")?;
    let rec_ns = platform.host_now().0 - start_host;
    say(&format!("STARTED {rec_ns} {}", dir.display()));
    if let Some(ms) = a.switch_at_ms {
        wait_s0_written(dir)?;
        std::thread::sleep(Duration::from_millis(ms).saturating_sub(t0.elapsed()));
        if let Some(p) = a.hold_at.as_deref().filter(|p| *p != "after_start") {
            arm(p);
        }
        say("SWITCH");
        session
            .switch_source(display_spec("b", Some(size(480, 270))), Via::OwnChooser)
            .map_err(|e| format!("switch: {e}"))?;
    }
    std::thread::sleep(Duration::from_secs(a.max_s).saturating_sub(t0.elapsed()));
    let summary = session
        .stop(StopReason::Duration)
        .map_err(|e| format!("stop: {e}"))?;
    say(&format!("STOPPED {}", summary.project_dir.display()));
    Ok(())
}

fn main() -> ExitCode {
    let a = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("sc-record-synth: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if a.hold_at.is_some() && !cfg!(feature = "test-hooks") {
        eprintln!("sc-record-synth: --hold-at needs the feature test-hooks");
        return ExitCode::from(3);
    }
    match run(a) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sc-record-synth: {e}");
            ExitCode::FAILURE
        }
    }
}
