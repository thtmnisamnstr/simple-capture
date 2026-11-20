use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sc_core::journal::{JournalEvent, read_journal};
use sc_core::project::{ProjectV1, SourceDescriptor, TrackKind};

use super::procstat::{self, host_now_ns};
use super::stats::{
    ProcRow, Report, Sample, TrackRow, cpu_pct, flash_schedule, rss_trend, screen_delivery,
    summary_markdown,
};

pub struct RunArgs {
    pub minutes: f64,
    pub switch_every_s: f64,
    pub cli: PathBuf,
    pub runsh: PathBuf,
    pub work: PathBuf,
    pub keep: bool,
    pub camera: String,
    pub mic: String,
    pub system_audio: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RunLog {
    pub minutes: f64,
    pub switch_every_s: f64,
    pub project_dir: Option<PathBuf>,
    pub started_host_ns: Option<i64>,
    pub exit_code: Option<i32>,
    pub flash_ready: Value,
    pub flashes_host_ns: Vec<i64>,
    pub samples: Vec<Sample>,
    /// (rec_ns, cumulative screen frames across segments)
    pub screen_ticks: Vec<(i64, u64)>,
    pub last_health: Option<Value>,
    pub notices: Vec<Value>,
    pub switch_errors: Vec<Value>,
    pub failed: Option<String>,
}

static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_sig: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn lines_of<R: std::io::Read + Send + 'static>(r: R) -> mpsc::Receiver<Value> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for l in BufReader::new(r).lines() {
            let Ok(l) = l else { break };
            if let Ok(v) = serde_json::from_str::<Value>(l.trim()) {
                if tx.send(v).is_err() {
                    return;
                }
            } else if !l.trim().is_empty() {
                eprintln!("soak: {l}");
            }
        }
    });
    rx
}

struct Flash {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: mpsc::Receiver<Value>,
}

impl Flash {
    fn start() -> Result<(Flash, Value)> {
        let exe = std::env::current_exe().context("current_exe")?;
        let mut child = Command::new(exe)
            .args(["soak", "flash"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("spawn soak flash")?;
        let stdin = child.stdin.take();
        let out = child.stdout.take().ok_or_else(|| anyhow!("flash stdout"))?;
        let rx = lines_of(out);
        let ready = rx
            .recv_timeout(Duration::from_secs(300))
            .context("soak flash did not report ready")?;
        if ready["ev"] != "ready" || ready["window_id"].as_i64().unwrap_or(0) <= 0 {
            bail!("soak flash: unexpected first line {ready}");
        }
        Ok((Flash { child, stdin, rx }, ready))
    }

    fn flash(&mut self) {
        if let Some(s) = &mut self.stdin {
            let _ = s.write_all(b"flash\n");
            let _ = s.flush();
        }
    }

    fn stop(mut self) -> Vec<Value> {
        self.stdin = None;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.rx.try_iter().collect()
    }
}

#[derive(Default)]
struct CpuTrack {
    prev: HashMap<i32, f64>,
    primed: bool,
}

impl CpuTrack {
    /// New pids count from zero once the first sample is taken (a short-lived encoder service
    /// that starts mid-run did all its work inside the run).
    fn delta(&mut self, pids: &[i32]) -> (f64, u64) {
        let mut d = 0.0;
        let mut rss = 0u64;
        let mut next = HashMap::new();
        for &p in pids {
            if let Some(i) = procstat::info(p) {
                let before =
                    self.prev
                        .get(&p)
                        .copied()
                        .unwrap_or(if self.primed { 0.0 } else { i.cpu_s });
                d += (i.cpu_s - before).max(0.0);
                rss += i.rss_bytes;
                next.insert(p, i.cpu_s);
            }
        }
        self.prev = next;
        self.primed = true;
        (d, rss)
    }
}

fn read_pid(path: &Path) -> Option<i32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(unix)]
const SIGKILL: i32 = libc::SIGKILL;
#[cfg(not(unix))]
const SIGKILL: i32 = 9;

#[cfg(unix)]
fn send_signal(pid: i32, sig: i32) {
    if pid > 0 {
        // SAFETY: plain kill(2).
        unsafe { libc::kill(pid, sig) };
    }
}

#[cfg(not(unix))]
fn send_signal(_pid: i32, _sig: i32) {}

pub fn run(a: &RunArgs) -> Result<bool> {
    #[cfg(unix)]
    // SAFETY: the handler only stores an atomic.
    unsafe {
        let h = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGTERM, h);
        libc::signal(libc::SIGINT, h);
        libc::signal(libc::SIGHUP, h);
    }
    std::fs::create_dir_all(&a.work).with_context(|| a.work.display().to_string())?;
    let projects = a.work.join("projects");
    let pid_file = a.work.join("child.pid");
    let _ = std::fs::remove_file(&pid_file);
    let dur_s = (a.minutes * 60.0).round();

