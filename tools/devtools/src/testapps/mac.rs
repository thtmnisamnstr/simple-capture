use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSPanel, NSScreen,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_avf_audio::AVAudioPlayer;
use objc2_core_graphics::CGMainDisplayID;
use objc2_foundation::{
    NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSURL,
};
use serde_json::{Value, json};

static TERM: AtomicBool = AtomicBool::new(false);
static USR1: AtomicBool = AtomicBool::new(false);
static USR2: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(sig: libc::c_int) {
    match sig {
        libc::SIGUSR1 => USR1.store(true, Ordering::SeqCst),
        libc::SIGUSR2 => USR2.store(true, Ordering::SeqCst),
        _ => TERM.store(true, Ordering::SeqCst),
    }
}

fn install_signals() {
    let h = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGTERM, h);
        libc::signal(libc::SIGINT, h);
        libc::signal(libc::SIGHUP, h);
        libc::signal(libc::SIGUSR1, h);
        libc::signal(libc::SIGUSR2, h);
    }
}

struct Args(Vec<String>);

impl Args {
    fn get(&self, name: &str) -> Option<&str> {
        let key = format!("--{name}");
        self.0
            .iter()
            .position(|a| *a == key)
            .and_then(|i| self.0.get(i + 1))
            .map(String::as_str)
    }

    fn flag(&self, name: &str) -> bool {
        let key = format!("--{name}");
        self.0.contains(&key)
    }

    fn pair(&self, name: &str, sep: char) -> Result<Option<(f64, f64)>> {
        let Some(v) = self.get(name) else {
            return Ok(None);
        };
        let (a, b) = v
            .split_once(sep)
            .ok_or_else(|| anyhow!("--{name} {v:?}: expected A{sep}B"))?;
        Ok(Some((a.trim().parse()?, b.trim().parse()?)))
    }
}

pub fn run(args: &[String]) -> Result<()> {
    let Some((cmd, rest)) = args.split_first() else {
        bail!("usage: testapps <window|tone|fixture> [args...]");
    };
    let a = Args(rest.to_vec());
    match cmd.as_str() {
        "window" => window_app(&a),
        "tone" => tone_app(&a),
        "fixture" => fixture(&a),
        other => bail!("unknown testapps command {other:?}"),
    }
}

fn rgb(name: &str) -> Result<[(f64, f64, f64); 2]> {
    Ok(match name {
        "red" => [(1.0, 0.0, 0.0), (0.9, 0.0, 0.0)],
        "green" => [(0.0, 0.8, 0.0), (0.0, 0.7, 0.0)],
        "blue" => [(0.0, 0.0, 1.0), (0.0, 0.0, 0.9)],
        "magenta" => [(1.0, 0.0, 1.0), (0.9, 0.0, 0.9)],
        "yellow" => [(1.0, 1.0, 0.0), (0.9, 0.9, 0.0)],
        "gray" => [(0.5, 0.5, 0.5), (0.45, 0.45, 0.45)],
        "white" => [(1.0, 1.0, 1.0), (0.95, 0.95, 0.95)],
        other => bail!("unknown colour {other:?}"),
    })
}

fn init_app(mtm: MainThreadMarker) -> Retained<NSApplication> {
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    app
}

fn primary_height(mtm: MainThreadMarker) -> f64 {
    NSScreen::screens(mtm)
        .firstObject()
        .map(|s| s.frame().size.height)
        .unwrap_or(1080.0)
}

struct Win {
    panel: Retained<NSPanel>,
    colors: [(f64, f64, f64); 2],
    rect: [f64; 4],
}

impl Win {
    fn new(
        mtm: MainThreadMarker,
        title: &str,
        rect: [f64; 4],
        colors: [(f64, f64, f64); 2],
        floating: bool,
    ) -> Win {
        let h = primary_height(mtm);
        let frame = NSRect::new(
            NSPoint::new(rect[0], h - rect[1] - rect[3]),
            NSSize::new(rect[2], rect[3]),
        );
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setTitle(&NSString::from_str(title));
        panel.setOpaque(true);
        panel.setHasShadow(false);
        panel.setHidesOnDeactivate(false);
        panel.setIgnoresMouseEvents(true);
        panel.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces);
        if floating {
            panel.setLevel(3);
        }
        let w = Win {
            panel,
            colors,
            rect,
        };
        w.paint(0);
        w.panel.orderFrontRegardless();
        w
    }

    fn paint(&self, i: usize) {
        let (r, g, b) = self.colors[i % 2];
        let c = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0);
        self.panel.setBackgroundColor(Some(&c));
        self.panel.display();
    }

    fn json(&self) -> Value {
        json!({"window_id": self.panel.windowNumber(), "rect_pt": self.rect})
    }
}

