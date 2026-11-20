use std::borrow::Cow;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, XlibDisplayHandle,
    XlibWindowHandle,
};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::platform::unix::WindowExtUnix;
use tao::window::WindowBuilder;
use wry::http::{Request, Response};
use wry::{WebViewBuilder, WebViewBuilderExtUnix};

use crate::gpu;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    GlArea,
    Cairo,
    Subsurface,
    X11Child,
    Pull,
}

struct NativeTarget {
    display: RawDisplayHandle,
    window: RawWindowHandle,
    w: u32,
    h: u32,
}
unsafe impl Send for NativeTarget {}

struct Shared {
    latest: Mutex<Option<Arc<Vec<u8>>>>,
    produced: AtomicU64,
    presented: AtomicU64,
    gl_renders: AtomicU64,
    gl_uploads: AtomicU64,
    served: AtomicU64,
    done: AtomicBool,
    native: Mutex<Option<NativeTarget>>,
    native_cv: Condvar,
    adapter: Mutex<String>,
}

extern "C" {
    fn gdk_wayland_display_get_wl_display(d: *mut gtk::gdk::ffi::GdkDisplay) -> *mut std::ffi::c_void;
    fn gdk_wayland_window_get_wl_surface(w: *mut gtk::gdk::ffi::GdkWindow) -> *mut std::ffi::c_void;
    fn gdk_x11_display_get_xdisplay(d: *mut gtk::gdk::ffi::GdkDisplay) -> *mut std::ffi::c_void;
    fn gdk_x11_window_get_xid(w: *mut gtk::gdk::ffi::GdkWindow) -> std::ffi::c_ulong;
}

fn env_u32(k: &str, d: u32) -> u32 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn log(msg: &str) {
    println!("[spike06] {msg}");
}

