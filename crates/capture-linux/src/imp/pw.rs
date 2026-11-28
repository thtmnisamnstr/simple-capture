//! A PipeWire video stream on a portal remote: shared-memory BGRx/RGBx frames converted to NV12
//! at the size the stream started with (letterboxed when a window is resized).

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use pipewire as pw;
use pw::spa;
use pw::spa::pod::Pod;
use sc_geometry::SizePx;
use sc_platform::{
    Error, FrameBuffer, FrameSink, Result, ScreenStream, SourceEvent, StreamTag, VideoFrame,
    WindowId,
};
use sc_tracks::nv12::{PackedOrder, fit_packed, packed_to_nv12_par};

use super::sys::host_now;

struct Format {
    size: SizePx,
    order: Option<PackedOrder>,
}

/// A read-only mapping of `size` bytes at `offset` in a shared-memory fd.
struct ReadMap {
    base: *mut libc::c_void,
    len: usize,
    start: usize,
    size: usize,
}

impl ReadMap {
    fn new(fd: i64, offset: u32, size: u32) -> Option<Self> {
        let fd = i32::try_from(fd).ok().filter(|fd| *fd >= 0)?;
        let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
        let offset = offset as usize;
        let size = size as usize;
        if size == 0 {
            return None;
        }
        let aligned = offset - offset % page.max(1);
        let start = offset - aligned;
        let len = start.checked_add(size)?;
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                libc::off_t::try_from(aligned).ok()?,
            )
        };
        (base != libc::MAP_FAILED).then_some(Self {
            base,
            len,
            start,
            size,
        })
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: `base..base + len` stays mapped readable until Drop, and start + size == len.
        unsafe { std::slice::from_raw_parts(self.base.cast::<u8>().add(self.start), self.size) }
    }
}

impl Drop for ReadMap {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.base, self.len) };
    }
}

fn order_of(f: spa::param::video::VideoFormat) -> Option<PackedOrder> {
    use spa::param::video::VideoFormat as F;
    match f {
        F::BGRx | F::BGRA => Some(PackedOrder::Bgra),
        F::RGBx | F::RGBA => Some(PackedOrder::Rgba),
        _ => None,
    }
}

fn format_params() -> std::result::Result<Vec<u8>, String> {
    use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
    use spa::param::video::VideoFormat;
    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
        spa::pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        spa::pod::property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA
        ),
        spa::pod::property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 30, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 240, denom: 1 }
        ),
    );
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map(|(c, _)| c.into_inner())
    .map_err(|e| format!("{e:?}"))
}

pub struct PwStream {
    tag: StreamTag,
    id: u64,
    conn: Arc<Connection>,
}

impl ScreenStream for PwStream {
    fn tag(&self) -> StreamTag {
        self.tag
    }

    fn update_exclusions(&self, _exclude: &[WindowId]) -> Result<()> {
        Ok(())
    }

    fn stop(self: Box<Self>) -> Result<()> {
        self.conn.stop(self.id);
        Ok(())
    }
}

type Reply = mpsc::Sender<std::result::Result<(), String>>;

enum Cmd {
    Probe {
        node: u32,
        reply: mpsc::Sender<SizePx>,
    },
    Start {
        id: u64,
        node: u32,
        tag: StreamTag,
        out: SizePx,
        fps: u32,
        sink: Arc<dyn FrameSink>,
        reply: Reply,
    },
    Stop {
        id: u64,
        reply: mpsc::Sender<()>,
    },
    Quit,
}

