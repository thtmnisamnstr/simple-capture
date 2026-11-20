use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sc_core::{Error, Result};

pub const DEFAULT_QUEUE_FRAMES: usize = 3;
pub const CANCEL_GRACE: Duration = Duration::from_millis(1500);
const STDERR_TAIL_LINES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub frame_bytes: usize,
    pub queue_frames: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub frame: u64,
    pub out_time_ns: i64,
    pub speed: Option<f64>,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitReport {
    pub code: Option<i32>,
    pub stderr_tail: Vec<String>,
    pub frames_written: u64,
}

impl ExitReport {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelReport {
    pub escalated_to_kill: bool,
    pub waited_ms: u64,
}

struct Item {
    buf: Vec<u8>,
    counts: bool,
}

#[derive(Default)]
struct Queue {
    items: VecDeque<Item>,
    reserved: usize,
    pool: Vec<Vec<u8>>,
    closed: bool,
    stop: bool,
    interrupted: bool,
    writer_done: bool,
    write_error: Option<String>,
    frames_written: u64,
}

struct Shared {
    q: Mutex<Queue>,
    not_full: Condvar,
    not_empty: Condvar,
    depth: usize,
    frame_bytes: usize,
    progress: Mutex<Option<Progress>>,
    tail: Mutex<VecDeque<String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn tail(&self) -> Vec<String> {
        lock(&self.tail).iter().cloned().collect()
    }
}

/// Makes a blocked or later `write_frame` return an error, from any thread. Use it to unstick
/// a producer before calling [`HelperProcess::cancel`].
#[derive(Clone)]
pub struct Interrupter(Arc<Shared>);

impl Interrupter {
    pub fn interrupt(&self) {
        lock(&self.0.q).interrupted = true;
        self.0.not_full.notify_all();
    }
}

impl std::fmt::Debug for Interrupter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Interrupter")
    }
}

/// A helper fed raw frames on stdin through a bounded queue.
pub struct HelperProcess {
    child: Child,
    pid: u32,
    program: PathBuf,
    shared: Arc<Shared>,
    writer: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<()>>,
    reaped: bool,
}

impl std::fmt::Debug for HelperProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HelperProcess")
            .field("pid", &self.pid)
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

fn spawn_error(program: &Path, e: &std::io::Error) -> Error {
    let msg = match e.kind() {
        std::io::ErrorKind::NotFound => format!("helper not found: {}", program.display()),
        std::io::ErrorKind::PermissionDenied => {
            format!("helper not executable: {}", program.display())
        }
        _ => format!("could not start helper {}: {e}", program.display()),
    };
    Error::Io(std::io::Error::new(e.kind(), msg))
}

