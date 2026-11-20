//! `soak flash`: a small floating, click-through, non-activating panel (the soak's window source)
//! that repaints between two near-equal greys at 30 Hz so SCK always has new frames, and on each
//! "flash" line on stdin turns white for 100 ms while playing a 100 ms 1 kHz tone. Prints JSON
//! lines: `ready` (pid, window id, rect, main display size in points) and `flash` (host ns).

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
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

use super::procstat::host_now_ns;

pub const TITLE: &str = "SC-SOAK-FLASH";
const FLASH_MS: u64 = 100;

static TERM: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_sig: libc::c_int) {
    TERM.store(true, Ordering::SeqCst);
}

fn pump(d: Duration) {
    // SAFETY: called on the main thread; runs the default run loop mode until `date`.
    unsafe {
        let date = NSDate::dateWithTimeIntervalSinceNow(d.as_secs_f64());
        NSRunLoop::currentRunLoop().runMode_beforeDate(NSDefaultRunLoopMode, &date);
    }
}

fn line(v: &serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn write_tone_wav(path: &Path, hz: f64, secs: f64) -> Result<()> {
    let sr = 48_000u32;
    let n = (secs * f64::from(sr)) as usize;
    let ramp = 48usize;
    let mut data = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f64 / f64::from(sr);
        let g = (i.min(n - 1 - i).min(ramp) as f64) / ramp as f64;
        let v = (2.0 * std::f64::consts::PI * hz * t).sin() * 0.5 * g;
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

fn paint(panel: &NSPanel, v: f64) {
    let c = NSColor::colorWithSRGBRed_green_blue_alpha(v, v, v, 1.0);
    panel.setBackgroundColor(Some(&c));
    panel.display();
}

pub fn run(args: &[String]) -> Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| anyhow!("not on the main thread"))?;
    let h = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: the handler only stores an atomic.
    unsafe {
        libc::signal(libc::SIGTERM, h);
        libc::signal(libc::SIGINT, h);
        libc::signal(libc::SIGHUP, h);
    }
    let (w, hgt) = match args.iter().position(|a| a == "--size") {
        Some(i) => {
            let v = args.get(i + 1).ok_or_else(|| anyhow!("--size needs WxH"))?;
            let (a, b) = v
                .split_once('x')
                .ok_or_else(|| anyhow!("--size {v:?}: expected WxH"))?;
            (a.parse::<f64>()?, b.parse::<f64>()?)
        }
        None => (240.0, 160.0),
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
    let screen = NSScreen::screens(mtm)
        .firstObject()
        .map(|s| s.frame().size)
        .unwrap_or(NSSize::new(1920.0, 1080.0));
    let rect = [screen.width - w - 40.0, screen.height - hgt - 140.0, w, hgt];
    let frame = NSRect::new(
        NSPoint::new(rect[0], screen.height - rect[1] - rect[3]),
        NSSize::new(rect[2], rect[3]),
    );
    let panel: Retained<NSPanel> = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        frame,
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    // SAFETY: we keep `panel` alive for the whole run.
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setTitle(&NSString::from_str(TITLE));
    panel.setOpaque(true);
    panel.setHasShadow(false);
    panel.setHidesOnDeactivate(false);
    panel.setIgnoresMouseEvents(true);
    panel.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces);
    panel.setLevel(3);
    paint(&panel, 0.2);
    panel.orderFrontRegardless();

    let wav = std::env::temp_dir().join(format!("sc-soak-beep-{}.wav", std::process::id()));
    write_tone_wav(&wav, 1000.0, FLASH_MS as f64 / 1000.0)?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(&wav.to_string_lossy()));
    // SAFETY: plain AVAudioPlayer calls on a file we just wrote.
    let player =
        unsafe { AVAudioPlayer::initWithContentsOfURL_error(AVAudioPlayer::alloc(), &url) }
            .map_err(|e| anyhow!("AVAudioPlayer: {}", e.localizedDescription()))?;
    // SAFETY: as above.
    unsafe {
        player.setVolume(0.6);
        if !player.prepareToPlay() {
            bail!("AVAudioPlayer could not prepare the beep");
        }
    }

    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for l in std::io::stdin().lock().lines() {
            let Ok(l) = l else { break };
            if tx.send(l).is_err() {
                return;
            }
        }
    });
    pump(Duration::from_millis(50));
    line(&json!({
        "ev": "ready", "pid": std::process::id(), "title": TITLE,
        "window_id": panel.windowNumber(), "rect_pt": rect,
        "display_pt": [screen.width, screen.height],
    }));

    let mut flash_until: Option<Instant> = None;
    let mut tick = 0u64;
    let mut next_paint = Instant::now();
    while !TERM.load(Ordering::SeqCst) {
        match rx.try_recv() {
            Ok(cmd) if cmd.trim() == "flash" => {
                let host_ns = host_now_ns();
                paint(&panel, 1.0);
                // SAFETY: plain AVAudioPlayer calls.
                unsafe {
                    player.setCurrentTime(0.0);
                    player.play();
                }
                flash_until = Some(Instant::now() + Duration::from_millis(FLASH_MS));
                line(&json!({"ev": "flash", "host_ns": host_ns}));
            }
            Ok(_) => {}
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
        let now = Instant::now();
        if flash_until.is_some_and(|u| now >= u) {
            flash_until = None;
        }
        if flash_until.is_none() && now >= next_paint {
            tick += 1;
            paint(&panel, if tick.is_multiple_of(2) { 0.2 } else { 0.22 });
            next_paint = now + Duration::from_millis(33);
        }
        pump(Duration::from_millis(4));
    }
    // SAFETY: plain AVAudioPlayer call.
    unsafe { player.stop() };
    panel.orderOut(None);
    let _ = std::fs::remove_file(&wav);
    Ok(())
}