    let (mut flash, ready) = Flash::start()?;
    let win = ready["window_id"].as_i64().unwrap_or(0);
    let mut cmd = Command::new(&a.runsh);
    cmd.arg(&a.cli).args([
        "record",
        "--source",
        "display:main",
        "--fps",
        "30",
        "--resolution",
        "native",
        "--camera",
        &a.camera,
        "--mic",
        &a.mic,
        "--system-audio",
        &a.system_audio,
        "--title",
        "soak",
        "--events",
        "--duration",
        &format!("{dur_s}"),
    ]);
    cmd.arg("--projects-dir").arg(&projects);
    let mut t = a.switch_every_s;
    let mut to_window = true;
    while a.switch_every_s > 0.0 && t < dur_s - 1.0 {
        let spec = if to_window {
            format!("window:{win}")
        } else {
            "display:main".to_string()
        };
        cmd.args(["--switch", &format!("{t}={spec}")]);
        to_window = !to_window;
        t += a.switch_every_s;
    }
    let stderr = std::fs::File::create(a.work.join("record.stderr"))?;
    let mut rec = cmd
        .env("SC_DEV_PID_FILE", &pid_file)
        .current_dir(&a.work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .context("spawn run.sh record")?;
    let rec_rx = lines_of(rec.stdout.take().ok_or_else(|| anyhow!("record stdout"))?);
    eprintln!(
        "soak: recording {:.0} min (display ↔ window {win} every {} s); flash window {}",
        a.minutes, a.switch_every_s, ready["rect_pt"]
    );

    let mut log = RunLog {
        minutes: a.minutes,
        switch_every_s: a.switch_every_s,
        flash_ready: ready,
        ..RunLog::default()
    };
    let flash_at = flash_schedule(dur_s);
    let mut flash_i = 0;
    let mut per_track: BTreeMap<String, u64> = BTreeMap::new();
    let mut cum = 0u64;
    let (mut ours, mut replayd, mut vtenc) = (
        CpuTrack::default(),
        CpuTrack::default(),
        CpuTrack::default(),
    );
    let mut child_pid: Option<i32> = None;
    let mut next_sample = 0.0f64;
    let mut last_sample: Option<i64> = None;
    let t_spawn = Instant::now();
    let mut term_sent: Option<Instant> = None;
    let exit = loop {
        for ev in rec_rx.try_iter() {
            match ev["ev"].as_str().unwrap_or("") {
                "started" => {
                    log.started_host_ns = ev["host_ns"].as_i64();
                    log.project_dir = ev["project_dir"].as_str().map(PathBuf::from);
                    eprintln!("soak: started {}", ev["project_dir"]);
                }
                "health" => {
                    for tr in ev["tracks"].as_array().into_iter().flatten() {
                        let id = tr["track_id"].as_str().unwrap_or("");
                        if !id.starts_with("screen") {
                            continue;
                        }
                        let f = tr["frames"].as_u64().unwrap_or(0);
                        let prev = per_track.insert(id.to_string(), f).unwrap_or(0);
                        cum += f.saturating_sub(prev);
                    }
                    log.screen_ticks
                        .push((ev["rec_ns"].as_i64().unwrap_or(0), cum));
                    log.last_health = Some(ev);
                }
                "notice" => {
                    eprintln!("soak: notice {ev}");
                    log.notices.push(ev);
                }
                "switch_error" => {
                    eprintln!("soak: {ev}");
                    log.switch_errors.push(ev);
                }
                "state" if ev["state"] == "failed" => {
                    log.failed = ev["error"].as_str().map(str::to_string);
                }
                _ => {}
            }
        }
        for ev in flash.rx.try_iter() {
            if let Some(h) = ev["host_ns"].as_i64() {
                log.flashes_host_ns.push(h);
            }
        }
        if let Some(st) = log.started_host_ns {
            let now = host_now_ns();
            let el = (now - st) as f64 / 1e9;
            if flash_i < flash_at.len() && el >= flash_at[flash_i] {
                flash.flash();
                flash_i += 1;
            }
            if child_pid.is_none() {
                child_pid = read_pid(&pid_file);
            }
            if el >= next_sample {
                let wall = last_sample.map(|l| (now - l) as f64 / 1e9);
                let (oc, orss) = ours.delta(&child_pid.into_iter().collect::<Vec<_>>());
                let (rc, rrss) = replayd.delta(&procstat::pids_named("replayd"));
                let (vc, _) = vtenc.delta(&procstat::pids_named("VTEncoderXPCService"));
                let pct = |d: f64| wall.map(|w| cpu_pct(0.0, d, w));
                log.samples.push(Sample {
                    t_s: el,
                    our_rss_mb: (orss > 0).then(|| orss as f64 / 1_048_576.0),
                    our_cpu_pct: pct(oc),
                    replayd_cpu_pct: pct(rc),
                    vtenc_cpu_pct: pct(vc),
                    replayd_rss_mb: (rrss > 0).then(|| rrss as f64 / 1_048_576.0),
                });
                last_sample = Some(now);
                next_sample += 5.0;
                if log.samples.len().is_multiple_of(12) {
                    eprintln!(
                        "soak: {:.0} s, rss {:.0} MB, cpu {:.0} %",
                        el,
                        log.samples.last().and_then(|s| s.our_rss_mb).unwrap_or(0.0),
                        log.samples
                            .last()
                            .and_then(|s| s.our_cpu_pct)
                            .unwrap_or(0.0)
                    );
                }
            }
        }
        if let Some(st) = rec.try_wait().context("wait run.sh")? {
            break st.code();
        }
        let overdue = match log.started_host_ns {
            Some(st) => (host_now_ns() - st) as f64 / 1e9 > dur_s + 120.0,
            None => t_spawn.elapsed() > Duration::from_secs(600),
        };
        if (STOP.load(Ordering::SeqCst) || overdue) && term_sent.is_none() {
            eprintln!("soak: stopping the recording (signal or timeout)");
            send_signal(rec.id() as i32, libc::SIGTERM);
            term_sent = Some(Instant::now());
        }
        if term_sent.is_some_and(|t| t.elapsed() > Duration::from_secs(60)) {
            if let Some(p) = child_pid.or_else(|| read_pid(&pid_file)) {
                send_signal(p, SIGKILL);
            }
            let _ = rec.kill();
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    std::thread::sleep(Duration::from_millis(300));
    for ev in rec_rx.try_iter() {
        if ev["ev"] == "notice" {
            log.notices.push(ev);
        }
    }
    for ev in flash.stop() {
        if let Some(h) = ev["host_ns"].as_i64() {
            log.flashes_host_ns.push(h);
        }
    }
    log.exit_code = exit;
    let path = a.work.join("run.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&log)?)
        .with_context(|| path.display().to_string())?;
    let mut report = analyze(&log)?;
    if a.keep {
        report.kept = log.project_dir.as_ref().map(|p| p.display().to_string());
    }
    let text = finish(&a.work, &report)?;
    print!("{text}");
    if !a.keep
        && let Some(p) = &log.project_dir
    {
        std::fs::remove_dir_all(p).with_context(|| format!("delete {}", p.display()))?;
    }
    Ok(report.all_pass() && exit == Some(0))
}

fn finish(work: &Path, r: &Report) -> Result<String> {
    std::fs::write(work.join("report.json"), serde_json::to_vec_pretty(r)?)?;
    let text = summary_markdown(r);
    std::fs::write(work.join("summary.md"), &text)?;
    Ok(text)
}

pub fn report(work: &Path) -> Result<bool> {
    let raw = std::fs::read(work.join("run.json")).context("run.json")?;
    let log: RunLog = serde_json::from_slice(&raw)?;
    let mut r = analyze(&log)?;
    r.kept = log.project_dir.as_ref().map(|p| p.display().to_string());
    print!("{}", finish(work, &r)?);
    Ok(r.all_pass())
}

fn sysctl(name: &str) -> String {
    Command::new("/usr/sbin/sysctl")
        .args(["-n", name])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn avg_max(v: impl Iterator<Item = f64>) -> (Option<f64>, Option<f64>) {
    let v: Vec<f64> = v.collect();
    if v.is_empty() {
        return (None, None);
    }
    (
        Some(v.iter().sum::<f64>() / v.len() as f64),
        Some(v.iter().copied().fold(f64::MIN, f64::max)),
    )
}

fn kind_text(k: TrackKind) -> &'static str {
    match k {
        TrackKind::Screen => "screen",
        TrackKind::Camera => "camera",
        TrackKind::Mic => "mic",
        TrackKind::SystemAudio => "system_audio",
    }
}

fn source_text(s: &SourceDescriptor) -> String {
    match s {
        SourceDescriptor::Display { display_id, .. } => format!("display {display_id}"),
        SourceDescriptor::Window { window_id, .. } => format!("window {window_id}"),
        SourceDescriptor::Application { display_id, .. } => format!("app on {display_id}"),
        SourceDescriptor::Region { display_id, .. } => format!("region on {display_id}"),
    }
}

pub fn analyze(log: &RunLog) -> Result<Report> {
    let dir = log
        .project_dir
        .as_ref()
        .ok_or_else(|| anyhow!("the recording never started (no project dir)"))?;
    let p = ProjectV1::load(dir).map_err(|e| anyhow!("load {}: {e}", dir.display()))?;
    let mut r = Report {
        minutes_requested: log.minutes,
        duration_s: p.duration_ns().unwrap_or(0) as f64 / 1e9,
        machine: sysctl("hw.model"),
        os: format!("macOS {}", sysctl("kern.osproductversion")),
        fps_requested: p.recording.screen.fps_requested,
        exit_code: log.exit_code,
        ..Report::default()
    };
    let segs = &p.recording.screen.segments;
    if let Some(s) = segs.first() {
        let scale = match &s.source {
            SourceDescriptor::Display { scale, .. } => *scale,
            _ => 0.0,
        };
        r.display = format!("{}x{} px at {scale}x", s.frame_px.w, s.frame_px.h);
    }
    r.segments = segs
        .iter()
        .map(|s| {
            format!(
                "{} {} @ {:.1} s",
                s.id,
                source_text(&s.source),
                s.start_rec_ns as f64 / 1e9
            )
        })
        .collect();
    let health: HashMap<String, Value> = log
        .last_health
        .as_ref()
        .and_then(|h| h["tracks"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|t| Some((t["track_id"].as_str()?.to_string(), t.clone())))
        .collect();
    let (mut sf, mut sd) = (0u64, 0u64);
    for t in &p.recording.tracks {
        let h = health.get(&t.id);
        let kind = kind_text(t.kind);
        let row = TrackRow {
            id: t.id.clone(),
            kind: kind.into(),
            source: t
                .segment_id
                .as_ref()
                .and_then(|id| segs.iter().find(|s| &s.id == id))
                .map(|s| source_text(&s.source))
                .unwrap_or_default(),
            frames: t.health.frames,
            dropped: t.health.dropped,
            writer_not_ready: t.health.writer_not_ready,
            gaps: t.health.gaps.len() as u64,
            overlaps: h.and_then(|h| h["overlaps"].as_u64()),
            gap_total_ms: t
                .health
                .gaps
                .iter()
                .map(|g| g.dur_ns as f64 / 1e6)
                .sum::<f64>()
                + 0.0,
            duration_s: t.duration_ns.map(|d| d as f64 / 1e9),
            drift_ms: matches!(t.kind, TrackKind::Mic | TrackKind::SystemAudio)
                .then(|| h.and_then(|h| h["drift_ns"].as_f64()).map(|d| d / 1e6))
                .flatten(),
            device: t.device.as_ref().map(|d| d.name.clone()),
        };
        if t.kind == TrackKind::Screen {
            sf += row.frames;
            sd += row.dropped;
        }
        r.tracks.push(row);
    }
    r.screen_dropped_pct = if sf + sd == 0 {
        100.0
    } else {
        sd as f64 * 100.0 / (sf + sd) as f64
    };
    r.rss = rss_trend(&log.samples, 120.0);
    type Pick = fn(&Sample) -> Option<f64>;
    let procs: [(&str, Pick); 3] = [
        ("simple-capture", |s| s.our_cpu_pct),
        ("replayd", |s| s.replayd_cpu_pct),
        ("VTEncoderXPCService", |s| s.vtenc_cpu_pct),
    ];
    for (name, f) in procs {
        let (cpu_avg, cpu_max) = avg_max(log.samples.iter().filter_map(f));
        r.procs.push(ProcRow {
            name: name.into(),
            cpu_avg,
            cpu_max,
        });
    }
    match read_journal(dir) {
        Ok(j) => {
            for e in &j.events {
                match e {
                    JournalEvent::SourceLost { reason, .. } => {
                        r.stream_stopped += 1;
                        r.notices.push(format!("source_lost: {reason}"));
                    }
                    JournalEvent::SwitchFailed { error, .. } => {
                        r.switch_failed += 1;
                        r.notices.push(format!("switch_failed: {error}"));
                    }
                    JournalEvent::Stop { reason, .. } => {
                        r.stop_reason = Some(format!("{reason:?}").to_lowercase());
                    }
                    _ => {}
                }
            }
        }
        Err(e) => r.notices.push(format!("journal unreadable: {e}")),
    }
    r.switch_failed += log.switch_errors.len() as u64;
    r.source_lost = log
        .notices
        .iter()
        .filter(|n| n["notice"] == "source_lost")
        .count() as u64;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for n in &log.notices {
        *counts
            .entry(n["notice"].as_str().unwrap_or("?").to_string())
            .or_default() += 1;
    }
    r.notices
        .extend(counts.into_iter().map(|(k, c)| format!("{k} ×{c}")));
    if let Some(f) = &log.failed {
        r.notices.push(format!("session failed: {f}"));
    }
    let (stall, min_fps) = screen_delivery(&log.screen_ticks, 3.0);
    r.longest_stall_s = stall;
    r.min_screen_fps_10s = min_fps;
    r.sync = sync_rows(log, dir, p.recording.start_host_ns);
    Ok(r)
}

#[cfg(target_os = "macos")]
fn sync_rows(log: &RunLog, dir: &Path, start_host_ns: i64) -> Vec<super::stats::SyncRow> {
    use super::media::{FlashGeom, measure_sync};
    use super::stats::merge_sync;
    let f = &log.flash_ready;
    let num = |v: &Value, i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0);
    let g = FlashGeom {
        rect_pt: [0, 1, 2, 3].map(|i| num(&f["rect_pt"], i)),
        display_pt: [0, 1].map(|i| num(&f["display_pt"], i)),
    };
    let half_ns = (log.minutes * 30.0 * 1e9) as i64;
    let flashes: Vec<(String, i64)> = log
        .flashes_host_ns
        .iter()
        .map(|h| {
            let t = h - start_host_ns;
            let label = if t < half_ns { "start" } else { "end" };
            (label.to_string(), t)
        })
        .collect();
    match measure_sync(dir, sc_capture_macos::decoder(), &g, &flashes) {
        Ok(rows) => merge_sync(rows),
        Err(e) => vec![super::stats::SyncRow {
            label: "all".into(),
            note: Some(format!("sync not measured: {e:#}")),
            ..Default::default()
        }],
    }
}

#[cfg(not(target_os = "macos"))]
fn sync_rows(_log: &RunLog, _dir: &Path, _start: i64) -> Vec<super::stats::SyncRow> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_track_counts_new_pids_after_the_first_sample() {
        let me = std::process::id() as i32;
        let mut t = CpuTrack::default();
        let (d0, rss) = t.delta(&[me]);
        assert_eq!(d0, 0.0);
        if cfg!(target_os = "macos") {
            assert!(rss > 0);
        }
        let (d1, _) = t.delta(&[]);
        assert_eq!(d1, 0.0);
        let (d2, _) = t.delta(&[me]);
        if cfg!(target_os = "macos") {
            assert!(d2 > 0.0, "a pid first seen after priming counts from zero");
        }
    }
}
