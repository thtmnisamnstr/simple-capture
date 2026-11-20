mod frames;
mod helper;
mod jobs;
mod probe;
mod sys;
mod wav;

use frames::{PixFmt, Synth};
use helper::{CancelMethod, CancelPlan, Container, RunSpec, Transport};
use jobs::{Job, Tool};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Opts {
    ffmpeg: PathBuf,
    gifski: PathBuf,
    ffprobe: PathBuf,
    ffmpeg_ref: PathBuf,
    out: PathBuf,
    seconds: u32,
    only: Vec<String>,
    skip_cancel: bool,
    skip_jobs: bool,
    queue_depth: usize,
}

fn parse_opts(args: &[String]) -> Opts {
    let mut o = Opts {
        ffmpeg: "ffmpeg".into(),
        gifski: "gifski".into(),
        ffprobe: "ffprobe".into(),
        ffmpeg_ref: "ffmpeg".into(),
        out: "results".into(),
        seconds: 10,
        only: vec![],
        skip_cancel: false,
        skip_jobs: false,
        queue_depth: 3,
    };
    let mut i = 0;
    while i < args.len() {
        let v = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--ffmpeg" => o.ffmpeg = v.into(),
            "--gifski" => o.gifski = v.into(),
            "--ffprobe" => o.ffprobe = v.into(),
            "--ffmpeg-ref" => o.ffmpeg_ref = v.into(),
            "--out" => o.out = v.into(),
            "--seconds" => o.seconds = v.parse().expect("--seconds"),
            "--only" => o.only = v.split(',').map(String::from).collect(),
            "--queue-depth" => o.queue_depth = v.parse().expect("--queue-depth"),
            "--skip-cancel" => {
                o.skip_cancel = true;
                i += 1;
                continue;
            }
            "--skip-jobs" => {
                o.skip_jobs = true;
                i += 1;
                continue;
            }
            other => panic!("unknown option {other}"),
        }
        i += 2;
    }
    o
}

