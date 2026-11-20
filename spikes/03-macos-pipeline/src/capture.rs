use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_core_foundation::{CFArray, CFRetained};
use objc2_core_graphics::{kCGColorSpaceSRGB, kCGDisplayStreamYCbCrMatrix_ITU_R_709_2, CGMainDisplayID};
use objc2_core_media::{CMSampleBuffer, CMSampleTimingInfo, CMTime};
use objc2_core_video::{CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString};
use objc2_screen_capture_kit::*;

use crate::util::*;
use crate::writer::{TrackWriter, VideoCfg};

pub const PIX_420V: u32 = u32::from_be_bytes(*b"420v");
pub const PIX_BGRA: u32 = u32::from_be_bytes(*b"BGRA");

#[derive(Default)]
pub struct ScreenTrack {
    pub writer: Option<TrackWriter>,
    pub cfg: Option<VideoCfg>,
    pub frag: f64,
    pub last_sb: Option<CFRetained<CMSampleBuffer>>,
    pub status_counts: [u64; 8],
    pub complete_pts: Vec<f64>,
    pub clock_probe: Vec<(f64, f64, Option<f64>)>,
    pub pix_fmt: Option<u32>,
    pub dims: Option<(usize, usize)>,
    pub writer_error: Option<String>,
}

#[derive(Default)]
pub struct AudioTrack {
    pub writer: Option<TrackWriter>,
    pub frag: f64,
    pub buffers: u64,
    pub frames: u64,
    pub clock_probe: Vec<(f64, f64)>,
    pub writer_error: Option<String>,
}

pub struct ScreenShared {
    pub dir: PathBuf,
    pub screen: Mutex<ScreenTrack>,
    pub sys: Mutex<AudioTrack>,
    pub stopped_with_error: Mutex<Option<String>>,
    pub capture_audio: bool,
    pub accepting: AtomicBool,
}

pub fn frame_info(sb: &CMSampleBuffer) -> (Option<i64>, Option<u64>) {
    unsafe {
        let Some(arr) = sb.sample_attachments_array(false) else { return (None, None) };
        let arr: &NSArray<NSDictionary<NSString, AnyObject>> = &*(&*arr as *const CFArray as *const NSArray<NSDictionary<NSString, AnyObject>>);
        let Some(d) = arr.firstObject() else { return (None, None) };
        let status = d
            .objectForKey(SCStreamFrameInfoStatus)
            .and_then(|o| o.downcast::<NSNumber>().ok())
            .map(|n| n.integerValue() as i64);
        let disp = d
            .objectForKey(SCStreamFrameInfoDisplayTime)
            .and_then(|o| o.downcast::<NSNumber>().ok())
            .map(|n| n.unsignedLongLongValue());
        (status, disp)
    }
}

