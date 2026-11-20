use crate::frames::{PixFmt, Synth};
use crate::sys::{self, Exit, RUsage};
use std::collections::{BTreeMap, VecDeque};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Stdin,
    Fifo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Y4m,
    Raw(PixFmt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelMethod {
    Quit,
    Term,
    Kill,
    Eof,
    StoppedThenTerm,
}

impl CancelMethod {
    pub fn name(&self) -> &'static str {
        match self {
            CancelMethod::Quit => "q-on-stdin",
            CancelMethod::Term => "SIGTERM",
            CancelMethod::Kill => "SIGKILL",
            CancelMethod::Eof => "close-input (EOF)",
            CancelMethod::StoppedThenTerm => "SIGTERM to hung helper -> SIGKILL",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CancelPlan {
    pub method: CancelMethod,
    pub after_frames: u64,
    pub grace: Duration,
}

pub struct RunSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub transport: Transport,
    pub fifo_path: Option<PathBuf>,
    pub container: Container,
    pub synth: Arc<Synth>,
    pub frames: u64,
    pub queue_depth: usize,
    pub progress: bool,
    pub stderr_log: Option<PathBuf>,
    pub cancel: Option<CancelPlan>,
}

#[derive(Debug, Default, Clone)]
pub struct ProgressState {
    pub updates: u64,
    pub last: BTreeMap<String, String>,
    pub ended: bool,
    pub first_at_s: Option<f64>,
}

#[derive(Debug)]
pub struct CancelOutcome {
    pub method: CancelMethod,
    pub exit_after_s: f64,
    pub escalated_to_kill: bool,
}

#[derive(Debug)]
pub struct RunResult {
    pub exit: Exit,
    pub pid: u32,
    pub wall_s: f64,
    pub frames_sent: u64,
    pub gen_wait_s: f64,
    pub write_wait_s: f64,
    pub helper: RUsage,
    pub progress: ProgressState,
    pub stderr_tail: Vec<String>,
    pub cancel: Option<CancelOutcome>,
    pub write_error: Option<String>,
}

pub fn spawn_error_message(program: &std::path::Path, e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => format!("helper not found: {} (is the app bundle complete?)", program.display()),
        std::io::ErrorKind::PermissionDenied => format!("helper not executable: {}", program.display()),
        _ => format!("could not start helper {}: {e}", program.display()),
    }
}

pub fn run(spec: RunSpec) -> Result<RunResult, String> {
    let start = Instant::now();
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(if spec.progress { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = cmd.spawn().map_err(|e| spawn_error_message(&spec.program, &e))?;
    let pid = child.id();

    let tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
    let stderr = child.stderr.take().unwrap();
    let tail2 = tail.clone();
    let log_path = spec.stderr_log.clone();
    let stderr_thread = thread::spawn(move || {
        let mut log = log_path.and_then(|p| std::fs::File::create(p).ok());
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(f) = log.as_mut() {
                let _ = writeln!(f, "{line}");
            }
            let mut t = tail2.lock().unwrap();
            t.push_back(line);
            if t.len() > 40 {
                t.pop_front();
            }
        }
    });

    let progress = Arc::new(Mutex::new(ProgressState::default()));
    let progress_thread = child.stdout.take().map(|out| {
        let p = progress.clone();
        thread::spawn(move || {
            let mut cur = BTreeMap::new();
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let Some((k, v)) = line.split_once('=') else { continue };
                let (k, v) = (k.trim().to_string(), v.trim().to_string());
                if k == "progress" {
                    let mut s = p.lock().unwrap();
                    s.updates += 1;
                    if s.first_at_s.is_none() {
                        s.first_at_s = Some(start.elapsed().as_secs_f64());
                    }
                    s.ended = v == "end";
                    s.last = std::mem::take(&mut cur);
                    s.last.insert(k, v);
                } else {
                    cur.insert(k, v);
                }
            }
        })
    });

    let mut stdin = child.stdin.take();
    let sink: Box<dyn Write + Send> = match spec.transport {
        Transport::Stdin => Box::new(stdin.take().unwrap()),
        Transport::Fifo => {
            let path = spec.fifo_path.clone().ok_or("fifo transport needs a path")?;
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match OpenOptions::new().write(true).custom_flags(0x4).open(&path) {
                    Ok(f) => {
                        sys::clear_nonblocking(f.as_raw_fd());
                        break Box::new(f);
                    }
                    Err(e) if e.raw_os_error() == Some(6) => {
                        if sys::try_reap(pid).is_some() || Instant::now() > deadline {
                            return Err(format!("helper never opened the fifo: {}", tail.lock().unwrap().iter().cloned().collect::<Vec<_>>().join(" | ")));
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => return Err(format!("open fifo: {e}")),
                }
            }
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    let sent = Arc::new(AtomicU64::new(0));
    let depth = spec.queue_depth.max(1);
    let (free_tx, free_rx) = sync_channel::<Vec<u8>>(depth);
    let (full_tx, full_rx) = sync_channel::<Vec<u8>>(depth);
    let fb = spec.synth.frame_bytes();
    for _ in 0..depth {
        free_tx.send(vec![0u8; fb]).unwrap();
    }

    let gen_thread = {
        let synth = spec.synth.clone();
        let stop = stop.clone();
        let fmt = match spec.container {
            Container::Y4m => PixFmt::I420,
            Container::Raw(f) => f,
        };
        let frames = spec.frames;
        thread::spawn(move || {
            let mut waited = Duration::ZERO;
            let mut scratch = Vec::new();
            for n in 0..frames {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let t = Instant::now();
                let Ok(mut buf) = free_rx.recv() else { break };
                waited += t.elapsed();
                synth.render(n, fmt, &mut buf, &mut scratch);
                if full_tx.send(buf).is_err() {
                    break;
                }
            }
            waited
        })
    };

    let writer_thread = {
        let header = match spec.container {
            Container::Y4m => Some(spec.synth.y4m_header()),
            Container::Raw(_) => None,
        };
        let sent = sent.clone();
        let stop = stop.clone();
        let mut sink = sink;
        thread::spawn(move || -> (Duration, Option<String>) {
            let mut waited = Duration::ZERO;
            let mut err = None;
            if let Some(h) = &header {
                if let Err(e) = sink.write_all(h.as_bytes()) {
                    return (waited, Some(e.to_string()));
                }
            }
            for buf in full_rx.iter() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let t = Instant::now();
                let r = if header.is_some() {
                    sink.write_all(b"FRAME\n").and_then(|_| sink.write_all(&buf))
                } else {
                    sink.write_all(&buf)
                };
                waited += t.elapsed();
                if let Err(e) = r {
                    err = Some(e.to_string());
                    break;
                }
                sent.fetch_add(1, Ordering::Relaxed);
                let _ = free_tx.send(buf);
            }
            drop(sink);
            (waited, err)
        })
    };

    let mut cancel_outcome = None;
    let mut cancel_req: Option<Instant> = None;
    let mut killed = false;
    let (exit, ru) = loop {
        if let Some(r) = sys::try_reap(pid) {
            break r;
        }
        if let (Some(plan), None) = (spec.cancel, cancel_req) {
            if sent.load(Ordering::Relaxed) >= plan.after_frames {
                cancel_req = Some(Instant::now());
                match plan.method {
                    CancelMethod::Quit => {
                        if let Some(s) = stdin.as_mut() {
                            let _ = s.write_all(b"q\n");
                            let _ = s.flush();
                        }
                    }
                    CancelMethod::Term => {
                        sys::send_signal(pid, sys::SIGTERM);
                    }
                    CancelMethod::Kill => {
                        sys::send_signal(pid, sys::SIGKILL);
                        killed = true;
                    }
                    CancelMethod::Eof => stop.store(true, Ordering::Relaxed),
                    CancelMethod::StoppedThenTerm => {
                        sys::send_signal(pid, sys::SIGSTOP);
                        sys::send_signal(pid, sys::SIGTERM);
                    }
                }
            }
        }
        if let (Some(plan), Some(t)) = (spec.cancel, cancel_req) {
            if !killed && t.elapsed() > plan.grace {
                sys::send_signal(pid, sys::SIGKILL);
                killed = true;
            }
        }
        thread::sleep(Duration::from_millis(2));
    };
    let wall = start.elapsed().as_secs_f64();
    if let (Some(plan), Some(t)) = (spec.cancel, cancel_req) {
        cancel_outcome = Some(CancelOutcome {
            method: plan.method,
            exit_after_s: t.elapsed().as_secs_f64(),
            escalated_to_kill: killed && plan.method != CancelMethod::Kill,
        });
    }
    drop(stdin);
    stop.store(true, Ordering::Relaxed);
    let gen_wait = gen_thread.join().unwrap_or_default();
    let (write_wait, write_error) = writer_thread.join().unwrap_or((Duration::ZERO, Some("writer panicked".into())));
    let _ = stderr_thread.join();
    if let Some(t) = progress_thread {
        let _ = t.join();
    }
    drop(child);
    let progress = progress.lock().unwrap().clone();
    let stderr_tail = tail.lock().unwrap().iter().cloned().collect();
    Ok(RunResult {
        exit,
        pid,
        wall_s: wall,
        frames_sent: sent.load(Ordering::Relaxed),
        gen_wait_s: gen_wait.as_secs_f64(),
        write_wait_s: write_wait.as_secs_f64(),
        helper: ru,
        progress,
        stderr_tail,
        cancel: cancel_outcome,
        write_error,
    })
}