fn pump(d: Duration) {
    unsafe {
        let date = NSDate::dateWithTimeIntervalSinceNow(d.as_secs_f64());
        NSRunLoop::currentRunLoop().runMode_beforeDate(NSDefaultRunLoopMode, &date);
    }
}

fn print_line(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn parent_gone(ppid: libc::pid_t, watch: bool) -> bool {
    watch && unsafe { libc::getppid() } != ppid
}

fn rect_arg(a: &Args, default_size: (f64, f64)) -> Result<[f64; 4]> {
    let (x, y) = a.pair("pos", ',')?.unwrap_or((80.0, 120.0));
    let (w, h) = a.pair("size", 'x')?.unwrap_or(default_size);
    Ok([x, y, w, h])
}

fn window_app(a: &Args) -> Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| anyhow!("not on the main thread"))?;
    install_signals();
    let ppid = unsafe { libc::getppid() };
    let watch_parent = a.flag("exit-with-parent");
    let _app = init_app(mtm);
    let title = a.get("title").unwrap_or("SC-TEST");
    let colors = rgb(a.get("color").unwrap_or("red"))?;
    let rect = rect_arg(a, (640.0, 480.0))?;
    let animate = a.flag("animate");
    let close_on = a.get("close-on-signal").map(str::to_uppercase);
    let second_on = a.get("second-window-on-signal").map(str::to_uppercase);
    let second_rect = match a.pair("second-pos", ',')? {
        Some((x, y)) => [x, y, 320.0, 240.0],
        None => [rect[0], rect[1] + rect[3] + 40.0, 320.0, 240.0],
    };
    let mut win = Some(Win::new(mtm, title, rect, colors, a.flag("floating")));
    let mut second: Option<Win> = None;
    pump(Duration::from_millis(50));
    let mut ready = json!({"event": "ready", "pid": std::process::id(), "title": title});
    if let Some(w) = &win {
        ready["window_id"] = w.json()["window_id"].clone();
        ready["rect_pt"] = json!(rect);
        if a.flag("minimized") {
            ready["minimized"] = json!(minimize(w));
        }
    }
    print_line(&ready);
    let mut tick = 0usize;
    let mut last = Instant::now();
    while !TERM.load(Ordering::SeqCst) && !parent_gone(ppid, watch_parent) {
        for (sig, flag) in [("USR1", &USR1), ("USR2", &USR2)] {
            if !flag.swap(false, Ordering::SeqCst) {
                continue;
            }
            if close_on.as_deref() == Some(sig)
                && let Some(w) = win.take()
            {
                w.panel.close();
                print_line(&json!({"event": "closed", "title": title}));
            }
            if second_on.as_deref() == Some(sig) && second.is_none() {
                let w = Win::new(
                    mtm,
                    &format!("{title}-2"),
                    second_rect,
                    rgb("magenta")?,
                    true,
                );
                pump(Duration::from_millis(20));
                let mut v = w.json();
                v["event"] = json!("second_window");
                print_line(&v);
                second = Some(w);
            }
        }
        if animate && last.elapsed() >= Duration::from_millis(16) {
            last = Instant::now();
            tick += 1;
            if let Some(w) = &win {
                w.paint(tick);
            }
        }
        pump(Duration::from_millis(4));
    }
    Ok(())
}

/// `--minimized`: the window keeps its window number but is not on screen, so a capture of it gets
/// no frames. A borderless panel has no minimize button, so it gets one for the call; an app that
/// cannot minimize (no Dock) falls back to ordering the window out. Returns how it is hidden.
fn minimize(w: &Win) -> &'static str {
    let mask = w.panel.styleMask();
    w.panel
        .setStyleMask(mask | NSWindowStyleMask::Titled | NSWindowStyleMask::Miniaturizable);
    w.panel.miniaturize(None);
    for _ in 0..50 {
        if w.panel.isMiniaturized() {
            return "miniaturized";
        }
        pump(Duration::from_millis(20));
    }
    w.panel.orderOut(None);
    pump(Duration::from_millis(50));
    "ordered_out"
}

fn write_tone_wav(path: &Path, hz: f64, secs: f64) -> Result<()> {
    let sr = 48_000u32;
    let n = (secs * f64::from(sr)) as usize;
    let mut data = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f64 / f64::from(sr);
        let v = (2.0 * std::f64::consts::PI * hz * t).sin() * 0.25;
        data.extend_from_slice(&((v * 32767.0) as i16).to_le_bytes());
    }
    let mut f = std::fs::File::create(path).with_context(|| path.display().to_string())?;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data.len() as u32).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?;
    f.write_all(&(sr * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&(data.len() as u32).to_le_bytes())?;
    f.write_all(&data)?;
    Ok(())
}