pub fn run() {
    let mode = match std::env::var("SC_MODE").as_deref() {
        Ok("glarea") => Mode::GlArea,
        Ok("cairo") => Mode::Cairo,
        Ok("subsurface") => Mode::Subsurface,
        Ok("x11child") => Mode::X11Child,
        Ok("pull") => Mode::Pull,
        other => panic!("SC_MODE must be glarea|subsurface|x11child|pull, got {other:?}"),
    };
    let seconds = env_u32("SC_SECONDS", 12);
    let rw = env_u32("SC_W", 1280);
    let rh = env_u32("SC_H", 720);

    let shared = Arc::new(Shared {
        latest: Mutex::new(None),
        produced: AtomicU64::new(0),
        presented: AtomicU64::new(0),
        gl_renders: AtomicU64::new(0),
        gl_uploads: AtomicU64::new(0),
        served: AtomicU64::new(0),
        done: AtomicBool::new(false),
        native: Mutex::new(None),
        native_cv: Condvar::new(),
        adapter: Mutex::new(String::new()),
    });

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title(format!("spike06 {mode:?}"))
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_transparent(true)
        .build(&event_loop)
        .expect("window");

    let gdk_display = gtk::gdk::Display::default().expect("gdk display");
    let backend = if gdk_display.type_().name() == "GdkWaylandDisplay" { "wayland" } else { "x11" };
    log(&format!("mode={mode:?} gdk_backend={backend} readback={rw}x{rh}"));
    log(&format!(
        "env WEBKIT_DISABLE_DMABUF_RENDERER={:?} WEBKIT_DISABLE_COMPOSITING_MODE={:?}",
        std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").ok(),
        std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").ok()
    ));

    let vbox = window.default_vbox().expect("default vbox");
    let overlay = gtk::Overlay::new();
    vbox.pack_start(&overlay, true, true, 0);
    let webbox = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let mut bottom: Option<gtk::Widget> = None;
    match mode {
        Mode::GlArea => {
            let area = gtk::GLArea::new();
            area.set_has_alpha(false);
            setup_glarea(&area, shared.clone(), backend == "wayland");
            bottom = Some(area.upcast());
        }
        Mode::Cairo => {
            let da = gtk::DrawingArea::new();
            setup_cairo(&da, shared.clone());
            bottom = Some(da.upcast());
        }
        Mode::Subsurface | Mode::X11Child => {
            let da = gtk::DrawingArea::new();
            let m = env_u32("SC_MARGIN", 0) as i32;
            da.set_margin_start(m);
            da.set_margin_end(m);
            da.set_margin_top(m);
            da.set_margin_bottom(m);
            bottom = Some(da.upcast());
        }
        Mode::Pull => {}
    }
    match &bottom {
        Some(b) => {
            overlay.add(b);
            overlay.add_overlay(&webbox);
        }
        None => overlay.add(&webbox),
    }
    overlay.show_all();

    let proto_shared = shared.clone();
    let page = include_str!("page.html").replace("__MODE__", &format!("{mode:?}"));
    let webview = WebViewBuilder::new()
        .with_transparent(true)
        .with_custom_protocol("sc".into(), move |_id, req: Request<Vec<u8>>| {
            let path = req.uri().path().to_string();
            if path == "/frame" {
                let f = proto_shared.latest.lock().unwrap().clone();
                proto_shared.served.fetch_add(1, Ordering::Relaxed);
                match f {
                    Some(b) => Response::builder()
                        .header("Content-Type", "application/octet-stream")
                        .header("Cache-Control", "no-store")
                        .body(Cow::Owned(b.as_ref().clone()))
                        .unwrap(),
                    None => Response::builder().status(204).body(Cow::Borrowed(&[][..])).unwrap(),
                }
            } else {
                Response::builder()
                    .header("Content-Type", "text/html")
                    .body(Cow::Owned(page.clone().into_bytes()))
                    .unwrap()
            }
        })
        .with_initialization_script(
            "if (location.protocol === 'webkit:') setTimeout(() => { for (const b of document.querySelectorAll('button')) if (b.textContent.includes('stdout')) b.click(); }, 2000);",
        )
        .with_ipc_handler(|req: Request<String>| println!("[js] {}", req.body()))
        .with_url(std::env::var("SC_URL").unwrap_or_else(|_| "sc://localhost/index.html".into()))
        .build_gtk(&webbox)
        .expect("webview");

    if std::env::var("SC_HIDE_WEB").is_ok_and(|v| !v.is_empty()) {
        webbox.hide();
        log("webview hidden (SC_HIDE_WEB)");
    }
    let gtk_window = window.gtk_window().clone();
    if let Some(b) = bottom.clone() {
        if mode == Mode::Subsurface || mode == Mode::X11Child {
            let sh = shared.clone();
            let gw = gtk_window.clone();
            let disp = gdk_display.clone();
            glib::timeout_add_local_once(Duration::from_millis(800), move || {
                let t = match mode {
                    Mode::Subsurface => make_subsurface(&disp, &gw, &b),
                    _ => make_x11_child(&disp, &b),
                };
                match t {
                    Ok(t) => {
                        log(&format!("native target {}x{} px", t.w, t.h));
                        *sh.native.lock().unwrap() = Some(t);
                        sh.native_cv.notify_all();
                    }
                    Err(e) => log(&format!("native target FAILED: {e}")),
                }
                gw.queue_draw();
            });
        }
    }

    {
        let sh = shared.clone();
        std::thread::spawn(move || producer(sh, mode, rw, rh));
    }

    {
        let sh = shared.clone();
        let start = Instant::now();
        let mut last = (0u64, 0u64, 0u64, 0u64, 0u64);
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let cur = (
                sh.produced.load(Ordering::Relaxed),
                sh.presented.load(Ordering::Relaxed),
                sh.gl_renders.load(Ordering::Relaxed),
                sh.gl_uploads.load(Ordering::Relaxed),
                sh.served.load(Ordering::Relaxed),
            );
            log(&format!(
                "t={:.0}s produced/s={} presented/s={} gl_renders/s={} gl_uploads/s={} frames_served/s={} adapter={}",
                start.elapsed().as_secs_f64(),
                cur.0 - last.0,
                cur.1 - last.1,
                cur.2 - last.2,
                cur.3 - last.3,
                cur.4 - last.4,
                sh.adapter.lock().unwrap()
            ));
            last = cur;
            if start.elapsed().as_secs() >= seconds as u64 {
                sh.done.store(true, Ordering::Relaxed);
                log("done");
                std::process::exit(0);
            }
            glib::ControlFlow::Continue
        });
    }

    let _keep = webview;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *control_flow = ControlFlow::Exit;
        }
    });
}

fn widget_rect_px(w: &gtk::Widget, top: &gtk::Widget) -> (i32, i32, u32, u32, i32) {
    let (x, y) = w.translate_coordinates(top, 0, 0).unwrap_or((0, 0));
    let s = w.scale_factor();
    (x, y, (w.allocated_width() * s) as u32, (w.allocated_height() * s) as u32, s)
}