/// One PipeWire connection, on its own thread, that screen streams start and stop on. A portal
/// pick keeps one for as long as it lives: opening a portal remote can stall
/// xdg-desktop-portal for 25 s at a time (its new PipeWire context asks the portal's own
/// Realtime interface while the portal waits for it), so a pick asks for one remote only.
pub struct Connection {
    cmds: pw::channel::Sender<Cmd>,
    next: AtomicU64,
    alive: Arc<AtomicBool>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Connection {
    /// Connects through a portal remote, or to the user's own daemon with None.
    pub fn open(fd: Option<OwnedFd>) -> Result<Arc<Connection>> {
        let (cmds, rx) = pw::channel::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        let thread = std::thread::Builder::new()
            .name("pipewire-remote".into())
            .spawn({
                let alive = alive.clone();
                move || serve(fd, rx, ready_tx, alive)
            })?;
        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Arc::new(Connection {
                cmds,
                next: AtomicU64::new(1),
                alive,
                thread: Mutex::new(Some(thread)),
            })),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(Error::Other(e))
            }
            Err(_) => Err(Error::Timeout("PipeWire did not answer".into())),
        }
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// The size the node's stream negotiates: the source's real pixels (the portal reports the
    /// logical size, which on a scaled screen is smaller than what the compositor sends).
    pub fn probe(&self, node: u32) -> Option<SizePx> {
        let (reply, rx) = mpsc::channel();
        self.cmds.send(Cmd::Probe { node, reply }).ok()?;
        let size = rx.recv_timeout(Duration::from_secs(2)).ok()?;
        (size.w >= 2 && size.h >= 2).then_some(SizePx {
            w: size.w & !1,
            h: size.h & !1,
        })
    }

    pub fn start(
        self: &Arc<Self>,
        node: u32,
        tag: StreamTag,
        out: SizePx,
        fps: u32,
        sink: Arc<dyn FrameSink>,
    ) -> Result<PwStream> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (reply, rx) = mpsc::channel();
        self.cmds
            .send(Cmd::Start {
                id,
                node,
                tag,
                out,
                fps,
                sink,
                reply,
            })
            .map_err(|_| Error::Other("the PipeWire connection is closed".into()))?;
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(PwStream {
                tag,
                id,
                conn: self.clone(),
            }),
            Ok(Err(e)) => Err(Error::Other(e)),
            Err(_) => Err(Error::Timeout("the PipeWire stream did not start".into())),
        }
    }

    fn stop(&self, id: u64) {
        let (reply, rx) = mpsc::channel();
        if self.cmds.send(Cmd::Stop { id, reply }).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(2));
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.cmds.send(Cmd::Quit);
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(t) = thread
            && t.thread().id() != std::thread::current().id()
        {
            let _ = t.join();
        }
    }
}

type Live = (pw::stream::StreamRc, pw::stream::StreamListener<Format>);
type Probe = (pw::stream::StreamRc, pw::stream::StreamListener<()>);