impl ScreenShared {
    fn on_sample(&self, sb: &CMSampleBuffer, ty: SCStreamOutputType) {
        if !self.accepting.load(Ordering::Acquire) {
            return;
        }
        let now = host_now_secs();
        let pts = secs(unsafe { sb.presentation_time_stamp() });
        if ty == SCStreamOutputType::Screen {
            let (status, disp) = frame_info(sb);
            let mut s = self.screen.lock().unwrap();
            let st = status.unwrap_or(7).clamp(0, 7) as usize;
            s.status_counts[st] += 1;
            if s.clock_probe.len() < 400 {
                s.clock_probe.push((pts, now, disp.map(mach_to_secs)));
            }
            if status != Some(SCFrameStatus::Complete.0 as i64) {
                return;
            }
            let Some(ib) = (unsafe { sb.image_buffer() }) else { return };
            if s.writer.is_none() && s.writer_error.is_none() {
                let (w, h) = (CVPixelBufferGetWidth(&ib), CVPixelBufferGetHeight(&ib));
                s.dims = Some((w, h));
                s.pix_fmt = Some(CVPixelBufferGetPixelFormatType(&ib));
                let mut cfg = s.cfg.clone().unwrap();
                cfg.width = w;
                cfg.height = h;
                match TrackWriter::video("screen", &self.dir.join("screen.mov"), &cfg, s.frag) {
                    Ok(wr) => s.writer = Some(wr),
                    Err(e) => s.writer_error = Some(e.to_string()),
                }
            }
            s.complete_pts.push(pts);
            if let Some(wr) = s.writer.as_mut() {
                wr.append(sb);
            }
            s.last_sb = Some(unsafe { CFRetained::retain(std::ptr::NonNull::from(sb)) });
        } else if ty == SCStreamOutputType::Audio {
            let mut a = self.sys.lock().unwrap();
            a.buffers += 1;
            a.frames += unsafe { sb.num_samples() } as u64;
            if a.clock_probe.len() < 50 {
                a.clock_probe.push((pts, now));
            }
            if a.writer.is_none() && a.writer_error.is_none() {
                match TrackWriter::aac_for("system_audio", &self.dir.join("system_audio.mov"), sb, a.frag) {
                    Ok(w) => a.writer = Some(w),
                    Err(e) => a.writer_error = Some(e.to_string()),
                }
            }
            if let Some(w) = a.writer.as_mut() {
                w.append(sb);
            }
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SCSpikeStreamOutput"]
    #[ivars = Arc<ScreenShared>]
    pub struct StreamOutput;

    unsafe impl NSObjectProtocol for StreamOutput {}

    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(&self, _stream: &SCStream, sb: &CMSampleBuffer, ty: SCStreamOutputType) {
            self.ivars().on_sample(sb, ty);
        }
    }

    unsafe impl SCStreamDelegate for StreamOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, err: &NSError) {
            let msg = format!("{} (code {})", err.localizedDescription(), err.code());
            eprintln!("SCStream stopped with error: {msg}");
            *self.ivars().stopped_with_error.lock().unwrap() = Some(msg);
        }
    }
);

impl StreamOutput {
    fn new(shared: Arc<ScreenShared>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(shared);
        unsafe { msg_send![super(this), init] }
    }
}

pub fn fetch_content() -> Result<Retained<SCShareableContent>, String> {
    let (tx, rx) = mpsc::channel::<Result<Retained<SCShareableContent>, String>>();
    let blk = RcBlock::new(move |c: *mut SCShareableContent, e: *mut NSError| {
        let r = unsafe {
            if !c.is_null() {
                Ok(Retained::retain(c).unwrap())
            } else if !e.is_null() {
                let e = &*e;
                Err(format!("{} (domain {}, code {})", e.localizedDescription(), e.domain(), e.code()))
            } else {
                Err("unknown".into())
            }
        };
        let _ = tx.send(r);
    });
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&blk) };
    pump_recv(&rx, Duration::from_secs(20)).unwrap_or_else(|| Err("timeout".into()))
}

pub fn main_display(content: &SCShareableContent) -> Option<Retained<SCDisplay>> {
    let main = CGMainDisplayID();
    let displays = unsafe { content.displays() };
    let found = displays.iter().find(|d| unsafe { d.displayID() } == main);
    found.or_else(|| displays.firstObject())
}

pub enum Exclude {
    None,
    SelfApp,
    SelfWindows,
}

pub struct FilterInfo {
    pub filter: Retained<SCContentFilter>,
    pub point_pixel_scale: f64,
    pub content_rect_pts: (f64, f64, f64, f64),
    pub pixel_size: (usize, usize),
    pub excluded_desc: String,
}

