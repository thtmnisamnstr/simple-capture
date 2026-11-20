use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSPanel, NSScreen, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use sc_capture_macos::sck::{audio_sample_buffer_of, pcm_f32_interleaved};
use sc_core::project::Resolution;
use sc_geometry::SizePx;
use sc_platform::{
    AudioBuffer, AudioData, AudioTrackKind, FrameSink, Nv12View, ScreenStreamConfig, SourceEvent,
    VideoFrame,
};

pub type R = Result<(), String>;

pub fn host_ns() -> i64 {
    static P: OnceLock<Arc<dyn sc_platform::Platform>> = OnceLock::new();
    P.get_or_init(sc_capture_macos::platform).host_now().0
}

pub fn measure(name: &str, v: impl std::fmt::Display) {
    println!("MEASURE {name}: {v}");
}

#[derive(Debug, Clone)]
pub struct FixtureApp {
    pub title: String,
    pub pid: i32,
    pub window_id: Option<u32>,
    pub rect_pt: Option<[f64; 4]>,
    pub second_rect_pt: Option<[f64; 4]>,
}

#[derive(Debug, Clone)]
pub struct Fixture {
    pub display_id: String,
    pub apps: Vec<FixtureApp>,
}

impl Fixture {
    pub fn app(&self, title: &str) -> Result<&FixtureApp, String> {
        self.apps
            .iter()
            .find(|a| a.title == title)
            .ok_or_else(|| format!("fixture has no {title}"))
    }
}

fn json_field<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let k = format!("\"{key}\"");
    let i = obj.find(&k)? + k.len();
    let rest = obj[i..].trim_start().strip_prefix(':')?.trim_start();
    let end = if rest.starts_with('[') {
        rest.find(']')? + 1
    } else if let Some(r) = rest.strip_prefix('"') {
        return r.find('"').map(|e| &r[..e]);
    } else {
        rest.find([',', '}', '\n']).unwrap_or(rest.len())
    };
    Some(rest[..end].trim())
}

fn rect(v: Option<&str>) -> Option<[f64; 4]> {
    let v = v?.trim_start_matches('[').trim_end_matches(']');
    let n: Vec<f64> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    (n.len() == 4).then(|| [n[0], n[1], n[2], n[3]])
}

fn parse_fixture(text: &str) -> Result<Fixture, String> {
    let display_id = json_field(text, "display_id")
        .ok_or("fixture: no display_id")?
        .to_string();
    let apps_start = text.find("\"apps\"").ok_or("fixture: no apps")?;
    let mut apps = Vec::new();
    for chunk in text[apps_start..].split('{').skip(1) {
        let obj = chunk.split('}').next().unwrap_or("");
        let Some(title) = json_field(obj, "title") else {
            continue;
        };
        apps.push(FixtureApp {
            title: title.to_string(),
            pid: json_field(obj, "pid")
                .and_then(|p| p.parse().ok())
                .ok_or("fixture: pid")?,
            window_id: json_field(obj, "window_id").and_then(|p| p.parse().ok()),
            rect_pt: rect(json_field(obj, "rect_pt")),
            second_rect_pt: rect(json_field(obj, "second_rect_pt")),
        });
    }
    Ok(Fixture { display_id, apps })
}

static SPAWNED: Mutex<Option<Child>> = Mutex::new(None);

extern "C" fn kill_spawned_fixture() {
    if let Ok(mut g) = SPAWNED.lock()
        && let Some(c) = g.as_mut()
    {
        unsafe { libc::kill(c.id() as i32, libc::SIGTERM) };
        let _ = c.wait();
    }
}

