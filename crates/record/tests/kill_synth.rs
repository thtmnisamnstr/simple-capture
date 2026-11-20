//! Crash safety, CI version (M1 plan §8.3 wp-kill-recovery-test): `sc-record-synth` is killed
//! (SIGKILL / TerminateProcess) mid-segment and at the switch's hold points, then
//! `recover_project` must rebuild a `recovered` project losing at most 1 s per track.
//! The hold-point tests need `--features test-hooks` and print `SKIPPED:` without it.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use sc_core::journal::{JournalEvent, read_journal};
use sc_core::project::{EndReason, ProjectState, ProjectV1, TrackKind};
use sc_platform::synthetic::SctrkDecoder;
use sc_record::recovery::{Outcome, recover_project};

const S: i64 = 1_000_000_000;
const MAX_LOSS_NS: i64 = S;
/// `STARTED` is printed after `Session::start` returns, a little after `start_host`.
const START_SLACK_NS: i64 = S / 2;

struct Rec {
    child: Child,
    err: Receiver<String>,
    err_seen: Vec<String>,
    dir: PathBuf,
    t0: Instant,
}

impl Drop for Rec {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn lines(r: impl std::io::Read + Send + 'static) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(r).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("kill_synth")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn wait_line(rx: &Receiver<String>, timeout: Duration, want: impl Fn(&str) -> bool) -> String {
    let end = Instant::now() + timeout;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(l) if want(&l) => return l,
            Ok(_) => {}
            Err(e) => panic!("no expected line within {timeout:?}: {e}"),
        }
    }
}

