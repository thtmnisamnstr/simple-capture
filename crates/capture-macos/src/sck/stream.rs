use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_core_foundation::{CFArray, CFDictionary, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImage,
    CGRectMakeWithDictionaryRepresentation, kCGColorSpaceSRGB,
    kCGDisplayStreamYCbCrMatrix_ITU_R_709_2,
};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{CVPixelBufferGetHeight, CVPixelBufferGetWidth};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString};
use objc2_screen_capture_kit::{
    SCContentFilter, SCFrameStatus, SCScreenshotManager, SCShareableContent, SCStream,
    SCStreamConfiguration, SCStreamDelegate, SCStreamErrorCode, SCStreamFrameInfoContentRect,
    SCStreamFrameInfoContentScale, SCStreamFrameInfoScaleFactor, SCStreamFrameInfoStatus,
    SCStreamOutput, SCStreamOutputType, SCWindow,
};
use sc_core::image::RgbaImage;
use sc_core::project::{RegionMethod, Resolution, SourceDescriptor};
use sc_core::time::HostTime;
use sc_geometry::{RectPt, SizePx, even_round, snap_region_even_px};
use sc_platform::{
    AudioBuffer, AudioData, AudioTrackKind, DisplayInfo, Error, FrameBuffer, FrameSink, Result,
    ScreenSource, ScreenStream, ScreenStreamConfig, SourceEvent, SourceList, SourceSpec, StreamTag,
    VideoFrame, WindowId,
};

use super::content::{self, CgDisplay};
use super::frames::{PIX_420V, ScAudio, ScFrame};
use super::glue::SendBox;
use super::picker::PickedFilter;
use super::system::macos_version;
use crate::{tcc, util};

const START_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
pub struct MacScreenSource {
    _priv: (),
}

impl MacScreenSource {
    pub fn new() -> MacScreenSource {
        MacScreenSource::default()
    }

    /// `ScreenSource::start` with the concrete stream type (for stats in tests).
    pub fn start_mac(
        &self,
        tag: StreamTag,
        spec: &SourceSpec,
        cfg: &ScreenStreamConfig,
        sink: Arc<dyn FrameSink>,
    ) -> Result<MacScreenStream> {
        tcc::guard("SCStream start")?;
        let content = content::fetch(false)?;
        let built = build(Some(&content), spec, cfg)?;
        let config = stream_config(&built, cfg);
        let window = match built.kind {
            Kind::Window { window } => Some(window),
            _ => None,
        };
        let shared = Arc::new(StreamShared::new(tag, sink, true, false, window));
        let handle = open(&built.filter, &config, shared.clone(), true, false)?;
        handle.start()?;
        if window.is_some() {
            spawn_window_watch(shared.clone());
        }
        Ok(MacScreenStream {
            handle,
            shared,
            kind: built.kind,
            cfg_exclude: cfg.exclude.clone(),
            size: built.size,
        })
    }
}

impl ScreenSource for MacScreenSource {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        content::displays()
    }

    fn list(&self, hide: &[WindowId]) -> Result<SourceList> {
        content::list(hide)
    }

    fn describe(
        &self,
        spec: &SourceSpec,
        cfg: &ScreenStreamConfig,
    ) -> Result<(SourceDescriptor, SizePx)> {
        let needs_content = matches!(
            spec,
            SourceSpec::Window { .. } | SourceSpec::Application { .. }
        );
        let content = if needs_content {
            Some(content::fetch(false)?)
        } else {
            None
        };
        let b = build(content.as_deref(), spec, cfg)?;
        Ok((b.descriptor, b.size))
    }

    fn start(
        &self,
        tag: StreamTag,
        spec: &SourceSpec,
        cfg: &ScreenStreamConfig,
        sink: Arc<dyn FrameSink>,
    ) -> Result<Box<dyn ScreenStream>> {
        Ok(Box::new(self.start_mac(tag, spec, cfg, sink)?))
    }

    fn snapshot(&self, spec: &SourceSpec, max: SizePx) -> Result<sc_core::image::RgbaImage> {
        tcc::guard("SCScreenshotManager")?;
        let content = content::fetch(false)?;
        let cfg = ScreenStreamConfig {
            fps: 1,
            resolution: Resolution::Native,
            show_cursor: false,
            show_clicks: false,
            queue_depth: 3,
            exclude: Vec::new(),
        };
        let built = build(Some(&content), spec, &cfg)?;
        snapshot(&built, max)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Display { display: u32 },
    Window { window: u32 },
    Application { display: u32, pid: i32 },
    Region { display: u32 },
    Picked,
}