fn serve(
    fd: Option<OwnedFd>,
    rx: pw::channel::Receiver<Cmd>,
    ready: Reply,
    alive: Arc<AtomicBool>,
) {
    let fail = |e: String| {
        let _ = ready.send(Err(e));
    };
    pw::init();
    let Ok(mainloop) = pw::main_loop::MainLoopRc::new(None) else {
        return fail("PipeWire main loop".into());
    };
    let Ok(context) = pw::context::ContextRc::new(&mainloop, None) else {
        return fail("PipeWire context".into());
    };
    let connected = match fd {
        Some(fd) => context.connect_fd_rc(fd, None),
        None => context.connect_rc(None),
    };
    let core = match connected {
        Ok(c) => c,
        Err(e) => return fail(format!("PipeWire remote: {e}")),
    };
    let _core_listener = core
        .add_listener_local()
        .error({
            let alive = alive.clone();
            move |id, _seq, res, message| {
                if id == pw::core::PW_ID_CORE {
                    tracing::warn!(res, "PipeWire connection: {message}");
                    alive.store(false, Ordering::SeqCst);
                }
            }
        })
        .register();
    let streams: Rc<RefCell<HashMap<u64, Live>>> = Rc::default();
    let probes: Rc<RefCell<Vec<Probe>>> = Rc::default();
    let _attached = rx.attach(mainloop.loop_(), {
        let (mainloop, core) = (mainloop.clone(), core.clone());
        move |cmd| match cmd {
            Cmd::Start {
                id,
                node,
                tag,
                out,
                fps,
                sink,
                reply,
            } => {
                probes.borrow_mut().clear();
                match screen_stream(&core, node, tag, out, fps, sink) {
                    Ok(live) => {
                        streams.borrow_mut().insert(id, live);
                        let _ = reply.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Cmd::Stop { id, reply } => {
                if let Some((stream, listener)) = streams.borrow_mut().remove(&id) {
                    let _ = stream.disconnect();
                    drop(listener);
                }
                let _ = reply.send(());
            }
            Cmd::Probe { node, reply } => {
                probes.borrow_mut().clear();
                match probe_stream(&core, node, reply) {
                    Ok(p) => probes.borrow_mut().push(p),
                    Err(e) => tracing::warn!("PipeWire probe: {e}"),
                }
            }
            Cmd::Quit => {
                probes.borrow_mut().clear();
                for (_, (stream, _)) in streams.borrow_mut().drain() {
                    let _ = stream.disconnect();
                }
                mainloop.quit();
            }
        }
    });
    let _ = ready.send(Ok(()));
    mainloop.run();
    alive.store(false, Ordering::SeqCst);
}

fn video_props() -> pw::properties::PropertiesBox {
    pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Video",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Screen",
    }
}

fn connect(
    stream: &pw::stream::StreamRc,
    node: u32,
    flags: pw::stream::StreamFlags,
) -> std::result::Result<(), String> {
    let values = format_params().map_err(|e| format!("PipeWire format: {e}"))?;
    let pod = Pod::from_bytes(&values).ok_or("PipeWire format pod")?;
    let mut params = [pod];
    stream
        .connect(spa::utils::Direction::Input, Some(node), flags, &mut params)
        .map_err(|e| format!("PipeWire connect: {e}"))
}

fn probe_stream(
    core: &pw::core::CoreRc,
    node: u32,
    reply: mpsc::Sender<SizePx>,
) -> std::result::Result<Probe, String> {
    let stream = pw::stream::StreamRc::new(core.clone(), "simple-capture-probe", video_props())
        .map_err(|e| format!("PipeWire stream: {e}"))?;
    let listener = stream
        .add_local_listener_with_user_data(())
        .param_changed(move |stream, _, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let mut info = spa::param::video::VideoInfoRaw::default();
            if info.parse(param).is_ok() {
                let _ = reply.send(SizePx {
                    w: info.size().width,
                    h: info.size().height,
                });
                let _ = stream.set_active(false);
            }
        })
        .register()
        .map_err(|e| format!("PipeWire listener: {e}"))?;
    connect(&stream, node, pw::stream::StreamFlags::AUTOCONNECT)?;
    Ok((stream, listener))
}

fn screen_stream(
    core: &pw::core::CoreRc,
    node: u32,
    tag: StreamTag,
    out: SizePx,
    fps: u32,
    sink: Arc<dyn FrameSink>,
) -> std::result::Result<Live, String> {
    let stream = pw::stream::StreamRc::new(core.clone(), "simple-capture-screen", video_props())
        .map_err(|e| format!("PipeWire stream: {e}"))?;
    let min_interval = 1_000_000_000 / i64::from(fps.max(1)) * 3 / 4;
    let lost = sink.clone();
    let mut last_ns: Option<i64> = None;
    let listener = stream
        .add_local_listener_with_user_data(Format {
            size: out,
            order: None,
        })
        .state_changed(move |_, _, _, new| {
            if let pw::stream::StreamState::Error(e) = new {
                lost.on_event(SourceEvent::SourceLost {
                    stream: tag,
                    reason: format!("PipeWire: {e}"),
                });
            }
        })
        .param_changed(move |_, f, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let mut info = spa::param::video::VideoInfoRaw::default();
            if info.parse(param).is_ok() {
                f.size = SizePx {
                    w: info.size().width,
                    h: info.size().height,
                };
                f.order = order_of(info.format());
                tracing::info!(
                    w = f.size.w,
                    h = f.size.h,
                    format = ?info.format(),
                    out_w = out.w,
                    out_h = out.h,
                    "screen stream format"
                );
            }
        })
        .process(move |stream, f| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(order) = f.order else { return };
            let now = host_now().0;
            if last_ns.is_some_and(|l| now - l < min_interval) {
                return;
            }
            let datas = buffer.datas_mut();
            let Some(d) = datas.first_mut() else { return };
            let stride = d.chunk().stride().max(0) as usize;
            let offset = d.chunk().offset() as usize;
            let size = f.size;
            // PipeWire maps a memfd with only the access its flags grant, and some producers
            // (xdg-desktop-portal-wlr) don't flag theirs readable: that mapping is PROT_NONE.
            let raw = d.as_raw();
            let own_map;
            let bytes: &[u8] = if d.type_() == spa::buffer::DataType::MemFd
                && raw.flags & spa::sys::SPA_DATA_FLAG_READABLE == 0
            {
                let Some(m) = ReadMap::new(raw.fd, raw.mapoffset, raw.maxsize) else {
                    return;
                };
                own_map = m;
                own_map.bytes()
            } else {
                match d.data() {
                    Some(b) => b,
                    None => return,
                }
            };
            let stride = if stride == 0 {
                size.w as usize * 4
            } else {
                stride
            };
            let need = offset + stride * size.h as usize;
            if size.w == 0 || size.h == 0 || bytes.len() < need {
                return;
            }
            last_ns = Some(now);
            let src = &bytes[offset..need];
            let nv12 = if size == out {
                packed_to_nv12_par(src, stride, out, order)
            } else {
                let (fit, _) = fit_packed(src, stride, size, out);
                packed_to_nv12_par(&fit, out.w as usize * 4, out, order)
            };
            sink.on_screen(VideoFrame {
                stream: tag,
                pts: sc_platform::HostTime(now),
                size: out,
                content_rect_pt: None,
                content_scale: Some(1.0),
                buffer: FrameBuffer(Arc::new(nv12)),
            });
        })
        .register()
        .map_err(|e| format!("PipeWire listener: {e}"))?;
    connect(
        &stream,
        node,
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
    )?;
    Ok((stream, listener))
}