fn make_x11_child(disp: &gtk::gdk::Display, b: &gtk::Widget) -> Result<NativeTarget, String> {
    let gw = b.window().ok_or("bottom widget has no GdkWindow")?;
    let native = gw.ensure_native();
    let xid = unsafe { gdk_x11_window_get_xid(gw.to_glib_none().0) };
    let xdpy = unsafe { gdk_x11_display_get_xdisplay(disp.to_glib_none().0) };
    let s = b.scale_factor();
    log(&format!("x11 child: ensure_native={native} xid=0x{xid:x}"));
    Ok(NativeTarget {
        display: RawDisplayHandle::Xlib(XlibDisplayHandle::new(NonNull::new(xdpy), 0)),
        window: RawWindowHandle::Xlib(XlibWindowHandle::new(xid)),
        w: (b.allocated_width() * s) as u32,
        h: (b.allocated_height() * s) as u32,
    })
}

mod wl {
    use wayland_client::protocol::{wl_compositor, wl_region, wl_registry, wl_subcompositor, wl_subsurface, wl_surface};
    use wayland_client::{Connection, Dispatch, QueueHandle};
    pub struct St;
    impl Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents> for St {
        fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &wayland_client::globals::GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
    }
    macro_rules! noop {
        ($($t:ty),*) => {$(
            impl Dispatch<$t, ()> for St {
                fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
            }
        )*};
    }
    noop!(wl_compositor::WlCompositor, wl_subcompositor::WlSubcompositor, wl_surface::WlSurface, wl_subsurface::WlSubsurface, wl_region::WlRegion);
}

fn make_subsurface(disp: &gtk::gdk::Display, top: &gtk::ApplicationWindow, b: &gtk::Widget) -> Result<NativeTarget, String> {
    use wayland_backend::client::{Backend, ObjectId};
    use wayland_client::protocol::{wl_compositor::WlCompositor, wl_subcompositor::WlSubcompositor, wl_surface::WlSurface};
    use wayland_client::{globals::registry_queue_init, Connection, Proxy};

    let gdk_top = top.window().ok_or("toplevel has no GdkWindow")?;
    let wl_display = unsafe { gdk_wayland_display_get_wl_display(disp.to_glib_none().0) };
    let parent_ptr = unsafe { gdk_wayland_window_get_wl_surface(gdk_top.to_glib_none().0) };
    if wl_display.is_null() || parent_ptr.is_null() {
        return Err("no wl_display / wl_surface (not Wayland?)".into());
    }
    let backend = unsafe { Backend::from_foreign_display(wl_display as _) };
    let conn = Connection::from_backend(backend);
    let (globals, mut queue) = registry_queue_init::<wl::St>(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let compositor: WlCompositor = globals.bind(&qh, 1..=4, ()).map_err(|e| e.to_string())?;
    let subcomp: WlSubcompositor = globals.bind(&qh, 1..=1, ()).map_err(|e| e.to_string())?;
    let parent_id = unsafe { ObjectId::from_ptr(WlSurface::interface(), parent_ptr as _) }.map_err(|e| e.to_string())?;
    let parent = WlSurface::from_id(&conn, parent_id).map_err(|e| e.to_string())?;
    let child = compositor.create_surface(&qh, ());
    let sub = subcomp.get_subsurface(&child, &parent, &qh, ());
    let (x, y, w, h, s) = widget_rect_px(b, top.upcast_ref());
    sub.set_position(x, y);
    sub.place_below(&parent);
    sub.set_desync();
    let empty = compositor.create_region(&qh, ());
    child.set_input_region(Some(&empty));
    child.set_buffer_scale(s);
    child.commit();
    queue.roundtrip(&mut wl::St).map_err(|e| e.to_string())?;
    log(&format!("wayland subsurface at logical ({x},{y}) size {w}x{h}px scale {s}, placed below parent"));
    let child_ptr = child.id().as_ptr();
    std::mem::forget((sub, child, empty, compositor, subcomp, parent, queue, conn, globals));
    Ok(NativeTarget {
        display: RawDisplayHandle::Wayland(WaylandDisplayHandle::new(NonNull::new(wl_display).unwrap())),
        window: RawWindowHandle::Wayland(WaylandWindowHandle::new(NonNull::new(child_ptr as *mut std::ffi::c_void).unwrap())),
        w,
        h,
    })
}

fn producer(shared: Arc<Shared>, mode: Mode, rw: u32, rh: u32) {
    let backends = match std::env::var("SC_WGPU_BACKEND").as_deref() {
        Ok("gl") => wgpu::Backends::GL,
        _ => wgpu::Backends::VULKAN,
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let native = mode == Mode::Subsurface || mode == Mode::X11Child;
    let mut surface_state = None;
    let mut g = if native {
        let t = {
            let mut g = shared.native.lock().unwrap();
            while g.is_none() {
                g = shared.native_cv.wait(g).unwrap();
            }
            g.take().unwrap()
        };
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(t.display),
                raw_window_handle: t.window,
            })
        }
        .expect("create surface");
        let g = gpu::Gpu::new(&instance, Some(&surface));
        let caps = surface.get_capabilities(&g.adapter);
        log(&format!("surface caps formats={:?} present={:?} alpha={:?}", caps.formats, caps.present_modes, caps.alpha_modes));
        let format = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm]
            .into_iter()
            .find(|f| caps.formats.contains(f))
            .unwrap_or(caps.formats[0]);
        let present_mode = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Fifo]
            .into_iter()
            .find(|p| caps.present_modes.contains(p))
            .unwrap_or(caps.present_modes[0]);
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes[0]
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: t.w.max(1),
            height: t.h.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&g.device, &config);
        log(&format!("surface configured {:?} {:?} {:?}", format, present_mode, alpha_mode));
        surface_state = Some((surface, config));
        g
    } else {
        let mut g = gpu::Gpu::new(&instance, None);
        g.set_readback_size(rw, rh);
        g
    };
    *shared.adapter.lock().unwrap() = format!("{:?}/{}/{:?}", g.adapter_info.backend, g.adapter_info.name, g.adapter_info.device_type);

    let period = Duration::from_secs_f64(1.0 / 60.0);
    let t0 = Instant::now();
    let mut next = Instant::now();
    let mut id = 0u32;
    while !shared.done.load(Ordering::Relaxed) {
        id += 1;
        let t = t0.elapsed().as_secs_f32();
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64() * 1e3;
        if let Some((surface, config)) = surface_state.as_ref() {
            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(tex) | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => {
                    let view = tex.texture.create_view(&Default::default());
                    g.render_to_view(&view, config.format, id, ts, t);
                    g.queue.present(tex);
                    shared.presented.fetch_add(1, Ordering::Relaxed);
                }
                other => log(&format!("acquire failed: {:?}", std::mem::discriminant(&other))),
            }
        } else {
            let (bytes, _) = g.render_readback(id, ts, t);
            *shared.latest.lock().unwrap() = Some(Arc::new(bytes));
        }
        shared.produced.fetch_add(1, Ordering::Relaxed);
        next += period;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else if now - next > period {
            next = now;
        }
    }
}