pub(crate) struct Built {
    pub filter: Retained<SCContentFilter>,
    pub size: SizePx,
    pub source_rect: Option<RectPt>,
    pub descriptor: SourceDescriptor,
    pub kind: Kind,
}

fn exclusion_is_app() -> bool {
    std::env::var("SC_EXCLUSION").ok().as_deref() == Some("app")
}

fn even_size(w: f64, h: f64) -> SizePx {
    SizePx {
        w: even_round(w),
        h: even_round(h),
    }
}

fn display_size(d: &CgDisplay, res: Resolution) -> (SizePx, f64) {
    match res {
        Resolution::Native => (
            even_size(f64::from(d.size_px.w), f64::from(d.size_px.h)),
            d.scale,
        ),
        Resolution::OneX => (even_size(d.frame_pt.w, d.frame_pt.h), 1.0),
    }
}

fn display_filter(
    content: Option<&SCShareableContent>,
    display: u32,
    exclude: &[WindowId],
) -> Result<Retained<SCContentFilter>> {
    let Some(content) = content else {
        return Ok(unsafe { SCContentFilter::new() });
    };
    let d = content::find_display(content, display)
        .ok_or_else(|| Error::NotFound(format!("display {display} not shareable")))?;
    if exclusion_is_app()
        && let Some(app) = content::find_app(content, content::our_pid())
    {
        let apps = NSArray::from_retained_slice(&[app]);
        return Ok(unsafe {
            SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
                SCContentFilter::alloc(),
                &d,
                &apps,
                &NSArray::new(),
            )
        });
    }
    let wins = content::exclusion_windows(content, exclude);
    let arr = NSArray::from_retained_slice(&wins);
    Ok(unsafe {
        SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), &d, &arr)
    })
}

fn app_filter(
    content: &SCShareableContent,
    display: u32,
    pid: i32,
    exclude: &[WindowId],
) -> Result<Retained<SCContentFilter>> {
    let d = content::find_display(content, display)
        .ok_or_else(|| Error::NotFound(format!("display {display} not shareable")))?;
    let app = content::find_app(content, pid)
        .ok_or_else(|| Error::NotFound(format!("application pid {pid} not shareable")))?;
    let excepting: Vec<Retained<SCWindow>> = content::windows_by_id(content, exclude);
    Ok(unsafe {
        SCContentFilter::initWithDisplay_includingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            &d,
            &NSArray::from_retained_slice(&[app]),
            &NSArray::from_retained_slice(&excepting),
        )
    })
}

pub(crate) fn rebuild_filter(
    kind: Kind,
    content: &SCShareableContent,
    exclude: &[WindowId],
) -> Result<Option<Retained<SCContentFilter>>> {
    Ok(match kind {
        Kind::Display { display } | Kind::Region { display } => {
            Some(display_filter(Some(content), display, exclude)?)
        }
        Kind::Application { display, pid } => Some(app_filter(content, display, pid, exclude)?),
        Kind::Window { .. } | Kind::Picked => None,
    })
}

fn filter_info(filter: &SCContentFilter) -> (RectPt, f64) {
    let info = unsafe { SCShareableContent::infoForFilter(filter) };
    let r = unsafe { info.contentRect() };
    (
        RectPt {
            x: r.origin.x,
            y: r.origin.y,
            w: r.size.width,
            h: r.size.height,
        },
        f64::from(unsafe { info.pointPixelScale() }),
    )
}

fn region(region_pt: RectPt, scale: f64) -> (RectPt, SizePx) {
    let snapped = snap_region_even_px(region_pt, scale);
    let size = SizePx {
        w: (snapped.w * scale).round() as u32,
        h: (snapped.h * scale).round() as u32,
    };
    (snapped, size)
}