pub fn build_filter(content: &SCShareableContent, display: &SCDisplay, ex: &Exclude) -> Result<FilterInfo> {
    let pid = std::process::id() as i32;
    unsafe {
        let (filter, desc) = match ex {
            Exclude::None => (
                SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), display, &NSArray::new()),
                "nothing excluded".to_string(),
            ),
            Exclude::SelfApp => {
                let apps = content.applications();
                let mine: Vec<Retained<SCRunningApplication>> = apps.iter().filter(|a| a.processID() == pid).collect();
                if mine.is_empty() {
                    return Err(anyhow!("this process (pid {pid}) is not listed in SCShareableContent.applications"));
                }
                let bid = mine[0].bundleIdentifier();
                let same_bundle: Vec<i32> = apps.iter().filter(|a| a.bundleIdentifier() == bid).map(|a| a.processID()).collect();
                let own_windows = content.windows().iter().filter(|w| w.owningApplication().map(|a| a.processID() == pid).unwrap_or(false)).count();
                let d = format!(
                    "excluding SCRunningApplication pid {pid} (bundle id '{}', name '{}'); apps sharing that bundle id: {:?}; SCWindows owned by pid: {}",
                    bid,
                    mine[0].applicationName(),
                    same_bundle,
                    own_windows
                );
                let arr = NSArray::from_retained_slice(&mine);
                (
                    SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(SCContentFilter::alloc(), display, &arr, &NSArray::new()),
                    d,
                )
            }
            Exclude::SelfWindows => {
                let wins: Vec<Retained<SCWindow>> = content
                    .windows()
                    .iter()
                    .filter(|w| w.owningApplication().map(|a| a.processID() == pid).unwrap_or(false))
                    .collect();
                let d = format!("excluding {} SCWindow(s) owned by pid {pid}", wins.len());
                let arr = NSArray::from_retained_slice(&wins);
                (SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), display, &arr), d)
            }
        };
        let info = SCShareableContent::infoForFilter(&filter);
        let scale = info.pointPixelScale() as f64;
        let r = info.contentRect();
        let px = ((r.size.width * scale).round() as usize, (r.size.height * scale).round() as usize);
        Ok(FilterInfo {
            filter,
            point_pixel_scale: scale,
            content_rect_pts: (r.origin.x, r.origin.y, r.size.width, r.size.height),
            pixel_size: px,
            excluded_desc: desc,
        })
    }
}

pub fn window_filter(w: &SCWindow) -> Result<FilterInfo> {
    unsafe {
        let filter = SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), w);
        let info = SCShareableContent::infoForFilter(&filter);
        let scale = info.pointPixelScale() as f64;
        let r = info.contentRect();
        let px = (((r.size.width * scale).round() as usize) & !1, ((r.size.height * scale).round() as usize) & !1);
        Ok(FilterInfo {
            filter,
            point_pixel_scale: scale,
            content_rect_pts: (r.origin.x, r.origin.y, r.size.width, r.size.height),
            pixel_size: px,
            excluded_desc: format!("single window {} ('{}')", w.windowID(), w.title().map(|t| t.to_string()).unwrap_or_default()),
        })
    }
}

pub struct ScreenCapture {
    pub stream: Retained<SCStream>,
    pub shared: Arc<ScreenShared>,
    _output: Retained<StreamOutput>,
    _vq: dispatch2::DispatchRetained<DispatchQueue>,
    _aq: dispatch2::DispatchRetained<DispatchQueue>,
}

pub struct ScreenOpts {
    pub fps: f64,
    pub pixel_format: u32,
    pub queue_depth: isize,
    pub capture_audio: bool,
    pub bitrate: f64,
    pub hevc: bool,
    pub frag: f64,
}

