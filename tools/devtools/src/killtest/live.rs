//! `sc-devtools killtest live`: the dev-host kill test (M1 plan §8.3 wp-kill-recovery-test),
//! started by `scripts/killtest/run.sh` inside `scripts/lib/live.sh`.
//!
//! ```text
//! sc-devtools killtest live --runsh PATH --hook-cli PATH --cli PATH --work DIR
//!                           [--runs N] [--case NAME]... [--report FILE]
//! ```
//!
//! Every recording is `run.sh <hook-cli> record …` (a `simple-capture` built with
//! `test-hooks`), SIGKILLed through `SC_DEV_PID_FILE`: 6 s into a display recording with
//! camera, mic and system audio, and at `after_switch_begin`, `after_segment_prepare` and
//! `after_segment_open` of a display → region switch. The router journals the first segment
//! with the same hold points, so for those the first `SC_HOLD` (which blocks for 30 s) is let
//! through and the switch comes at 34 s. Then: the journal must show the kill landed between
//! the intended events, `<hook-cli> recover` must report `recovered`, every track loses at
//! most 1 s, every file decodes (`$SC_FFPROBE`), and `<cli> export` gives a valid file.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use sc_core::journal::{JournalEvent, read_journal};
use sc_core::project::{ProjectState, ProjectV1, TrackKind};

const USAGE: &str = "usage: sc-devtools killtest live --runsh PATH --hook-cli PATH --cli PATH \
                     --work DIR [--runs N] [--case NAME]... [--report FILE]";
/// The [S3] target; a kill past it warns.
const TARGET_LOSS_NS: i64 = 1_000_000_000;
/// The most a kill may lose (owner Q-a, 2026-11-24): the measured 1.08 s with 1 s fragments is
/// accepted, anything longer fails.
const ACCEPTED_LOSS_NS: i64 = 1_100_000_000;
const REGION: &str = "region:main:100,100,640,360";

struct Case {
    name: &'static str,
    hold: Option<&'static str>,
    args: &'static [&'static str],
    kill_after_s: u64,
    tail: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "plain_6s",
        hold: None,
        args: &[
            "--camera",
            "default",
            "--mic",
            "default",
            "--system-audio",
            "all",
        ],
        kill_after_s: 6,
        tail: &["segment_open:s0"],
    },
    Case {
        name: "after_switch_begin",
        hold: Some("after_switch_begin"),
        args: &["--switch", "3=REGION"],
        kill_after_s: 0,
        tail: &["segment_open:s0", "switch_begin"],
    },
    Case {
        name: "after_segment_prepare",
        hold: Some("after_segment_prepare"),
        args: &["--switch", "34=REGION"],
        kill_after_s: 0,
        tail: &["segment_open:s0", "switch_begin", "segment_prepare:s1"],
    },
    Case {
        name: "after_segment_open",
        hold: Some("after_segment_open"),
        args: &["--switch", "34=REGION"],
        kill_after_s: 0,
        tail: &["switch_begin", "segment_prepare:s1", "segment_open:s1"],
    },
];

struct Opts {
    runsh: PathBuf,
    hook_cli: PathBuf,
    cli: PathBuf,
    work: PathBuf,
    runs: u32,
    cases: Vec<String>,
    report: Option<PathBuf>,
}

pub(super) fn run(args: &[String]) -> u8 {
    if args.first().map(String::as_str) != Some("live") {
        eprintln!("{USAGE}");
        return 2;
    }
    let opts = match parse(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("sc-devtools killtest: {e}\n{USAGE}");
            return 2;
        }
    };
    match live(&opts) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("sc-devtools killtest: {e:#}");
            1
        }
    }
}