pub(crate) fn build(
    content: Option<&SCShareableContent>,
    spec: &SourceSpec,
    cfg: &ScreenStreamConfig,
) -> Result<Built> {
    match spec {
        SourceSpec::Display { display } => {
            let id = content::parse_display_id(display)?;
            let d = content::cg_display_by_id(id)?;
            let (size, scale) = display_size(&d, cfg.resolution);
            Ok(Built {
                filter: display_filter(content, id, &cfg.exclude)?,
                size,
                source_rect: None,
                descriptor: SourceDescriptor::Display {
                    display_id: display.0.clone(),
                    display_name: display_name(id),
                    scale,
                    frame_pt: Some(d.frame_pt),
                },
                kind: Kind::Display { display: id },
            })
        }
        SourceSpec::Region { display, region_pt } => {
            let id = content::parse_display_id(display)?;
            let d = content::cg_display_by_id(id)?;
            let scale = match cfg.resolution {
                Resolution::Native => d.scale,
                Resolution::OneX => 1.0,
            };
            let (snapped, size) = region(*region_pt, scale);
            Ok(Built {
                filter: display_filter(content, id, &cfg.exclude)?,
                size,
                source_rect: Some(snapped),
                descriptor: SourceDescriptor::Region {
                    display_id: display.0.clone(),
                    scale,
                    region_pt: snapped,
                    method: RegionMethod::SourceRect,
                },
                kind: Kind::Region { display: id },
            })
        }
        SourceSpec::Window { window } => {
            let content = content.ok_or_else(|| Error::Other("window: no content".into()))?;
            let w = content::find_window(content, *window)
                .ok_or_else(|| Error::NotFound(format!("window {}", window.0)))?;
            let filter = unsafe {
                SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &w)
            };
            let (rect, pscale) = filter_info(&filter);
            let scale = match cfg.resolution {
                Resolution::Native => pscale,
                Resolution::OneX => 1.0,
            };
            let app = unsafe { w.owningApplication() };
            let bid = app
                .as_ref()
                .map(|a| unsafe { a.bundleIdentifier() }.to_string())
                .filter(|s| !s.is_empty());
            Ok(Built {
                filter,
                size: even_size(rect.w * scale, rect.h * scale),
                source_rect: None,
                descriptor: SourceDescriptor::Window {
                    window_id: window.0,
                    app_bundle_id: bid,
                    app_name: app
                        .as_ref()
                        .map(|a| unsafe { a.applicationName() }.to_string()),
                    window_title: unsafe { w.title() }.map(|t| t.to_string()),
                    scale,
                },
                kind: Kind::Window { window: window.0 },
            })
        }
        SourceSpec::Application { display, app } => {
            let content = content.ok_or_else(|| Error::Other("application: no content".into()))?;
            let id = content::parse_display_id(display)?;
            let d = content::cg_display_by_id(id)?;
            let (size, scale) = display_size(&d, cfg.resolution);
            let filter = app_filter(content, id, app.pid, &cfg.exclude)?;
            let name = content::find_app(content, app.pid)
                .map(|a| unsafe { a.applicationName() }.to_string());
            Ok(Built {
                filter,
                size,
                source_rect: None,
                descriptor: SourceDescriptor::Application {
                    display_id: display.0.clone(),
                    app_bundle_id: app.bundle_id.clone(),
                    app_pid: Some(app.pid),
                    app_name: name,
                    scale,
                },
                kind: Kind::Application {
                    display: id,
                    pid: app.pid,
                },
            })
        }
        SourceSpec::Picked(p) => {
            let pf = p
                .handle
                .downcast_ref::<PickedFilter>()
                .ok_or_else(|| Error::Other("picked source: foreign handle".into()))?;
            let filter = pf.filter();
            let (rect, pscale) = filter_info(&filter);
            let scale = match cfg.resolution {
                Resolution::Native => pscale,
                Resolution::OneX => 1.0,
            };
            let (size, source_rect, descriptor) = match (p.region_pt, &p.descriptor) {
                (Some(r), SourceDescriptor::Display { display_id, .. }) => {
                    let (snapped, size) = region(r, scale);
                    (
                        size,
                        Some(snapped),
                        SourceDescriptor::Region {
                            display_id: display_id.clone(),
                            scale,
                            region_pt: snapped,
                            method: RegionMethod::SourceRect,
                        },
                    )
                }
                _ => (
                    even_size(rect.w * scale, rect.h * scale),
                    None,
                    p.descriptor.clone(),
                ),
            };
            Ok(Built {
                filter,
                size,
                source_rect,
                descriptor,
                kind: Kind::Picked,
            })
        }
    }
}