impl HelperProcess {
    /// Starts `program` in its own process group (unix) with stdin piped, stdout discarded and
    /// stderr parsed for `-progress pipe:2` blocks. `queue_frames == 0` means
    /// [`DEFAULT_QUEUE_FRAMES`].
    pub fn spawn(spec: SpawnSpec) -> Result<HelperProcess> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| spawn_error(&spec.program, &e))?;
        let pid = child.id();
        tracing::info!(pid, program = %spec.program.display(), args = ?spec.args, "helper spawned");

        let depth = if spec.queue_frames == 0 {
            DEFAULT_QUEUE_FRAMES
        } else {
            spec.queue_frames
        };
        let shared = Arc::new(Shared {
            q: Mutex::new(Queue::default()),
            not_full: Condvar::new(),
            not_empty: Condvar::new(),
            depth,
            frame_bytes: spec.frame_bytes,
            progress: Mutex::new(None),
            tail: Mutex::new(VecDeque::new()),
        });

        let (Some(stdin), Some(stderr)) = (child.stdin.take(), child.stderr.take()) else {
            kill_now(&mut child);
            return Err(Error::Other(format!(
                "helper {}: stdin/stderr pipes missing after spawn",
                spec.program.display()
            )));
        };
        let threads = (|| -> std::io::Result<(JoinHandle<()>, JoinHandle<()>)> {
            let s = shared.clone();
            let writer = std::thread::Builder::new()
                .name(format!("helper-stdin-{pid}"))
                .spawn(move || writer_loop(&s, stdin))?;
            let s = shared.clone();
            let reader = std::thread::Builder::new()
                .name(format!("helper-stderr-{pid}"))
                .spawn(move || stderr_loop(&s, stderr))?;
            Ok((writer, reader))
        })();
        let (writer, reader) = match threads {
            Ok(t) => t,
            Err(e) => {
                lock(&shared.q).stop = true;
                shared.not_empty.notify_all();
                kill_now(&mut child);
                return Err(Error::Other(format!("helper {pid}: start threads: {e}")));
            }
        };
        Ok(HelperProcess {
            child,
            pid,
            program: spec.program,
            shared,
            writer: Some(writer),
            stderr: Some(reader),
            reaped: false,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn interrupter(&self) -> Interrupter {
        Interrupter(self.shared.clone())
    }

    /// Blocks when the queue is full.
    pub fn write_frame(&self, bytes: &[u8]) -> Result<()> {
        self.push(&[bytes], true)
    }

    /// One frame from several slices (e.g. [`crate::y4m::FRAME`] then the planes), copied once.
    pub fn write_frame_parts(&self, parts: &[&[u8]]) -> Result<()> {
        self.push(parts, true)
    }

    /// Bytes that are not a frame (a stream header); not counted in `frames_written`.
    pub fn write_bytes(&self, bytes: &[u8]) -> Result<()> {
        self.push(&[bytes], false)
    }

    fn push(&self, parts: &[&[u8]], counts: bool) -> Result<()> {
        let sh = &self.shared;
        let mut buf = {
            let mut q = lock(&sh.q);
            loop {
                if q.interrupted {
                    return Err(Error::Other(format!(
                        "helper {} ({}): write interrupted",
                        self.pid,
                        self.program.display()
                    )));
                }
                if q.writer_done || q.stop || q.closed {
                    return Err(self.dead_error(&q));
                }
                if q.items.len() + q.reserved < sh.depth {
                    break;
                }
                q = sh.not_full.wait(q).unwrap_or_else(PoisonError::into_inner);
            }
            q.reserved += 1;
            q.pool.pop().unwrap_or_default()
        };
        let len: usize = parts.iter().map(|p| p.len()).sum();
        buf.clear();
        buf.reserve(len.max(sh.frame_bytes));
        for p in parts {
            buf.extend_from_slice(p);
        }
        let mut q = lock(&sh.q);
        q.reserved -= 1;
        if q.writer_done || q.stop {
            let err = self.dead_error(&q);
            sh.not_full.notify_all();
            return Err(err);
        }
        q.items.push_back(Item { buf, counts });
        sh.not_empty.notify_one();
        Ok(())
    }

    fn dead_error(&self, q: &Queue) -> Error {
        let why = q
            .write_error
            .as_deref()
            .unwrap_or("the helper stopped reading");
        Error::Other(format!(
            "helper {} ({}) is not accepting frames: {why}; stderr: {}",
            self.pid,
            self.program.display(),
            self.shared.tail().join(" | ")
        ))
    }

    pub fn progress(&self) -> Option<Progress> {
        lock(&self.shared.progress).clone()
    }

    /// Closes stdin after the queue drains (EOF), waits for the helper to exit and reports.
    /// A non-zero exit is reported in `code`, not as an error.
    pub fn finish(mut self) -> Result<ExitReport> {
        {
            lock(&self.shared.q).closed = true;
            self.shared.not_empty.notify_all();
            self.shared.not_full.notify_all();
        }
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
        let status = self.child.wait().map_err(|e| {
            Error::Other(format!(
                "wait for helper {} ({}): {e}",
                self.pid,
                self.program.display()
            ))
        })?;
        self.reaped = true;
        if let Some(r) = self.stderr.take() {
            let _ = r.join();
        }
        let report = ExitReport {
            code: status.code(),
            stderr_tail: self.shared.tail(),
            frames_written: lock(&self.shared.q).frames_written,
        };
        tracing::info!(pid = self.pid, code = ?report.code, frames = report.frames_written, "helper finished");
        Ok(report)
    }

    /// Stops writing, then SIGTERM to the process group, [`CANCEL_GRACE`], SIGKILL.
    /// `waited_ms` runs from the first signal to the exit. Windows: immediate `kill`.
    pub fn cancel(mut self) -> CancelReport {
        {
            let mut q = lock(&self.shared.q);
            q.stop = true;
            q.interrupted = true;
            q.items.clear();
            self.shared.not_empty.notify_all();
            self.shared.not_full.notify_all();
        }
        let t0 = Instant::now();
        let escalated = self.terminate();
        self.reaped = true;
        let waited_ms = t0.elapsed().as_millis() as u64;
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
        if let Some(r) = self.stderr.take() {
            let _ = r.join();
        }
        tracing::info!(pid = self.pid, escalated, waited_ms, "helper cancelled");
        CancelReport {
            escalated_to_kill: escalated,
            waited_ms,
        }
    }
}

impl HelperProcess {
    #[cfg(unix)]
    fn terminate(&mut self) -> bool {
        let t0 = Instant::now();
        signal_group(self.pid, libc::SIGTERM);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return false,
                Ok(None) if t0.elapsed() < CANCEL_GRACE => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                _ => {
                    signal_group(self.pid, libc::SIGKILL);
                    let _ = self.child.wait();
                    return true;
                }
            }
        }
    }

    #[cfg(not(unix))]
    fn terminate(&mut self) -> bool {
        let _ = self.child.kill();
        let _ = self.child.wait();
        false
    }
}

