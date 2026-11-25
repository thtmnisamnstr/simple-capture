//! A PipeWire video stream on a portal remote: shared-memory BGRx/RGBx frames converted to NV12
//! at the size the stream started with (letterboxed when a window is resized).

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::mpsc;
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
    quit: pw::channel::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ScreenStream for PwStream {
    fn tag(&self) -> StreamTag {
        self.tag
    }

    fn update_exclusions(&self, _exclude: &[WindowId]) -> Result<()> {
        Ok(())
    }

    fn stop(mut self: Box<Self>) -> Result<()> {
        let _ = self.quit.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    fd: Option<OwnedFd>,
    node: u32,
    tag: StreamTag,
    out: SizePx,
    fps: u32,
    sink: Arc<dyn FrameSink>,
    quit_rx: pw::channel::Receiver<()>,
    ready: mpsc::Sender<std::result::Result<(), String>>,
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
    let stream = match pw::stream::StreamBox::new(
        &core,
        "simple-capture-screen",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    ) {
        Ok(s) => s,
        Err(e) => return fail(format!("PipeWire stream: {e}")),
    };
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
        .param_changed(|_, f, id, param| {
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
            let Some(bytes) = d.data() else { return };
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
        .register();
    let _listener = match listener {
        Ok(l) => l,
        Err(e) => return fail(format!("PipeWire listener: {e}")),
    };
    let values = match format_params() {
        Ok(v) => v,
        Err(e) => return fail(format!("PipeWire format: {e}")),
    };
    let Some(pod) = Pod::from_bytes(&values) else {
        return fail("PipeWire format pod".into());
    };
    let mut params = [pod];
    if let Err(e) = stream.connect(
        spa::utils::Direction::Input,
        Some(node),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    ) {
        return fail(format!("PipeWire connect: {e}"));
    }
    let ml = mainloop.clone();
    let _quit = quit_rx.attach(mainloop.loop_(), move |()| ml.quit());
    let _ = ready.send(Ok(()));
    mainloop.run();
    let _ = stream.disconnect();
}

pub fn start(
    fd: Option<OwnedFd>,
    node: u32,
    tag: StreamTag,
    out: SizePx,
    fps: u32,
    sink: Arc<dyn FrameSink>,
) -> Result<PwStream> {
    let (quit, quit_rx) = pw::channel::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("pipewire-screen".into())
        .spawn(move || run(fd, node, tag, out, fps, sink, quit_rx, ready_tx))?;
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => Ok(PwStream {
            tag,
            quit,
            thread: Some(thread),
        }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(Error::Other(e))
        }
        Err(_) => {
            let _ = quit.send(());
            Err(Error::Timeout("the PipeWire stream did not start".into()))
        }
    }
}
