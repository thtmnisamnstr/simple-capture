#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gpu;
#[cfg(target_os = "macos")]
mod native_mac;
mod procstats;

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{State, WebviewUrl, WebviewWindowBuilder};

// In-flight = frames sent on the Channel minus frames the webview has acked.
// If acks stop arriving entirely, send anyway after this long so a lost ack
// can never wedge the stream.
const BACKPRESSURE_TIMEOUT_MS: f64 = 250.0;

pub fn wall_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1e3
}

#[derive(Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Transport {
    Idle,
    Channel,
    Protocol,
    Native,
}

#[derive(Clone, Serialize)]
struct ModeCfg {
    name: String,
    transport: Transport,
    renderer: String,
    width: u32,
    height: u32,
    fps: u32,
    duration_s: f64,
    warmup_s: f64,
    max_in_flight: u32,
    fetch_per_raf: bool,
    native_above: bool,
    snapshot: bool,
}

fn parse_mode(name: &str, fps: u32, duration_s: f64, warmup_s: f64, max_in_flight: u32, snapshot: bool) -> Result<ModeCfg, String> {
    let (base, fetch_per_raf) = match name.strip_suffix("-raf") {
        Some(b) if b.starts_with("b-") => (b, true),
        _ => (name, false),
    };
    let parts: Vec<&str> = base.split('-').collect();
    let (transport, renderer, height) = match parts.as_slice() {
        ["idle"] => (Transport::Idle, "none", 1080),
        ["a", r @ ("gl" | "2d"), h] => (Transport::Channel, *r, h.parse::<u32>().map_err(|e| e.to_string())?),
        ["b", r @ ("gl" | "2d"), h] => (Transport::Protocol, *r, h.parse::<u32>().map_err(|e| e.to_string())?),
        ["c", "native", h] => (Transport::Native, "native", h.parse::<u32>().map_err(|e| e.to_string())?),
        ["c", "above", h] => (Transport::Native, "native-above", h.parse::<u32>().map_err(|e| e.to_string())?),
        _ => return Err(format!("unknown mode {name}")),
    };
    let width = match height {
        540 => 960,
        720 => 1280,
        1080 => 1920,
        _ => return Err(format!("unsupported height {height}")),
    };
    Ok(ModeCfg {
        name: name.to_string(),
        transport,
        renderer: renderer.to_string(),
        width,
        height,
        fps,
        duration_s,
        warmup_s,
        max_in_flight,
        fetch_per_raf,
        native_above: renderer == "native-above",
        snapshot,
    })
}

#[derive(Default, Serialize, Clone)]
struct PStats {
    produced: u64,
    delivered: u64,
    dropped_backpressure: u64,
    backpressure_overrides: u64,
    dropped_unfetched: u64,
    late_ticks: u64,
    presented: u64,
    surface_errors: u64,
    encode_ms: Vec<f64>,
    gpu_wait_ms: Vec<f64>,
    copy_ms: Vec<f64>,
    send_ms: Vec<f64>,
    frame_total_ms: Vec<f64>,
    acquire_ms: Vec<f64>,
    present_latency_ms: Vec<f64>,
}

struct Slot {
    id: u32,
    data: Option<Vec<u8>>,
}

struct NativeTarget {
    layer_ptr: usize,
    width_px: u32,
    height_px: u32,
}

struct Shared {
    cfg: ModeCfg,
    results_dir: PathBuf,
    started: Instant,
    producer_started: AtomicBool,
    channel: Mutex<Option<Channel<InvokeResponseBody>>>,
    acked: AtomicU32,
    ack_calls: AtomicU32,
    sent: AtomicU32,
    last_ack_us: AtomicU64,
    slot: Mutex<Slot>,
    slot_cv: Condvar,
    native: Mutex<Option<NativeTarget>>,
    native_cv: Condvar,
    native_desc: Mutex<Option<String>>,
    stats: Mutex<PStats>,
    adapter: Mutex<Option<String>>,
    begin: Mutex<Option<(Instant, Vec<procstats::ProcSample>, Vec<(i32, String)>)>>,
    measuring: AtomicBool,
    gpu_util: Mutex<Vec<f64>>,
    done: AtomicBool,
}

