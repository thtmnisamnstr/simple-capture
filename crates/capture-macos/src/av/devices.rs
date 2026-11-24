use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureAudioDataOutput, AVCaptureAudioDataOutputSampleBufferDelegate,
    AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceDiscoverySession, AVCaptureDeviceFormat,
    AVCaptureDeviceInput, AVCaptureDevicePosition, AVCaptureDeviceTypeBuiltInWideAngleCamera,
    AVCaptureDeviceTypeContinuityCamera, AVCaptureDeviceTypeDeskViewCamera,
    AVCaptureDeviceTypeExternal, AVCaptureDeviceTypeMicrophone,
    AVCaptureDeviceWasConnectedNotification, AVCaptureDeviceWasDisconnectedNotification,
    AVCaptureOutput, AVCaptureSession, AVCaptureSystemUserInterface, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeAudio, AVMediaTypeMuxed,
    AVMediaTypeVideo,
};
use objc2_core_foundation::CFType;
use objc2_core_media::{
    CMClock, CMSampleBuffer, CMSyncConvertTime, CMTime, CMVideoFormatDescriptionGetDimensions,
};
use objc2_core_video::kCVPixelBufferPixelFormatTypeKey;
use objc2_foundation::{NSArray, NSDictionary, NSNotification, NSNotificationCenter, NSString};
use sc_core::time::HostTime;
use sc_geometry::SizePx;
use sc_platform::{
    AudioTrackKind, CameraLimit, CaptureDevice, DeviceKind, DeviceStream, Error, FrameBuffer,
    FrameSink, PermissionKind, Result, SourceEvent, StreamTag, VideoFrame,
};

use super::frames::{MacAudioSample, MacPixelFrame, PIX_420F, PIX_420V, asbd_of_desc};
use super::writer::key;
use crate::util::{
    as_obj, cfstr_as_ns, cmtime_to_ns, dict, fourcc, ns_error_text, nsstr, num_b, num_f, num_i,
    objc_try,
};

pub const CAMERA_TAG_BASE: u64 = 1 << 48;
static NEXT_CAMERA_TAG: AtomicU64 = AtomicU64::new(CAMERA_TAG_BASE);
static RUNNING: Mutex<Vec<(DeviceKind, String)>> = Mutex::new(Vec::new());

fn running() -> MutexGuard<'static, Vec<(DeviceKind, String)>> {
    RUNNING.lock().unwrap_or_else(|p| p.into_inner())
}

fn media_type(kind: DeviceKind) -> Result<&'static NSString> {
    // SAFETY: framework constants.
    unsafe {
        match kind {
            DeviceKind::Camera => key(AVMediaTypeVideo, "AVMediaTypeVideo"),
            DeviceKind::Mic => key(AVMediaTypeAudio, "AVMediaTypeAudio"),
        }
    }
}

#[repr(C)]
struct CmioAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreMediaIO", kind = "framework")]
unsafe extern "C" {
    fn CMIOObjectSetPropertyData(
        object: u32,
        address: *const CmioAddress,
        qualifier_size: u32,
        qualifier: *const std::ffi::c_void,
        data_size: u32,
        data: *const std::ffi::c_void,
    ) -> i32;
}

/// Lets plugged-in iPhones and iPads show up as capture devices (their screens, as QuickTime
/// records them). They appear a moment later, with a device-connected notification.
pub(crate) fn allow_screen_devices() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let address = CmioAddress {
            selector: u32::from_be_bytes(*b"yes "),
            scope: u32::from_be_bytes(*b"glob"),
            element: 0,
        };
        let allow: u32 = 1;
        // SAFETY: the system object (1) takes a UInt32 for this property.
        let status = unsafe {
            CMIOObjectSetPropertyData(
                1,
                &address,
                0,
                std::ptr::null(),
                4,
                (&raw const allow).cast(),
            )
        };
        if status != 0 {
            tracing::warn!(status, "could not allow device screens");
        }
    });
}