fn display_name(id: u32) -> Option<String> {
    content::displays()
        .ok()?
        .into_iter()
        .find(|d| d.id.0 == id.to_string())
        .map(|d| d.name)
}

fn cgrect(r: RectPt) -> CGRect {
    CGRect::new(CGPoint::new(r.x, r.y), CGSize::new(r.w, r.h))
}

pub(crate) fn stream_config(
    b: &Built,
    cfg: &ScreenStreamConfig,
) -> Retained<SCStreamConfiguration> {
    unsafe {
        let c = SCStreamConfiguration::new();
        c.setWidth(b.size.w as usize);
        c.setHeight(b.size.h as usize);
        c.setScalesToFit(matches!(cfg.resolution, Resolution::OneX));
        c.setPixelFormat(PIX_420V);
        c.setColorMatrix(kCGDisplayStreamYCbCrMatrix_ITU_R_709_2);
        c.setColorSpaceName(kCGColorSpaceSRGB);
        c.setShowsCursor(cfg.show_cursor);
        if cfg.show_clicks && macos_version().0 >= 15 {
            c.setShowMouseClicks(true);
        }
        c.setMinimumFrameInterval(CMTime::new(1, cfg.fps.max(1) as i32));
        c.setQueueDepth(cfg.queue_depth.clamp(1, 8) as isize);
        c.setCapturesAudio(false);
        if let Some(r) = b.source_rect {
            c.setSourceRect(cgrect(r));
        }
        c
    }
}

/// Counters of one stream, by `SCFrameStatus` (0 Complete … 5 Stopped, 6 = unknown).
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    pub status: [u64; 7],
    pub frames: u64,
    pub audio_buffers: u64,
    pub first_frame_host_ns: Option<i64>,
    pub last_frame_host_ns: Option<i64>,
    pub stop_error: Option<(i64, String)>,
    pub resized_events: u64,
}

pub(crate) struct StreamShared {
    tag: StreamTag,
    sink: Arc<dyn FrameSink>,
    video: bool,
    audio: bool,
    window: Option<u32>,
    suspended: AtomicBool,
    accepting: AtomicBool,
    lost_sent: AtomicBool,
    last_geom: Mutex<Option<(Option<RectPt>, Option<f64>)>>,
    stats: Mutex<StreamStats>,
}

impl StreamShared {
    pub(crate) fn new(
        tag: StreamTag,
        sink: Arc<dyn FrameSink>,
        video: bool,
        audio: bool,
        window: Option<u32>,
    ) -> StreamShared {
        StreamShared {
            tag,
            sink,
            video,
            audio,
            window,
            suspended: AtomicBool::new(false),
            accepting: AtomicBool::new(true),
            lost_sent: AtomicBool::new(false),
            last_geom: Mutex::new(None),
            stats: Mutex::new(StreamStats::default()),
        }
    }