fn producer(shared: Arc<Shared>) {
    let cfg = shared.cfg.clone();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let mut surface_state: Option<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration)> = None;
    let mut gpu = if cfg.transport == Transport::Native {
        let target = {
            let mut g = shared.native.lock().unwrap();
            while g.is_none() {
                g = shared.native_cv.wait(g).unwrap();
            }
            g.take().unwrap()
        };
        #[cfg(target_os = "macos")]
        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(target.layer_ptr as *mut std::ffi::c_void))
                .expect("surface")
        };
        #[cfg(not(target_os = "macos"))]
        let surface: wgpu::Surface<'static> = {
            let _ = target.layer_ptr;
            unimplemented!("native mode is only implemented on macOS in this spike")
        };
        let gpu = gpu::Gpu::new(&instance, Some(&surface));
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = if caps.formats.contains(&wgpu::TextureFormat::Bgra8Unorm) {
            wgpu::TextureFormat::Bgra8Unorm
        } else {
            caps.formats[0]
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: target.width_px,
            height: target.height_px,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&gpu.device, &config);
        surface_state = Some((surface, config));
        gpu
    } else {
        let mut gpu = gpu::Gpu::new(&instance, None);
        gpu.set_readback_size(cfg.width, cfg.height);
        gpu
    };
    *shared.adapter.lock().unwrap() = Some(format!("{:?} / {} / {:?}", gpu.adapter_info.backend, gpu.adapter_info.name, gpu.adapter_info.device_type));

    let period = Duration::from_secs_f64(1.0 / cfg.fps as f64);
    let t_start = Instant::now();
    let mut next = Instant::now();
    let mut frame_id: u32 = 0;
    while !shared.done.load(Ordering::Relaxed) {
        frame_id += 1;
        let ts = wall_ms();
        let tick = Instant::now();
        let anim_t = (tick - t_start).as_secs_f32();
        match cfg.transport {
            Transport::Channel | Transport::Protocol => {
                let (mut bytes, tm) = gpu.render_readback(frame_id, ts, anim_t);
                let ts_send = Instant::now();
                let mut delivered = false;
                let mut dropped_bp = false;
                let mut bp_override = false;
                let mut dropped_unfetched = false;
                if cfg.transport == Transport::Channel {
                    let in_flight = shared.sent.load(Ordering::Acquire).saturating_sub(shared.ack_calls.load(Ordering::Acquire));
                    let since_ack_ms = (shared.started.elapsed().as_micros() as u64).saturating_sub(shared.last_ack_us.load(Ordering::Acquire)) as f64 / 1e3;
                    if in_flight >= cfg.max_in_flight && since_ack_ms < BACKPRESSURE_TIMEOUT_MS {
                        dropped_bp = true;
                    } else if let Some(ch) = shared.channel.lock().unwrap().as_ref() {
                        bp_override = in_flight >= cfg.max_in_flight;
                        gpu::stamp_send_time(&mut bytes, wall_ms());
                        delivered = ch.send(InvokeResponseBody::Raw(bytes)).is_ok();
                        if delivered {
                            shared.sent.fetch_add(1, Ordering::AcqRel);
                        }
                    }
                } else {
                    gpu::stamp_send_time(&mut bytes, wall_ms());
                    let mut s = shared.slot.lock().unwrap();
                    if s.data.is_some() {
                        dropped_unfetched = true;
                    }
                    s.id = frame_id;
                    s.data = Some(bytes);
                    delivered = true;
                    shared.slot_cv.notify_all();
                }
                let send_ms = ts_send.elapsed().as_secs_f64() * 1e3;
                let mut st = shared.stats.lock().unwrap();
                st.produced += 1;
                if delivered {
                    st.delivered += 1;
                }
                if dropped_bp {
                    st.dropped_backpressure += 1;
                }
                if bp_override {
                    st.backpressure_overrides += 1;
                }
                if dropped_unfetched {
                    st.dropped_unfetched += 1;
                }
                st.encode_ms.push(tm.encode_ms);
                st.gpu_wait_ms.push(tm.gpu_wait_ms);
                st.copy_ms.push(tm.copy_ms);
                st.send_ms.push(send_ms);
                st.frame_total_ms.push(tick.elapsed().as_secs_f64() * 1e3);
            }
            Transport::Native => {
                let (surface, config) = surface_state.as_ref().unwrap();
                let ta = Instant::now();
                let st_tex = match surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
                    _ => None,
                };
                let acquire_ms = ta.elapsed().as_secs_f64() * 1e3;
                let mut ok = false;
                if let Some(tex) = st_tex {
                    let view = tex.texture.create_view(&Default::default());
                    gpu.render_to_view(&view, config.format, frame_id, ts, anim_t);
                    gpu.queue.present(tex);
                    ok = true;
                }
                let lat = wall_ms() - ts;
                let mut st = shared.stats.lock().unwrap();
                st.produced += 1;
                if ok {
                    st.presented += 1;
                    st.present_latency_ms.push(lat);
                } else {
                    st.surface_errors += 1;
                }
                st.acquire_ms.push(acquire_ms);
                st.frame_total_ms.push(tick.elapsed().as_secs_f64() * 1e3);
            }
            Transport::Idle => {}
        }
        next += period;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            shared.stats.lock().unwrap().late_ticks += 1;
            if now - next > period {
                next = now;
            }
        }
    }
}