impl Drop for HelperProcess {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        {
            let mut q = lock(&self.shared.q);
            q.stop = true;
            q.items.clear();
        }
        self.shared.not_empty.notify_all();
        self.shared.not_full.notify_all();
        tracing::warn!(
            pid = self.pid,
            "helper dropped without finish/cancel; killing it"
        );
        kill_now(&mut self.child);
    }
}

fn kill_now(child: &mut Child) {
    #[cfg(unix)]
    signal_group(child.id(), libc::SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn signal_group(pgid: u32, sig: libc::c_int) {
    let Ok(pgid) = libc::pid_t::try_from(pgid) else {
        return;
    };
    // SAFETY: kill(2) with a negative pid signals the process group the helper leads; the
    // leader is not reaped yet, so the id cannot have been reused.
    unsafe {
        libc::kill(-pgid, sig);
    }
}

fn writer_loop(sh: &Shared, mut stdin: ChildStdin) {
    loop {
        let item = {
            let mut q = lock(&sh.q);
            loop {
                if q.stop {
                    break None;
                }
                if let Some(item) = q.items.pop_front() {
                    sh.not_full.notify_all();
                    break Some(item);
                }
                if q.closed {
                    break None;
                }
                q = sh.not_empty.wait(q).unwrap_or_else(PoisonError::into_inner);
            }
        };
        let Some(Item { buf, counts }) = item else {
            break;
        };
        let r = stdin.write_all(&buf);
        let mut q = lock(&sh.q);
        match r {
            Ok(()) => {
                if counts {
                    q.frames_written += 1;
                }
                if q.pool.len() <= sh.depth {
                    q.pool.push(buf);
                }
            }
            Err(e) => {
                q.write_error = Some(e.to_string());
                break;
            }
        }
    }
    drop(stdin);
    let mut q = lock(&sh.q);
    q.writer_done = true;
    q.items.clear();
    sh.not_full.notify_all();
}

fn stderr_loop(sh: &Shared, stderr: impl Read) {
    let mut reader = BufReader::new(stderr);
    let mut parser = ProgressParser::default();
    let mut raw = Vec::new();
    loop {
        raw.clear();
        match reader.read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&raw);
        for seg in text.split(['\r', '\n']) {
            let line = seg.trim();
            if line.is_empty() {
                continue;
            }
            match parser.feed(line) {
                Feed::Progress(Some(p)) => *lock(&sh.progress) = Some(p),
                Feed::Progress(None) => {}
                Feed::Other => {
                    tracing::debug!(target: "sc_helpers::stderr", "{line}");
                    let mut t = lock(&sh.tail);
                    if t.len() == STDERR_TAIL_LINES {
                        t.pop_front();
                    }
                    t.push_back(line.to_owned());
                }
            }
        }
    }
}

const PROGRESS_KEYS: &[&str] = &[
    "frame",
    "fps",
    "bitrate",
    "total_size",
    "out_time_us",
    "out_time_ms",
    "out_time",
    "dup_frames",
    "drop_frames",
    "speed",
    "progress",
];

#[derive(Debug, PartialEq)]
enum Feed {
    Progress(Option<Progress>),
    Other,
}

#[derive(Debug, Default)]
struct ProgressParser {
    frame: u64,
    out_time_us: Option<i64>,
    speed: Option<f64>,
}

impl ProgressParser {
    fn feed(&mut self, line: &str) -> Feed {
        let Some((k, v)) = line.split_once('=') else {
            return Feed::Other;
        };
        let (k, v) = (k.trim(), v.trim());
        let known = PROGRESS_KEYS.contains(&k) || k.starts_with("stream_");
        if !known || v.contains(char::is_whitespace) {
            return Feed::Other;
        }
        match k {
            "frame" => {
                if let Ok(n) = v.parse() {
                    self.frame = n;
                }
            }
            "out_time_us" => {
                if let Ok(n) = v.parse() {
                    self.out_time_us = Some(n);
                }
            }
            "out_time_ms" => {
                if let (None, Ok(n)) = (self.out_time_us, v.parse()) {
                    self.out_time_us = Some(n);
                }
            }
            "speed" => self.speed = v.strip_suffix('x').and_then(|s| s.trim().parse().ok()),
            "progress" => {
                let p = Progress {
                    frame: self.frame,
                    out_time_ns: self.out_time_us.unwrap_or(0).saturating_mul(1000),
                    speed: self.speed,
                    done: v == "end",
                };
                self.out_time_us = None;
                return Feed::Progress(Some(p));
            }
            _ => {}
        }
        Feed::Progress(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{Feed, Progress, ProgressParser};

    fn run(lines: &[&str]) -> (Vec<Progress>, Vec<String>) {
        let mut p = ProgressParser::default();
        let (mut got, mut other) = (Vec::new(), Vec::new());
        for l in lines {
            match p.feed(l) {
                Feed::Progress(Some(x)) => got.push(x),
                Feed::Progress(None) => {}
                Feed::Other => other.push((*l).to_owned()),
            }
        }
        (got, other)
    }

    #[test]
    fn parses_progress_blocks_and_keeps_other_lines() {
        let (got, other) = run(&[
            "frame=12",
            "fps=0.00",
            "stream_0_0_q=28.0",
            "bitrate=N/A",
            "total_size=48",
            "out_time_us=400000",
            "out_time_ms=400000",
            "out_time=00:00:00.400000",
            "dup_frames=0",
            "drop_frames=0",
            "speed=0.79x",
            "progress=continue",
            "[libx264 @ 0x1] frame=I:1 Avg QP:20.00",
            "Error opening output file x.mp4.",
            "frame=60",
            "out_time_us=N/A",
            "out_time_ms=2000000",
            "speed=N/A",
            "progress=end",
        ]);
        assert_eq!(
            got,
            vec![
                Progress {
                    frame: 12,
                    out_time_ns: 400_000_000,
                    speed: Some(0.79),
                    done: false
                },
                Progress {
                    frame: 60,
                    out_time_ns: 2_000_000_000,
                    speed: None,
                    done: true
                },
            ]
        );
        assert_eq!(
            other,
            vec![
                "[libx264 @ 0x1] frame=I:1 Avg QP:20.00".to_owned(),
                "Error opening output file x.mp4.".to_owned()
            ]
        );
    }
}