/// `SC_FIXTURE` (started by `scripts/lib/live.sh fixture start`), else a fixture this process
/// starts itself from `$SC_DEVTOOLS` and stops at exit.
pub fn fixture() -> Result<Fixture, String> {
    static F: OnceLock<Result<Fixture, String>> = OnceLock::new();
    install_exception_logger();

    F.get_or_init(|| {
        let path = match std::env::var("SC_FIXTURE") {
            Ok(p) if !p.is_empty() => PathBuf::from(p),
            _ => {
                let tools = std::env::var("SC_DEVTOOLS")
                    .map_err(|_| "neither SC_FIXTURE nor SC_DEVTOOLS is set".to_string())?;
                let out =
                    std::env::temp_dir().join(format!("sc-fixture-{}.json", std::process::id()));
                let _ = std::fs::remove_file(&out);
                let child = Command::new(tools)
                    .args(["testapps", "fixture", "--out"])
                    .arg(&out)
                    .spawn()
                    .map_err(|e| format!("spawn fixture: {e}"))?;
                if let Ok(mut g) = SPAWNED.lock() {
                    *g = Some(child);
                }
                unsafe { libc::atexit(kill_spawned_fixture) };
                let t0 = Instant::now();
                while !out.exists() && t0.elapsed() < Duration::from_secs(20) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                println!("fixture: started by the test process (SC_FIXTURE unset)");
                out
            }
        };
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        std::thread::sleep(Duration::from_millis(300));
        parse_fixture(&text)
    })
    .clone()
}