fn start_producer(shared: &Arc<Shared>) {
    if !shared.producer_started.swap(true, Ordering::SeqCst) {
        let s = shared.clone();
        std::thread::Builder::new().name("producer".into()).spawn(move || producer(s)).unwrap();
    }
}

#[tauri::command]
fn get_config(s: State<'_, Arc<Shared>>) -> ModeCfg {
    s.cfg.clone()
}

#[tauri::command]
fn clock_ping() -> f64 {
    wall_ms()
}

#[tauri::command]
fn start_channel(s: State<'_, Arc<Shared>>, channel: Channel<InvokeResponseBody>) {
    *s.channel.lock().unwrap() = Some(channel);
    start_producer(&s);
}

#[tauri::command]
fn start_stream(s: State<'_, Arc<Shared>>) {
    start_producer(&s);
}

#[tauri::command]
fn ack(s: State<'_, Arc<Shared>>, id: u32) {
    s.acked.fetch_max(id, Ordering::AcqRel);
    s.last_ack_us.store(s.started.elapsed().as_micros() as u64, Ordering::Release);
    s.ack_calls.fetch_add(1, Ordering::AcqRel);
}

#[tauri::command]
fn log(msg: String) {
    eprintln!("[js] {msg}");
}

#[tauri::command]
async fn set_preview_rect(
    window: tauri::WebviewWindow,
    s: State<'_, Arc<Shared>>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    inner_h: f64,
    dpr: f64,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let above = s.cfg.native_above;
        let (tx, rx) = mpsc::channel();
        window
            .with_webview(move |pw| {
                let _ = tx.send(native_mac::attach_native_view(pw.inner(), x, y, w, h, inner_h, dpr, above));
            })
            .map_err(|e| e.to_string())?;
        let attached = rx.recv().map_err(|e| e.to_string())??;
        eprintln!("[native] {}", attached.description);
        *s.native_desc.lock().unwrap() = Some(attached.description.clone());
        *s.native.lock().unwrap() = Some(NativeTarget {
            layer_ptr: attached.layer_ptr,
            width_px: (w * dpr).round() as u32,
            height_px: (h * dpr).round() as u32,
        });
        s.native_cv.notify_all();
        start_producer(&s);
        Ok(attached.description)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, s, x, y, w, h, inner_h, dpr, mpsc::channel::<()>());
        Err("native mode is only implemented on macOS in this spike".into())
    }
}

#[derive(serde::Deserialize)]
struct SnapRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn decode_strip(img: &[u8], stride: usize, bpp: usize, x0: f64, y: f64, block: f64, nbits: u32) -> u32 {
    let mut v: u32 = 0;
    for k in 0..nbits {
        let x = (x0 + (k as f64 + 0.5) * block) as usize;
        let i = y as usize * stride + x * bpp;
        v = (v << 1) | u32::from(img[i] > 127);
    }
    v
}