struct GlState {
    gl: glow::Context,
    program: glow::Program,
    vao: glow::VertexArray,
    tex: glow::Texture,
    tex_size: (i32, i32),
    last_id: u32,
}

fn gl_loader(wayland: bool) -> glow::Context {
    let lib = if wayland { "libEGL.so.1" } else { "libGL.so.1" };
    let sym: &[u8] = if wayland { b"eglGetProcAddress\0" } else { b"glXGetProcAddress\0" };
    let lib = unsafe { libloading::Library::new(lib) }.expect("load GL loader lib");
    let get: libloading::Symbol<unsafe extern "C" fn(*const std::ffi::c_char) -> *const std::ffi::c_void> =
        unsafe { lib.get(sym) }.expect("get proc address symbol");
    let get = *get;
    std::mem::forget(lib);
    unsafe {
        glow::Context::from_loader_function(|s| {
            let c = std::ffi::CString::new(s).unwrap();
            get(c.as_ptr())
        })
    }
}

fn setup_glarea(area: &gtk::GLArea, shared: Arc<Shared>, wayland: bool) {
    use glow::HasContext;
    let state: std::rc::Rc<std::cell::RefCell<Option<GlState>>> = Default::default();
    let st = state.clone();
    area.connect_realize(move |a| {
        a.make_current();
        if let Some(e) = a.error() {
            log(&format!("GLArea error: {e}"));
            return;
        }
        let ctx = a.context().unwrap();
        let (maj, min) = ctx.version();
        log(&format!("GLArea context GL {maj}.{min} use_es={}", ctx.uses_es()));
        let gl = gl_loader(wayland);
        unsafe {
            let ver = if ctx.uses_es() { "#version 300 es\nprecision mediump float;\n" } else { "#version 330 core\n" };
            let vs_src = format!(
                "{ver}out vec2 uv; void main(){{ vec2 p = vec2(float((gl_VertexID<<1)&2), float(gl_VertexID&2)); uv = vec2(p.x, p.y); gl_Position = vec4(p.x*2.0-1.0, 1.0-p.y*2.0, 0.0, 1.0); }}"
            );
            let fs_src = format!("{ver}in vec2 uv; uniform sampler2D t; out vec4 c; void main(){{ c = vec4(texture(t, uv).rgb, 1.0); }}");
            let program = gl.create_program().unwrap();
            for (kind, src) in [(glow::VERTEX_SHADER, vs_src), (glow::FRAGMENT_SHADER, fs_src)] {
                let s = gl.create_shader(kind).unwrap();
                gl.shader_source(s, &src);
                gl.compile_shader(s);
                if !gl.get_shader_compile_status(s) {
                    panic!("{}", gl.get_shader_info_log(s));
                }
                gl.attach_shader(program, s);
            }
            gl.link_program(program);
            assert!(gl.get_program_link_status(program), "{}", gl.get_program_info_log(program));
            let vao = gl.create_vertex_array().unwrap();
            let tex = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            *st.borrow_mut() = Some(GlState { gl, program, vao, tex, tex_size: (0, 0), last_id: 0 });
        }
    });
    let st = state.clone();
    let sh = shared.clone();
    area.connect_render(move |_a, _ctx| {
        let mut b = st.borrow_mut();
        let Some(s) = b.as_mut() else { return glib::Propagation::Proceed };
        sh.gl_renders.fetch_add(1, Ordering::Relaxed);
        let frame = sh.latest.lock().unwrap().clone();
        unsafe {
            s.gl.bind_texture(glow::TEXTURE_2D, Some(s.tex));
            if let Some(f) = frame {
                let id = u32::from_le_bytes(f[4..8].try_into().unwrap());
                let w = u32::from_le_bytes(f[8..12].try_into().unwrap()) as i32;
                let h = u32::from_le_bytes(f[12..16].try_into().unwrap()) as i32;
                if id != s.last_id {
                    let px = &f[gpu::HEADER_LEN..];
                    if s.tex_size != (w, h) {
                        s.gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w, h, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(px)));
                        s.tex_size = (w, h);
                    } else {
                        s.gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, w, h, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(px)));
                    }
                    s.last_id = id;
                    sh.gl_uploads.fetch_add(1, Ordering::Relaxed);
                }
            }
            s.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            s.gl.clear(glow::COLOR_BUFFER_BIT);
            if s.tex_size.0 > 0 {
                s.gl.use_program(Some(s.program));
                s.gl.bind_vertex_array(Some(s.vao));
                s.gl.draw_arrays(glow::TRIANGLES, 0, 3);
            }
        }
        glib::Propagation::Stop
    });
    let a = area.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        a.queue_render();
        glib::ControlFlow::Continue
    });
}

