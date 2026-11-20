use std::io::Write;
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
use objc2_foundation::{
    NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSURL,
};
use serde_json::json;

use super::{EventKind, FlashLog, IssuedEvent, Opts, TONE_MS, host_now_ns};

const FLASH_MS: u64 = 100;
const FULLSCREEN_MS: u64 = 300;
const WAV_RATE: u32 = 48_000;
const TITLE: &str = "SC-SYNC-WIN";

static TERM: AtomicBool = AtomicBool::new(false);
static GO: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(sig: libc::c_int) {
    if sig == libc::SIGUSR1 {
        GO.store(true, Ordering::SeqCst);
    } else {
        TERM.store(true, Ordering::SeqCst);
    }
}

fn install_signals() {
    let h = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    unsafe {
        for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGUSR1] {
            libc::signal(s, h);
        }
    }
}

struct Planned {
    kind: EventKind,
    label: String,
    t_s: f64,
}

pub fn run(o: &Opts) -> Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| anyhow!("not on the main thread"))?;
    install_signals();
    let ppid = unsafe { libc::getppid() };
    let mut plan: Vec<Planned> = Vec::new();
    for (t_s, label) in super::parse_schedule(o.get("schedule").unwrap_or(""))? {
        plan.push(Planned {
            kind: EventKind::Flash,
            label,
            t_s,
        });
    }
    for (t_s, label) in super::parse_schedule(o.get("fullscreen-at").unwrap_or(""))? {
        plan.push(Planned {
            kind: EventKind::Fullscreen,
            label,
            t_s,
        });
    }
    if plan.is_empty() {
        bail!("flashbeep needs --schedule and/or --fullscreen-at");
    }
    plan.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
    let out = PathBuf::from(
        o.get("out")
            .ok_or_else(|| anyhow!("--out FILE is required"))?,
    );
    let (x, y) = o.pair("pos", ',')?.unwrap_or((80.0, 120.0));
    let (w, h) = o.pair("size", 'x')?.unwrap_or((800.0, 600.0));
    let linger = Duration::from_secs_f64(o.num("linger")?.unwrap_or(2.0));

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    let screen = NSScreen::screens(mtm)
        .firstObject()
        .ok_or_else(|| anyhow!("no screen"))?
        .frame();
    let (sw, sh) = (screen.size.width, screen.size.height);

    let win = panel(
        mtm,
        NSRect::new(NSPoint::new(x, sh - y - h), NSSize::new(w, h)),
        3,
    );
    paint(&win, 0.0);
    win.orderFrontRegardless();
    let full = panel(
        mtm,
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(sw, sh)),
        1000,
    );
    paint(&full, 1.0);

    let last_t = plan.last().map_or(0.0, |p| p.t_s);
    let wav = std::env::temp_dir().join(format!("sc-flashbeep-{}.wav", std::process::id()));
    let times: Vec<f64> = plan.iter().map(|p| p.t_s).collect();
    write_wav(&wav, &times, last_t + 1.0)?;
    let player = player(&wav);
    let _ = std::fs::remove_file(&wav);
    let player = player?;
    pump(Duration::from_millis(100));

    let mut log = FlashLog {
        title: TITLE.into(),
        pid: std::process::id(),
        window_id: win.windowNumber(),
        rect_pt: [x, y, w, h],
        screen_pt: [sw, sh],
        tone_hz: super::detect::TONE_HZ,
        tone_ms: TONE_MS,
        flash_ms: FLASH_MS,
        fullscreen_ms: FULLSCREEN_MS,
        t0_host_ns: None,
        events: Vec::new(),
    };
    print_line(&json!({"event": "ready", "pid": log.pid, "title": TITLE,
        "window_id": log.window_id, "rect_pt": log.rect_pt, "screen_pt": log.screen_pt}));

    let alive = || !TERM.load(Ordering::SeqCst) && unsafe { libc::getppid() } == ppid;
    if o.flag("start-on-signal") {
        while !GO.load(Ordering::SeqCst) {
            if !alive() {
                return Ok(());
            }
            pump(Duration::from_millis(5));
        }
    }
    if !unsafe { player.play() } {
        bail!("AVAudioPlayer refused to play");
    }
    let t0 = host_now_ns();
    log.t0_host_ns = Some(t0);
    print_line(&json!({"event": "started", "host_ns": t0}));

    let mut next = 0usize;
    let mut flash_off: Option<Instant> = None;
    let mut full_off: Option<Instant> = None;
    while alive() {
        let pos = unsafe { player.currentTime() };
        if let Some(p) = plan.get(next)
            && pos >= p.t_s
        {
            match p.kind {
                EventKind::Flash => {
                    paint(&win, 1.0);
                    flash_off = Some(Instant::now() + Duration::from_millis(FLASH_MS));
                }
                EventKind::Fullscreen => {
                    full.orderFrontRegardless();
                    full_off = Some(Instant::now() + Duration::from_millis(FULLSCREEN_MS));
                }
            }
            log.events.push(IssuedEvent {
                kind: p.kind,
                label: p.label.clone(),
                t_s: p.t_s,
                host_ns: host_now_ns(),
                player_s: pos,
            });
            next += 1;
        }
        let now = Instant::now();
        if flash_off.is_some_and(|t| now >= t) {
            paint(&win, 0.0);
            flash_off = None;
        }
        if full_off.is_some_and(|t| now >= t) {
            full.orderOut(None);
            full_off = None;
        }
        if next == plan.len() && flash_off.is_none() && full_off.is_none() {
            break;
        }
        pump(Duration::from_millis(1));
    }
    unsafe { player.stop() };
    write_log(&out, &log)?;
    print_line(&json!({"event": "done", "issued": log.events.len(), "planned": plan.len()}));
    let end = Instant::now() + linger;
    while alive() && Instant::now() < end {
        pump(Duration::from_millis(20));
    }
    Ok(())
}