fn jstr(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn jnum(v: f64) -> String {
    if v.is_finite() { format!("{v:.4}") } else { "null".into() }
}

struct Obj(Vec<(String, String)>);
impl Obj {
    fn new() -> Self {
        Obj(vec![])
    }
    fn s(mut self, k: &str, v: &str) -> Self {
        self.0.push((k.into(), jstr(v)));
        self
    }
    fn n(mut self, k: &str, v: f64) -> Self {
        self.0.push((k.into(), jnum(v)));
        self
    }
    fn b(mut self, k: &str, v: bool) -> Self {
        self.0.push((k.into(), v.to_string()));
        self
    }
    fn raw(mut self, k: &str, v: String) -> Self {
        self.0.push((k.into(), v));
        self
    }
    fn render(&self) -> String {
        let f: Vec<String> = self.0.iter().map(|(k, v)| format!("{}: {}", jstr(k), v)).collect();
        format!("{{{}}}", f.join(", "))
    }
}

fn sh(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

fn loadavg() -> String {
    sh("sysctl", &["-n", "vm.loadavg"]).trim_matches(|c| c == '{' || c == '}' || c == ' ').to_string()
}

struct Ctx {
    o: Opts,
    synths: HashMap<(usize, usize, u32), Arc<Synth>>,
    wav: PathBuf,
}

impl Ctx {
    fn synth(&mut self, w: usize, h: usize, fps: u32) -> Arc<Synth> {
        self.synths.entry((w, h, fps)).or_insert_with(|| Arc::new(Synth::new(w, h, fps))).clone()
    }
}

struct Row {
    job: Job,
    frames: u64,
    res: Result<helper::RunResult, String>,
    out: PathBuf,
    size: u64,
    val: probe::Validation,
    load_before: String,
}

fn run_job(ctx: &mut Ctx, job: &Job) -> Row {
    let synth = ctx.synth(job.w, job.h, job.fps);
    let frames = ctx.o.seconds as u64 * job.fps as u64;
    let outdir = ctx.o.out.join("out");
    let logdir = ctx.o.out.join("logs");
    let _ = std::fs::create_dir_all(&outdir);
    let _ = std::fs::create_dir_all(&logdir);
    let fin = outdir.join(format!("{}.{}", job.name, job.ext));
    let part = outdir.join(format!("{}.{}.part", job.name, job.ext));
    let _ = std::fs::remove_file(&fin);
    let _ = std::fs::remove_file(&part);
    let (program, args, progress) = match job.tool {
        Tool::Ffmpeg => (ctx.o.ffmpeg.clone(), job.ffmpeg_args("pipe:0", Some(ctx.wav.to_str().unwrap()), part.to_str().unwrap()), true),
        Tool::Gifski => (ctx.o.gifski.clone(), jobs::gifski_args(part.to_str().unwrap(), job.vargs[0].parse().unwrap()), false),
    };
    let _ = std::fs::write(logdir.join(format!("{}.cmd.txt", job.name)), format!("{} {}\n", program.display(), args.join(" ")));
    let load_before = loadavg();
    eprintln!("[job] {} ({}x{}@{} {:?}) ...", job.name, job.w, job.h, job.fps, job.container);
    let res = helper::run(RunSpec {
        program,
        args,
        transport: Transport::Stdin,
        fifo_path: None,
        container: job.container,
        synth: synth.clone(),
        frames,
        queue_depth: ctx.o.queue_depth,
        progress,
        stderr_log: Some(logdir.join(format!("{}.stderr.txt", job.name))),
        cancel: None,
    });
    let mut size = 0;
    let mut val = probe::Validation::default();
    if let Ok(r) = &res {
        if r.exit.success() {
            let _ = std::fs::rename(&part, &fin);
            size = std::fs::metadata(&fin).map(|m| m.len()).unwrap_or(0);
            val = probe::validate(&ctx.o.ffprobe, &ctx.o.ffmpeg_ref, job, &fin, frames, &synth);
        } else {
            let _ = std::fs::remove_file(&part);
        }
        let cpu = r.helper.cpu_s();
        eprintln!(
            "      {} wall {:.2}s, {:.1} fps, cpu {:.0}%, rss {} MB, progress updates {}, valid {}{}",
            r.exit.describe(),
            r.wall_s,
            r.frames_sent as f64 / r.wall_s,
            cpu / r.wall_s * 100.0,
            r.helper.maxrss / 1_048_576,
            r.progress.updates,
            val.ok(),
            if val.ok() { String::new() } else { format!(" {:?} {:?}", val.failures(), r.stderr_tail.iter().rev().take(3).collect::<Vec<_>>()) }
        );
    } else if let Err(e) = &res {
        eprintln!("      error: {e}");
    }
    Row { job: job.clone(), frames, res, out: fin, size, val, load_before }
}

fn gen_bench(ctx: &mut Ctx) -> Vec<Obj> {
    let mut out = vec![];
    for (w, h) in [(1920usize, 1080usize), (2560, 1440)] {
        let s = ctx.synth(w, h, 30);
        for fmt in [PixFmt::I420, PixFmt::Nv12] {
            let mut buf = vec![0u8; s.frame_bytes()];
            let mut scratch = vec![];
            let n = 300u64;
            let t = Instant::now();
            for i in 0..n {
                s.render(i, fmt, &mut buf, &mut scratch);
            }
            let fps = n as f64 / t.elapsed().as_secs_f64();
            eprintln!("[gen] {w}x{h} {fmt:?}: {fps:.0} frames/s (single thread)");
            out.push(Obj::new().s("res", &format!("{w}x{h}")).s("fmt", &format!("{fmt:?}")).n("frames_per_s", fps));
        }
    }
    out
}

struct CancelRow {
    name: String,
    transport: Transport,
    tool: &'static str,
    r: Result<helper::RunResult, String>,
    alive_after: bool,
    partial_exists: bool,
    partial_size: u64,
    partial_frames: String,
    partial_playable: bool,
    deleted: bool,
}

fn partial_state(ffprobe: &Path, p: &Path) -> (bool, u64, String, bool) {
    let exists = p.exists();
    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    if !exists {
        return (false, 0, "-".into(), false);
    }
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"])
        .arg(p)
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let f = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let playable = f.parse::<u64>().map(|n| n > 0).unwrap_or(false);
            (true, size, f, playable)
        }
        Ok(o) => (true, size, format!("unreadable: {}", String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("").trim()), false),
        Err(e) => (true, size, e.to_string(), false),
    }
}