    pub(crate) fn stats(&self) -> StreamStats {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub(crate) fn set_accepting(&self, on: bool) {
        self.accepting.store(on, Ordering::Release);
    }

    fn on_video(&self, sb: &CMSampleBuffer) {
        let info = frame_info(sb);
        let status = info.status.unwrap_or(6).clamp(0, 6) as usize;
        if let Ok(mut s) = self.stats.lock() {
            s.status[status] += 1;
        }
        if self.window.is_some() {
            if info.status == Some(SCFrameStatus::Suspended.0) {
                self.suspended.store(true, Ordering::Release);
            } else if info.status == Some(SCFrameStatus::Complete.0) {
                self.suspended.store(false, Ordering::Release);
            }
        }
        if info.status != Some(SCFrameStatus::Complete.0) {
            return;
        }
        let Some(ib) = (unsafe { sb.image_buffer() }) else {
            return;
        };
        let Some(pts) = util::cmtime_to_ns(unsafe { sb.presentation_time_stamp() }) else {
            return;
        };
        let size = SizePx {
            w: CVPixelBufferGetWidth(&ib) as u32,
            h: CVPixelBufferGetHeight(&ib) as u32,
        };
        let geom = (info.content_rect, info.content_scale);
        let resized = match self.last_geom.lock() {
            Ok(mut g) => {
                let changed = g.as_ref().is_some_and(|old| !same_geom(old, &geom));
                *g = Some(geom);
                changed
            }
            Err(_) => false,
        };
        if let Ok(mut s) = self.stats.lock() {
            s.frames += 1;
            s.first_frame_host_ns.get_or_insert(pts);
            s.last_frame_host_ns = Some(pts);
            if resized {
                s.resized_events += 1;
            }
        }
        if resized {
            self.sink.on_event(SourceEvent::SourceResized {
                stream: self.tag,
                content_rect_pt: info.content_rect,
                content_scale: info.content_scale,
            });
        }
        self.sink.on_screen(VideoFrame {
            stream: self.tag,
            pts: HostTime(pts),
            size,
            content_rect_pt: info.content_rect,
            content_scale: info.content_scale,
            buffer: FrameBuffer(Arc::new(ScFrame::retain(sb, size))),
        });
    }

    fn on_audio(&self, sb: &CMSampleBuffer) {
        let Some(pts) = util::cmtime_to_ns(unsafe { sb.presentation_time_stamp() }) else {
            return;
        };
        let (rate, channels) = super::audio::asbd_rate_channels(sb).unwrap_or((48_000, 2));
        let frames = unsafe { sb.num_samples() }.max(0) as u32;
        if let Ok(mut s) = self.stats.lock() {
            s.audio_buffers += 1;
        }
        self.sink.on_audio(
            AudioTrackKind::SystemAudio,
            AudioBuffer {
                pts: HostTime(pts),
                sample_rate: rate,
                channels,
                frames,
                data: AudioData::Native(Arc::new(ScAudio::retain(sb))),
            },
        );
    }

    fn lost(&self, reason: &str) {
        if !self.lost_sent.swap(true, Ordering::AcqRel) {
            self.sink.on_event(SourceEvent::SourceLost {
                stream: self.tag,
                reason: reason.to_string(),
            });
        }
    }

    fn on_stop_error(&self, code: i64, msg: String) {
        if let Ok(mut s) = self.stats.lock() {
            s.stop_error = Some((code, msg.clone()));
        }
        if !self.accepting.load(Ordering::Acquire) {
            return;
        }
        if code == SCStreamErrorCode::NoCaptureSource.0 as i64 {
            self.lost(&format!("didStopWithError: {msg}"));
        } else {
            self.sink.on_event(SourceEvent::StreamStopped {
                stream: self.tag,
                error: Some(msg),
            });
        }
    }
}

/// A closed window only produces a `Suspended` frame (no error), so while a window stream is
/// suspended this checks that the window still exists and reports `SourceLost` once it is gone.
fn spawn_window_watch(shared: Arc<StreamShared>) {
    let Some(id) = shared.window else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name(format!("sck-window-watch-{id}"))
        .spawn(move || {
            while shared.accepting.load(Ordering::Acquire)
                && !shared.lost_sent.load(Ordering::Acquire)
            {
                if shared.suspended.load(Ordering::Acquire) && !window_exists(id) {
                    shared.lost(&format!("window {id} closed"));
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
}

pub(crate) fn window_exists(id: u32) -> bool {
    use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
    CGWindowListCopyWindowInfo(CGWindowListOption::OptionIncludingWindow, id)
        .is_some_and(|a| a.count() > 0)
}

fn same_geom(a: &(Option<RectPt>, Option<f64>), b: &(Option<RectPt>, Option<f64>)) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() < 0.5;
    let rect = match (a.0, b.0) {
        (Some(p), Some(q)) => close(p.w, q.w) && close(p.h, q.h),
        (None, None) => true,
        _ => false,
    };
    let scale = match (a.1, b.1) {
        (Some(p), Some(q)) => (p - q).abs() < 1e-3,
        (None, None) => true,
        _ => false,
    };
    rect && scale
}

struct FrameInfo {
    status: Option<isize>,
    content_rect: Option<RectPt>,
    content_scale: Option<f64>,
}

fn frame_info(sb: &CMSampleBuffer) -> FrameInfo {
    let mut out = FrameInfo {
        status: None,
        content_rect: None,
        content_scale: None,
    };
    let Some(arr) = (unsafe { sb.sample_attachments_array(false) }) else {
        return out;
    };
    let arr: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(&*arr as *const CFArray as *const NSArray<NSDictionary<NSString, AnyObject>>) };
    let Some(d) = arr.firstObject() else {
        return out;
    };
    let num = |k: &NSString| {
        d.objectForKey(k)
            .and_then(|o| o.downcast::<NSNumber>().ok())
    };
    unsafe {
        out.status = num(SCStreamFrameInfoStatus).map(|n| n.integerValue());
        out.content_scale = num(SCStreamFrameInfoContentScale).map(|n| n.doubleValue());
        let _ = num(SCStreamFrameInfoScaleFactor);
        if let Some(o) = d.objectForKey(SCStreamFrameInfoContentRect) {
            let cf = &*(Retained::as_ptr(&o) as *const CFDictionary);
            let mut r = CGRect::default();
            if CGRectMakeWithDictionaryRepresentation(Some(cf), &mut r) {
                out.content_rect = Some(RectPt {
                    x: r.origin.x,
                    y: r.origin.y,
                    w: r.size.width,
                    h: r.size.height,
                });
            }
        }
    }
    out
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SCCaptureStreamOutput"]
    #[ivars = Arc<StreamShared>]
    pub(crate) struct StreamOutput;

    unsafe impl NSObjectProtocol for StreamOutput {}

    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(&self, _stream: &SCStream, sb: &CMSampleBuffer, ty: SCStreamOutputType) {
            let s = self.ivars();
            if !s.accepting.load(Ordering::Acquire) {
                return;
            }
            if ty == SCStreamOutputType::Screen && s.video {
                s.on_video(sb);
            } else if ty == SCStreamOutputType::Audio && s.audio {
                s.on_audio(sb);
            }
        }
    }

    unsafe impl SCStreamDelegate for StreamOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, err: &NSError) {
            self.ivars().on_stop_error(err.code() as i64, util::ns_error_text(err));
        }
    }
);