/// Grabs this app's own window with /usr/sbin/screencapture and checks two
/// things in the composited result: the frame-id/timestamp strips are readable
/// inside the preview rect (the video is really visible) and the HTML play
/// button is white at its CSS position (HTML really draws over the preview).
/// Opt-in (SC_SNAPSHOT=1) and guarded by CGPreflightScreenCaptureAccess, which
/// never prompts: without an existing grant it records "skipped" and moves on.
#[tauri::command]
async fn snapshot(
    window: tauri::WebviewWindow,
    s: State<'_, Arc<Shared>>,
    preview: SnapRect,
    button: SnapRect,
    inner_w: f64,
    inner_h: f64,
) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "macos")]
    {
        if !s.cfg.snapshot {
            return Ok(serde_json::Value::Null);
        }
        if !native_mac::screen_capture_preflight() {
            return Ok(serde_json::json!({ "skipped": "no existing screen-recording grant for this process; not requesting one" }));
        }
        let ptr = window.ns_window().map_err(|e| e.to_string())? as usize;
        let (tx, rx) = mpsc::channel();
        window
            .run_on_main_thread(move || {
                let _ = tx.send(native_mac::window_number(ptr as *mut std::ffi::c_void));
            })
            .map_err(|e| e.to_string())?;
        let wid = rx.recv().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&s.results_dir).map_err(|e| e.to_string())?;
        let png_path = s.results_dir.join(format!("{}.png", s.cfg.name));
        let now_lo = (wall_ms() as u64 & 0xffff_ffff) as u32;
        let st = std::process::Command::new("/usr/sbin/screencapture")
            .args(["-x", "-o", &format!("-l{wid}")])
            .arg(&png_path)
            .status()
            .map_err(|e| e.to_string())?;
        if !st.success() || !png_path.exists() {
            return Ok(serde_json::json!({ "error": format!("screencapture failed: {st}") }));
        }
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&png_path).map_err(|e| e.to_string())?));
        let mut reader = dec.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        if info.bit_depth != png::BitDepth::Eight {
            return Ok(serde_json::json!({ "error": format!("unexpected png bit depth {:?}", info.bit_depth) }));
        }
        let bpp = match info.color_type {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            c => return Ok(serde_json::json!({ "error": format!("unexpected png color type {c:?}") })),
        };
        let (iw, ih, stride) = (info.width as f64, info.height as f64, info.line_size);
        let scale = iw / inner_w;
        if scale < 1.0 || ih < inner_h * scale {
            return Ok(serde_json::json!({ "error": format!("degenerate window capture {}x{}", info.width, info.height) }));
        }
        let top = ih - inner_h * scale;
        let (px, py, pw) = (preview.x * scale, top + preview.y * scale, preview.w * scale);
        let block = 40.0 * pw / 1920.0;
        let id = decode_strip(&buf, stride, bpp, px, py + block * 0.5, block, 24);
        let ts_lo = decode_strip(&buf, stride, bpp, px, py + block * 1.5, block, 32);
        let ts_age_ms = now_lo.wrapping_sub(ts_lo) as i32 as i64;
        let (bx, by) = ((button.x + button.w / 2.0) * scale, top + (button.y + button.h / 2.0) * scale);
        let bi = by as usize * stride + bx as usize * bpp;
        let rgb = [buf[bi], buf[bi + 1], buf[bi + 2]];
        let preview_visible = id > 0 && (-5_000..60_000).contains(&ts_age_ms);
        let html_over_preview = rgb.iter().all(|&c| c > 200);
        Ok(serde_json::json!({
            "png": png_path.display().to_string(),
            "image_px": [info.width, info.height],
            "scale": scale,
            "titlebar_px": top,
            "decoded_frame_id": id,
            "decoded_ts_age_ms": ts_age_ms,
            "preview_visible": preview_visible,
            "button_center_rgb": rgb,
            "html_over_preview": html_over_preview,
        }))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, s, preview, button, inner_w, inner_h);
        Ok(serde_json::Value::Null)
    }
}