fn tone_app(a: &Args) -> Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| anyhow!("not on the main thread"))?;
    install_signals();
    let ppid = unsafe { libc::getppid() };
    let watch_parent = a.flag("exit-with-parent");
    let _app = init_app(mtm);
    let hz: f64 = a
        .get("hz")
        .ok_or_else(|| anyhow!("--hz is required"))?
        .parse()?;
    let title = a
        .get("title")
        .map(str::to_string)
        .unwrap_or_else(|| format!("SC-TONE-{hz}"));
    let win = if a.flag("window") {
        let rect = rect_arg(a, (240.0, 180.0))?;
        Some(Win::new(
            mtm,
            &title,
            rect,
            rgb(a.get("color").unwrap_or("blue"))?,
            true,
        ))
    } else {
        None
    };
    let wav = std::env::temp_dir().join(format!("sc-tone-{}-{hz}.wav", std::process::id()));
    write_tone_wav(&wav, hz, 1.0)?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(&wav.to_string_lossy()));
    let player =
        unsafe { AVAudioPlayer::initWithContentsOfURL_error(AVAudioPlayer::alloc(), &url) }
            .map_err(|e| anyhow!("AVAudioPlayer: {}", e.localizedDescription()))?;
    unsafe {
        player.setNumberOfLoops(-1);
        player.setVolume(0.3);
        if !player.play() {
            bail!("AVAudioPlayer refused to play");
        }
    }
    pump(Duration::from_millis(50));
    let mut ready = json!({"event": "ready", "pid": std::process::id(), "title": title, "hz": hz});
    if let Some(w) = &win {
        ready["window_id"] = w.json()["window_id"].clone();
        ready["rect_pt"] = json!(w.rect);
    }
    print_line(&ready);
    while !TERM.load(Ordering::SeqCst) && !parent_gone(ppid, watch_parent) {
        pump(Duration::from_millis(20));
    }
    unsafe { player.stop() };
    let _ = std::fs::remove_file(&wav);
    drop(win);
    Ok(())
}

struct Spawned {
    pid: libc::pid_t,
    ready: Value,
    exited: bool,
}

impl Spawned {
    fn terminate(&self) {
        if !self.exited {
            unsafe { libc::kill(self.pid, libc::SIGTERM) };
        }
    }

    fn wait(&mut self) {
        if !self.exited {
            let mut st = 0;
            unsafe { libc::waitpid(self.pid, &mut st, 0) };
            self.exited = true;
        }
    }

    fn has_exited(&mut self) -> bool {
        if !self.exited {
            let mut st = 0;
            self.exited = unsafe { libc::waitpid(self.pid, &mut st, libc::WNOHANG) } == self.pid;
        }
        self.exited
    }
}

type DisclaimFn = unsafe extern "C" fn(*mut libc::posix_spawnattr_t, libc::c_int) -> libc::c_int;

/// Children spawned normally share our responsible process, and CoreAudio attributes their
/// audio to it, so an app-filtered SCStream cannot tell two tone apps apart. Disclaiming makes
/// each child its own responsible process (no TCC involved: the apps only play audio).
fn disclaim_fn() -> Option<DisclaimFn> {
    let sym = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            c"responsibility_spawnattrs_setdisclaim".as_ptr(),
        )
    };
    (!sym.is_null()).then(|| unsafe { std::mem::transmute::<*mut libc::c_void, DisclaimFn>(sym) })
}

fn cstr(s: &str) -> Result<std::ffi::CString> {
    Ok(std::ffi::CString::new(s)?)
}