impl StreamOutput {
    fn create(shared: Arc<StreamShared>) -> Retained<StreamOutput> {
        let this = Self::alloc().set_ivars(shared);
        unsafe { msg_send![super(this), init] }
    }
}

/// An SCStream with its output object and queues.
pub(crate) struct StreamHandle {
    pub stream: SendBox<Retained<SCStream>>,
    _output: SendBox<Retained<StreamOutput>>,
    _queues: Vec<DispatchRetained<DispatchQueue>>,
    shared: Arc<StreamShared>,
    stopped: AtomicBool,
}

pub(crate) fn open(
    filter: &SCContentFilter,
    config: &SCStreamConfiguration,
    shared: Arc<StreamShared>,
    video: bool,
    audio: bool,
) -> Result<StreamHandle> {
    let output = StreamOutput::create(shared.clone());
    let delegate: &ProtocolObject<dyn SCStreamDelegate> = ProtocolObject::from_ref(&*output);
    let stream = unsafe {
        SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            filter,
            config,
            Some(delegate),
        )
    };
    let proto: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*output);
    let tag = shared.tag.0;
    let vq = DispatchQueue::new(
        &format!("simplecapture.sck.video.{tag}"),
        DispatchQueueAttr::SERIAL,
    );
    unsafe {
        stream
            .addStreamOutput_type_sampleHandlerQueue_error(
                proto,
                SCStreamOutputType::Screen,
                Some(&vq),
            )
            .map_err(|e| {
                Error::Other(format!(
                    "addStreamOutput screen: {}",
                    util::ns_error_text(&e)
                ))
            })?;
    }
    let mut queues = vec![vq];
    if audio {
        let aq = DispatchQueue::new(
            &format!("simplecapture.sck.audio.{tag}"),
            DispatchQueueAttr::SERIAL,
        );
        unsafe {
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(
                    proto,
                    SCStreamOutputType::Audio,
                    Some(&aq),
                )
                .map_err(|e| {
                    Error::Other(format!(
                        "addStreamOutput audio: {}",
                        util::ns_error_text(&e)
                    ))
                })?;
        }
        queues.push(aq);
    }
    let _ = video;
    Ok(StreamHandle {
        stream: SendBox(stream),
        _output: SendBox(output),
        _queues: queues,
        shared,
        stopped: AtomicBool::new(false),
    })
}