/// A stream on a connection of its own (tests, diagnostics): `fd` is a portal remote, or None for
/// the user's daemon.
pub fn start(
    fd: Option<OwnedFd>,
    node: u32,
    tag: StreamTag,
    out: SizePx,
    fps: u32,
    sink: Arc<dyn FrameSink>,
) -> Result<PwStream> {
    Connection::open(fd)?.start(node, tag, out, fps, sink)
}

#[cfg(test)]
mod tests {
    use super::ReadMap;
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd};

    fn memfd(bytes: &[u8]) -> std::fs::File {
        let fd = unsafe { libc::memfd_create(c"sc-readmap".as_ptr(), 0) };
        assert!(fd >= 0, "memfd_create");
        let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
        f.write_all(bytes).expect("write");
        f
    }

    #[test]
    fn read_map_reads_any_offset_whatever_the_page_size() {
        let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).expect("page");
        let data: Vec<u8> = (0..page * 3).map(|i| (i % 251) as u8).collect();
        let f = memfd(&data);
        let fd = i64::from(f.as_raw_fd());
        for (offset, size) in [(0, page * 3), (page + 100, 1000), (7, page)] {
            let m = ReadMap::new(fd, offset as u32, size as u32).expect("map");
            assert_eq!(m.bytes(), &data[offset..offset + size], "offset {offset}");
        }
        assert!(ReadMap::new(fd, 0, 0).is_none());
        assert!(ReadMap::new(-1, 0, 16).is_none());
    }
}