fn spawn(args: &[&str]) -> Result<Spawned> {
    use std::os::fd::FromRawFd;
    let exe = std::env::current_exe()?;
    let exe_c = cstr(&exe.to_string_lossy())?;
    let mut argv_c = vec![exe_c.clone(), cstr("testapps")?];
    for a in args {
        argv_c.push(cstr(a)?);
    }
    argv_c.push(cstr("--exit-with-parent")?);
    let mut argv: Vec<*mut libc::c_char> = argv_c.iter().map(|c| c.as_ptr() as *mut _).collect();
    argv.push(std::ptr::null_mut());
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        bail!("pipe: {}", std::io::Error::last_os_error());
    }
    let mut pid: libc::pid_t = 0;
    let rc = unsafe {
        let mut fa: libc::posix_spawn_file_actions_t = std::ptr::null_mut();
        let mut attr: libc::posix_spawnattr_t = std::ptr::null_mut();
        libc::posix_spawn_file_actions_init(&mut fa);
        libc::posix_spawnattr_init(&mut attr);
        libc::posix_spawn_file_actions_addopen(
            &mut fa,
            0,
            c"/dev/null".as_ptr(),
            libc::O_RDONLY,
            0,
        );
        libc::posix_spawn_file_actions_adddup2(&mut fa, fds[1], 1);
        libc::posix_spawn_file_actions_addclose(&mut fa, fds[0]);
        libc::posix_spawn_file_actions_addclose(&mut fa, fds[1]);
        if let Some(f) = disclaim_fn() {
            f(&mut attr, 1);
        }
        let rc = libc::posix_spawn(
            &mut pid,
            exe_c.as_ptr(),
            &fa,
            &attr,
            argv.as_ptr(),
            *libc::_NSGetEnviron(),
        );
        libc::posix_spawn_file_actions_destroy(&mut fa);
        libc::posix_spawnattr_destroy(&mut attr);
        libc::close(fds[1]);
        rc
    };
    if rc != 0 {
        unsafe { libc::close(fds[0]) };
        bail!(
            "posix_spawn {args:?}: {}",
            std::io::Error::from_raw_os_error(rc)
        );
    }
    let out = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    let mut lines = BufReader::new(out).lines();
    let line = lines
        .next()
        .ok_or_else(|| anyhow!("{args:?}: exited before ready"))??;
    let mut ready: Value = serde_json::from_str(&line).with_context(|| line.clone())?;
    ready["responsibility_disclaimed"] = json!(disclaim_fn().is_some());
    std::thread::spawn(move || for _ in lines.by_ref() {});
    Ok(Spawned {
        pid,
        ready,
        exited: false,
    })
}

type FixtureSpec = (&'static str, Vec<&'static str>, Option<[f64; 4]>);

// Every fixture window fits a 1440×900 pt main display (the smallest current Mac default), so
// none sits off screen on a laptop: RED 40..680 × 100..580 (second window 40,620 below it),
// GREEN 720..1360 × 100..580 (second 720,600), TONE-440 380..620 × 620..800, STATIC
// 1080..1400 × 600..840.

fn fixture_specs() -> Vec<FixtureSpec> {
    vec![
        (
            "SC-TEST-RED",
            vec![
                "window",
                "--title",
                "SC-TEST-RED",
                "--color",
                "red",
                "--size",
                "640x480",
                "--pos",
                "40,100",
                "--floating",
                "--animate",
                "--close-on-signal",
                "USR1",
                "--second-window-on-signal",
                "USR2",
            ],
            None,
        ),
        (
            "SC-TEST-GREEN",
            vec![
                "window",
                "--title",
                "SC-TEST-GREEN",
                "--color",
                "green",
                "--size",
                "640x480",
                "--pos",
                "720,100",
                "--floating",
                "--animate",
                "--second-window-on-signal",
                "USR2",
                "--second-pos",
                "720,600",
            ],
            Some([720.0, 600.0, 320.0, 240.0]),
        ),
        (
            "SC-TONE-440",
            vec![
                "tone",
                "--hz",
                "440",
                "--window",
                "--title",
                "SC-TONE-440",
                "--pos",
                "380,620",
            ],
            None,
        ),
        (
            "SC-TONE-660",
            vec!["tone", "--hz", "660", "--title", "SC-TONE-660"],
            None,
        ),
        (
            "SC-STATIC",
            vec![
                "window",
                "--title",
                "SC-STATIC",
                "--color",
                "gray",
                "--size",
                "320x240",
                "--pos",
                "1080,600",
                "--floating",
            ],
            None,
        ),
    ]
}

fn atomic_json(path: &Path, v: &Value) -> Result<()> {
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(v)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn fixture(a: &Args) -> Result<()> {
    install_signals();
    let mut children: Vec<Spawned> = Vec::new();
    let result = (|| -> Result<Value> {
        let mut apps = Vec::new();
        for (title, args, second) in fixture_specs() {
            let s = spawn(&args).with_context(|| title.to_string())?;
            let mut v = s.ready.clone();
            v["second_rect_pt"] = json!(second);
            apps.push(v);
            children.push(s);
        }
        Ok(json!({"display_id": CGMainDisplayID().to_string(), "apps": apps}))
    })();
    let stop = |children: &mut Vec<Spawned>| {
        for c in children.iter() {
            c.terminate();
        }
        for c in children.iter_mut() {
            c.wait();
        }
    };
    let v = match result {
        Ok(v) => v,
        Err(e) => {
            stop(&mut children);
            return Err(e);
        }
    };
    match a.get("out") {
        Some(p) => atomic_json(Path::new(p), &v)?,
        None => print_line(&v),
    }
    while !TERM.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(50));
        if children.iter_mut().any(Spawned::has_exited) {
            eprintln!("testapps fixture: a fixture app exited");
            break;
        }
    }
    stop(&mut children);
    Ok(())
}