fn panel(mtm: MainThreadMarker, frame: NSRect, level: isize) -> Retained<NSPanel> {
    let p = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        frame,
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe { p.setReleasedWhenClosed(false) };
    p.setTitle(&NSString::from_str(TITLE));
    p.setOpaque(true);
    p.setHasShadow(false);
    p.setHidesOnDeactivate(false);
    p.setIgnoresMouseEvents(true);
    p.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces);
    p.setLevel(level);
    p
}

fn paint(p: &NSPanel, v: f64) {
    let c = NSColor::colorWithSRGBRed_green_blue_alpha(v, v, v, 1.0);
    p.setBackgroundColor(Some(&c));
    p.display();
}

fn pump(d: Duration) {
    unsafe {
        let date = NSDate::dateWithTimeIntervalSinceNow(d.as_secs_f64());
        NSRunLoop::currentRunLoop().runMode_beforeDate(NSDefaultRunLoopMode, &date);
    }
}

fn print_line(v: &serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn write_log(path: &Path, log: &FlashLog) -> Result<()> {
    let text = serde_json::to_string_pretty(log)?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn player(wav: &Path) -> Result<Retained<AVAudioPlayer>> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&wav.to_string_lossy()));
    let p = unsafe { AVAudioPlayer::initWithContentsOfURL_error(AVAudioPlayer::alloc(), &url) }
        .map_err(|e| anyhow!("AVAudioPlayer: {}", e.localizedDescription()))?;
    unsafe {
        p.setVolume(1.0);
        p.prepareToPlay();
    }
    Ok(p)
}

/// One continuous file (silence with a tone at each event) keeps the output device running, so
/// every tone has the same output latency, and the flash is issued off the player's clock.
pub fn tone_track(times_s: &[f64], len_s: f64) -> Vec<i16> {
    let n = (len_s * f64::from(WAV_RATE)) as usize;
    let mut pcm = vec![0i16; n];
    let tone_n = (TONE_MS as f64 / 1000.0 * f64::from(WAV_RATE)) as usize;
    for &t in times_s {
        let start = (t * f64::from(WAV_RATE)).round() as usize;
        for i in 0..tone_n {
            let Some(s) = pcm.get_mut(start + i) else {
                break;
            };
            let ph = 2.0 * std::f64::consts::PI * super::detect::TONE_HZ * i as f64
                / f64::from(WAV_RATE);
            *s = (ph.sin() * 0.5 * 32767.0) as i16;
        }
    }
    pcm
}

fn write_wav(path: &Path, times_s: &[f64], len_s: f64) -> Result<()> {
    let pcm = tone_track(times_s, len_s);
    let bytes = u32::try_from(pcm.len() * 2).context("WAV too long")?;
    let mut buf = Vec::with_capacity(44 + pcm.len() * 2);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + bytes).to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&WAV_RATE.to_le_bytes());
    buf.extend_from_slice(&(WAV_RATE * 2).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&bytes.to_le_bytes());
    for s in pcm {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, buf).with_context(|| format!("writing {}", path.display()))
}
