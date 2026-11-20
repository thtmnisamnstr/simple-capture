mod gpu;
#[cfg(windows)]
mod win;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dcomp,
    ChildBelow,
    ChildAbove,
    Parent,
}

impl Mode {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "dcomp" => Some(Self::Dcomp),
            "child-below" => Some(Self::ChildBelow),
            "child-above" => Some(Self::ChildAbove),
            "parent" => Some(Self::Parent),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Dcomp => "dcomp",
            Self::ChildBelow => "child-below",
            Self::ChildAbove => "child-above",
            Self::Parent => "parent",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CssRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub dpr: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl CssRect {
    fn parse(msg: &str) -> Option<Self> {
        let v: Vec<f64> = msg.strip_prefix("rect ")?.split(' ').filter_map(|s| s.parse().ok()).collect();
        (v.len() == 5).then(|| Self { x: v[0], y: v[1], w: v[2], h: v[3], dpr: v[4] })
    }
    pub fn to_px(self) -> PxRect {
        let x0 = (self.x * self.dpr).round();
        let y0 = (self.y * self.dpr).round();
        let x1 = ((self.x + self.w) * self.dpr).round();
        let y1 = ((self.y + self.h) * self.dpr).round();
        PxRect { x: x0 as i32, y: y0 as i32, w: (x1 - x0).max(1.0) as u32, h: (y1 - y0).max(1.0) as u32 }
    }
}

enum UserEvent {
    Rect(CssRect),
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn main() {
    let default_mode = if cfg!(windows) { "dcomp" } else { "parent" };
    let arg = std::env::args().nth(1).unwrap_or_else(|| default_mode.to_string());
    let mode = Mode::parse(&arg).unwrap_or_else(|| {
        eprintln!("usage: native-preview-windows-spike [dcomp|child-below|child-above|parent]");
        std::process::exit(2);
    });
    if !cfg!(windows) && mode != Mode::Parent {
        eprintln!("mode {} is Windows-only; use `parent` on this OS", mode.name());
        std::process::exit(2);
    }
    let stress = std::env::var("SC_STRESS").map(|v| v == "1").unwrap_or(false);
    let probe_at = Duration::from_secs(env_u64("SC_PROBE_AT", 4));
    let run_for = env_u64("SC_SECONDS", 0);

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let mut wb = WindowBuilder::new()
        .with_title(format!("Native preview spike: {}", mode.name()))
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_always_on_top(true);
    if mode != Mode::Parent {
        // In parent mode the swapchain owns the window's bottom layer, so tao must not GDI-erase it.
        wb = wb.with_background_color((24, 24, 28, 255));
    }
    let window = wb.build(&event_loop).expect("window");

    let html = include_str!("page.html")
        .replace("__MODE__", mode.name())
        .replace("__STRESS__", if stress { "true" } else { "false" });
    let proxy = event_loop.create_proxy();
    let _webview = WebViewBuilder::new()
        .with_transparent(true)
        .with_html(html)
        .with_ipc_handler(move |req| {
            if let Some(r) = CssRect::parse(req.body()) {
                let _ = proxy.send_event(UserEvent::Rect(r));
            }
        })
        .build(&window)
        .expect("webview");

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: if cfg!(windows) { wgpu::Backends::DX12 } else { wgpu::Backends::PRIMARY },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    #[cfg(windows)]
    let mut host = win::Host::new(mode, &window).expect("native preview host");

    let renderer: Arc<Mutex<Option<gpu::Renderer>>> = Arc::new(Mutex::new(None));
    let quit = Arc::new(AtomicBool::new(false));
    let frames = Arc::new(AtomicU64::new(0));
    let mut last_rect: Option<CssRect> = None;
    let mut rect_updates: u64 = 0;
    let mut first_rect_at: Option<Instant> = None;
    let mut probed = false;
    let started = Instant::now();

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250));
        match event {
            Event::UserEvent(UserEvent::Rect(css)) => {
                let px = css.to_px();
                rect_updates += 1;
                last_rect = Some(css);
                let mut guard = renderer.lock().unwrap();
                if guard.is_none() {
                    #[cfg(windows)]
                    let target = host.surface_target();
                    #[cfg(not(windows))]
                    let target =
                        unsafe { wgpu::SurfaceTargetUnsafe::from_display_and_window(&window, &window) }.expect("raw handle");
                    let surface = unsafe { instance.create_surface_unsafe(target) }.expect("surface");
                    let (w, h) = if mode == Mode::Parent {
                        let s = window.inner_size();
                        (s.width, s.height)
                    } else {
                        (px.w, px.h)
                    };
                    let r = gpu::Renderer::new(&instance, surface, w, h);
                    eprintln!("[spike] mode={} adapter={}", mode.name(), r.adapter_desc);
                    *guard = Some(r);
                    spawn_render_thread(renderer.clone(), quit.clone(), frames.clone());
                    first_rect_at = Some(Instant::now());
                }
                let r = guard.as_mut().unwrap();
                if mode == Mode::Parent {
                    let s = window.inner_size();
                    r.resize(s.width, s.height);
                    r.viewport = Some(px);
                } else {
                    r.resize(px.w, px.h);
                }
                drop(guard);
                #[cfg(windows)]
                if let Err(e) = host.place(px) {
                    eprintln!("[spike] place failed: {e}");
                }
            }
            Event::WindowEvent { event: WindowEvent::Resized(size), .. } if mode == Mode::Parent => {
                if let Some(r) = renderer.lock().unwrap().as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                quit.store(true, Ordering::SeqCst);
                *control_flow = ControlFlow::Exit;
            }
            Event::MainEventsCleared => {
                if let (Some(t0), false, Some(css)) = (first_rect_at, probed, last_rect) {
                    if t0.elapsed() >= probe_at {
                        probed = true;
                        eprintln!(
                            "[spike] rect updates so far: {rect_updates}, frames presented: {}",
                            frames.load(Ordering::Relaxed)
                        );
                        #[cfg(windows)]
                        win::probe(&window, mode, css);
                        #[cfg(not(windows))]
                        eprintln!("[probe] pixel probes are Windows-only (rect {css:?})");
                    }
                }
                if run_for > 0 && started.elapsed() >= Duration::from_secs(run_for) {
                    quit.store(true, Ordering::SeqCst);
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}

fn spawn_render_thread(renderer: Arc<Mutex<Option<gpu::Renderer>>>, quit: Arc<AtomicBool>, frames: Arc<AtomicU64>) {
    std::thread::spawn(move || {
        let mut window_start = Instant::now();
        let mut window_frames = 0u64;
        while !quit.load(Ordering::Relaxed) {
            let presented = renderer.lock().unwrap().as_mut().map(|r| r.render()).unwrap_or(false);
            if presented {
                frames.fetch_add(1, Ordering::Relaxed);
                window_frames += 1;
            } else {
                std::thread::sleep(Duration::from_millis(4));
            }
            if window_start.elapsed() >= Duration::from_secs(1) {
                let secs = window_start.elapsed().as_secs_f64();
                eprintln!("[fps] {:.1}", window_frames as f64 / secs);
                window_start = Instant::now();
                window_frames = 0;
            }
        }
    });
}