/// Plugged-in devices whose screen can be recorded.
fn screen_devices() -> Vec<Retained<AVCaptureDevice>> {
    allow_screen_devices();
    // SAFETY: framework constants; discovery needs no permission and never prompts.
    unsafe {
        let Some(muxed) = AVMediaTypeMuxed else {
            return Vec::new();
        };
        let arr = NSArray::from_slice(&[AVCaptureDeviceTypeExternal]);
        AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
            &arr,
            Some(muxed),
            AVCaptureDevicePosition::Unspecified,
        )
        .devices()
        .iter()
        .collect()
    }
}

fn is_screen_device(dev: &AVCaptureDevice) -> bool {
    // SAFETY: plain getter; framework constant.
    unsafe { AVMediaTypeMuxed.is_some_and(|m| dev.hasMediaType(m)) }
}

/// The device gives `kind`; a device screen counts as a camera.
fn offers(dev: &AVCaptureDevice, kind: DeviceKind) -> Result<bool> {
    // SAFETY: plain getter.
    let direct = unsafe { dev.hasMediaType(media_type(kind)?) };
    Ok(direct || (kind == DeviceKind::Camera && is_screen_device(dev)))
}

fn discover(kind: DeviceKind) -> Result<Vec<Retained<AVCaptureDevice>>> {
    // SAFETY: framework constants; discovery needs no permission and never prompts.
    unsafe {
        let types: Vec<&NSString> = match kind {
            DeviceKind::Camera => vec![
                AVCaptureDeviceTypeBuiltInWideAngleCamera,
                AVCaptureDeviceTypeExternal,
                AVCaptureDeviceTypeContinuityCamera,
                AVCaptureDeviceTypeDeskViewCamera,
            ],
            DeviceKind::Mic => vec![AVCaptureDeviceTypeMicrophone, AVCaptureDeviceTypeExternal],
        };
        let arr = NSArray::from_slice(&types);
        let ds =
            AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
                &arr,
                Some(media_type(kind)?),
                AVCaptureDevicePosition::Unspecified,
            );
        let mut found: Vec<Retained<AVCaptureDevice>> = ds.devices().iter().collect();
        if kind == DeviceKind::Camera {
            found.extend(screen_devices());
        }
        Ok(found)
    }
}

fn kind_hint(dev: &AVCaptureDevice) -> Option<String> {
    // SAFETY: plain getters; framework constants.
    unsafe {
        let t = dev.deviceType();
        let hint = if is_screen_device(dev) {
            "screen"
        } else if dev.isContinuityCamera() || &*t == AVCaptureDeviceTypeContinuityCamera {
            "continuity"
        } else if &*t == AVCaptureDeviceTypeDeskViewCamera {
            "desk_view"
        } else if &*t == AVCaptureDeviceTypeBuiltInWideAngleCamera {
            "builtin"
        } else if &*t == AVCaptureDeviceTypeExternal {
            "external"
        } else if &*t == AVCaptureDeviceTypeMicrophone {
            "microphone"
        } else {
            return Some(t.to_string());
        };
        Some(hint.to_string())
    }
}

pub(crate) fn list(kind: DeviceKind) -> Result<Vec<CaptureDevice>> {
    // SAFETY: plain getters.
    let default_id = unsafe { AVCaptureDevice::defaultDeviceWithMediaType(media_type(kind)?) }
        .map(|d| unsafe { d.uniqueID() }.to_string());
    let mut out: Vec<CaptureDevice> = Vec::new();
    for d in discover(kind)? {
        // SAFETY: plain getters.
        let (id, name, model) = unsafe {
            (
                d.uniqueID().to_string(),
                d.localizedName().to_string(),
                d.modelID().to_string(),
            )
        };
        if out.iter().any(|c| c.id == id) {
            continue;
        }
        out.push(CaptureDevice {
            is_default: default_id.as_deref() == Some(id.as_str()),
            id,
            name,
            model_id: (!model.is_empty()).then_some(model),
            kind_hint: kind_hint(&d),
        });
    }
    Ok(out)
}