fn cancel_tests(ctx: &mut Ctx) -> Vec<CancelRow> {
    let synth = ctx.synth(1920, 1080, 30);
    let dir = ctx.o.out.join("cancel");
    let _ = std::fs::create_dir_all(&dir);
    let job = jobs::all_jobs().into_iter().find(|j| j.name == "x264_medium_crf20_aacat").unwrap();
    let cases: [(&str, CancelMethod, Transport); 5] = [
        ("x264_q_fifo", CancelMethod::Quit, Transport::Fifo),
        ("x264_sigterm", CancelMethod::Term, Transport::Stdin),
        ("x264_sigkill", CancelMethod::Kill, Transport::Stdin),
        ("x264_eof", CancelMethod::Eof, Transport::Stdin),
        ("x264_hung_sigterm_then_kill", CancelMethod::StoppedThenTerm, Transport::Stdin),
    ];
    let mut rows = vec![];
    for (name, method, transport) in cases {
        let part = dir.join(format!("{name}.mp4.part"));
        let _ = std::fs::remove_file(&part);
        let fifo = dir.join(format!("{name}.fifo"));
        let input = if transport == Transport::Fifo {
            sys::make_fifo(&fifo).expect("mkfifo");
            fifo.to_str().unwrap().to_string()
        } else {
            "pipe:0".into()
        };
        let args = job.ffmpeg_args(&input, Some(ctx.wav.to_str().unwrap()), part.to_str().unwrap());
        eprintln!("[cancel] {name} via {}", method.name());
        let r = helper::run(RunSpec {
            program: ctx.o.ffmpeg.clone(),
            args,
            transport,
            fifo_path: (transport == Transport::Fifo).then(|| fifo.clone()),
            container: Container::Y4m,
            synth: synth.clone(),
            frames: 30 * 60,
            queue_depth: ctx.o.queue_depth,
            progress: true,
            stderr_log: Some(dir.join(format!("{name}.stderr.txt"))),
            cancel: Some(CancelPlan { method, after_frames: 90, grace: Duration::from_millis(1500) }),
        });
        let _ = std::fs::remove_file(&fifo);
        rows.push(finish_cancel_row(&ctx.o.ffprobe, name, transport, "ffmpeg", r, &part));
    }
    let gsynth = ctx.synth(jobs::GIF_W, jobs::GIF_H, jobs::GIF_FPS);
    for (name, method) in [("gifski_sigterm", CancelMethod::Term), ("gifski_eof", CancelMethod::Eof)] {
        let part = dir.join(format!("{name}.gif.part"));
        let _ = std::fs::remove_file(&part);
        eprintln!("[cancel] {name} via {}", method.name());
        let r = helper::run(RunSpec {
            program: ctx.o.gifski.clone(),
            args: jobs::gifski_args(part.to_str().unwrap(), 90),
            transport: Transport::Stdin,
            fifo_path: None,
            container: Container::Y4m,
            synth: gsynth.clone(),
            frames: 15 * 60,
            queue_depth: ctx.o.queue_depth,
            progress: false,
            stderr_log: Some(dir.join(format!("{name}.stderr.txt"))),
            cancel: Some(CancelPlan { method, after_frames: 45, grace: Duration::from_millis(1500) }),
        });
        rows.push(finish_cancel_row(&ctx.o.ffprobe, name, Transport::Stdin, "gifski", r, &part));
    }
    rows
}

fn finish_cancel_row(ffprobe: &Path, name: &str, transport: Transport, tool: &'static str, r: Result<helper::RunResult, String>, part: &Path) -> CancelRow {
    let alive_after = r.as_ref().map(|x| sys::pid_alive(x.pid)).unwrap_or(false);
    let (partial_exists, partial_size, partial_frames, partial_playable) = partial_state(ffprobe, part);
    let _ = std::fs::remove_file(part);
    let deleted = !part.exists();
    if let Ok(x) = &r {
        let c = x.cancel.as_ref();
        eprintln!(
            "      {} after {:.3}s (escalated {}), sent {} frames, partial: {} bytes, frames {}, alive {}",
            x.exit.describe(),
            c.map(|c| c.exit_after_s).unwrap_or(-1.0),
            c.map(|c| c.escalated_to_kill).unwrap_or(false),
            x.frames_sent,
            partial_size,
            partial_frames,
            alive_after
        );
    }
    CancelRow { name: name.into(), transport, tool, r, alive_after, partial_exists, partial_size, partial_frames, partial_playable, deleted }
}

