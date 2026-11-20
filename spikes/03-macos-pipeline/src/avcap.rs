use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_av_foundation::*;
use objc2_core_media::{CMSampleBuffer, CMTime, CMVideoFormatDescriptionGetDimensions};
use objc2_core_video::{kCVPixelBufferPixelFormatTypeKey, CVPixelBufferGetHeight, CVPixelBufferGetWidth};
use objc2_foundation::{NSArray, NSString};

use crate::capture::PIX_420V;
use crate::util::*;
use crate::writer::{TrackWriter, VideoCfg};

pub fn auth_status(video: bool) -> AVAuthorizationStatus {
    unsafe {
        let mt = if video { AVMediaTypeVideo.unwrap() } else { AVMediaTypeAudio.unwrap() };
        AVCaptureDevice::authorizationStatusForMediaType(mt)
    }
}

pub fn status_name(s: AVAuthorizationStatus) -> &'static str {
    match s {
        AVAuthorizationStatus::NotDetermined => "not-determined",
        AVAuthorizationStatus::Restricted => "restricted",
        AVAuthorizationStatus::Denied => "denied",
        AVAuthorizationStatus::Authorized => "authorized",
        _ => "unknown",
    }
}

pub fn ensure_access(video: bool, timeout: Duration) -> bool {
    let what = if video { "Camera" } else { "Microphone" };
    let st = auth_status(video);
    if st == AVAuthorizationStatus::Authorized {
        return true;
    }
    if st == AVAuthorizationStatus::NotDetermined {
        eprintln!(">>> {what} permission needed: macOS will show a prompt (attributed to the shared dev host \"Simple Capture Dev\" when run through tools/macos-dev-host/run.sh). Please click Allow. Waiting up to {}s...", timeout.as_secs());
        let (tx, rx) = mpsc::channel::<bool>();
        let blk = RcBlock::new(move |granted: objc2::runtime::Bool| {
            let _ = tx.send(granted.as_bool());
        });
        unsafe {
            let mt = if video { AVMediaTypeVideo.unwrap() } else { AVMediaTypeAudio.unwrap() };
            AVCaptureDevice::requestAccessForMediaType_completionHandler(mt, &blk);
        }
        if let Some(g) = pump_recv(&rx, timeout) {
            return g;
        }
    } else {
        eprintln!(">>> {what} permission is '{}'. Enable it in System Settings > Privacy & Security > {what}; polling for {}s...", status_name(st), timeout.as_secs());
    }
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if auth_status(video) == AVAuthorizationStatus::Authorized {
            return true;
        }
        pump_for(Duration::from_millis(500));
    }
    false
}

pub fn video_device_types() -> Vec<&'static NSString> {
    unsafe {
        vec![
            AVCaptureDeviceTypeBuiltInWideAngleCamera,
            AVCaptureDeviceTypeExternal,
            AVCaptureDeviceTypeContinuityCamera,
            AVCaptureDeviceTypeDeskViewCamera,
        ]
    }
}