fn parse(args: &[String]) -> Result<Opts> {
    let mut o = Opts {
        runsh: PathBuf::new(),
        hook_cli: PathBuf::new(),
        cli: PathBuf::new(),
        work: PathBuf::new(),
        runs: 3,
        cases: Vec::new(),
        report: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut v = || {
            it.next()
                .cloned()
                .ok_or_else(|| anyhow!("{a} needs a value"))
        };
        match a.as_str() {
            "--runsh" => o.runsh = v()?.into(),
            "--hook-cli" => o.hook_cli = v()?.into(),
            "--cli" => o.cli = v()?.into(),
            "--work" => o.work = v()?.into(),
            "--runs" => o.runs = v()?.parse().context("--runs")?,
            "--case" => {
                let c = v()?;
                if !CASES.iter().any(|k| k.name == c) {
                    bail!("unknown case {c:?}");
                }
                o.cases.push(c);
            }
            "--report" => o.report = Some(v()?.into()),
            other => bail!("unknown argument {other:?}"),
        }
    }
    for (p, what) in [
        (&o.runsh, "--runsh"),
        (&o.hook_cli, "--hook-cli"),
        (&o.cli, "--cli"),
        (&o.work, "--work"),
    ] {
        if p.as_os_str().is_empty() {
            bail!("{what} is required");
        }
    }
    Ok(o)
}

fn live(o: &Opts) -> Result<bool> {
    std::env::var_os("SC_FFPROBE").context("SC_FFPROBE is not set")?;
    let mut results = Vec::new();
    let mut failed = 0;
    for run in 1..=o.runs {
        for case in CASES {
            if !o.cases.is_empty() && !o.cases.iter().any(|c| c == case.name) {
                continue;
            }
            let t0 = Instant::now();
            println!("killtest run {run}: {}", case.name);
            let mut st = Step::default();
            if let Err(e) = one(o, run, case, &mut st) {
                st.fails.push(format!("{e:#}"));
            }
            st.num(
                "wall_s",
                json!((t0.elapsed().as_secs_f64() * 10.0).round() / 10.0),
            );
            let pass = st.fails.is_empty();
            if !pass {
                failed += 1;
            }
            println!(
                "{} run {run} {}: {}",
                if pass { "PASS" } else { "FAIL" },
                case.name,
                Value::Object(st.numbers.clone())
            );
            for w in &st.warns {
                println!("  WARN {w}");
            }
            for f in &st.fails {
                println!("  FAIL {f}");
            }
            results.push(json!({
                "run": run, "case": case.name, "pass": pass,
                "fails": st.fails, "warns": st.warns, "numbers": st.numbers,
            }));
        }
    }
    let total = results.len();
    println!(
        "killtest: {total} kills, {} passed, {failed} failed",
        total - failed
    );
    if let Some(path) = &o.report {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).with_context(|| format!("create {}", d.display()))?;
        }
        let text = serde_json::to_string_pretty(&json!({ "results": results }))?;
        std::fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
        println!("killtest: report {}", path.display());
    }
    Ok(failed == 0 && total > 0)
}

#[derive(Default)]
struct Step {
    fails: Vec<String>,
    warns: Vec<String>,
    numbers: serde_json::Map<String, Value>,
}

impl Step {
    fn check(&mut self, ok: bool, what: impl FnOnce() -> String) -> bool {
        if !ok {
            self.fails.push(what());
        }
        ok
    }
    fn num(&mut self, k: &str, v: Value) {
        self.numbers.insert(k.to_string(), v);
    }
}

fn host_now_ns() -> i64 {
    // SAFETY: plain clock read. CLOCK_UPTIME_RAW is mach_absolute_time in ns, the timeline of
    // the journal's host times (sc_capture_macos `host_now`).
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut ts) };
    ts.tv_sec * 1_000_000_000 + ts.tv_nsec
}

fn lines(r: impl std::io::Read + Send + 'static) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for l in BufReader::new(r).lines().map_while(Result::ok) {
            if tx.send(l).is_err() {
                break;
            }
        }
    });
    rx
}

struct Recording {
    host: Child,
    pid_file: PathBuf,
}

impl Recording {
    fn child_pid(&self) -> Option<i32> {
        std::fs::read_to_string(&self.pid_file)
            .ok()
            .and_then(|s| s.trim().parse().ok())
    }