fn setup_cairo(da: &gtk::DrawingArea, shared: Arc<Shared>) {
    use gtk::cairo;
    let surf: std::rc::Rc<std::cell::RefCell<Option<(cairo::ImageSurface, u32)>>> = Default::default();
    da.connect_draw(move |w, cr| {
        let f = shared.latest.lock().unwrap().clone();
        let Some(f) = f else { return glib::Propagation::Stop };
        let id = u32::from_le_bytes(f[4..8].try_into().unwrap());
        let fw = u32::from_le_bytes(f[8..12].try_into().unwrap()) as i32;
        let fh = u32::from_le_bytes(f[12..16].try_into().unwrap()) as i32;
        let mut slot = surf.borrow_mut();
        if slot.as_ref().map(|(s, _)| (s.width(), s.height())) != Some((fw, fh)) {
            *slot = Some((cairo::ImageSurface::create(cairo::Format::Rgb24, fw, fh).unwrap(), 0));
        }
        let (s, last) = slot.as_mut().unwrap();
        if *last != id {
            let stride = s.stride() as usize;
            {
                let mut d = s.data().unwrap();
                let px = &f[gpu::HEADER_LEN..];
                for y in 0..fh as usize {
                    let src = &px[y * fw as usize * 4..(y + 1) * fw as usize * 4];
                    let dst = &mut d[y * stride..y * stride + fw as usize * 4];
                    for (o, i) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                        o[0] = i[2];
                        o[1] = i[1];
                        o[2] = i[0];
                        o[3] = 255;
                    }
                }
            }
            *last = id;
            shared.gl_uploads.fetch_add(1, Ordering::Relaxed);
        }
        shared.gl_renders.fetch_add(1, Ordering::Relaxed);
        let (ww, wh) = (w.allocated_width() as f64, w.allocated_height() as f64);
        cr.scale(ww / fw as f64, wh / fh as f64);
        cr.set_source_surface(&*s, 0.0, 0.0).unwrap();
        cr.paint().unwrap();
        glib::Propagation::Stop
    });
    let d = da.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        d.queue_draw();
        glib::ControlFlow::Continue
    });
}