impl ScreenCapture {
    pub fn new(dir: &std::path::Path, fi: &FilterInfo, o: &ScreenOpts) -> Result<Self> {
        let shared = Arc::new(ScreenShared {
            dir: dir.to_path_buf(),
            screen: Mutex::new(ScreenTrack {
                cfg: Some(VideoCfg {
                    width: fi.pixel_size.0,
                    height: fi.pixel_size.1,
                    fps: o.fps,
                    bitrate: o.bitrate,
                    hevc: o.hevc,
                    realtime: true,
                }),
                frag: o.frag,
                ..Default::default()
            }),
            sys: Mutex::new(AudioTrack { frag: o.frag, ..Default::default() }),
            stopped_with_error: Mutex::new(None),
            capture_audio: o.capture_audio,
            accepting: AtomicBool::new(true),
        });
        unsafe {
            let cfg = SCStreamConfiguration::new();
            cfg.setWidth(fi.pixel_size.0);
            cfg.setHeight(fi.pixel_size.1);
            cfg.setScalesToFit(false);
            cfg.setPixelFormat(o.pixel_format);
            if o.pixel_format == PIX_420V {
                cfg.setColorMatrix(kCGDisplayStreamYCbCrMatrix_ITU_R_709_2);
            }
            cfg.setColorSpaceName(kCGColorSpaceSRGB);
            cfg.setShowsCursor(true);
            cfg.setMinimumFrameInterval(CMTime::new(1000, (o.fps * 1000.0) as i32));
            cfg.setQueueDepth(o.queue_depth);
            cfg.setCapturesAudio(o.capture_audio);
            if o.capture_audio {
                cfg.setExcludesCurrentProcessAudio(true);
                cfg.setSampleRate(48_000);
                cfg.setChannelCount(2);
            }
            let output = StreamOutput::new(shared.clone());
            let delegate: &ProtocolObject<dyn SCStreamDelegate> = ProtocolObject::from_ref(&*output);
            let stream = SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &fi.filter, &cfg, Some(delegate));
            let vq = DispatchQueue::new("simplecapture.spike.screen", DispatchQueueAttr::SERIAL);
            let aq = DispatchQueue::new("simplecapture.spike.sysaudio", DispatchQueueAttr::SERIAL);
            let out_proto: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*output);
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(out_proto, SCStreamOutputType::Screen, Some(&vq))
                .map_err(|e| anyhow!("addStreamOutput screen: {}", e.localizedDescription()))?;
            if o.capture_audio {
                stream
                    .addStreamOutput_type_sampleHandlerQueue_error(out_proto, SCStreamOutputType::Audio, Some(&aq))
                    .map_err(|e| anyhow!("addStreamOutput audio: {}", e.localizedDescription()))?;
            }
            Ok(Self { stream, shared, _output: output, _vq: vq, _aq: aq })
        }
    }

    pub fn start(&self) -> Result<()> {
        wait_err(|blk| unsafe { self.stream.startCaptureWithCompletionHandler(Some(blk)) }, "startCapture")
    }

    pub fn stop(&self) -> Result<()> {
        let r = wait_err(|blk| unsafe { self.stream.stopCaptureWithCompletionHandler(Some(blk)) }, "stopCapture");
        self.shared.accepting.store(false, Ordering::Release);
        r
    }

    pub fn sync_clock_vs_host(&self) -> Option<(f64, f64)> {
        unsafe {
            let c = self.stream.synchronizationClock()?;
            let a = secs(c.time());
            let h = host_now_secs();
            Some((a, h))
        }
    }
}

pub fn wait_err(f: impl FnOnce(&block2::DynBlock<dyn Fn(*mut NSError)>), what: &str) -> Result<()> {
    let (tx, rx) = mpsc::channel::<Option<String>>();
    let blk = RcBlock::new(move |e: *mut NSError| {
        let m = if e.is_null() { None } else { Some(unsafe { format!("{} (code {})", (*e).localizedDescription(), (*e).code()) }) };
        let _ = tx.send(m);
    });
    f(&blk);
    match pump_recv(&rx, Duration::from_secs(15)) {
        None => Err(anyhow!("{what}: timeout")),
        Some(Some(e)) => Err(anyhow!("{what}: {e}")),
        Some(None) => Ok(()),
    }
}

pub fn extend_screen_to(shared: &ScreenShared, stop: CMTime) -> Option<f64> {
    let mut s = shared.screen.lock().unwrap();
    let last = s.last_sb.take()?;
    let last_pts = secs(unsafe { last.presentation_time_stamp() });
    if secs(stop) <= last_pts + 1e-4 {
        return Some(0.0);
    }
    let timing = CMSampleTimingInfo { duration: unsafe { objc2_core_media::kCMTimeInvalid }, presentationTimeStamp: stop, decodeTimeStamp: unsafe { objc2_core_media::kCMTimeInvalid } };
    let mut out: *mut CMSampleBuffer = std::ptr::null_mut();
    let st = unsafe { CMSampleBuffer::create_copy_with_new_timing(None, &last, 1, &timing, std::ptr::NonNull::from(&mut out)) };
    if st != 0 || out.is_null() {
        eprintln!("CMSampleBufferCreateCopyWithNewTiming failed: {st}");
        return None;
    }
    let copy = unsafe { CFRetained::from_raw(std::ptr::NonNull::new(out).unwrap()) };
    let w = s.writer.as_mut()?;
    for _ in 0..200 {
        if w.ready() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if w.append(&copy) {
        Some(secs(stop) - last_pts)
    } else {
        None
    }
}