impl StreamHandle {
    pub(crate) fn start(&self) -> Result<()> {
        let s = &self.stream.0;
        util::wait_err(
            |b| unsafe { s.startCaptureWithCompletionHandler(Some(b)) },
            "SCStream startCapture",
            START_TIMEOUT,
        )
        .map_err(map_start_error)
    }

    pub(crate) fn update_filter(&self, filter: &SCContentFilter) -> Result<()> {
        let s = &self.stream.0;
        util::wait_err(
            |b| unsafe { s.updateContentFilter_completionHandler(filter, Some(b)) },
            "SCStream updateContentFilter",
            START_TIMEOUT,
        )
    }

    pub(crate) fn stop(&self) -> Result<()> {
        if self.stopped.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        self.shared.set_accepting(false);
        let s = &self.stream.0;
        util::wait_err(
            |b| unsafe { s.stopCaptureWithCompletionHandler(Some(b)) },
            "SCStream stopCapture",
            START_TIMEOUT,
        )
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        if !self.stopped.swap(true, Ordering::AcqRel) {
            self.shared.set_accepting(false);
            unsafe { self.stream.0.stopCaptureWithCompletionHandler(None) };
        }
    }
}

fn map_start_error(e: Error) -> Error {
    match &e {
        Error::Other(m) if m.contains("-3801") => {
            Error::PermissionDenied(sc_platform::PermissionKind::Screen)
        }
        _ => e,
    }
}

pub struct MacScreenStream {
    handle: StreamHandle,
    shared: Arc<StreamShared>,
    kind: Kind,
    cfg_exclude: Vec<WindowId>,
    size: SizePx,
}

impl MacScreenStream {
    pub fn stats(&self) -> StreamStats {
        self.shared.stats()
    }

    pub fn size(&self) -> SizePx {
        self.size
    }
}

impl ScreenStream for MacScreenStream {
    fn tag(&self) -> StreamTag {
        self.shared.tag
    }

    fn update_exclusions(&self, exclude: &[WindowId]) -> Result<()> {
        match self.kind {
            Kind::Picked => Err(Error::Unsupported(
                "update_exclusions on a picked stream (plan §10 item 10)",
            )),
            Kind::Window { .. } => Ok(()),
            kind => {
                tcc::guard("SCShareableContent")?;
                let content = content::fetch(false)?;
                let mut all = self.cfg_exclude.clone();
                all.extend_from_slice(exclude);
                match rebuild_filter(kind, &content, &all)? {
                    Some(f) => self.handle.update_filter(&f),
                    None => Ok(()),
                }
            }
        }
    }

    fn stop(self: Box<Self>) -> Result<()> {
        self.handle.stop()
    }
}

fn fit_within(size: SizePx, max: SizePx) -> SizePx {
    if max.w == 0 || max.h == 0 || (size.w <= max.w && size.h <= max.h) {
        return size;
    }
    let s = (f64::from(max.w) / f64::from(size.w)).min(f64::from(max.h) / f64::from(size.h));
    SizePx {
        w: ((f64::from(size.w) * s).round() as u32).max(1),
        h: ((f64::from(size.h) * s).round() as u32).max(1),
    }
}