fn start(name: &str, args: &[&str]) -> Rec {
    let projects = tmp(name);
    let mut child = Command::new(env!("CARGO_BIN_EXE_sc-record-synth"))
        .arg("--projects-dir")
        .arg(&projects)
        .args(args)
        .args(["--max-s", "40"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sc-record-synth");
    let out = lines(child.stdout.take().expect("stdout"));
    let err = lines(child.stderr.take().expect("stderr"));
    let started = wait_line(&out, Duration::from_secs(20), |l| l.starts_with("STARTED "));
    let t0 = Instant::now();
    let dir = PathBuf::from(started.trim_start_matches("STARTED "));
    Rec {
        child,
        err,
        err_seen: Vec::new(),
        dir,
        t0,
    }
}

impl Rec {
    fn wait_hold(&mut self, point: &str) {
        let want = format!("SC_HOLD {point}");
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.err.recv_timeout(left) {
                Ok(l) if l == want => return,
                Ok(l) => self.err_seen.push(l),
                Err(e) => panic!("no {want:?} within 10 s ({e}); stderr {:?}", self.err_seen),
            }
        }
    }

    fn wait_journal(&self, timeout: Duration, want: impl Fn(&[JournalEvent]) -> bool) {
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            if want(&events(&self.dir)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("journal never reached the state: {:#?}", events(&self.dir));
    }

    /// Returns the recording time of the kill, a lower bound.
    fn kill(mut self) -> (PathBuf, i64) {
        let at = self.t0.elapsed();
        self.child.kill().expect("kill");
        self.child.wait().expect("wait");
        let elapsed = i64::try_from(at.as_nanos()).unwrap_or(i64::MAX);
        let health = events(&self.dir)
            .iter()
            .filter_map(|e| match e {
                JournalEvent::Health { rec_ns, .. } => Some(*rec_ns),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        (self.dir.clone(), elapsed.max(health))
    }
}

fn events(dir: &Path) -> Vec<JournalEvent> {
    read_journal(dir).expect("read journal").events
}

fn name(e: &JournalEvent) -> String {
    let v = serde_json::to_value(e).expect("event json");
    v.get("ev")
        .and_then(|x| x.as_str())
        .unwrap_or("?")
        .to_string()
}

fn story(dir: &Path) -> Vec<String> {
    events(dir)
        .iter()
        .filter(|e| !matches!(e, JournalEvent::Health { .. }))
        .map(|e| match e {
            JournalEvent::SegmentPrepare { seg, .. }
            | JournalEvent::SegmentOpen { seg, .. }
            | JournalEvent::SegmentClose { seg, .. } => format!("{}:{seg}", name(e)),
            _ => name(e),
        })
        .collect()
}

fn switched(ev: &[JournalEvent]) -> bool {
    ev.iter()
        .any(|e| matches!(e, JournalEvent::SegmentClose { seg, .. } if seg == "s0"))
}

fn recover_and_check(dir: &Path, kill_rec: i64, segments: usize) -> ProjectV1 {
    let r = recover_project(dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Recovered, "{r:#?}");
    assert_eq!(r.segments, segments, "{r:#?}");
    let p = ProjectV1::load(dir).expect("load recovered project");
    assert_eq!(p.state, ProjectState::Recovered);
    let segs = &p.recording.screen.segments;
    assert_eq!(segs.len(), segments, "{segs:#?}");
    assert_eq!(segs[0].start_rec_ns, 0);
    for w in segs.windows(2) {
        assert_eq!(w[0].end_rec_ns, Some(w[1].start_rec_ns), "{segs:#?}");
    }
    let last = segs.last().expect("a segment");
    assert_eq!(last.end_reason, Some(EndReason::Recovered));
    let mut ends = vec![("screen".to_string(), last.end_rec_ns.expect("end"))];
    for t in &p.recording.tracks {
        if t.kind != TrackKind::Screen {
            let d = t.duration_ns.expect("recovered track duration");
            ends.push((t.id.clone(), t.first_sample_offset_ns + d));
        }
    }
    for (id, end) in &ends {
        let loss = kill_rec - end;
        assert!(
            (-START_SLACK_NS..=MAX_LOSS_NS).contains(&loss),
            "{id}: ends at {end} ns, killed at {kill_rec} ns (loss {loss} ns)"
        );
    }
    let total = p.duration_ns().expect("duration");
    assert!(
        (kill_rec - total).abs() <= MAX_LOSS_NS,
        "duration {total} vs kill {kill_rec}"
    );
    let again = recover_project(dir, &SctrkDecoder).expect("recover again");
    assert_eq!(again.outcome, Outcome::AlreadyFinal);
    p
}

fn hooks() -> bool {
    if !cfg!(feature = "test-hooks") {
        println!("SKIPPED: needs `--features test-hooks`");
    }
    cfg!(feature = "test-hooks")
}

#[test]
fn kill_mid_first_segment_with_camera_mic_and_system_audio() {
    let rec = start("mid-s0", &["--audio", "--camera"]);
    rec.wait_journal(Duration::from_secs(10), |ev| {
        ev.iter().any(|e| matches!(e, JournalEvent::Health { .. }))
    });
    std::thread::sleep(Duration::from_millis(800));
    let (dir, kill_rec) = rec.kill();
    let s = story(&dir);
    assert_eq!(
        s.last().map(String::as_str),
        Some("segment_open:s0"),
        "{s:?}"
    );
    assert!(!s.iter().any(|e| e == "stop"), "{s:?}");
    let p = recover_and_check(&dir, kill_rec, 1);
    let kinds: Vec<TrackKind> = p.recording.tracks.iter().map(|t| t.kind).collect();
    for k in [TrackKind::Camera, TrackKind::Mic, TrackKind::SystemAudio] {
        assert!(kinds.contains(&k), "{k:?} missing from {kinds:?}");
    }
}

#[test]
fn kill_mid_second_segment() {
    let rec = start("mid-s1", &["--switch-at-ms", "500", "--audio"]);
    rec.wait_journal(Duration::from_secs(10), switched);
    std::thread::sleep(Duration::from_millis(700));
    let (dir, kill_rec) = rec.kill();
    let s = story(&dir);
    let kept: Vec<&str> = s
        .iter()
        .map(String::as_str)
        .filter(|e| *e != "track_finished")
        .collect();
    let tail = &kept[kept.len().saturating_sub(4)..];
    assert_eq!(
        tail,
        [
            "switch_begin",
            "segment_prepare:s1",
            "segment_open:s1",
            "segment_close:s0"
        ],
        "{s:?}"
    );
    let p = recover_and_check(&dir, kill_rec, 2);
    assert_eq!(
        p.recording.screen.segments[0].end_reason,
        Some(EndReason::UserSwitch)
    );
}

#[test]
fn kill_after_switch_begin() {
    if !hooks() {
        return;
    }
    let mut rec = start(
        "switch-begin",
        &["--switch-at-ms", "700", "--hold-at", "after_switch_begin"],
    );
    rec.wait_hold("after_switch_begin");
    let (dir, kill_rec) = rec.kill();
    let s = story(&dir);
    assert_eq!(
        s[s.len() - 2..],
        ["segment_open:s0", "switch_begin"],
        "{s:?}"
    );
    assert!(!s.iter().any(|e| e.contains(":s1")), "{s:?}");
    recover_and_check(&dir, kill_rec, 1);
}

#[test]
fn kill_after_segment_prepare() {
    if !hooks() {
        return;
    }
    let mut rec = start(
        "prepare",
        &[
            "--switch-at-ms",
            "700",
            "--hold-at",
            "after_segment_prepare",
            "--audio",
        ],
    );
    rec.wait_hold("after_segment_prepare");
    let (dir, kill_rec) = rec.kill();
    let s = story(&dir);
    assert_eq!(
        s[s.len() - 3..],
        ["segment_open:s0", "switch_begin", "segment_prepare:s1"],
        "{s:?}"
    );
    let r = recover_and_check(&dir, kill_rec, 1);
    let notes = serde_json::to_string(&r.recording.extra).expect("json");
    assert!(notes.contains("prepared but never opened"), "{notes}");
}

/// Kills at `after_segment_open` and returns the folder, the kill time and s1's file.
fn killed_after_segment_open(name: &str) -> Option<(PathBuf, i64, PathBuf)> {
    if !hooks() {
        return None;
    }
    let mut rec = start(
        name,
        &[
            "--switch-at-ms",
            "700",
            "--hold-at",
            "after_segment_open",
            "--audio",
        ],
    );
    rec.wait_hold("after_segment_open");
    let (dir, kill_rec) = rec.kill();
    let s = story(&dir);
    assert_eq!(
        s[s.len() - 3..],
        ["switch_begin", "segment_prepare:s1", "segment_open:s1"],
        "{s:?}"
    );
    assert!(!s.iter().any(|e| e.starts_with("segment_close")), "{s:?}");
    let file = events(&dir)
        .iter()
        .find_map(|e| match e {
            JournalEvent::SegmentOpen { seg, file, .. } if seg == "s1" => Some(dir.join(file)),
            _ => None,
        })
        .expect("segment_open:s1 names its file");
    Some((dir, kill_rec, file))
}

#[test]
fn kill_after_segment_open() {
    let Some((dir, kill_rec, _)) = killed_after_segment_open("open") else {
        return;
    };
    // The router holds before s1's first append, so recovery drops the empty s1.
    let p = recover_and_check(&dir, kill_rec, 1);
    assert_eq!(p.recording.screen.segments[0].id, "s0");
}

/// A fragmented-MOV writer has written nothing yet at `after_segment_open`: the live kill test
/// finds s1 at 0 bytes. Recovery must drop it like an empty segment, not call the project damaged.
#[test]
fn kill_after_segment_open_with_zero_byte_segment_file() {
    let Some((dir, kill_rec, s1)) = killed_after_segment_open("open-empty") else {
        return;
    };
    std::fs::File::create(&s1).expect("truncate s1 to 0 bytes");
    let p = recover_and_check(&dir, kill_rec, 1);
    assert_eq!(p.recording.screen.segments[0].id, "s0");
    assert!(s1.exists(), "recovery keeps the original file");
}