    fn kill_child(&self) -> Result<i64> {
        let pid = self
            .child_pid()
            .context("no child pid in SC_DEV_PID_FILE")?;
        let at = host_now_ns();
        // SAFETY: kill(2) on the pid the dev host recorded for this recording.
        if unsafe { libc::kill(pid, libc::SIGKILL) } != 0 {
            bail!("kill -9 {pid}: {}", std::io::Error::last_os_error());
        }
        Ok(at)
    }

    fn reap(&mut self) -> Option<i32> {
        let end = Instant::now() + Duration::from_secs(30);
        while Instant::now() < end {
            if let Ok(Some(s)) = self.host.try_wait() {
                return s.code();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if let Ok(None) = self.host.try_wait() {
            if let Some(pid) = self.child_pid() {
                // SAFETY: as in `kill_child`.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
            let _ = self.host.kill();
            let _ = self.host.wait();
        }
    }
}

fn one(o: &Opts, run: u32, case: &Case, st: &mut Step) -> Result<()> {
    let out = o.work.join(format!("run{run}-{}", case.name));
    let projects = out.join("projects");
    std::fs::create_dir_all(&projects).with_context(|| format!("create {}", projects.display()))?;
    let pid_file = out.join("child.pid");
    let mut cmd = Command::new(&o.runsh);
    cmd.arg(&o.hook_cli)
        .args([
            "record",
            "--source",
            "display:main",
            "--duration",
            "60",
            "--events",
        ])
        .arg("--projects-dir")
        .arg(&projects)
        .args(case.args.iter().map(|a| a.replace("REGION", REGION)))
        .env("SC_DEV_PID_FILE", &pid_file)
        .env_remove("SC_TEST_HOLD_AT")
        .current_dir(&o.work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(p) = case.hold {
        cmd.env("SC_TEST_HOLD_AT", p);
    }
    let mut host = cmd.spawn().context("spawn run.sh")?;
    let stdout = lines(host.stdout.take().context("stdout")?);
    let stderr = lines(host.stderr.take().context("stderr")?);
    let mut rec = Recording { host, pid_file };

    let dir = wait_started(&stdout, Duration::from_secs(180))?;
    let t_started = Instant::now();
    let mut err_tail: Vec<String> = Vec::new();
    let mut holds = 0;
    let kill_host = match case.hold {
        None => {
            std::thread::sleep(Duration::from_secs(case.kill_after_s));
            rec.kill_child()?
        }
        Some(point) => {
            let want = format!("SC_HOLD {point}");
            loop {
                let line = stderr
                    .recv_timeout(Duration::from_secs(90).saturating_sub(t_started.elapsed()))
                    .map_err(|_| {
                        anyhow!("no switch-time {want:?} within 90 s; stderr {err_tail:?}")
                    })?;
                if line != want {
                    err_tail.push(line);
                    if err_tail.len() > 20 {
                        err_tail.remove(0);
                    }
                    continue;
                }
                holds += 1;
                let in_switch = journal(&dir)
                    .iter()
                    .any(|e| matches!(e, JournalEvent::SwitchBegin { .. }));
                if in_switch {
                    break rec.kill_child()?;
                }
            }
        }
    };
    st.num("holds_seen", json!(holds));
    let rc = rec.reap();
    st.num("runsh_exit", json!(rc));
    drop(rec);

    let events = journal(&dir);
    let start_host = events
        .iter()
        .find_map(|e| match e {
            JournalEvent::Start { start_host_ns, .. } => Some(*start_host_ns),
            _ => None,
        })
        .context("journal has no start line")?;
    let kill_rec = kill_host - start_host;
    st.num("kill_rec_s", json!(secs(kill_rec)));
    let s = story(&events);
    let kept: Vec<&str> = s
        .iter()
        .map(String::as_str)
        .filter(|e| !matches!(*e, "track_finished" | "device" | "audio_gap"))
        .collect();
    let tail = &kept[kept.len().saturating_sub(case.tail.len())..];
    st.check(tail == case.tail, || {
        format!("journal ends {tail:?}, want {:?} (all: {s:?})", case.tail)
    });
    st.check(!s.iter().any(|e| e == "stop"), || {
        format!("journal has a stop line: {s:?}")
    });

    // The hook CLI comes from a private target dir, so it always carries this tree's recovery
    // code; a shared Skia target can hand back a `simple-capture` built from another worktree.
    let r = cmd_json(
        &o.hook_cli,
        &["--json", "recover", "--projects-dir"],
        &projects,
    )?;
    let reports = r["reports"].as_array().cloned().unwrap_or_default();
    st.check(reports.len() == 1, || {
        format!("recover reports {reports:?}")
    });
    let outcome = reports
        .first()
        .and_then(|x| x["outcome"].as_str())
        .unwrap_or("none")
        .to_string();
    st.num("outcome", json!(outcome));
    if let Some(rep) = reports.first() {
        st.num("recover_notes", rep["notes"].clone());
    }
    st.check(outcome == "recovered", || {
        format!("recover outcome {outcome}")
    });

    let p = ProjectV1::load(&dir).context("load the recovered project")?;
    st.check(p.state == ProjectState::Recovered, || {
        format!("state {:?}", p.state)
    });
    let segs = &p.recording.screen.segments;
    st.num("segments", json!(segs.len()));
    st.check(!segs.is_empty() && segs[0].start_rec_ns == 0, || {
        format!("segments {segs:?}")
    });
    let tiles = segs
        .windows(2)
        .all(|w| w[0].end_rec_ns == Some(w[1].start_rec_ns));
    st.check(tiles, || "segments do not tile".to_string());
    let mut ends = Vec::new();
    if let Some(end) = segs.last().and_then(|s| s.end_rec_ns) {
        ends.push(("screen".to_string(), end));
    }
    for t in &p.recording.tracks {
        if t.kind != TrackKind::Screen {
            match t.duration_ns {
                Some(d) => ends.push((t.id.clone(), t.first_sample_offset_ns + d)),
                None => st.fails.push(format!("{}: no duration", t.id)),
            }
        }
    }
    let limit = ACCEPTED_LOSS_NS;
    for (id, end) in &ends {
        let loss = kill_rec - end;
        st.num(&format!("{id}_loss_ms"), json!(loss / 1_000_000));
        let what = format!(
            "{id}: ends at {:.3} s, killed at {:.3} s (loss {} ms)",
            secs(*end),
            secs(kill_rec),
            loss / 1_000_000
        );
        if loss > limit {
            st.fails
                .push(format!("{what}, over {} ms", limit / 1_000_000));
        } else if loss > TARGET_LOSS_NS {
            st.warns.push(format!("{what}, over the 1 s target"));
        }
    }
    for t in &p.recording.tracks {
        let path = dir.join(&t.file);
        match ffprobe(&path) {
            Ok(v) => {
                let s0 = &v["streams"][0];
                let frames: u64 = s0["nb_read_frames"]
                    .as_str()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0);
                let dur: f64 = v["format"]["duration"]
                    .as_str()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0.0);
                st.num(&format!("{}_ffprobe_s", t.id), json!(dur));
                if matches!(t.kind, TrackKind::Screen | TrackKind::Camera) {
                    st.check(frames > 0, || format!("{}: no frames", t.file));
                }
            }
            Err(e) => st.fails.push(format!("{}: {e:#}", t.file)),
        }
    }

    let mp4 = out.join("export.mp4");
    let ex = Command::new(&o.cli)
        .arg("export")
        .arg(&dir)
        .arg("--out")
        .arg(&mp4)
        .args(["--size", "480"])
        .output()
        .context("run export")?;
    if st.check(ex.status.success(), || {
        format!("export: {}", tail_text(&ex.stderr))
    }) {
        export_check(o, &p, &mp4, st)?;
    }
    Ok(())
}

fn export_check(o: &Opts, p: &ProjectV1, mp4: &Path, st: &mut Step) -> Result<()> {
    let v = ffprobe(mp4)?;
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let Some(vid) = streams.iter().find(|s| s["codec_type"] == "video") else {
        st.fails.push("export has no video stream".into());
        return Ok(());
    };
    let (w, h) = (
        vid["width"].as_u64().unwrap_or(0),
        vid["height"].as_u64().unwrap_or(0),
    );
    st.num("export_px", json!(format!("{w}x{h}")));
    let dur = p.duration_ns().unwrap_or(0);
    let mut args = vec![
        "--json".to_string(),
        "validate".to_string(),
        mp4.display().to_string(),
        "--size".to_string(),
        format!("{w}x{h}"),
        "--fps".to_string(),
        p.recording.screen.fps_requested.to_string(),
        "--duration".to_string(),
        format!("{:.3}", secs(dur)),
    ];
    if streams.iter().any(|s| s["codec_type"] == "audio") {
        args.push("--audio".into());
    }
    let r = Command::new(&o.cli)
        .args(&args)
        .output()
        .context("run validate")?;
    st.check(r.status.success(), || {
        format!("validate: {}", tail_text(&r.stderr))
    });
    Ok(())
}

fn wait_started(stdout: &Receiver<String>, timeout: Duration) -> Result<PathBuf> {
    let end = Instant::now() + timeout;
    loop {
        let line = stdout
            .recv_timeout(end.saturating_duration_since(Instant::now()))
            .map_err(|_| anyhow!("no \"started\" event within {timeout:?}"))?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if v["ev"] == "started" {
            let dir = v["project_dir"]
                .as_str()
                .context("started without project_dir")?;
            return Ok(PathBuf::from(dir));
        }
    }
}

fn journal(dir: &Path) -> Vec<JournalEvent> {
    read_journal(dir).map(|j| j.events).unwrap_or_default()
}

fn story(events: &[JournalEvent]) -> Vec<String> {
    events
        .iter()
        .filter(|e| !matches!(e, JournalEvent::Health { .. }))
        .map(|e| {
            let name = serde_json::to_value(e)
                .ok()
                .and_then(|v| v["ev"].as_str().map(str::to_string))
                .unwrap_or_default();
            match e {
                JournalEvent::SegmentPrepare { seg, .. }
                | JournalEvent::SegmentOpen { seg, .. }
                | JournalEvent::SegmentClose { seg, .. } => format!("{name}:{seg}"),
                _ => name,
            }
        })
        .collect()
}

fn cmd_json(bin: &Path, args: &[&str], last: &Path) -> Result<Value> {
    let r = Command::new(bin)
        .args(args)
        .arg(last)
        .output()
        .with_context(|| format!("run {}", bin.display()))?;
    if !r.status.success() {
        bail!("{args:?}: {}", tail_text(&r.stderr));
    }
    let text = String::from_utf8_lossy(&r.stdout);
    let line = text.lines().last().unwrap_or_default();
    serde_json::from_str(line).with_context(|| format!("{args:?}: not JSON: {line}"))
}

fn ffprobe(path: &Path) -> Result<Value> {
    let bin = std::env::var_os("SC_FFPROBE").context("SC_FFPROBE is not set")?;
    let r = Command::new(bin)
        .args(["-v", "error", "-count_frames", "-show_entries"])
        .arg("stream=codec_type,width,height,nb_read_frames,duration")
        .args(["-show_entries", "format=duration", "-of", "json"])
        .arg(path)
        .output()
        .context("run ffprobe")?;
    if !r.status.success() {
        bail!("ffprobe: {}", tail_text(&r.stderr));
    }
    let v: Value = serde_json::from_slice(&r.stdout).context("ffprobe json")?;
    if v["streams"].as_array().is_none_or(Vec::is_empty) {
        bail!("ffprobe: no streams");
    }
    Ok(v)
}

fn tail_text(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let s = s.trim();
    let start = s.char_indices().rev().nth(299).map_or(0, |(i, _)| i);
    s[start..].to_string()
}

fn secs(ns: i64) -> f64 {
    (ns as f64 / 1e6).round() / 1e3
}