fn crash_child(args: &[String]) {
    let o = parse_opts(args);
    let synth = Synth::new(1920, 1080, 30);
    let job = jobs::all_jobs().into_iter().find(|j| j.name == "x264_medium_crf20").unwrap();
    let out = o.out.join("crash-child.mp4.part");
    let mut child = Command::new(&o.ffmpeg)
        .args(job.ffmpeg_args("pipe:0", None, out.to_str().unwrap()))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    println!("PID {}", child.id());
    std::io::stdout().flush().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let _ = stdin.write_all(synth.y4m_header().as_bytes());
    let mut buf = vec![0u8; synth.frame_bytes()];
    for n in 0.. {
        synth.render_i420(n, &mut buf);
        if stdin.write_all(b"FRAME\n").and_then(|_| stdin.write_all(&buf)).is_err() {
            break;
        }
    }
}

fn parent_crash_test(ctx: &Ctx) -> Obj {
    let me = std::env::current_exe().unwrap();
    let mut c = Command::new(me)
        .args(["crash-child", "--ffmpeg", ctx.o.ffmpeg.to_str().unwrap(), "--out", ctx.o.out.join("cancel").to_str().unwrap()])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn crash-child");
    let mut line = String::new();
    BufReader::new(c.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let helper_pid: u32 = line.trim().trim_start_matches("PID ").parse().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    eprintln!("[cancel] parent crash: SIGKILL spike process {} (helper {helper_pid})", c.id());
    sys::send_signal(c.id(), sys::SIGKILL);
    let _ = c.wait();
    let t = Instant::now();
    let mut gone = false;
    while t.elapsed() < Duration::from_secs(15) {
        if !sys::pid_alive(helper_pid) {
            gone = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let secs = t.elapsed().as_secs_f64();
    if !gone {
        sys::send_signal(helper_pid, sys::SIGKILL);
    }
    let part = ctx.o.out.join("cancel").join("crash-child.mp4.part");
    let (ex, size, frames, playable) = partial_state(&ctx.o.ffprobe, &part);
    let _ = std::fs::remove_file(&part);
    eprintln!("      helper exited on its own: {gone} after {secs:.3}s; leftover partial {ex} {size} bytes frames {frames}");
    Obj::new()
        .b("helper_exited_on_its_own", gone)
        .n("seconds_until_helper_gone", secs)
        .b("leftover_partial_exists", ex)
        .n("leftover_partial_bytes", size as f64)
        .s("leftover_partial_frames", &frames)
        .b("leftover_partial_playable", playable)
}

fn robustness(ctx: &Ctx) -> Vec<Obj> {
    let mut v = vec![];
    let missing = PathBuf::from("/nonexistent/simple-capture/ffmpeg");
    let e = Command::new(&missing).spawn().err().map(|e| helper::spawn_error_message(&missing, &e)).unwrap_or_default();
    eprintln!("[robust] missing helper -> {e}");
    v.push(Obj::new().s("case", "missing helper binary").b("handled", e.contains("not found")).s("result", &e));

    let synth = Arc::new(Synth::new(640, 360, 30));
    let r = helper::run(RunSpec {
        program: ctx.o.ffmpeg.clone(),
        args: ["-hide_banner", "-f", "yuv4mpegpipe", "-i", "pipe:0", "-c:v", "libaom-av1", "-f", "mp4", "-y", "/dev/null"].iter().map(|s| s.to_string()).collect(),
        transport: Transport::Stdin,
        fifo_path: None,
        container: Container::Y4m,
        synth,
        frames: 30,
        queue_depth: 2,
        progress: false,
        stderr_log: None,
        cancel: None,
    });
    let (desc, tail) = match &r {
        Ok(x) => (format!("{}; writer: {:?}", x.exit.describe(), x.write_error), x.stderr_tail.last().cloned().unwrap_or_default()),
        Err(e) => (e.clone(), String::new()),
    };
    eprintln!("[robust] unknown encoder -> {desc} / {tail}");
    v.push(Obj::new().s("case", "encoder not compiled in (libaom-av1)").b("handled", r.as_ref().map(|x| !x.exit.success()).unwrap_or(true)).s("result", &format!("{desc}; stderr: {tail}")));

    for (codec, w, h) in [("h264_videotoolbox", 4096, 2304), ("h264_videotoolbox", 4096, 4096), ("h264_videotoolbox", 8192, 4320), ("hevc_videotoolbox", 8192, 4320), ("hevc_videotoolbox", 7680, 4320), ("h264_videotoolbox", 1918, 1080)] {
        let out = Command::new(&ctx.o.ffmpeg)
            .args(["-hide_banner", "-v", "error", "-f", "lavfi", "-i", &format!("testsrc2=s={w}x{h}:r=30:d=0.2"), "-c:v", codec, "-allow_sw", "0", "-f", "null", "-"])
            .output();
        let (ok, msg) = match out {
            Ok(o) => (o.status.success(), String::from_utf8_lossy(&o.stderr).lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join(" | ")),
            Err(e) => (false, e.to_string()),
        };
        eprintln!("[robust] {codec} {w}x{h}: ok={ok} {msg}");
        v.push(Obj::new().s("case", &format!("{codec} at {w}x{h} (probe encode, 6 frames)")).b("encoded", ok).s("result", &msg));
    }
    v
}

fn transport_test(ctx: &mut Ctx) -> Vec<Obj> {
    let mut v = vec![];
    for (w, h, container, label) in [
        (1920usize, 1080usize, Container::Y4m, "Y4M"),
        (1920, 1080, Container::Raw(PixFmt::Nv12), "NV12"),
        (2560, 1440, Container::Y4m, "Y4M"),
        (2560, 1440, Container::Raw(PixFmt::Nv12), "NV12"),
    ] {
        let synth = ctx.synth(w, h, 30);
        let input: Vec<String> = match container {
            Container::Y4m => vec!["-f".into(), "yuv4mpegpipe".into(), "-i".into(), "pipe:0".into()],
            _ => vec!["-f".into(), "rawvideo".into(), "-pix_fmt".into(), "nv12".into(), "-video_size".into(), format!("{w}x{h}"), "-framerate".into(), "30".into(), "-i".into(), "pipe:0".into()],
        };
        let mut args: Vec<String> = ["-hide_banner", "-loglevel", "warning", "-progress", "pipe:1", "-nostats"].iter().map(|s| s.to_string()).collect();
        args.extend(input);
        args.extend(["-c:v", "wrapped_avframe", "-f", "null", "-"].iter().map(|s| s.to_string()));
        let frames = 600u64;
        let r = helper::run(RunSpec {
            program: ctx.o.ffmpeg.clone(),
            args,
            transport: Transport::Stdin,
            fifo_path: None,
            container,
            synth: synth.clone(),
            frames,
            queue_depth: ctx.o.queue_depth,
            progress: true,
            stderr_log: None,
            cancel: None,
        });
        if let Ok(x) = r {
            let fps = x.frames_sent as f64 / x.wall_s;
            let mbs = fps * synth.frame_bytes() as f64 / 1e6;
            eprintln!("[transport] {w}x{h} {label} -> ffmpeg null: {fps:.0} frames/s, {mbs:.0} MB/s, gen wait {:.2}s", x.gen_wait_s);
            v.push(Obj::new().s("res", &format!("{w}x{h}")).s("input", label).n("frames", x.frames_sent as f64).n("frames_per_s", fps).n("mb_per_s", mbs).n("gen_wait_s", x.gen_wait_s).n("helper_cpu_pct", x.helper.cpu_s() / x.wall_s * 100.0).s("exit", &x.exit.describe()));
        }
    }
    v
}

fn machine_info() -> Obj {
    Obj::new()
        .s("cpu", &sh("sysctl", &["-n", "machdep.cpu.brand_string"]))
        .s("ncpu", &sh("sysctl", &["-n", "hw.ncpu"]))
        .s("perf_cores", &sh("sysctl", &["-n", "hw.perflevel0.physicalcpu"]))
        .s("eff_cores", &sh("sysctl", &["-n", "hw.perflevel1.physicalcpu"]))
        .s("mem_bytes", &sh("sysctl", &["-n", "hw.memsize"]))
        .s("macos", &sh("sw_vers", &["-productVersion"]))
        .s("loadavg_start", &loadavg())
}

fn bench(args: &[String]) {
    let o = parse_opts(args);
    let _ = std::fs::create_dir_all(&o.out);
    let wav = o.out.join("beep_1khz.wav");
    wav::write_beep_wav(&wav, o.seconds).expect("wav");
    let mut ctx = Ctx { o, synths: HashMap::new(), wav };
    let started = sh("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]);
    let machine = machine_info();
    let ffv = sh(ctx.o.ffmpeg.to_str().unwrap(), &["-hide_banner", "-version"]).lines().next().unwrap_or("").to_string();
    let gv = sh(ctx.o.gifski.to_str().unwrap(), &["--version"]);
    let refv = sh(ctx.o.ffprobe.to_str().unwrap(), &["-hide_banner", "-version"]).lines().next().unwrap_or("").to_string();
    eprintln!("helper: {ffv} | {gv} | validator: {refv}");
    let generator = gen_bench(&mut ctx);
    let transport = transport_test(&mut ctx);

    let mut rows = vec![];
    if !ctx.o.skip_jobs {
        for job in jobs::all_jobs() {
            if !ctx.o.only.is_empty() && !ctx.o.only.iter().any(|f| job.name.contains(f.as_str()) || job.group == f) {
                continue;
            }
            rows.push(run_job(&mut ctx, &job));
        }
    }
    let (cancels, crash, robust) = if ctx.o.skip_cancel {
        (vec![], None, vec![])
    } else {
        (cancel_tests(&mut ctx), Some(parent_crash_test(&ctx)), robustness(&ctx))
    };
    let finished = sh("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]);
    write_reports(&ctx, &started, &finished, machine, &ffv, &gv, &refv, generator, transport, &rows, &cancels, crash, robust);
}

#[allow(clippy::too_many_arguments)]
fn write_reports(ctx: &Ctx, started: &str, finished: &str, machine: Obj, ffv: &str, gv: &str, refv: &str, generator: Vec<Obj>, transport: Vec<Obj>, rows: &[Row], cancels: &[CancelRow], crash: Option<Obj>, robust: Vec<Obj>) {
    let mut jrows = vec![];
    let mut md = String::new();
    let _ = writeln!(md, "# Spike 1 helper benchmark results\n");
    let _ = writeln!(md, "- Run: {started} -> {finished} (UTC); clip {} s per encode; queue depth {} frames", ctx.o.seconds, ctx.o.queue_depth);
    let _ = writeln!(md, "- Machine: `{}`", machine.render());
    let _ = writeln!(md, "- Helper: `{ffv}`, `{gv}`; validator: `{refv}`");
    let _ = writeln!(md, "- Load average at end: {} (other heavy jobs may be running: numbers are preliminary)\n", loadavg());
    let _ = writeln!(md, "## Synthetic frame generator (single thread)\n");
    for g in &generator {
        let _ = writeln!(md, "- {}", g.render());
    }
    let _ = writeln!(md, "\n## Pipe transport ceiling (generator -> bounded queue -> stdin -> ffmpeg demux -> null)\n");
    for t in &transport {
        let _ = writeln!(md, "- {}", t.render());
    }
    let _ = writeln!(md, "\n## Encode speed\n");
    let _ = writeln!(md, "| job | group | input | frames | wall s | enc fps | x realtime | helper CPU % | helper CPU s | peak RSS MB | size MB | Mbps | gen wait s | pipe write s | progress updates | load before |");
    let _ = writeln!(md, "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for r in rows {
        let input = match r.job.container {
            Container::Y4m => "Y4M",
            Container::Raw(PixFmt::Nv12) => "NV12",
            Container::Raw(_) => "raw",
        };
        let mut o = Obj::new().s("job", &r.job.name).s("group", r.job.group).s("label", &r.job.label).s("input", input).n("width", r.job.w as f64).n("height", r.job.h as f64).n("fps", r.job.fps as f64).n("frames", r.frames as f64).s("output", &r.out.display().to_string());
        match &r.res {
            Ok(x) => {
                let cpu = x.helper.cpu_s();
                let dur = r.frames as f64 / r.job.fps as f64;
                o = o
                    .s("exit", &x.exit.describe())
                    .n("wall_s", x.wall_s)
                    .n("encode_fps", x.frames_sent as f64 / x.wall_s)
                    .n("realtime_factor", dur / x.wall_s)
                    .n("helper_cpu_s", cpu)
                    .n("helper_cpu_pct", cpu / x.wall_s * 100.0)
                    .n("helper_peak_rss_mb", x.helper.maxrss as f64 / 1_048_576.0)
                    .n("size_bytes", r.size as f64)
                    .n("mbps", r.size as f64 * 8.0 / dur / 1e6)
                    .n("gen_wait_s", x.gen_wait_s)
                    .n("pipe_write_s", x.write_wait_s)
                    .n("progress_updates", x.progress.updates as f64)
                    .b("progress_end_seen", x.progress.ended)
                    .s("last_progress", &format!("{:?}", x.progress.last))
                    .s("load_before", &r.load_before)
                    .b("valid", r.val.ok())
                    .raw("validation", format!("[{}]", r.val.checks.iter().map(|c| Obj::new().s("check", &c.name).b("ok", c.ok).s("detail", &c.detail).render()).collect::<Vec<_>>().join(", ")))
                    .raw("probe", Obj(r.val.fields.iter().map(|(k, v)| (k.clone(), jstr(v))).collect()).render());
                let _ = writeln!(
                    md,
                    "| {} | {} | {} | {} | {:.2} | {:.1} | {:.2} | {:.0} | {:.1} | {:.0} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {} |",
                    r.job.name, r.job.group, input, x.frames_sent, x.wall_s, x.frames_sent as f64 / x.wall_s, dur / x.wall_s, cpu / x.wall_s * 100.0, cpu,
                    x.helper.maxrss as f64 / 1_048_576.0, r.size as f64 / 1e6, r.size as f64 * 8.0 / dur / 1e6, x.gen_wait_s, x.write_wait_s, x.progress.updates, r.load_before
                );
            }
            Err(e) => {
                o = o.s("error", e);
                let _ = writeln!(md, "| {} | {} | {} | - | ERROR: {} |||||||||||", r.job.name, r.job.group, input, e);
            }
        }
        jrows.push(o.render());
    }
    let _ = writeln!(md, "\n## Validation (ffprobe: {refv})\n");
    let _ = writeln!(md, "| job | result | codec | tag | size | r_frame_rate | frames | duration s | pix_fmt | primaries/trc/space/range | audio | color max err | GIF delays (cs: count) | loop |");
    let _ = writeln!(md, "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for r in rows.iter().filter(|r| r.res.as_ref().map(|x| x.exit.success()).unwrap_or(false)) {
        let f = |k: &str| r.val.fields.get(k).cloned().unwrap_or_default();
        let dur = if f("duration").is_empty() || f("duration") == "N/A" { format!("{} (fmt)", f("format.duration")) } else { f("duration") };
        let _ = writeln!(
            md,
            "| {} | {} | {} | {} | {}x{} | {} | {} | {} | {} | {}/{}/{}/{} | {} | {} | {} | {} |",
            r.job.name,
            if r.val.ok() { "PASS".to_string() } else if r.job.group == "demo" { format!("EXPECTED FAIL (pitfall demo): {}", r.val.failures().join("; ")) } else { format!("FAIL: {}", r.val.failures().join("; ")) },
            f("codec_name"), f("codec_tag_string"), f("width"), f("height"), f("r_frame_rate"), f("nb_read_frames"), dur, f("pix_fmt"),
            f("color_primaries"), f("color_transfer"), f("color_space"), f("color_range"), f("audio"), f("color_max_err"), f("gif_delays_cs"), f("gif_loop")
        );
    }
    let mut jc = vec![];
    if !cancels.is_empty() {
        let _ = writeln!(md, "\n## Cancel tests (1080p30; cancel requested after 90 frames for x264, 45 for gifski; grace 1.5 s then SIGKILL)\n");
        let _ = writeln!(md, "| case | tool | transport | method | exit | time to exit s | escalated to SIGKILL | frames sent | pid alive after | partial bytes | partial frames (ffprobe) | partial playable | deleted |");
        let _ = writeln!(md, "|---|---|---|---|---|---|---|---|---|---|---|---|---|");
        for c in cancels {
            let mut o = Obj::new().s("case", &c.name).s("tool", c.tool).s("transport", &format!("{:?}", c.transport));
            match &c.r {
                Ok(x) => {
                    let co = x.cancel.as_ref().unwrap();
                    o = o
                        .s("method", co.method.name())
                        .s("exit", &x.exit.describe())
                        .n("time_to_exit_s", co.exit_after_s)
                        .b("escalated_to_sigkill", co.escalated_to_kill)
                        .n("frames_sent", x.frames_sent as f64)
                        .b("pid_alive_after", c.alive_after)
                        .b("partial_exists", c.partial_exists)
                        .n("partial_bytes", c.partial_size as f64)
                        .s("partial_frames", &c.partial_frames)
                        .b("partial_playable", c.partial_playable)
                        .b("partial_deleted", c.deleted)
                        .s("writer_error", &format!("{:?}", x.write_error));
                    let _ = writeln!(
                        md,
                        "| {} | {} | {:?} | {} | {} | {:.3} | {} | {} | {} | {} | {} | {} | {} |",
                        c.name, c.tool, c.transport, co.method.name(), x.exit.describe(), co.exit_after_s, co.escalated_to_kill, x.frames_sent, c.alive_after, c.partial_size, c.partial_frames, c.partial_playable, c.deleted
                    );
                }
                Err(e) => {
                    o = o.s("error", e);
                    let _ = writeln!(md, "| {} | {} | {:?} | ERROR {} |||||||||", c.name, c.tool, c.transport, e);
                }
            }
            jc.push(o.render());
        }
    }
    if let Some(c) = &crash {
        let _ = writeln!(md, "\n## Parent crash (spike process SIGKILLed while feeding the helper)\n\n`{}`", c.render());
    }
    if !robust.is_empty() {
        let _ = writeln!(md, "\n## Robustness / capability probes\n");
        for r in &robust {
            let _ = writeln!(md, "- `{}`", r.render());
        }
    }
    let json = Obj::new()
        .s("started_utc", started)
        .s("finished_utc", finished)
        .n("clip_seconds", ctx.o.seconds as f64)
        .n("queue_depth_frames", ctx.o.queue_depth as f64)
        .raw("machine", machine.render())
        .s("ffmpeg_version", ffv)
        .s("gifski_version", gv)
        .s("validator", refv)
        .s("loadavg_end", &loadavg())
        .raw("generator", format!("[{}]", generator.iter().map(|g| g.render()).collect::<Vec<_>>().join(", ")))
        .raw("transport", format!("[{}]", transport.iter().map(|g| g.render()).collect::<Vec<_>>().join(", ")))
        .raw("encodes", format!("[\n{}\n]", jrows.join(",\n")))
        .raw("cancel", format!("[\n{}\n]", jc.join(",\n")))
        .raw("parent_crash", crash.map(|c| c.render()).unwrap_or("null".into()))
        .raw("robustness", format!("[{}]", robust.iter().map(|r| r.render()).collect::<Vec<_>>().join(", ")));
    std::fs::write(ctx.o.out.join("results.json"), json.render() + "\n").unwrap();
    std::fs::write(ctx.o.out.join("results.md"), md).unwrap();
    eprintln!("wrote {} and results.md", ctx.o.out.join("results.json").display());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("bench") => bench(&args[2..]),
        Some("crash-child") => crash_child(&args[2..]),
        Some("list") => {
            for j in jobs::all_jobs() {
                println!("{:<32} {:<7} {}", j.name, j.group, j.label);
            }
        }
        _ => {
            eprintln!("usage: spike-helpers bench --ffmpeg P --gifski P --ffprobe P --ffmpeg-ref P --out DIR [--seconds N] [--only a,b] [--skip-cancel] [--skip-jobs] [--queue-depth N]\n       spike-helpers list");
            std::process::exit(2);
        }
    }
}