fn snapshot(b: &Built, max: SizePx) -> Result<RgbaImage> {
    let size = fit_within(b.size, max);
    let cfg = unsafe {
        let c = SCStreamConfiguration::new();
        c.setWidth(size.w as usize);
        c.setHeight(size.h as usize);
        c.setShowsCursor(false);
        c.setColorSpaceName(kCGColorSpaceSRGB);
        if let Some(r) = b.source_rect {
            c.setSourceRect(cgrect(r));
        }
        c
    };
    let (tx, rx) = std::sync::mpsc::channel::<
        std::result::Result<SendBox<objc2_core_foundation::CFRetained<CGImage>>, String>,
    >();
    let blk = block2::RcBlock::new(move |img: *mut CGImage, e: *mut NSError| {
        let r = match std::ptr::NonNull::new(img) {
            Some(p) => Ok(SendBox(unsafe {
                objc2_core_foundation::CFRetained::retain(p)
            })),
            None => Err(unsafe { e.as_ref() }
                .map(util::ns_error_text)
                .unwrap_or_else(|| "no image".into())),
        };
        let _ = tx.send(r);
    });
    unsafe {
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
            &b.filter,
            &cfg,
            Some(&blk),
        );
    }
    let img = util::wait_on(&rx, START_TIMEOUT, "SCScreenshotManager")?
        .map_err(|e| Error::Other(format!("SCScreenshotManager: {e}")))?
        .0;
    cgimage_to_rgba(&img)
}

pub(crate) fn cgimage_to_rgba(img: &CGImage) -> Result<RgbaImage> {
    let w = CGImage::width(Some(img));
    let h = CGImage::height(Some(img));
    if w == 0 || h == 0 {
        return Err(Error::Other("snapshot: empty image".into()));
    }
    let stride = w * 4;
    let mut data = vec![0u8; stride * h];
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or_else(|| Error::Other("sRGB colour space".into()))?;
    const PREMULTIPLIED_LAST: u32 = 1;
    const BYTE_ORDER_32_BIG: u32 = 4 << 12;
    let ctx = unsafe {
        CGBitmapContextCreate(
            data.as_mut_ptr().cast(),
            w,
            h,
            8,
            stride,
            Some(&space),
            PREMULTIPLIED_LAST | BYTE_ORDER_32_BIG,
        )
    }
    .ok_or_else(|| Error::Other("CGBitmapContextCreate".into()))?;
    CGContext::draw_image(
        Some(&ctx),
        CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)),
        Some(img),
    );
    drop(ctx);
    Ok(RgbaImage {
        size: SizePx {
            w: w as u32,
            h: h as u32,
        },
        stride,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_within_keeps_aspect() {
        let s = fit_within(SizePx { w: 3840, h: 2160 }, SizePx { w: 640, h: 640 });
        assert_eq!(s, SizePx { w: 640, h: 360 });
        let s = fit_within(SizePx { w: 100, h: 50 }, SizePx { w: 640, h: 640 });
        assert_eq!(s, SizePx { w: 100, h: 50 });
    }

    #[test]
    fn region_sizes_are_even_px() {
        let (r, s) = region(
            RectPt {
                x: 10.0,
                y: 10.0,
                w: 617.0,
                h: 283.5,
            },
            2.0,
        );
        assert_eq!(s, SizePx { w: 1234, h: 566 });
        assert_eq!(r.w * 2.0, 1234.0);
        let (_, s) = region(
            RectPt {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            2.0,
        );
        assert_eq!(s, SizePx { w: 2, h: 2 });
        let (_, s) = region(
            RectPt {
                x: 0.0,
                y: 0.0,
                w: 560.5,
                h: 320.5,
            },
            2.0,
        );
        assert_eq!(s, SizePx { w: 1120, h: 640 });
    }

    #[test]
    fn geometry_change_detection() {
        let a = (
            Some(RectPt {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 50.0,
            }),
            Some(2.0),
        );
        let mut b = a;
        assert!(same_geom(&a, &b));
        b.1 = Some(1.0);
        assert!(!same_geom(&a, &b));
    }
}