pub fn cfg() -> ScreenStreamConfig {
    ScreenStreamConfig {
        fps: 30,
        resolution: Resolution::Native,
        show_cursor: false,
        show_clicks: false,
        queue_depth: 6,
        exclude: Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Red,
    Green,
    Blue,
    Magenta,
    Gray,
    Other,
}

pub fn classify(r: f64, g: f64, b: f64) -> Class {
    if r > 0.6 && g < 0.3 && b < 0.3 {
        Class::Red
    } else if g > 0.45 && r < 0.3 && b < 0.3 {
        Class::Green
    } else if b > 0.6 && r < 0.3 && g < 0.3 {
        Class::Blue
    } else if r > 0.6 && b > 0.6 && g < 0.3 {
        Class::Magenta
    } else if (r - g).abs() < 0.08 && (g - b).abs() < 0.08 && r > 0.3 && r < 0.7 {
        Class::Gray
    } else {
        Class::Other
    }
}

pub fn nv12_rgb(v: &Nv12View, x: usize, y: usize) -> (f64, f64, f64) {
    let yy = f64::from(v.y[y * v.y_stride + x]);
    let ui = (y / 2) * v.uv_stride + (x / 2) * 2;
    let (u, vv) = (f64::from(v.uv[ui]), f64::from(v.uv[ui + 1]));
    let (yn, cb, cr) = if v.full_range {
        (yy / 255.0, (u - 128.0) / 255.0, (vv - 128.0) / 255.0)
    } else {
        (
            (yy - 16.0) / 219.0,
            (u - 128.0) / 224.0,
            (vv - 128.0) / 224.0,
        )
    };
    let r = yn + 1.5748 * cr;
    let g = yn - 0.1873 * cb - 0.4681 * cr;
    let b = yn + 1.8556 * cb;
    (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
}

/// Majority class over a 5×5 grid inside `rect` (frame pixels, clipped).
pub fn classify_rect(v: &Nv12View, rect: [f64; 4]) -> Class {
    let (w, h) = (v.size.w as f64, v.size.h as f64);
    let x0 = rect[0].clamp(0.0, w - 1.0);
    let y0 = rect[1].clamp(0.0, h - 1.0);
    let x1 = (rect[0] + rect[2]).clamp(x0 + 1.0, w);
    let y1 = (rect[1] + rect[3]).clamp(y0 + 1.0, h);
    let mut counts: HashMap<Class, u32> = HashMap::new();
    for i in 0..5 {
        for j in 0..5 {
            let x = x0 + (x1 - x0) * (f64::from(i) + 0.5) / 5.0;
            let y = y0 + (y1 - y0) * (f64::from(j) + 0.5) / 5.0;
            let (r, g, b) = nv12_rgb(v, x as usize, y as usize);
            *counts.entry(classify(r, g, b)).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(c, _)| c)
        .unwrap_or(Class::Other)
}

#[derive(Debug, Clone)]
pub struct FrameRec {
    pub tag: u64,
    pub pts: i64,
    pub arrive: i64,
    pub size: SizePx,
    pub classes: Vec<Class>,
    pub content_rect: Option<sc_geometry::RectPt>,
}

#[derive(Debug, Clone)]
pub struct AudioRec {
    pub pts: i64,
    pub frames: u32,
    pub rate: u32,
    pub mono: Vec<f32>,
}

/// Probes are frame-pixel rects classified on the capture queue for every frame of a tag.
#[derive(Default)]
pub struct Collector {
    pub frames: Mutex<Vec<FrameRec>>,
    pub events: Mutex<Vec<(i64, SourceEvent)>>,
    pub audio: Mutex<Vec<AudioRec>>,
    pub probes: Mutex<HashMap<u64, Vec<[f64; 4]>>>,
}

impl Collector {
    pub fn new() -> Arc<Collector> {
        Arc::new(Collector::default())
    }

    pub fn probe(&self, tag: u64, rects: Vec<[f64; 4]>) {
        if let Ok(mut p) = self.probes.lock() {
            p.insert(tag, rects);
        }
    }

    pub fn frames_of(&self, tag: u64) -> Vec<FrameRec> {
        self.frames
            .lock()
            .map(|f| f.iter().filter(|r| r.tag == tag).cloned().collect())
            .unwrap_or_default()
    }

    pub fn wait_frames(&self, tag: u64, n: usize, timeout: Duration) -> Vec<FrameRec> {
        let t0 = Instant::now();
        loop {
            let f = self.frames_of(tag);
            if f.len() >= n || t0.elapsed() >= timeout {
                return f;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn wait_event(
        &self,
        timeout: Duration,
        pred: impl Fn(&SourceEvent) -> bool,
    ) -> Option<(i64, SourceEvent)> {
        let t0 = Instant::now();
        loop {
            if let Some(e) = self
                .events
                .lock()
                .ok()
                .and_then(|ev| ev.iter().find(|(_, e)| pred(e)).cloned())
            {
                return Some(e);
            }
            if t0.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn audio_since(&self, from_pts: i64) -> Vec<AudioRec> {
        self.audio
            .lock()
            .map(|a| a.iter().filter(|r| r.pts >= from_pts).cloned().collect())
            .unwrap_or_default()
    }
}

impl FrameSink for Collector {
    fn on_screen(&self, f: VideoFrame) {
        let probes = self
            .probes
            .lock()
            .ok()
            .and_then(|p| p.get(&f.stream.0).cloned())
            .unwrap_or_default();
        let mut classes = Vec::new();
        let _ = f.buffer.0.with_nv12(&mut |v| {
            classes = probes.iter().map(|r| classify_rect(&v, *r)).collect();
        });
        let rec = FrameRec {
            tag: f.stream.0,
            pts: f.pts.0,
            arrive: host_ns(),
            size: f.size,
            classes,
            content_rect: f.content_rect_pt,
        };
        if let Ok(mut v) = self.frames.lock() {
            v.push(rec);
        }
    }

    fn on_camera(&self, _f: VideoFrame) {}

    fn on_audio(&self, _t: AudioTrackKind, a: AudioBuffer) {
        let AudioData::Native(n) = &a.data else {
            return;
        };
        let Some(sb) = audio_sample_buffer_of(n.as_ref()) else {
            return;
        };
        let Ok((pcm, rate, ch)) = pcm_f32_interleaved(sb) else {
            return;
        };
        let ch = usize::from(ch.max(1));
        let mono: Vec<f32> = pcm
            .chunks(ch)
            .map(|c| c.iter().sum::<f32>() / ch as f32)
            .collect();
        if let Ok(mut v) = self.audio.lock() {
            v.push(AudioRec {
                pts: a.pts.0,
                frames: a.frames,
                rate,
                mono,
            });
        }
    }

    fn on_event(&self, e: SourceEvent) {
        if let Ok(mut v) = self.events.lock() {
            v.push((host_ns(), e));
        }
    }
}

/// Amplitude of `hz` in `x` (Goertzel).
pub fn tone_amplitude(x: &[f32], rate: u32, hz: f64) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    let w = 2.0 * std::f64::consts::PI * hz / f64::from(rate);
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s0 = f64::from(v) + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - c * s1 * s2;
    2.0 * power.max(0.0).sqrt() / x.len() as f64
}

/// Largest hole between consecutive buffers, in ns.
pub fn max_audio_gap(recs: &[AudioRec]) -> i64 {
    recs.windows(2)
        .map(|w| {
            let end =
                w[0].pts + i64::from(w[0].frames) * 1_000_000_000 / i64::from(w[0].rate.max(1));
            w[1].pts - end
        })
        .max()
        .unwrap_or(0)
}

pub struct TestPanel(pub Retained<NSWindow>);

unsafe impl Send for TestPanel {}
unsafe impl Sync for TestPanel {}

pub fn primary_height(mtm: MainThreadMarker) -> f64 {
    NSScreen::screens(mtm)
        .firstObject()
        .map(|s| s.frame().size.height)
        .unwrap_or(1080.0)
}

/// A blue window at `rect` (top-left points of the primary display), level 25: a
/// non-activating NSPanel, or a plain NSWindow when `plain` (for the overlay class swap).
pub fn make_panel(mtm: MainThreadMarker, rect: [f64; 4], show: bool, plain: bool) -> TestPanel {
    let h = primary_height(mtm);
    let frame = NSRect::new(
        NSPoint::new(rect[0], h - rect[1] - rect[3]),
        NSSize::new(rect[2], rect[3]),
    );
    let win: Retained<NSWindow> = if plain {
        unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                true,
            )
        }
    } else {
        Retained::into_super(NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            true,
        ))
    };
    unsafe { win.setReleasedWhenClosed(false) };
    win.setTitle(&NSString::from_str("SC-OWN-TEST"));
    win.setOpaque(true);
    win.setHasShadow(false);
    win.setHidesOnDeactivate(false);
    win.setIgnoresMouseEvents(true);
    win.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces);
    win.setLevel(25);
    win.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
        0.0, 0.0, 1.0, 1.0,
    )));
    if show {
        win.orderFrontRegardless();
    }
    TestPanel(win)
}