pub fn discover(video: bool) -> Vec<Retained<AVCaptureDevice>> {
    unsafe {
        let types: Vec<&NSString> = if video { video_device_types() } else { vec![AVCaptureDeviceTypeMicrophone] };
        let arr = NSArray::from_slice(&types);
        let mt = if video { AVMediaTypeVideo.unwrap() } else { AVMediaTypeAudio.unwrap() };
        let ds = AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(&arr, Some(mt), AVCaptureDevicePosition::Unspecified);
        ds.devices().iter().collect()
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct FormatChoice {
    pub index: usize,
    pub width: i32,
    pub height: i32,
    pub subtype: String,
    pub max_fps: f64,
}

pub fn best_format(dev: &AVCaptureDevice, max_w: i32, max_h: i32, fps: f64) -> Option<(Retained<AVCaptureDeviceFormat>, FormatChoice)> {
    let mut best: Option<(Retained<AVCaptureDeviceFormat>, FormatChoice, i64)> = None;
    unsafe {
        for (i, f) in dev.formats().iter().enumerate() {
            let fd = f.formatDescription();
            let dims = CMVideoFormatDescriptionGetDimensions(&fd);
            if dims.width > max_w || dims.height > max_h {
                continue;
            }
            let max_fps = f.videoSupportedFrameRateRanges().iter().map(|r| r.maxFrameRate()).fold(0.0, f64::max);
            if max_fps + 0.01 < fps {
                continue;
            }
            let sub = fd.media_sub_type();
            let sub_bonus = if sub == PIX_420V || sub == u32::from_be_bytes(*b"420f") { 1 } else { 0 };
            let score = (dims.width as i64 * dims.height as i64) * 4 + sub_bonus * 2 - (max_fps > 60.0) as i64;
            let choice = FormatChoice { index: i, width: dims.width, height: dims.height, subtype: fourcc(sub), max_fps };
            if best.as_ref().map(|b| score > b.2).unwrap_or(true) {
                best = Some((f.clone(), choice, score));
            }
        }
    }
    best.map(|b| (b.0, b.1))
}

#[derive(Default)]
pub struct AvTrack {
    pub kind: &'static str,
    pub path: PathBuf,
    pub writer: Option<TrackWriter>,
    pub writer_error: Option<String>,
    pub video_cfg: Option<VideoCfg>,
    pub frag: f64,
    pub received: u64,
    pub dropped_by_capture: u64,
    pub pts: Vec<f64>,
    pub clock_probe: Vec<(f64, f64)>,
    pub dims: Option<(usize, usize)>,
    pub audio_bufs: Vec<(f64, u64, f64)>,
}

pub struct AvShared {
    pub track: Mutex<AvTrack>,
    pub accepting: AtomicBool,
}

impl AvShared {
    fn on_sample(&self, sb: &CMSampleBuffer) {
        if !self.accepting.load(Ordering::Acquire) {
            return;
        }
        let now = host_now_secs();
        let pts = secs(unsafe { sb.presentation_time_stamp() });
        let mut t = self.track.lock().unwrap();
        t.received += 1;
        if t.clock_probe.len() < 50 {
            t.clock_probe.push((pts, now));
        }
        if t.writer.is_none() && t.writer_error.is_none() {
            let r = if let Some(mut cfg) = t.video_cfg.clone() {
                if let Some(ib) = unsafe { sb.image_buffer() } {
                    cfg.width = CVPixelBufferGetWidth(&ib);
                    cfg.height = CVPixelBufferGetHeight(&ib);
                    t.dims = Some((cfg.width, cfg.height));
                }
                TrackWriter::video(t.kind, &t.path, &cfg, t.frag)
            } else {
                TrackWriter::aac_for(t.kind, &t.path, sb, t.frag)
            };
            match r {
                Ok(w) => t.writer = Some(w),
                Err(e) => t.writer_error = Some(e.to_string()),
            }
        }
        if t.video_cfg.is_some() {
            t.pts.push(pts);
        } else if t.audio_bufs.len() < 20_000 {
            let n = unsafe { sb.num_samples() } as u64;
            let sr = crate::writer::asbd_of(sb).map(|a| a.mSampleRate).unwrap_or(48_000.0);
            t.audio_bufs.push((pts, n, sr));
        }
        if let Some(w) = t.writer.as_mut() {
            w.append(sb);
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SCSpikeCaptureDelegate"]
    #[ivars = Arc<AvShared>]
    pub struct CaptureDelegate;

    unsafe impl NSObjectProtocol for CaptureDelegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CaptureDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_output(&self, _o: &AVCaptureOutput, sb: &CMSampleBuffer, _c: &AVCaptureConnection) {
            self.ivars().on_sample(sb);
        }

        #[unsafe(method(captureOutput:didDropSampleBuffer:fromConnection:))]
        fn did_drop(&self, _o: &AVCaptureOutput, _sb: &CMSampleBuffer, _c: &AVCaptureConnection) {
            self.ivars().track.lock().unwrap().dropped_by_capture += 1;
        }
    }

    unsafe impl AVCaptureAudioDataOutputSampleBufferDelegate for CaptureDelegate {}
);

impl CaptureDelegate {
    fn new(shared: Arc<AvShared>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(shared);
        unsafe { msg_send![super(this), init] }
    }
}

pub struct AvCapture {
    pub session: Retained<AVCaptureSession>,
    pub shared: Arc<AvShared>,
    pub device_name: String,
    pub device_type: String,
    pub format: Option<FormatChoice>,
    _delegate: Retained<CaptureDelegate>,
    _queue: DispatchRetained<DispatchQueue>,
}

impl AvCapture {
    pub fn camera(dir: &Path, dev: &AVCaptureDevice, fps: f64, frag: f64) -> Result<Self> {
        unsafe {
            let session = AVCaptureSession::new();
            session.beginConfiguration();
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(dev).map_err(|e| anyhow!("camera input: {}", e.localizedDescription()))?;
            if !session.canAddInput(&input) {
                return Err(anyhow!("cannot add camera input"));
            }
            session.addInput(&input);
            let out = AVCaptureVideoDataOutput::new();
            let pf = num_i(PIX_420V as i64);
            let vs = dict(&[(cfstr_as_ns(kCVPixelBufferPixelFormatTypeKey), as_obj(&*pf))]);
            out.setVideoSettings(Some(&*(Retained::as_ptr(&vs) as *const objc2_foundation::NSDictionary<NSString, AnyObject>)));
            out.setAlwaysDiscardsLateVideoFrames(true);
            let shared = Arc::new(AvShared {
                track: Mutex::new(AvTrack {
                    kind: "camera",
                    path: dir.join("camera.mov"),
                    video_cfg: Some(VideoCfg { width: 0, height: 0, fps, bitrate: 10e6, hevc: true, realtime: true }),
                    frag,
                    ..Default::default()
                }),
                accepting: AtomicBool::new(true),
            });
            let delegate = CaptureDelegate::new(shared.clone());
            let queue = DispatchQueue::new("simplecapture.spike.camera", DispatchQueueAttr::SERIAL);
            out.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*delegate)), Some(&queue));
            if !session.canAddOutput(&out) {
                return Err(anyhow!("cannot add camera output"));
            }
            session.addOutput(&out);
            let fmt = best_format(dev, 1920, 1080, fps);
            if let Some((f, _)) = &fmt {
                dev.lockForConfiguration().map_err(|e| anyhow!("lockForConfiguration: {}", e.localizedDescription()))?;
                dev.setActiveFormat(f);
                let d = CMTime::new(1000, (fps * 1000.0) as i32);
                dev.setActiveVideoMinFrameDuration(d);
                dev.setActiveVideoMaxFrameDuration(d);
                dev.unlockForConfiguration();
            }
            session.commitConfiguration();
            Ok(Self {
                session,
                shared,
                device_name: dev.localizedName().to_string(),
                device_type: dev.deviceType().to_string(),
                format: fmt.map(|f| f.1),
                _delegate: delegate,
                _queue: queue,
            })
        }
    }

    pub fn mic(dir: &Path, dev: &AVCaptureDevice, frag: f64) -> Result<Self> {
        unsafe {
            let session = AVCaptureSession::new();
            session.beginConfiguration();
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(dev).map_err(|e| anyhow!("mic input: {}", e.localizedDescription()))?;
            if !session.canAddInput(&input) {
                return Err(anyhow!("cannot add mic input"));
            }
            session.addInput(&input);
            let out = AVCaptureAudioDataOutput::new();
            let shared = Arc::new(AvShared {
                track: Mutex::new(AvTrack { kind: "mic", path: dir.join("mic.mov"), frag, ..Default::default() }),
                accepting: AtomicBool::new(true),
            });
            let delegate = CaptureDelegate::new(shared.clone());
            let queue = DispatchQueue::new("simplecapture.spike.mic", DispatchQueueAttr::SERIAL);
            out.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*delegate)), Some(&queue));
            if !session.canAddOutput(&out) {
                return Err(anyhow!("cannot add mic output"));
            }
            session.addOutput(&out);
            session.commitConfiguration();
            Ok(Self {
                session,
                shared,
                device_name: dev.localizedName().to_string(),
                device_type: dev.deviceType().to_string(),
                format: None,
                _delegate: delegate,
                _queue: queue,
            })
        }
    }

    pub fn start(&self) {
        unsafe { self.session.startRunning() };
    }

    pub fn stop(&self) {
        unsafe { self.session.stopRunning() };
        self.shared.accepting.store(false, Ordering::Release);
    }

    pub fn sync_clock_vs_host(&self) -> Option<(f64, f64)> {
        unsafe {
            let c = self.session.synchronizationClock()?;
            Some((secs(c.time()), host_now_secs()))
        }
    }
}