#[tauri::command]
async fn measure_begin(s: State<'_, Arc<Shared>>) -> Result<(), String> {
    let procs = procstats::discover(s.started.elapsed().as_secs_f64());
    let snap = procstats::sample(&procs);
    *s.stats.lock().unwrap() = PStats::default();
    *s.begin.lock().unwrap() = Some((Instant::now(), snap, procs));
    s.measuring.store(true, Ordering::SeqCst);
    let sh = s.inner().clone();
    std::thread::spawn(move || {
        while sh.measuring.load(Ordering::SeqCst) {
            if let Some(u) = procstats::gpu_utilization() {
                sh.gpu_util.lock().unwrap().push(u);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    Ok(())
}

fn summarize(v: &[f64]) -> serde_json::Value {
    if v.is_empty() {
        return serde_json::Value::Null;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| s[((s.len() - 1) as f64 * p).round() as usize];
    serde_json::json!({
        "n": s.len(),
        "mean": s.iter().sum::<f64>() / s.len() as f64,
        "p50": pct(0.5),
        "p95": pct(0.95),
        "p99": pct(0.99),
        "max": s[s.len() - 1],
    })
}

fn os_version() -> String {
    std::process::Command::new("sw_vers")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

fn write_result(shared: &Shared, js: serde_json::Value, error: Option<String>) -> Result<PathBuf, String> {
    shared.measuring.store(false, Ordering::SeqCst);
    let end_t = Instant::now();
    let begin = shared.begin.lock().unwrap().take();
    let mut procs_out = vec![];
    let mut window_s = 0.0;
    let mut total_cpu = 0.0;
    let mut total_mem = 0.0;
    if let Some((t0, snap0, procs)) = begin {
        window_s = (end_t - t0).as_secs_f64();
        let snap1 = procstats::sample(&procs);
        for p1 in &snap1 {
            if let Some(p0) = snap0.iter().find(|p| p.pid == p1.pid) {
                let cpu_pct = (p1.cpu_s - p0.cpu_s) / window_s * 100.0;
                let mem = p1.footprint_mb.unwrap_or(p1.rss_mb);
                if !p1.role.starts_with("windowserver") {
                    total_cpu += cpu_pct;
                    total_mem += mem;
                }
                procs_out.push(serde_json::json!({
                    "pid": p1.pid, "role": p1.role, "cpu_pct": cpu_pct,
                    "rss_mb": p1.rss_mb, "footprint_mb": p1.footprint_mb,
                }));
            }
        }
    }
    let st = shared.stats.lock().unwrap().clone();
    let gpu_util = shared.gpu_util.lock().unwrap().clone();
    let result = serde_json::json!({
        "mode": shared.cfg,
        "error": error,
        "timestamp_ms": wall_ms(),
        "os": os_version(),
        "tauri": tauri::VERSION,
        "adapter": shared.adapter.lock().unwrap().clone(),
        "native_view": shared.native_desc.lock().unwrap().clone(),
        "measure_window_s": window_s,
        "processes": procs_out,
        "app_plus_webkit_cpu_pct": total_cpu,
        "app_plus_webkit_mem_mb": total_mem,
        "gpu_util_pct": summarize(&gpu_util),
        "producer": {
            "produced": st.produced,
            "delivered": st.delivered,
            "dropped_backpressure": st.dropped_backpressure,
            "backpressure_overrides": st.backpressure_overrides,
            "sent": shared.sent.load(Ordering::Relaxed),
            "dropped_unfetched": st.dropped_unfetched,
            "late_ticks": st.late_ticks,
            "acked_max_id": shared.acked.load(Ordering::Relaxed),
            "ack_calls_total": shared.ack_calls.load(Ordering::Relaxed),
            "presented": st.presented,
            "surface_errors": st.surface_errors,
            "produced_fps": st.produced as f64 / window_s.max(1e-9),
            "presented_fps": st.presented as f64 / window_s.max(1e-9),
            "encode_ms": summarize(&st.encode_ms),
            "gpu_wait_ms": summarize(&st.gpu_wait_ms),
            "copy_ms": summarize(&st.copy_ms),
            "send_ms": summarize(&st.send_ms),
            "frame_total_ms": summarize(&st.frame_total_ms),
            "acquire_ms": summarize(&st.acquire_ms),
            "present_latency_ms": summarize(&st.present_latency_ms),
        },
        "js": js,
    });
    std::fs::create_dir_all(&shared.results_dir).map_err(|e| e.to_string())?;
    let path = shared.results_dir.join(format!("{}.json", shared.cfg.name));
    std::fs::write(&path, serde_json::to_string_pretty(&result).unwrap()).map_err(|e| e.to_string())?;
    Ok(path)
}

#[tauri::command]
async fn report(app: tauri::AppHandle, s: State<'_, Arc<Shared>>, js: serde_json::Value) -> Result<String, String> {
    let path = write_result(&s, js, None)?;
    s.done.store(true, Ordering::SeqCst);
    eprintln!("[bench] wrote {}", path.display());
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        app.exit(0);
    });
    Ok(path.display().to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg_mode = args.iter().position(|a| a == "--mode").and_then(|i| args.get(i + 1).cloned());
    let env = |k: &str| std::env::var(k).ok();
    let mode_name = arg_mode.or_else(|| env("SC_MODE")).unwrap_or_else(|| "a-gl-540".into());
    let fps: u32 = env("SC_FPS").and_then(|v| v.parse().ok()).unwrap_or(60);
    let duration_s: f64 = env("SC_DURATION").and_then(|v| v.parse().ok()).unwrap_or(10.0);
    let warmup_s: f64 = env("SC_WARMUP").and_then(|v| v.parse().ok()).unwrap_or(3.0);
    let max_in_flight: u32 = env("SC_MAX_IN_FLIGHT").and_then(|v| v.parse().ok()).unwrap_or(2);
    let results_dir = PathBuf::from(env("SC_RESULTS").unwrap_or_else(|| "results".into()));
    let want_snapshot = env("SC_SNAPSHOT").as_deref() == Some("1");
    let cfg = parse_mode(&mode_name, fps, duration_s, warmup_s, max_in_flight, want_snapshot).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });

    let shared = Arc::new(Shared {
        cfg: cfg.clone(),
        results_dir,
        started: Instant::now(),
        producer_started: AtomicBool::new(false),
        channel: Mutex::new(None),
        acked: AtomicU32::new(0),
        ack_calls: AtomicU32::new(0),
        sent: AtomicU32::new(0),
        last_ack_us: AtomicU64::new(0),
        slot: Mutex::new(Slot { id: 0, data: None }),
        slot_cv: Condvar::new(),
        native: Mutex::new(None),
        native_cv: Condvar::new(),
        native_desc: Mutex::new(None),
        stats: Mutex::new(PStats::default()),
        adapter: Mutex::new(None),
        begin: Mutex::new(None),
        measuring: AtomicBool::new(false),
        gpu_util: Mutex::new(vec![]),
        done: AtomicBool::new(false),
    });

    {
        let sh = shared.clone();
        let limit = cfg.warmup_s + cfg.duration_s + 45.0;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs_f64(limit));
            let _ = write_result(&sh, serde_json::Value::Null, Some("timeout: JS never reported".into()));
            eprintln!("[bench] watchdog timeout");
            std::process::exit(3);
        });
    }

    let (req_tx, req_rx) = mpsc::channel::<(u32, tauri::UriSchemeResponder)>();
    {
        let sh = shared.clone();
        std::thread::Builder::new()
            .name("frames-protocol".into())
            .spawn(move || {
                for (after, responder) in req_rx {
                    let deadline = Instant::now() + Duration::from_millis(250);
                    let mut g = sh.slot.lock().unwrap();
                    while !(g.id > after && g.data.is_some()) {
                        let now = Instant::now();
                        if now >= deadline {
                            break;
                        }
                        g = sh.slot_cv.wait_timeout(g, deadline - now).unwrap().0;
                    }
                    let body = if g.id > after { g.data.take() } else { None };
                    drop(g);
                    let resp = match body {
                        Some(b) => tauri::http::Response::builder()
                            .status(200)
                            .header("Content-Type", "application/octet-stream")
                            .header("Access-Control-Allow-Origin", "*")
                            .header("Cache-Control", "no-store")
                            .body(b)
                            .unwrap(),
                        None => tauri::http::Response::builder()
                            .status(204)
                            .header("Access-Control-Allow-Origin", "*")
                            .body(Vec::new())
                            .unwrap(),
                    };
                    responder.respond(resp);
                }
            })
            .unwrap();
    }
    let req_tx = Mutex::new(req_tx);

    tauri::Builder::default()
        .manage(shared.clone())
        .register_asynchronous_uri_scheme_protocol("frames", move |_ctx, request, responder| {
            let after = request
                .uri()
                .query()
                .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("after=")))
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0);
            let _ = req_tx.lock().unwrap().send((after, responder));
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            clock_ping,
            start_channel,
            start_stream,
            ack,
            log,
            set_preview_rect,
            measure_begin,
            snapshot,
            report
        ])
        .setup(move |app| {
            let title = format!("Preview spike: {}", cfg.name);
            let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title(title)
                .inner_size(1360.0, 820.0)
                .resizable(false)
                .always_on_top(true)
                .focused(true)
                .center()
                .background_color(tauri::window::Color(24, 24, 28, 255));
            builder.build()?;
            Ok(())
        })
        .on_window_event(|_w, e| {
            if let tauri::WindowEvent::CloseRequested { .. } = e {
                eprintln!("[bench] window close requested before the run finished");
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, e| {
            if let tauri::RunEvent::Exit = e {
                eprintln!("[bench] event loop exit");
            }
        });
}