pub fn send_signal(pid: i32, sig: libc::c_int) -> R {
    if unsafe { libc::kill(pid, sig) } != 0 {
        return Err(format!(
            "kill({pid}, {sig}): {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

pub fn ensure(cond: bool, msg: impl FnOnce() -> String) -> R {
    if cond { Ok(()) } else { Err(msg()) }
}

type Preprocessor = extern "C" fn(*mut objc2::runtime::AnyObject) -> *mut objc2::runtime::AnyObject;

unsafe extern "C" {
    fn objc_setExceptionPreprocessor(f: Preprocessor) -> Preprocessor;
}

extern "C" fn log_exception(e: *mut objc2::runtime::AnyObject) -> *mut objc2::runtime::AnyObject {
    if let Some(obj) = unsafe { e.as_ref() } {
        let desc: Option<Retained<NSString>> = unsafe { objc2::msg_send![obj, description] };
        let name: Option<Retained<NSString>> = unsafe { objc2::msg_send![obj, name] };
        eprintln!("OBJC EXCEPTION: {name:?}: {desc:?}");
        eprintln!("{}", std::backtrace::Backtrace::force_capture());
    }
    e
}

/// Prints every Objective-C exception as it is thrown (the runner aborts on foreign unwinds).
pub fn install_exception_logger() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| unsafe {
        objc_setExceptionPreprocessor(log_exception);
    });
}