fn find(kind: DeviceKind, id: &str) -> Result<Retained<AVCaptureDevice>> {
    // SAFETY: plain lookup.
    let dev = unsafe { AVCaptureDevice::deviceWithUniqueID(&nsstr(id)) }
        .ok_or_else(|| Error::NotFound(format!("{kind:?} device {id}")))?;
    if !offers(&dev, kind)? {
        return Err(Error::NotFound(format!("{id} is not a {kind:?} device")));
    }
    Ok(dev)
}

fn require_authorized(kind: DeviceKind) -> Result<()> {
    // SAFETY: a status read; never prompts.
    let st = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type(kind)?) };
    if st == AVAuthorizationStatus::Authorized {
        Ok(())
    } else {
        Err(Error::PermissionDenied(match kind {
            DeviceKind::Camera => PermissionKind::Camera,
            DeviceKind::Mic => PermissionKind::Microphone,
        }))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FormatChoice {
    pub size: SizePx,
    pub subtype: u32,
    pub max_fps: f64,
}

/// Largest format within `limit` that reaches `limit.max_fps`, preferring 420v/420f; when none
/// reaches that rate, the fastest one within the size, then the largest.
pub(crate) fn best_format(
    dev: &AVCaptureDevice,
    limit: CameraLimit,
) -> Option<(Retained<AVCaptureDeviceFormat>, FormatChoice)> {
    let mut fast: Option<(Retained<AVCaptureDeviceFormat>, FormatChoice, i64)> = None;
    let mut slow: Option<(Retained<AVCaptureDeviceFormat>, FormatChoice, (i64, i64))> = None;
    // SAFETY: plain getters on a device and its formats.
    unsafe {
        for f in dev.formats().iter() {
            let fd = f.formatDescription();
            let dims = CMVideoFormatDescriptionGetDimensions(&fd);
            let (w, h) = (
                u32::try_from(dims.width).unwrap_or(0),
                u32::try_from(dims.height).unwrap_or(0),
            );
            if w == 0 || h == 0 || w > limit.max.w || h > limit.max.h {
                continue;
            }
            let max_fps = f
                .videoSupportedFrameRateRanges()
                .iter()
                .map(|r| r.maxFrameRate())
                .fold(0.0, f64::max);
            let sub = fd.media_sub_type();
            let nv12 = i64::from(sub == PIX_420V || sub == PIX_420F);
            let area = i64::from(w) * i64::from(h);
            let choice = FormatChoice {
                size: SizePx { w, h },
                subtype: sub,
                max_fps,
            };
            if max_fps + 0.01 >= f64::from(limit.max_fps) {
                let score = area * 4 + nv12 * 2 - i64::from(max_fps > 60.0);
                if fast.as_ref().is_none_or(|b| score > b.2) {
                    fast = Some((f.clone(), choice, score));
                }
            } else {
                let score = ((max_fps * 100.0) as i64, area * 4 + nv12 * 2);
                if slow.as_ref().is_none_or(|b| score > b.2) {
                    slow = Some((f.clone(), choice, score));
                }
            }
        }
    }
    fast.map(|b| (b.0, b.1)).or(slow.map(|b| (b.0, b.1)))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RateRange {
    pub min_fps: f64,
    pub max_fps: f64,
    pub min_duration: (i64, i32),
    pub max_duration: (i64, i32),
}

fn rate_ranges(f: &AVCaptureDeviceFormat) -> Vec<RateRange> {
    // SAFETY: plain getters on a format's frame rate ranges.
    unsafe {
        f.videoSupportedFrameRateRanges()
            .iter()
            .map(|r| {
                let (mn, mx) = (r.minFrameDuration(), r.maxFrameDuration());
                RateRange {
                    min_fps: r.minFrameRate(),
                    max_fps: r.maxFrameRate(),
                    min_duration: ({ mn.value }, { mn.timescale }),
                    max_duration: ({ mx.value }, { mx.timescale }),
                }
            })
            .collect()
    }
}

/// The frame duration (value, timescale) closest to `target_fps` that the device accepts, and
/// its rate. AVFoundation raises an exception for any duration outside every supported range,
/// and a discrete range's own duration (1000000/30000030 on some webcams) never equals a
/// rounded 1/30, so range ends are always taken verbatim from the device.
pub(crate) fn frame_duration_for(
    ranges: &[RateRange],
    target_fps: f64,
) -> Option<((i64, i32), f64)> {
    ranges
        .iter()
        .map(|r| {
            if target_fps >= r.max_fps {
                (r.min_duration, r.max_fps)
            } else if target_fps <= r.min_fps {
                (r.max_duration, r.min_fps)
            } else {
                ((1000, (target_fps * 1000.0).round() as i32), target_fps)
            }
        })
        .filter(|((v, ts), fps)| *v > 0 && *ts > 0 && fps.is_finite() && *fps > 0.0)
        .min_by(|a, b| {
            let (da, db) = ((a.1 - target_fps).abs(), (b.1 - target_fps).abs());
            da.total_cmp(&db).then(b.1.total_cmp(&a.1))
        })
}

struct SendClock(Retained<CMClock>);
// SAFETY: CMClocks are thread-safe CF objects.
unsafe impl Send for SendClock {}

pub(crate) struct DelegateState {
    kind: DeviceKind,
    audio: AudioTrackKind,
    tag: StreamTag,
    sink: Arc<dyn FrameSink>,
    accepting: AtomicBool,
    clock: Mutex<Option<SendClock>>,
    delivered: AtomicU64,
    dropped: AtomicU64,
}

impl DelegateState {
    fn host_ns(&self, pts: CMTime) -> Option<i64> {
        let g = self.clock.lock().unwrap_or_else(|p| p.into_inner());
        let t = match &*g {
            Some(c) => {
                // SAFETY: the host time clock is a process-wide singleton.
                let host = unsafe { CMClock::host_time_clock() };
                let from: &CFType = &c.0;
                let to: &CFType = &host;
                // SAFETY: both arguments are clocks.
                unsafe { CMSyncConvertTime(pts, from, to) }
            }
            None => pts,
        };
        cmtime_to_ns(t)
    }

    fn on_sample(&self, sb: &CMSampleBuffer) {
        if !self.accepting.load(Ordering::Acquire) {
            return;
        }
        // SAFETY: valid sample buffer.
        let Some(pts) = self.host_ns(unsafe { sb.presentation_time_stamp() }) else {
            return;
        };
        match self.kind {
            DeviceKind::Camera => {
                let Some(frame) = MacPixelFrame::from_sample_buffer(sb) else {
                    return;
                };
                let size = sc_platform::NativeFrame::size(&frame);
                self.sink.on_camera(VideoFrame {
                    stream: self.tag,
                    pts: HostTime(pts),
                    size,
                    content_rect_pt: None,
                    content_scale: None,
                    buffer: FrameBuffer(Arc::new(frame)),
                });
            }
            DeviceKind::Mic => {
                let Some(mut a) = MacAudioSample::audio_buffer(sb) else {
                    return;
                };
                a.pts = HostTime(pts);
                self.sink.on_audio(self.audio, a);
            }
        }
        self.delivered.fetch_add(1, Ordering::Relaxed);
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SCAvCaptureDelegate"]
    #[ivars = Arc<DelegateState>]
    struct CaptureDelegate;

    unsafe impl NSObjectProtocol for CaptureDelegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CaptureDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_output(&self, _o: &AVCaptureOutput, sb: &CMSampleBuffer, _c: &AVCaptureConnection) {
            self.ivars().on_sample(sb);
        }

        #[unsafe(method(captureOutput:didDropSampleBuffer:fromConnection:))]
        fn did_drop(&self, _o: &AVCaptureOutput, _sb: &CMSampleBuffer, _c: &AVCaptureConnection) {
            self.ivars().dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    unsafe impl AVCaptureAudioDataOutputSampleBufferDelegate for CaptureDelegate {}
);

impl CaptureDelegate {
    fn new(state: Arc<DelegateState>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(state);
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }
}

enum Output {
    Video(Retained<AVCaptureVideoDataOutput>),
    Audio(Retained<AVCaptureAudioDataOutput>),
}

/// The sound output a plugged-in device's screen session also carries.
struct Sound {
    output: Retained<AVCaptureAudioDataOutput>,
    state: Arc<DelegateState>,
    _delegate: Retained<CaptureDelegate>,
    _queue: DispatchRetained<DispatchQueue>,
}

pub(crate) struct AvDeviceStream {
    session: Retained<AVCaptureSession>,
    output: Output,
    sound: Option<Sound>,
    state: Arc<DelegateState>,
    _delegate: Retained<CaptureDelegate>,
    _queue: DispatchRetained<DispatchQueue>,
    format: String,
    device_id: String,
}

// SAFETY: AVCaptureSession is documented as thread-safe for start/stop; the stream is only
// stopped once (`self: Box<Self>`).
unsafe impl Send for AvDeviceStream {}

impl AvDeviceStream {
    #[allow(dead_code)]
    pub(crate) fn delivered(&self) -> u64 {
        self.state.delivered.load(Ordering::Relaxed)
    }
}

impl DeviceStream for AvDeviceStream {
    fn format(&self) -> String {
        self.format.clone()
    }

    fn stop(self: Box<Self>) -> Result<()> {
        self.state.accepting.store(false, Ordering::Release);
        if let Some(s) = &self.sound {
            s.state.accepting.store(false, Ordering::Release);
        }
        // SAFETY: stopping a configured session; detaching the delegates afterwards.
        unsafe {
            self.session.stopRunning();
            match &self.output {
                Output::Video(o) => o.setSampleBufferDelegate_queue(None, None),
                Output::Audio(o) => o.setSampleBufferDelegate_queue(None, None),
            }
            if let Some(s) = &self.sound {
                s.output.setSampleBufferDelegate_queue(None, None);
            }
        }
        let kind = self.state.kind;
        running().retain(|(k, id)| !(*k == kind && *id == self.device_id));
        Ok(())
    }
}

fn stereo_f32_settings(rate: f64) -> Retained<NSDictionary<NSString, AnyObject>> {
    let lpcm = num_i(i64::from(u32::from_be_bytes(*b"lpcm")));
    let (yes, no, bits, sr, ch) = (num_b(true), num_b(false), num_i(32), num_f(rate), num_i(2));
    let keys = [
        nsstr("AVFormatIDKey"),
        nsstr("AVLinearPCMIsFloatKey"),
        nsstr("AVLinearPCMBitDepthKey"),
        nsstr("AVLinearPCMIsNonInterleaved"),
        nsstr("AVLinearPCMIsBigEndianKey"),
        nsstr("AVSampleRateKey"),
        nsstr("AVNumberOfChannelsKey"),
    ];
    dict(&[
        (&keys[0], as_obj(&*lpcm)),
        (&keys[1], as_obj(&*yes)),
        (&keys[2], as_obj(&*bits)),
        (&keys[3], as_obj(&*no)),
        (&keys[4], as_obj(&*no)),
        (&keys[5], as_obj(&*sr)),
        (&keys[6], as_obj(&*ch)),
    ])
}

fn apply_camera_format(
    dev: &AVCaptureDevice,
    f: &AVCaptureDeviceFormat,
    choice: &FormatChoice,
    limit: CameraLimit,
) -> Result<String> {
    let target = f64::from(limit.max_fps).min(choice.max_fps).max(1.0);
    let picked = frame_duration_for(&rate_ranges(f), target);
    // SAFETY: the device is locked for configuration around every setter.
    unsafe {
        dev.lockForConfiguration()
            .map_err(|e| Error::Other(format!("lockForConfiguration: {}", ns_error_text(&e))))?;
        let set = objc_try("setting the camera format", || {
            dev.setActiveFormat(f);
            if let Some(((value, timescale), _)) = picked {
                let d = CMTime::new(value, timescale);
                dev.setActiveVideoMinFrameDuration(d);
                dev.setActiveVideoMaxFrameDuration(d);
            }
        });
        dev.unlockForConfiguration();
        let size = format!(
            "{}x{} {}",
            choice.size.w,
            choice.size.h,
            fourcc(choice.subtype)
        );
        Ok(match (set, picked) {
            (Ok(()), Some((_, fps))) => format!("{size} @ {fps:.2} fps (delivered as 420v)"),
            (Ok(()), None) => format!("{size} @ device default fps (delivered as 420v)"),
            (Err(e), _) => format!("{size}, device default rate ({e})"),
        })
    }
}

pub(crate) fn start(
    kind: DeviceKind,
    device_id: &str,
    limit: Option<CameraLimit>,
    sink: Arc<dyn FrameSink>,
) -> Result<Box<dyn DeviceStream>> {
    crate::tcc::guard(match kind {
        DeviceKind::Camera => "AVCaptureSession camera",
        DeviceKind::Mic => "AVCaptureSession microphone",
    })?;
    require_authorized(kind)?;
    let dev = find(kind, device_id)?;
    let tag = match kind {
        DeviceKind::Camera => StreamTag(NEXT_CAMERA_TAG.fetch_add(1, Ordering::Relaxed)),
        DeviceKind::Mic => StreamTag(0),
    };
    let screen_device = kind == DeviceKind::Camera && is_screen_device(&dev);
    let sound_state = screen_device.then(|| {
        Arc::new(DelegateState {
            kind: DeviceKind::Mic,
            audio: AudioTrackKind::DeviceAudio,
            tag: StreamTag(0),
            sink: sink.clone(),
            accepting: AtomicBool::new(true),
            clock: Mutex::new(None),
            delivered: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        })
    });
    let state = Arc::new(DelegateState {
        kind,
        audio: AudioTrackKind::Mic,
        tag,
        sink,
        accepting: AtomicBool::new(true),
        clock: Mutex::new(None),
        delivered: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
    });
    let delegate = CaptureDelegate::new(state.clone());
    let queue = DispatchQueue::new(
        match kind {
            DeviceKind::Camera => "com.simplecapture.av.camera",
            DeviceKind::Mic => "com.simplecapture.av.mic",
        },
        DispatchQueueAttr::SERIAL,
    );
    let configure = || -> Result<_> {
        // SAFETY: standard AVCaptureSession configuration on a device this process may open.
        let configured = unsafe {
            let session = AVCaptureSession::new();
            session.beginConfiguration();
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&dev).map_err(|e| {
                Error::Other(format!(
                    "opening {kind:?} {device_id}: {}",
                    ns_error_text(&e)
                ))
            })?;
            if !session.canAddInput(&input) {
                session.commitConfiguration();
                return Err(Error::Busy(format!(
                    "cannot add {kind:?} input {device_id}"
                )));
            }
            session.addInput(&input);
            let (output, format) = match kind {
                DeviceKind::Camera => {
                    let out = AVCaptureVideoDataOutput::new();
                    let pf = num_i(i64::from(PIX_420V));
                    let vs = dict(&[(cfstr_as_ns(kCVPixelBufferPixelFormatTypeKey), as_obj(&*pf))]);
                    out.setVideoSettings(Some(&vs));
                    out.setAlwaysDiscardsLateVideoFrames(true);
                    out.setSampleBufferDelegate_queue(
                        Some(ProtocolObject::from_ref(&*delegate)),
                        Some(&queue),
                    );
                    if !session.canAddOutput(&out) {
                        session.commitConfiguration();
                        return Err(Error::Other("cannot add the camera output".into()));
                    }
                    session.addOutput(&out);
                    let limit = limit.unwrap_or(CameraLimit {
                        max: SizePx { w: 1920, h: 1080 },
                        max_fps: 30,
                    });
                    let format = match best_format(&dev, limit) {
                        Some((f, choice)) => apply_camera_format(&dev, &f, &choice, limit)?,
                        None => {
                            let fd = dev.activeFormat().formatDescription();
                            let dims = CMVideoFormatDescriptionGetDimensions(&fd);
                            format!(
                                "{}x{} {} (no format within {}x{} @ {}; device default)",
                                dims.width,
                                dims.height,
                                fourcc(fd.media_sub_type()),
                                limit.max.w,
                                limit.max.h,
                                limit.max_fps
                            )
                        }
                    };
                    (Output::Video(out), format)
                }
                DeviceKind::Mic => {
                    let out = AVCaptureAudioDataOutput::new();
                    out.setSampleBufferDelegate_queue(
                        Some(ProtocolObject::from_ref(&*delegate)),
                        Some(&queue),
                    );
                    let fd = dev.activeFormat().formatDescription();
                    let asbd = asbd_of_desc(&fd);
                    let downmix = asbd.as_ref().filter(|a| a.mChannelsPerFrame > 2);
                    if let Some(a) = downmix {
                        out.setAudioSettings(Some(&stereo_f32_settings(a.mSampleRate)));
                    }
                    if !session.canAddOutput(&out) {
                        session.commitConfiguration();
                        return Err(Error::Other("cannot add the microphone output".into()));
                    }
                    session.addOutput(&out);
                    let format = match &asbd {
                        Some(a) => format!(
                            "{} Hz {} ch {} {}-bit{}",
                            a.mSampleRate,
                            a.mChannelsPerFrame,
                            fourcc(a.mFormatID),
                            a.mBitsPerChannel,
                            if downmix.is_some() {
                                " (downmixed to stereo)"
                            } else {
                                ""
                            }
                        ),
                        None => "audio".to_string(),
                    };
                    (Output::Audio(out), format)
                }
            };
            let sound = sound_state.clone().and_then(|st| {
                let delegate = CaptureDelegate::new(st.clone());
                let queue = DispatchQueue::new(
                    "com.simplecapture.av.device-sound",
                    DispatchQueueAttr::SERIAL,
                );
                let out = AVCaptureAudioDataOutput::new();
                out.setSampleBufferDelegate_queue(
                    Some(ProtocolObject::from_ref(&*delegate)),
                    Some(&queue),
                );
                if !session.canAddOutput(&out) {
                    tracing::info!("{device_id}: the device offers no sound output");
                    return None;
                }
                session.addOutput(&out);
                Some(Sound {
                    output: out,
                    state: st,
                    _delegate: delegate,
                    _queue: queue,
                })
            });
            session.commitConfiguration();
            (session, output, sound, format)
        };
        Ok(configured)
    };
    let (session, output, sound, format) =
        objc_try(&format!("configuring {kind:?} {device_id}"), configure)??;
    // SAFETY: plain getter.
    if let Some(c) = unsafe { session.synchronizationClock() } {
        if let Some(s) = &sound {
            *s.state.clock.lock().unwrap_or_else(|p| p.into_inner()) = Some(SendClock(c.clone()));
        }
        *state.clock.lock().unwrap_or_else(|p| p.into_inner()) = Some(SendClock(c));
    }
    // SAFETY: blocks until running; called off the main thread per the threading contract.
    let ok = objc_try(&format!("starting {kind:?} {device_id}"), || unsafe {
        session.startRunning();
        session.isRunning()
    })
    .unwrap_or(false);
    if !ok {
        state.accepting.store(false, Ordering::Release);
        if let Some(s) = &sound {
            s.state.accepting.store(false, Ordering::Release);
        }
        return Err(Error::Other(format!(
            "{kind:?} {device_id}: the capture session did not start"
        )));
    }
    running().push((kind, device_id.to_string()));
    Ok(Box::new(AvDeviceStream {
        session,
        output,
        sound,
        state,
        _delegate: delegate,
        _queue: queue,
        format,
        device_id: device_id.to_string(),
    }))
}

pub(crate) fn show_mic_modes() -> Result<()> {
    show_system_ui(AVCaptureSystemUserInterface::MicrophoneModes)
}

pub(crate) fn show_system_ui(ui: AVCaptureSystemUserInterface) -> Result<()> {
    let show = move || {
        // SAFETY: AppKit-side system UI request; on the main thread here.
        unsafe { AVCaptureDevice::showSystemUserInterface(ui) }
    };
    if MainThreadMarker::new().is_some() {
        show();
    } else {
        DispatchQueue::main().exec_async(show);
    }
    Ok(())
}

struct Observer(Retained<ProtocolObject<dyn NSObjectProtocol>>);
// SAFETY: observer tokens are only passed back to NSNotificationCenter, which is thread-safe.
unsafe impl Send for Observer {}

pub(crate) struct DeviceWatcher {
    observers: Vec<Observer>,
}

impl DeviceStream for DeviceWatcher {
    fn format(&self) -> String {
        "AVCaptureDevice connect/disconnect notifications".into()
    }

    fn stop(self: Box<Self>) -> Result<()> {
        let center = NSNotificationCenter::defaultCenter();
        for o in self.observers {
            // SAFETY: a token returned by addObserverForName.
            unsafe { center.removeObserver(as_obj(&*o.0)) };
        }
        Ok(())
    }
}

fn on_device_note(note: &NSNotification, connected: bool, sink: &Arc<dyn FrameSink>) {
    let Some(obj) = note.object() else {
        return;
    };
    let Ok(dev) = obj.downcast::<AVCaptureDevice>() else {
        return;
    };
    for kind in [DeviceKind::Camera, DeviceKind::Mic] {
        if !offers(&dev, kind).unwrap_or(false) {
            continue;
        }
        sink.on_event(SourceEvent::DevicesChanged { kind });
        if !connected {
            // SAFETY: plain getter.
            let id = unsafe { dev.uniqueID() }.to_string();
            let was_running = running().iter().any(|(k, d)| *k == kind && *d == id);
            if was_running {
                sink.on_event(SourceEvent::DeviceLost {
                    kind,
                    device_id: id,
                });
            }
        }
    }
}

pub(crate) fn watch(sink: Arc<dyn FrameSink>) -> Result<Box<dyn DeviceStream>> {
    allow_screen_devices();
    let center = NSNotificationCenter::defaultCenter();
    let mut observers = Vec::new();
    // SAFETY: framework notification names.
    let names = unsafe {
        [
            (AVCaptureDeviceWasConnectedNotification, true),
            (AVCaptureDeviceWasDisconnectedNotification, false),
        ]
    };
    for (name, connected) in names {
        let sink = sink.clone();
        let block = RcBlock::new(move |n: NonNull<NSNotification>| {
            // SAFETY: the center passes a valid notification.
            on_device_note(unsafe { n.as_ref() }, connected, &sink);
        });
        // SAFETY: no queue: the block runs on the posting thread and only forwards to the sink.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
        };
        observers.push(Observer(token));
    }
    Ok(Box::new(DeviceWatcher { observers }))
}
