use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{anyhow, Result};
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{kCGColorSpaceSRGB, kCGDisplayStreamYCbCrMatrix_ITU_R_709_2};
use objc2_core_media::{CMSampleBuffer, CMSampleTimingInfo, CMTime};
use objc2_core_video::*;
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString};
use objc2_screen_capture_kit::*;
use serde_json::{json, Value};

use crate::avcap::{self, AvCapture};
use crate::capture::{self, wait_err, PIX_420V};
use crate::record::{ensure_screen_access, summarize};
use crate::testwin::{self, TestWindow};
use crate::util::*;
use crate::writer::TrackWriter;
use crate::Args;

struct SendSb(CFRetained<CMSampleBuffer>);
unsafe impl Send for SendSb {}

struct Seg {
    label: &'static str,
    dims: (usize, usize),
    content_pts: (f64, f64),
    tag: u32,
    requested_host: f64,
    writer: Option<TrackWriter>,
    writer_error: Option<String>,
    first_pts: Option<f64>,
    last_pts: Option<f64>,
    last_sb: Option<SendSb>,
    frames: u64,
    closed: bool,
    end_host: Option<f64>,
    summary: Option<Value>,
}

#[derive(Clone)]
struct FrameRec {
    tag: u32,
    pts: f64,
    arrive: f64,
    w: usize,
    h: usize,
    sig: [u8; 10],
    seg: i32,
    geom: [f32; 6],
}

fn frame_geom(sb: &CMSampleBuffer) -> [f32; 6] {
    let mut g = [f32::NAN; 6];
    unsafe {
        let Some(arr) = sb.sample_attachments_array(false) else { return g };
        let arr: &NSArray<NSDictionary<NSString, AnyObject>> = &*(&*arr as *const objc2_core_foundation::CFArray as *const NSArray<NSDictionary<NSString, AnyObject>>);
        let Some(d) = arr.firstObject() else { return g };
        if let Some(o) = d.objectForKey(SCStreamFrameInfoContentRect) {
            let cf = &*(Retained::as_ptr(&o) as *const objc2_core_foundation::CFDictionary);
            let mut r = CGRect::default();
            if objc2_core_graphics::CGRectMakeWithDictionaryRepresentation(Some(cf), &mut r) {
                g[0] = r.origin.x as f32;
                g[1] = r.origin.y as f32;
                g[2] = r.size.width as f32;
                g[3] = r.size.height as f32;
            }
        }
        if let Some(n) = d.objectForKey(SCStreamFrameInfoContentScale).and_then(|o| o.downcast::<NSNumber>().ok()) {
            g[4] = n.doubleValue() as f32;
        }
        if let Some(n) = d.objectForKey(SCStreamFrameInfoScaleFactor).and_then(|o| o.downcast::<NSNumber>().ok()) {
            g[5] = n.doubleValue() as f32;
        }
    }
    g
}

#[derive(Default)]
struct SysAudio {
    writer: Option<TrackWriter>,
    writer_error: Option<String>,
    bufs: Vec<(f64, u64, f64, u32)>,
}

struct State {
    segs: Vec<Seg>,
    frames: Vec<FrameRec>,
    status: std::collections::BTreeMap<u32, [u64; 8]>,
    finishers: Vec<JoinHandle<(usize, Value, Option<String>)>>,
}

struct Shared {
    dir: PathBuf,
    fps: f64,
    bpp: f64,
    frag: f64,
    st: Mutex<State>,
    sys: Mutex<SysAudio>,
    audio_tag: AtomicU32,
    accepting: AtomicBool,
    stop_errors: Mutex<Vec<String>>,
    route_geom: bool,
}

fn sig_of(ib: &CVPixelBuffer) -> [u8; 10] {
    let mut out = [b'?'; 10];
    if CVPixelBufferGetPixelFormatType(ib) != PIX_420V {
        return out;
    }
    let (w, h) = (CVPixelBufferGetWidth(ib), CVPixelBufferGetHeight(ib));
    unsafe {
        if CVPixelBufferLockBaseAddress(ib, CVPixelBufferLockFlags::ReadOnly) != 0 {
            return out;
        }
        let y0 = CVPixelBufferGetBaseAddressOfPlane(ib, 0) as *const u8;
        let c0 = CVPixelBufferGetBaseAddressOfPlane(ib, 1) as *const u8;
        let ys = CVPixelBufferGetBytesPerRowOfPlane(ib, 0);
        let cs = CVPixelBufferGetBytesPerRowOfPlane(ib, 1);
        let pts = [(0.1, 0.1), (0.5, 0.1), (0.9, 0.1), (0.1, 0.5), (0.5, 0.5), (0.9, 0.5), (0.1, 0.9), (0.5, 0.9), (0.9, 0.9), (0.01, 0.01)];
        if !y0.is_null() && !c0.is_null() {
            for (i, (fx, fy)) in pts.iter().enumerate() {
                let x = ((fx * w as f64) as usize).min(w - 1);
                let y = ((fy * h as f64) as usize).min(h - 1);
                let yy = *y0.add(y * ys + x) as f64;
                let cb = *c0.add((y / 2) * cs + (x / 2) * 2) as f64;
                let cr = *c0.add((y / 2) * cs + (x / 2) * 2 + 1) as f64;
                let l = (yy - 16.0) / 219.0;
                let b_ = (cb - 128.0) / 224.0;
                let r_ = (cr - 128.0) / 224.0;
                let r = l + 1.5748 * r_;
                let g = l - 0.1873 * b_ - 0.4681 * r_;
                let b = l + 1.8556 * b_;
                out[i] = if g > 0.4 && r < 0.3 && b < 0.3 {
                    b'G'
                } else if r > 0.75 && g > 0.25 && g < 0.75 && b < 0.3 {
                    b'O'
                } else if r < 0.06 && g < 0.06 && b < 0.06 {
                    b'K'
                } else {
                    b'X'
                };
            }
        }
        CVPixelBufferUnlockBaseAddress(ib, CVPixelBufferLockFlags::ReadOnly);
    }
    out
}

fn class_of(sig: &[u8; 10]) -> &'static str {
    let g = sig.iter().filter(|c| **c == b'G').count();
    let o = sig.iter().filter(|c| **c == b'O').count();
    let k = sig.iter().filter(|c| **c == b'K').count();
    if sig.contains(&b'?') {
        "unknown"
    } else if g == 10 {
        "window"
    } else if o == 10 {
        "region"
    } else if g + o > 0 {
        "mixed"
    } else if k >= 8 {
        "black"
    } else {
        "display"
    }
}

fn append_hold(w: &mut TrackWriter, last: &CMSampleBuffer, at: f64) -> Option<f64> {
    let last_pts = secs(unsafe { last.presentation_time_stamp() });
    if at <= last_pts + 1e-3 {
        return Some(0.0);
    }
    let timing = CMSampleTimingInfo { duration: unsafe { objc2_core_media::kCMTimeInvalid }, presentationTimeStamp: cmtime_secs(at), decodeTimeStamp: unsafe { objc2_core_media::kCMTimeInvalid } };
    let mut out: *mut CMSampleBuffer = std::ptr::null_mut();
    let st = unsafe { CMSampleBuffer::create_copy_with_new_timing(None, last, 1, &timing, std::ptr::NonNull::from(&mut out)) };
    if st != 0 || out.is_null() {
        return None;
    }
    let copy = unsafe { CFRetained::from_raw(std::ptr::NonNull::new(out).unwrap()) };
    if w.append_wait(&copy) {
        Some(at - last_pts)
    } else {
        None
    }
}

fn close_seg(idx: usize, seg: &mut Seg, end: f64, fps: f64) -> Option<JoinHandle<(usize, Value, Option<String>)>> {
    if seg.closed {
        return None;
    }
    seg.closed = true;
    seg.end_host = Some(end);
    let mut w = seg.writer.take()?;
    let last = seg.last_sb.take();
    Some(std::thread::spawn(move || {
        let held = last.as_ref().and_then(|l| append_hold(&mut w, &l.0, end - 1.0 / fps));
        let err = w.finish(Some(cmtime_secs(end))).err().map(|e| e.to_string());
        let mut v = w.summary();
        v["held_last_frame_s"] = json!(held);
        (idx, v, err)
    }))
}

impl Shared {
    fn on_screen(&self, tag: u32, sb: &CMSampleBuffer) {
        let now = host_now_secs();
        let (status, _) = capture::frame_info(sb);
        let mut st = self.st.lock().unwrap();
        st.status.entry(tag).or_insert([0; 8])[status.unwrap_or(7).clamp(0, 7) as usize] += 1;
        if status != Some(SCFrameStatus::Complete.0 as i64) {
            return;
        }
        let Some(ib) = (unsafe { sb.image_buffer() }) else { return };
        let (w, h) = (CVPixelBufferGetWidth(&ib), CVPixelBufferGetHeight(&ib));
        let sig = sig_of(&ib);
        let geom = frame_geom(sb);
        let pts = secs(unsafe { sb.presentation_time_stamp() });
        let cur = st.segs.len() - 1;
        let geom_ok = |s: &Seg| {
            !self.route_geom
                || ((geom[2].is_nan() || ((geom[2] as f64 - s.content_pts.0).abs() < 1.0 && (geom[3] as f64 - s.content_pts.1).abs() < 1.0))
                    && (geom[4].is_nan() || (geom[4] - 1.0).abs() < 0.01))
        };
        let matches = |s: &Seg| s.tag == tag && s.dims == (w, h) && !s.closed && geom_ok(s);
        let target = if matches(&st.segs[cur]) {
            if cur > 0 && !st.segs[cur - 1].closed {
                let fps = self.fps;
                if let Some(j) = close_seg(cur - 1, &mut st.segs[cur - 1], pts, fps) {
                    st.finishers.push(j);
                }
            }
            Some(cur)
        } else if cur > 0 && matches(&st.segs[cur - 1]) {
            Some(cur - 1)
        } else {
            None
        };
        st.frames.push(FrameRec { tag, pts, arrive: now, w, h, sig, seg: target.map(|t| t as i32).unwrap_or(-1), geom });
        let Some(t) = target else { return };
        let seg = &mut st.segs[t];
        if seg.writer.is_none() && seg.writer_error.is_none() && seg.first_pts.is_none() {
            let cfg = crate::writer::VideoCfg { width: w, height: h, fps: self.fps, bitrate: (w * h) as f64 * self.fps * self.bpp, hevc: true, realtime: true };
            match TrackWriter::video("screen", &self.dir.join(format!("screen-{t:03}.mov")), &cfg, self.frag) {
                Ok(wr) => seg.writer = Some(wr),
                Err(e) => seg.writer_error = Some(e.to_string()),
            }
        }
        if seg.first_pts.is_none() {
            seg.first_pts = Some(pts);
        }
        seg.last_pts = Some(pts);
        seg.frames += 1;
        if let Some(wr) = seg.writer.as_mut() {
            wr.append(sb);
        }
        seg.last_sb = Some(SendSb(unsafe { CFRetained::retain(std::ptr::NonNull::from(sb)) }));
    }

    fn on_audio(&self, tag: u32, sb: &CMSampleBuffer) {
        if tag != self.audio_tag.load(Ordering::Acquire) {
            return;
        }
        let pts = secs(unsafe { sb.presentation_time_stamp() });
        let n = unsafe { sb.num_samples() } as u64;
        let sr = crate::writer::asbd_of(sb).map(|a| a.mSampleRate).unwrap_or(48_000.0);
        let mut a = self.sys.lock().unwrap();
        if a.bufs.len() < 20_000 {
            a.bufs.push((pts, n, sr, tag));
        }
        if a.writer.is_none() && a.writer_error.is_none() {
            match TrackWriter::aac_for("system_audio", &self.dir.join("system_audio.mov"), sb, self.frag) {
                Ok(w) => a.writer = Some(w),
                Err(e) => a.writer_error = Some(e.to_string()),
            }
        }
        if let Some(w) = a.writer.as_mut() {
            w.append(sb);
        }
    }
}

pub struct OutIvars {
    shared: Arc<Shared>,
    tag: u32,
    video: bool,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SCSpikeSwapOutput"]
    #[ivars = OutIvars]
    pub struct SwapOutput;

    unsafe impl NSObjectProtocol for SwapOutput {}

    unsafe impl SCStreamOutput for SwapOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(&self, _stream: &SCStream, sb: &CMSampleBuffer, ty: SCStreamOutputType) {
            let iv = self.ivars();
            if !iv.shared.accepting.load(Ordering::Acquire) {
                return;
            }
            if ty == SCStreamOutputType::Screen && iv.video {
                iv.shared.on_screen(iv.tag, sb);
            } else if ty == SCStreamOutputType::Audio {
                iv.shared.on_audio(iv.tag, sb);
            }
        }
    }

    unsafe impl SCStreamDelegate for SwapOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, err: &NSError) {
            let msg = format!("tag {}: {} (code {})", self.ivars().tag, err.localizedDescription(), err.code());
            eprintln!("SCStream stopped with error: {msg}");
            self.ivars().shared.stop_errors.lock().unwrap().push(msg);
        }
    }
);

impl SwapOutput {
    fn new(iv: OutIvars) -> Retained<Self> {
        let this = Self::alloc().set_ivars(iv);
        unsafe { msg_send![super(this), init] }
    }
}

struct Spec {
    label: &'static str,
    filter_key: &'static str,
    filter: Retained<SCContentFilter>,
    pixels: (usize, usize),
    content_pts: (f64, f64),
    source_rect_pts: Option<(f64, f64, f64, f64)>,
    descriptor: Value,
}

struct Stream {
    stream: Retained<SCStream>,
    tag: u32,
    _out: Retained<SwapOutput>,
    _vq: DispatchRetained<DispatchQueue>,
    _aq: DispatchRetained<DispatchQueue>,
}

fn make_cfg(pixels: (usize, usize), source_rect: Option<(f64, f64, f64, f64)>, fps: f64, audio: bool) -> Retained<SCStreamConfiguration> {
    unsafe {
        let cfg = SCStreamConfiguration::new();
        cfg.setWidth(pixels.0);
        cfg.setHeight(pixels.1);
        cfg.setScalesToFit(false);
        cfg.setPixelFormat(PIX_420V);
        cfg.setColorMatrix(kCGDisplayStreamYCbCrMatrix_ITU_R_709_2);
        cfg.setColorSpaceName(kCGColorSpaceSRGB);
        cfg.setShowsCursor(true);
        cfg.setMinimumFrameInterval(CMTime::new(1000, (fps * 1000.0) as i32));
        cfg.setQueueDepth(6);
        if let Some(r) = source_rect {
            cfg.setSourceRect(CGRect::new(CGPoint::new(r.0, r.1), CGSize::new(r.2, r.3)));
        }
        cfg.setCapturesAudio(audio);
        if audio {
            cfg.setExcludesCurrentProcessAudio(true);
            cfg.setSampleRate(48_000);
            cfg.setChannelCount(2);
        }
        cfg
    }
}

fn open_stream(shared: &Arc<Shared>, tag: u32, filter: &SCContentFilter, cfg: &SCStreamConfiguration, video: bool, audio: bool) -> Result<Stream> {
    unsafe {
        let out = SwapOutput::new(OutIvars { shared: shared.clone(), tag, video });
        let delegate: &ProtocolObject<dyn SCStreamDelegate> = ProtocolObject::from_ref(&*out);
        let stream = SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), filter, cfg, Some(delegate));
        let vq = DispatchQueue::new(&format!("simplecapture.spike.swap.v{tag}"), DispatchQueueAttr::SERIAL);
        let aq = DispatchQueue::new(&format!("simplecapture.spike.swap.a{tag}"), DispatchQueueAttr::SERIAL);
        let proto: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*out);
        stream
            .addStreamOutput_type_sampleHandlerQueue_error(proto, SCStreamOutputType::Screen, Some(&vq))
            .map_err(|e| anyhow!("addStreamOutput screen: {}", e.localizedDescription()))?;
        if audio {
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(proto, SCStreamOutputType::Audio, Some(&aq))
                .map_err(|e| anyhow!("addStreamOutput audio: {}", e.localizedDescription()))?;
        }
        Ok(Stream { stream, tag, _out: out, _vq: vq, _aq: aq })
    }
}

fn start_stream(s: &Stream) -> Result<()> {
    wait_err(|b| unsafe { s.stream.startCaptureWithCompletionHandler(Some(b)) }, "startCapture")
}

fn stop_stream(s: &Stream) -> Result<()> {
    wait_err(|b| unsafe { s.stream.stopCaptureWithCompletionHandler(Some(b)) }, "stopCapture")
}

fn timed(what: &str, f: impl FnOnce() -> Result<()>) -> Value {
    let t0 = host_now_secs();
    let r = f();
    let t1 = host_now_secs();
    json!({"step": what, "issued_host_s": t0, "done_host_s": t1, "ms": (t1 - t0) * 1e3, "error": r.err().map(|e| e.to_string())})
}

fn write_tone_wav(path: &Path, hz: f64, secs_total: f64) -> Result<()> {
    let sr = 48_000u32;
    let n = (secs_total * sr as f64) as usize;
    let mut data = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f64 / sr as f64;
        let fade = (t / 0.05).min(1.0).min((secs_total - t) / 0.05).max(0.0);
        let v = (2.0 * std::f64::consts::PI * hz * t).sin() * 0.25 * fade;
        data.extend_from_slice(&((v * 32767.0) as i16).to_le_bytes());
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data.len() as u32).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&sr.to_le_bytes())?;
    f.write_all(&(sr * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&(data.len() as u32).to_le_bytes())?;
    f.write_all(&data)?;
    Ok(())
}

pub fn swaphelper(a: &Args) -> Result<()> {
    let mtm = MainThreadMarker::new().unwrap();
    let _app = testwin::init_app(mtm);
    let secs_total = a.num("secs", 20.0);
    let hz = a.num("hz", 0.0);
    let wav = a.get("wav").map(PathBuf::from);
    let windows = a.flag("windows");
    let mut wins = vec![];
    if windows {
        let screen_w = objc2_app_kit::NSScreen::mainScreen(mtm).map(|s| s.frame().size.width).unwrap_or(1920.0);
        wins.push(TestWindow::new(mtm, 900.0, 200.0, 360.0, 240.0, (0.0, 0.75, 0.1)));
        let ox = (screen_w - 700.0).min(2200.0);
        wins.push(TestWindow::new(mtm, ox, 300.0, 640.0, 400.0, (1.0, 0.55, 0.0)));
    }
    pump_for(Duration::from_millis(300));
    let mut player = None;
    if let (Some(p), true) = (&wav, hz > 0.0) {
        write_tone_wav(p, hz, secs_total)?;
        let url = objc2_foundation::NSURL::fileURLWithPath(&NSString::from_str(p.to_str().unwrap()));
        let pl = unsafe { objc2_avf_audio::AVAudioPlayer::initWithContentsOfURL_error(objc2_avf_audio::AVAudioPlayer::alloc(), &url) }
            .map_err(|e| anyhow!("AVAudioPlayer: {}", e.localizedDescription()))?;
        unsafe {
            pl.setVolume(0.5);
            pl.play();
        }
        player = Some(pl);
    }
    let rects: Vec<Value> = wins.iter().map(|w| {
        let r = w.rect_top_left_points(mtm);
        json!([r.0, r.1, r.2, r.3])
    }).collect();
    println!("{}", json!({"event": "ready", "pid": std::process::id(), "windows": rects, "tone_hz": hz, "tone_started_host_s": host_now_secs()}));
    std::io::stdout().flush().ok();
    let t0 = host_now_secs();
    let mut toggle = false;
    let mut last = 0.0;
    while host_now_secs() - t0 < secs_total {
        let t = host_now_secs();
        if t - last >= 0.010 && !wins.is_empty() {
            toggle = !toggle;
            last = t;
            wins[0].set_color(if toggle { (0.0, 0.75, 0.1) } else { (0.0, 0.55, 0.1) });
            wins[1].set_color(if toggle { (1.0, 0.55, 0.0) } else { (1.0, 0.45, 0.0) });
            for w in &wins {
                w.window.display();
            }
        }
        pump(Duration::from_millis(2));
    }
    drop(player);
    Ok(())
}

fn spawn_helper(args: &[String]) -> Result<(Child, Value)> {
    let exe = std::env::current_exe()?;
    let mut ch = Command::new(exe).arg("swaphelper").args(args).stdout(Stdio::piped()).spawn()?;
    let out = ch.stdout.take().unwrap();
    let mut r = BufReader::new(out);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let v: Value = serde_json::from_str(line.trim()).map_err(|e| anyhow!("helper said {line:?}: {e}"))?;
    Ok((ch, v))
}

fn audio_gaps(bufs: &[(f64, u64, f64)], switches: &[f64]) -> Value {
    let mut gaps = vec![];
    let mut overlaps = 0u64;
    for w in bufs.windows(2) {
        let exp = w[0].0 + w[0].1 as f64 / w[0].2;
        let d = w[1].0 - exp;
        if d > 0.001 {
            let near = switches.iter().map(|s| w[0].0 - s).min_by(|a, b| a.abs().partial_cmp(&b.abs()).unwrap());
            gaps.push(json!({"at_host_s": exp, "gap_ms": d * 1e3, "nearest_switch_offset_s": near}));
        } else if d < -0.001 {
            overlaps += 1;
        }
    }
    let total: f64 = bufs.iter().map(|b| b.1 as f64 / b.2).sum();
    json!({
        "buffers": bufs.len(),
        "first_pts_host_s": bufs.first().map(|b| b.0),
        "end_host_s": bufs.last().map(|b| b.0 + b.1 as f64 / b.2),
        "samples_s": total,
        "gaps_over_1ms": gaps.len(),
        "gaps": gaps.into_iter().take(20).collect::<Vec<_>>(),
        "overlaps_over_1ms": overlaps,
        "buffer_frames": summarize(&bufs.iter().map(|b| b.1 as f64).collect::<Vec<_>>()),
    })
}

pub fn swap(a: &Args) -> Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| anyhow!("main thread"))?;
    let dir = PathBuf::from(a.get("out").ok_or_else(|| anyhow!("--out DIR required"))?);
    std::fs::create_dir_all(&dir)?;
    let strategy = a.get("strategy").unwrap_or("update").to_string();
    if !["update", "restart", "overlap"].contains(&strategy.as_str()) {
        return Err(anyhow!("--strategy update|restart|overlap"));
    }
    let audio_mode = a.get("audio").map(|s| s.to_string()).unwrap_or_else(|| if strategy == "update" { "shared".into() } else { "separate".into() });
    let order = a.get("order").unwrap_or("filter-first").to_string();
    let secs_total = a.num("secs", 12.0);
    let interval = a.num("interval", 3.0);
    let fps = a.num("fps", 30.0);
    let shared_audio = audio_mode == "shared";
    if shared_audio && strategy == "overlap" {
        return Err(anyhow!("overlap needs --audio separate"));
    }

    let _app = testwin::init_app(mtm);
    if !ensure_screen_access(Duration::from_secs(60)) {
        return Err(anyhow!("Screen Recording permission not granted"));
    }
    let tone_a = dir.join("tone440.wav");
    let tone_b = dir.join("tone660.wav");
    let hs = format!("{}", secs_total + 10.0);
    let (mut h1, r1) = spawn_helper(&["--secs".into(), hs.clone(), "--windows".into(), "--hz".into(), "440".into(), "--wav".into(), tone_a.to_string_lossy().into()])?;
    let (mut h2, r2) = spawn_helper(&["--secs".into(), hs, "--hz".into(), "660".into(), "--wav".into(), tone_b.to_string_lossy().into()])?;
    let hpid = h1.id() as i32;

    let mut content = capture::fetch_content().map_err(|e| anyhow!("SCShareableContent: {e}"))?;
    let display = capture::main_display(&content).ok_or_else(|| anyhow!("no display"))?;
    let mut green = None;
    let mut orange = None;
    for _ in 0..40 {
        for w in unsafe { content.windows() }.iter() {
            let owned = unsafe { w.owningApplication().map(|a| a.processID() == hpid).unwrap_or(false) && w.isOnScreen() };
            if !owned {
                continue;
            }
            let f = unsafe { w.frame() };
            if (f.size.width - 360.0).abs() < 1.0 {
                green = Some(w.clone());
            } else if (f.size.width - 640.0).abs() < 1.0 {
                orange = Some(w.clone());
            }
        }
        if green.is_some() && orange.is_some() {
            break;
        }
        pump_for(Duration::from_millis(100));
        content = capture::fetch_content().map_err(|e| anyhow!("SCShareableContent: {e}"))?;
    }
    let green = green.ok_or_else(|| anyhow!("green helper window not found"))?;
    let orange = orange.ok_or_else(|| anyhow!("orange helper window not found"))?;

    let dfi = capture::build_filter(&content, &display, &capture::Exclude::SelfWindows)?;
    let wfi = capture::window_filter(&green)?;
    let scale = dfi.point_pixel_scale;
    let of = unsafe { orange.frame() };
    let rr = (of.origin.x + 40.0, of.origin.y + 40.0, of.size.width - 80.0, of.size.height - 80.0);
    let rpx = (((rr.2 * scale).round() as usize) & !1, ((rr.3 * scale).round() as usize) & !1);
    let did = unsafe { display.displayID() };
    let rfi = capture::build_filter(&content, &display, &capture::Exclude::SelfWindows)?;
    let specs = [
        Spec { label: "display", filter_key: "display", filter: dfi.filter.clone(), pixels: dfi.pixel_size, content_pts: (dfi.content_rect_pts.2, dfi.content_rect_pts.3), source_rect_pts: None,
               descriptor: json!({"kind": "display", "display_id": did, "excluded": dfi.excluded_desc, "content_rect_pts": dfi.content_rect_pts}) },
        Spec { label: "window", filter_key: "window", filter: wfi.filter.clone(), pixels: wfi.pixel_size, content_pts: (wfi.content_rect_pts.2, wfi.content_rect_pts.3), source_rect_pts: None,
               descriptor: json!({"kind": "window", "window_id": unsafe { green.windowID() }, "owner_pid": hpid, "filter": "desktopIndependentWindow", "content_rect_pts": wfi.content_rect_pts, "point_pixel_scale": wfi.point_pixel_scale}) },
        Spec { label: "region", filter_key: "display", filter: rfi.filter.clone(), pixels: rpx, content_pts: (rr.2, rr.3), source_rect_pts: Some(rr),
               descriptor: json!({"kind": "region", "display_id": did, "source_rect_pts": [rr.0, rr.1, rr.2, rr.3], "point_pixel_scale": scale, "inside_window_id": unsafe { orange.windowID() }}) },
    ];
    let plan: Vec<usize> = vec![0, 1, 2, 0];

    let mic_dev = if avcap::ensure_access(false, Duration::from_secs(30)) {
        unsafe { objc2_av_foundation::AVCaptureDevice::defaultDeviceWithMediaType(objc2_av_foundation::AVMediaTypeAudio.unwrap()) }
    } else {
        None
    };
    let mic = match &mic_dev {
        Some(d) => Some(AvCapture::mic(&dir, d, 1.0)?),
        None => None,
    };

    let shared = Arc::new(Shared {
        dir: dir.clone(),
        fps,
        bpp: a.num("bpp", 0.05),
        frag: 1.0,
        st: Mutex::new(State { segs: vec![], frames: vec![], status: Default::default(), finishers: vec![] }),
        sys: Mutex::new(SysAudio::default()),
        audio_tag: AtomicU32::new(if shared_audio { 1 } else { 0 }),
        accepting: AtomicBool::new(true),
        stop_errors: Mutex::new(vec![]),
        route_geom: a.get("route") != Some("dims"),
    });

    let mut project = json!({
        "schema_version": 1,
        "kind": "swap-spike",
        "permission_host": std::env::var("__CFBundleIdentifier").unwrap_or_else(|_| "unknown".into()),
        "status": "recording",
        "clock": "mach host time (CMClockGetHostTimeClock), seconds",
        "strategy": strategy, "audio_mode": audio_mode, "update_order": order, "frame_routing": if a.get("route") == Some("dims") { "dims" } else { "dims+content_rect+content_scale" },
        "fps": fps, "codec": "hevc", "pixel_format": "420v", "movie_fragment_interval_s": 1.0,
        "helpers": [r1, r2],
        "sources": specs.iter().map(|s| json!({"label": s.label, "pixels": [s.pixels.0, s.pixels.1], "descriptor": s.descriptor})).collect::<Vec<_>>(),
    });
    write_json(&dir.join("project.json"), &project)?;

    let cpu_names = ["replayd", "VTEncoderXPCService", "WindowServer", "coreaudiod"];
    let mut streams: Vec<Stream> = vec![];
    let mut audio_stream = None;
    if let Some(m) = &mic {
        m.start();
    }
    if !shared_audio {
        let cfg = make_cfg((128, 72), None, 1.0, true);
        let s = open_stream(&shared, 0, &specs[0].filter, &cfg, false, true)?;
        start_stream(&s)?;
        audio_stream = Some(s);
    }
    pump_for(Duration::from_millis(300));

    let push_seg = |shared: &Shared, spec: &Spec, tag: u32, req: f64| {
        let mut st = shared.st.lock().unwrap();
        st.segs.push(Seg { label: spec.label, dims: spec.pixels, content_pts: spec.content_pts, tag, requested_host: req, writer: None, writer_error: None, first_pts: None, last_pts: None, last_sb: None, frames: 0, closed: false, end_host: None, summary: None });
    };

    let mut cpu_marks = vec![(host_now_secs(), cpu_snapshot(&cpu_names))];
    let t_req0 = host_now_secs();
    push_seg(&shared, &specs[plan[0]], 1, t_req0);
    let s0 = open_stream(&shared, 1, &specs[plan[0]].filter, &make_cfg(specs[plan[0]].pixels, None, fps, shared_audio), true, shared_audio)?;
    let st0 = timed("startCapture", || start_stream(&s0));
    streams.push(s0);
    let t_start = t_req0;
    project["recording_start_host_s"] = json!(t_start);
    let mut switches = vec![json!({"index": 0, "to": "display", "request_host_s": t_req0, "steps": [st0]})];
    let mut next_tag = 2u32;
    let mut rss_max = 0u64;
    for k in 1..plan.len() {
        let due = t_start + interval * k as f64;
        while host_now_secs() < due {
            pump(Duration::from_millis(3));
            rss_max = rss_max.max(self_rss());
        }
        let from = &specs[plan[k - 1]];
        let to = &specs[plan[k]];
        let req = host_now_secs();
        cpu_marks.push((req, cpu_snapshot(&cpu_names)));
        let mut steps = vec![];
        match strategy.as_str() {
            "update" => {
                let tag = streams.last().unwrap().tag;
                push_seg(&shared, to, tag, req);
                let s = &streams.last().unwrap().stream;
                let cfg = make_cfg(to.pixels, to.source_rect_pts, fps, shared_audio);
                let filter_changes = from.filter_key != to.filter_key;
                let filter_step = || timed("updateContentFilter", || wait_err(|b| unsafe { s.updateContentFilter_completionHandler(&to.filter, Some(b)) }, "updateContentFilter"));
                let cfg_step = || timed("updateConfiguration", || wait_err(|b| unsafe { s.updateConfiguration_completionHandler(&cfg, Some(b)) }, "updateConfiguration"));
                if order == "config-first" {
                    steps.push(cfg_step());
                    if filter_changes {
                        steps.push(filter_step());
                    }
                } else {
                    if filter_changes {
                        steps.push(filter_step());
                    }
                    steps.push(cfg_step());
                }
            }
            "restart" => {
                let tag = next_tag;
                next_tag += 1;
                push_seg(&shared, to, tag, req);
                let old = streams.last().unwrap();
                steps.push(timed("stopCapture(old)", || stop_stream(old)));
                let t0 = host_now_secs();
                let cfg = make_cfg(to.pixels, to.source_rect_pts, fps, shared_audio);
                let ns = open_stream(&shared, tag, &to.filter, &cfg, true, shared_audio)?;
                steps.push(json!({"step": "create SCStream", "ms": (host_now_secs() - t0) * 1e3}));
                if shared_audio {
                    shared.audio_tag.store(tag, Ordering::Release);
                }
                steps.push(timed("startCapture(new)", || start_stream(&ns)));
                streams.push(ns);
            }
            _ => {
                let tag = next_tag;
                next_tag += 1;
                push_seg(&shared, to, tag, req);
                let t0 = host_now_secs();
                let cfg = make_cfg(to.pixels, to.source_rect_pts, fps, false);
                let ns = open_stream(&shared, tag, &to.filter, &cfg, true, false)?;
                steps.push(json!({"step": "create SCStream", "ms": (host_now_secs() - t0) * 1e3}));
                steps.push(timed("startCapture(new)", || start_stream(&ns)));
                streams.push(ns);
                let t_wait = host_now_secs();
                while host_now_secs() < t_wait + 2.0 {
                    if shared.st.lock().unwrap().segs.last().unwrap().first_pts.is_some() {
                        break;
                    }
                    pump(Duration::from_millis(2));
                }
                steps.push(json!({"step": "wait first new frame", "ms": (host_now_secs() - t_wait) * 1e3}));
                let old = &streams[streams.len() - 2];
                steps.push(timed("stopCapture(old)", || stop_stream(old)));
            }
        }
        switches.push(json!({"index": k, "from": from.label, "to": to.label, "request_host_s": req, "steps": steps}));
    }
    while host_now_secs() < t_start + secs_total {
        pump(Duration::from_millis(3));
        rss_max = rss_max.max(self_rss());
    }
    let t_stop = host_now_secs();
    cpu_marks.push((t_stop, cpu_snapshot(&cpu_names)));
    let stop_step = timed("stopCapture(final)", || stop_stream(streams.last().unwrap()));
    if let Some(s) = &audio_stream {
        let _ = stop_stream(s);
    }
    if let Some(m) = &mic {
        m.stop();
    }
    shared.accepting.store(false, Ordering::Release);
    std::thread::sleep(Duration::from_millis(50));

    let mut errors = vec![];
    let finishers = {
        let mut st = shared.st.lock().unwrap();
        let n = st.segs.len();
        for i in 0..n {
            if !st.segs[i].closed {
                let end = if i + 1 < n { st.segs[i + 1].first_pts.unwrap_or(t_stop) } else { t_stop };
                if let Some(j) = close_seg(i, &mut st.segs[i], end, fps) {
                    st.finishers.push(j);
                }
            }
        }
        std::mem::take(&mut st.finishers)
    };
    for j in finishers {
        match j.join() {
            Ok((i, v, e)) => {
                if let Some(e) = e {
                    errors.push(format!("segment {i}: {e}"));
                }
                shared.st.lock().unwrap().segs[i].summary = Some(v);
            }
            Err(_) => errors.push("finisher panicked".into()),
        }
    }
    let mut tracks = vec![];
    {
        let mut sa = shared.sys.lock().unwrap();
        if let Some(w) = sa.writer.as_mut() {
            if let Err(e) = w.finish(None) {
                errors.push(e.to_string());
            }
            tracks.push(w.summary());
        }
        if let Some(e) = &sa.writer_error {
            errors.push(format!("system audio writer: {e}"));
        }
    }
    let mut mic_bufs = vec![];
    if let Some(m) = &mic {
        let mut t = m.shared.track.lock().unwrap();
        if let Some(w) = t.writer.as_mut() {
            if let Err(e) = w.finish(None) {
                errors.push(e.to_string());
            }
            let mut v = w.summary();
            v["device"] = json!(m.device_name);
            tracks.push(v);
        }
        mic_bufs = t.audio_bufs.clone();
    }
    let _ = h1.kill();
    let _ = h2.kill();
    let _ = h1.wait();
    let _ = h2.wait();
    let _ = std::fs::remove_file(&tone_a);
    let _ = std::fs::remove_file(&tone_b);

    let st = shared.st.lock().unwrap();
    let switch_hosts: Vec<f64> = switches.iter().skip(1).map(|s| s["request_host_s"].as_f64().unwrap()).collect();
    let mut segs_json = vec![];
    for (i, s) in st.segs.iter().enumerate() {
        let spec = specs.iter().find(|x| x.label == s.label).unwrap();
        let pts: Vec<f64> = st.frames.iter().filter(|f| f.seg == i as i32).map(|f| f.pts).collect();
        let mut iv: Vec<f64> = pts.windows(2).map(|w| (w[1] - w[0]) * 1e3).collect();
        iv.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let late: u64 = pts.windows(2).map(|w| { let g = (w[1] - w[0]) * fps; if g > 1.5 { (g.round() as u64).saturating_sub(1) } else { 0 } }).sum();
        let mut classes = std::collections::BTreeMap::new();
        for f in st.frames.iter().filter(|f| f.seg == i as i32) {
            *classes.entry(class_of(&f.sig)).or_insert(0u64) += 1;
        }
        let start = s.first_pts;
        let end = s.end_host;
        segs_json.push(json!({
            "index": i,
            "file": format!("screen-{i:03}.mov"),
            "source": spec.descriptor,
            "pixels": [s.dims.0, s.dims.1],
            "stream_tag": s.tag,
            "requested_host_s": s.requested_host,
            "start_host_s": start,
            "end_host_s": end,
            "last_real_frame_host_s": s.last_pts,
            "frames_written": s.frames,
            "fps": match (start, s.last_pts) { (Some(a), Some(b)) if b > a => json!((pts.len() as f64 - 1.0) / (b - a)), _ => json!(null) },
            "interval_ms_p50": iv.get(iv.len() / 2), "interval_ms_max": iv.last(),
            "late_or_missing_frames": late,
            "content_classes": classes,
            "writer": s.summary,
            "writer_error": s.writer_error,
        }));
    }
    let mut sw_metrics = vec![];
    for k in 1..st.segs.len() {
        let old = &st.segs[k - 1];
        let new = &st.segs[k];
        let req = new.requested_host;
        let old_cls = old.label;
        let new_cls = new.label;
        let next_req = st.segs.get(k + 1).map(|n| n.requested_host).unwrap_or(f64::INFINITY);
        let after: Vec<&FrameRec> = st.frames.iter().filter(|f| f.arrive >= req && f.arrive < next_req).collect();
        let first_new = new.first_pts;
        let last_old = old.last_pts;
        let done = switches[k]["steps"].as_array().unwrap().iter().filter_map(|s| s["done_host_s"].as_f64()).fold(f64::NAN, f64::max);
        let old_content_after_req = after.iter().filter(|f| class_of(&f.sig) == old_cls).count();
        let old_content_pts_after_req = after.iter().filter(|f| class_of(&f.sig) == old_cls && f.pts >= req).count();
        let old_content_after_done = after.iter().filter(|f| class_of(&f.sig) == old_cls && done.is_finite() && f.pts >= done).count();
        let contaminated_old = st.frames.iter().filter(|f| f.seg == k as i32 - 1 && f.arrive >= req && class_of(&f.sig) != old_cls).count();
        let contaminated_new = after.iter().filter(|f| f.seg == k as i32 && class_of(&f.sig) != new_cls).count();
        let mut geoms = std::collections::BTreeMap::new();
        for f in st.frames.iter().filter(|f| f.arrive >= req - 0.2 && f.arrive < req + 1.5_f64.max(first_new.map(|p| p - req + 0.3).unwrap_or(0.0)).min(next_req - req)) {
            let key = format!("{}x{} {} rect[{},{},{},{}] cs{} sf{} seg{}", f.w, f.h, class_of(&f.sig), f.geom[0], f.geom[1], f.geom[2], f.geom[3], f.geom[4], f.geom[5], f.seg);
            *geoms.entry(key).or_insert(0u64) += 1;
        }
        let dropped: Vec<&FrameRec> = st.frames.iter().filter(|f| f.seg == -1 && f.arrive >= req - 0.05 && first_new.map(|p| f.pts <= p + 0.5).unwrap_or(true)).collect();
        let mut dropped_kinds = std::collections::BTreeMap::new();
        for f in &dropped {
            *dropped_kinds.entry(format!("{}x{} {} tag{}", f.w, f.h, class_of(&f.sig), f.tag)).or_insert(0u64) += 1;
        }
        let mut dims_seq: Vec<String> = vec![];
        for f in st.frames.iter().filter(|f| f.arrive >= req - 0.2 && first_new.map(|p| f.pts <= p + 0.3).unwrap_or(false)) {
            let d = format!("{}x{}:{}", f.w, f.h, class_of(&f.sig));
            if dims_seq.last() != Some(&d) {
                dims_seq.push(d);
            }
        }
        let first_new_arrive = st.frames.iter().find(|f| f.seg == k as i32).map(|f| f.arrive);
        sw_metrics.push(json!({
            "switch": k, "from": old_cls, "to": new_cls,
            "request_host_s": req,
            "api_done_host_s": if done.is_finite() { json!(done) } else { json!(null) },
            "api_ms": if done.is_finite() { json!((done - req) * 1e3) } else { json!(null) },
            "last_old_frame_pts_host_s": last_old,
            "first_new_frame_pts_host_s": first_new,
            "gap_last_old_to_first_new_ms": match (last_old, first_new) { (Some(a), Some(b)) => json!((b - a) * 1e3), _ => json!(null) },
            "request_to_first_new_pts_ms": first_new.map(|p| (p - req) * 1e3),
            "request_to_first_new_arrival_ms": first_new_arrive.map(|p| (p - req) * 1e3),
            "old_content_frames_arriving_after_request": old_content_after_req,
            "old_content_frames_with_pts_after_request": old_content_pts_after_req,
            "old_content_frames_after_api_done_not_in_old_segment": old_content_after_done,
            "frames_in_old_segment_not_old_content": contaminated_old,
            "frames_in_new_segment_not_new_content": contaminated_new,
            "transition_frames_dropped": dropped.len(),
            "transition_dropped_kinds": dropped_kinds,
            "frame_sequence_around_switch": dims_seq,
            "frame_geometry_around_switch": geoms,
            "size_change": [old.dims, new.dims],
        }));
    }
    let sys_bufs: Vec<(f64, u64, f64)> = shared.sys.lock().unwrap().bufs.iter().map(|b| (b.0, b.1, b.2)).collect();
    let sys_tags: std::collections::BTreeSet<u32> = shared.sys.lock().unwrap().bufs.iter().map(|b| b.3).collect();
    let mut cpu_seg = vec![];
    for w in cpu_marks.windows(2) {
        let dt = w[1].0 - w[0].0;
        let v: serde_json::Map<String, Value> = w[0].1.iter().zip(w[1].1.iter()).map(|(a, b)| (a.0.clone(), json!(((b.1 - a.1) / dt * 1000.0).round() / 10.0))).collect();
        cpu_seg.push(json!({"from_host_s": w[0].0, "to_host_s": w[1].0, "cpu_pct_of_one_core": v}));
    }
    let total_dt = cpu_marks.last().unwrap().0 - cpu_marks[0].0;
    let cpu_total: serde_json::Map<String, Value> = cpu_marks[0].1.iter().zip(cpu_marks.last().unwrap().1.iter()).map(|(a, b)| (a.0.clone(), json!(((b.1 - a.1) / total_dt * 1000.0).round() / 10.0))).collect();
    let status_names = ["complete", "idle", "blank", "suspended", "started", "stopped", "?", "missing"];
    let status: serde_json::Map<String, Value> = st.status.iter().map(|(t, c)| {
        let m: serde_json::Map<String, Value> = status_names.iter().enumerate().filter(|(i, _)| c[*i] > 0).map(|(i, n)| (n.to_string(), json!(c[i]))).collect();
        (format!("tag{t}"), Value::Object(m))
    }).collect();

    project["status"] = json!("finished");
    project["recording_stop_host_s"] = json!(t_stop);
    project["duration_s"] = json!(t_stop - t_start);
    project["switches"] = json!(switches);
    project["final_stop"] = stop_step;
    project["screen_segments"] = json!(segs_json);
    project["switch_metrics"] = json!(sw_metrics);
    project["tracks"] = json!(tracks);
    project["system_audio_continuity"] = audio_gaps(&sys_bufs, &switch_hosts);
    project["system_audio_stream_tags"] = json!(sys_tags);
    project["mic_continuity"] = audio_gaps(&mic_bufs, &switch_hosts);
    project["sck_frame_status"] = json!(status);
    project["cpu_pct_of_one_core_total"] = json!(cpu_total);
    project["cpu_per_segment"] = json!(cpu_seg);
    project["self_rss_max_mb"] = json!(rss_max as f64 / 1048576.0);
    project["stream_errors"] = json!(*shared.stop_errors.lock().unwrap());
    project["errors"] = json!(errors);
    write_json(&dir.join("project.json"), &project)?;
    let frames: Vec<Value> = st.frames.iter().map(|f| json!([f.tag, f.pts, f.arrive, f.w, f.h, String::from_utf8_lossy(&f.sig), f.seg, f.geom.iter().map(|v| if v.is_finite() { json!(v) } else { json!(null) }).collect::<Vec<_>>()])).collect();
    write_json(&dir.join("frames.json"), &json!({"columns": ["tag", "pts_host_s", "arrive_host_s", "w", "h", "sig", "segment", "content_rect_xywh_contentscale_scalefactor"], "frames": frames}))?;
    println!("{}", serde_json::to_string(&json!({"switch_metrics": project["switch_metrics"], "errors": project["errors"]}))?);
    drop(streams);
    drop(audio_stream);
    Ok(())
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SCSpikePickerObserver"]
    #[ivars = Mutex<Vec<String>>]
    pub struct PickerObserver;

    unsafe impl NSObjectProtocol for PickerObserver {}

    unsafe impl SCContentSharingPickerObserver for PickerObserver {
        #[unsafe(method(contentSharingPicker:didCancelForStream:))]
        fn did_cancel(&self, _p: &SCContentSharingPicker, _s: Option<&SCStream>) {
            self.ivars().lock().unwrap().push("didCancel".into());
        }
        #[unsafe(method(contentSharingPicker:didUpdateWithFilter:forStream:))]
        fn did_update(&self, _p: &SCContentSharingPicker, _f: &SCContentFilter, _s: Option<&SCStream>) {
            self.ivars().lock().unwrap().push("didUpdate".into());
        }
        #[unsafe(method(contentSharingPickerStartDidFailWithError:))]
        fn did_fail(&self, e: &NSError) {
            self.ivars().lock().unwrap().push(format!("startDidFail: {}", e.localizedDescription()));
        }
    }
);

fn modes(m: SCContentSharingPickerMode) -> Vec<&'static str> {
    let mut v = vec![];
    for (f, n) in [
        (SCContentSharingPickerMode::SingleWindow, "SingleWindow"),
        (SCContentSharingPickerMode::MultipleWindows, "MultipleWindows"),
        (SCContentSharingPickerMode::SingleApplication, "SingleApplication"),
        (SCContentSharingPickerMode::MultipleApplications, "MultipleApplications"),
        (SCContentSharingPickerMode::SingleDisplay, "SingleDisplay"),
    ] {
        if m.contains(f) {
            v.push(n);
        }
    }
    if m.bits() & !0x1f != 0 {
        v.push("other-bits");
    }
    v
}

fn cfg_json(c: &SCContentSharingPickerConfiguration) -> Value {
    unsafe {
        json!({
            "allowedPickerModes": modes(c.allowedPickerModes()),
            "allowedPickerModes_raw": c.allowedPickerModes().bits(),
            "allowsChangingSelectedContent": c.allowsChangingSelectedContent(),
            "excludedBundleIDs": c.excludedBundleIDs().iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            "excludedWindowIDs": c.excludedWindowIDs().iter().map(|n| n.integerValue()).collect::<Vec<_>>(),
        })
    }
}

pub fn picker(_a: &Args) -> Result<()> {
    let mtm = MainThreadMarker::new().unwrap();
    let _app = testwin::init_app(mtm);
    let mut out = json!({"screen_permission_preflight": objc2_core_graphics::CGPreflightScreenCaptureAccess()});
    unsafe {
        let cls = objc2::runtime::AnyClass::get(c"SCContentSharingPicker");
        out["class_present"] = json!(cls.is_some());
        let p = SCContentSharingPicker::sharedPicker();
        for sel in ["present", "presentPickerUsingContentStyle:", "presentPickerForStream:", "presentPickerForStream:usingContentStyle:", "setConfiguration:forStream:", "addObserver:", "setActive:", "setMaximumStreamCount:"] {
            let s = objc2::runtime::Sel::register(&std::ffi::CString::new(sel).unwrap());
            let r: bool = msg_send![&*p, respondsToSelector: s];
            out["responds"][sel] = json!(r);
        }
        out["default_configuration_before"] = cfg_json(&p.defaultConfiguration());
        out["maximumStreamCount_before"] = json!(p.maximumStreamCount().map(|n| n.integerValue()));
        out["isActive_before"] = json!(p.isActive());

        let cfg = SCContentSharingPickerConfiguration::new();
        cfg.setAllowedPickerModes(SCContentSharingPickerMode::SingleWindow | SCContentSharingPickerMode::SingleDisplay | SCContentSharingPickerMode::SingleApplication);
        cfg.setAllowsChangingSelectedContent(true);
        let bid = std::env::var("__CFBundleIdentifier").unwrap_or_else(|_| "com.simplecapture.dev".into());
        cfg.setExcludedBundleIDs(&NSArray::from_retained_slice(&[NSString::from_str(&bid)]));
        cfg.setExcludedWindowIDs(&NSArray::from_retained_slice(&[NSNumber::new_isize(1)]));
        p.setDefaultConfiguration(&cfg);
        out["default_configuration_after_set"] = cfg_json(&p.defaultConfiguration());
        p.setMaximumStreamCount(Some(&NSNumber::new_isize(1)));
        out["maximumStreamCount_after_set"] = json!(p.maximumStreamCount().map(|n| n.integerValue()));

        let obs = PickerObserver::alloc().set_ivars(Mutex::new(vec![]));
        let obs: Retained<PickerObserver> = msg_send![super(obs), init];
        let proto: &ProtocolObject<dyn SCContentSharingPickerObserver> = ProtocolObject::from_ref(&*obs);
        p.addObserver(proto);

        let content = capture::fetch_content().map_err(|e| anyhow!("SCShareableContent: {e}"))?;
        let display = capture::main_display(&content).ok_or_else(|| anyhow!("no display"))?;
        let fi = capture::build_filter(&content, &display, &capture::Exclude::None)?;
        let scfg = make_cfg((256, 144), None, 1.0, false);
        let stream = SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &fi.filter, &scfg, None);
        p.setConfiguration_forStream(Some(&cfg), &stream);
        out["setConfiguration_forStream"] = json!("called (unstarted stream), no exception");

        p.setActive(true);
        pump_for(Duration::from_millis(300));
        out["isActive_after_setActive_true"] = json!(p.isActive());
        p.setActive(false);
        pump_for(Duration::from_millis(200));
        out["isActive_after_setActive_false"] = json!(p.isActive());
        p.setConfiguration_forStream(None, &stream);
        p.removeObserver(proto);
        p.setMaximumStreamCount(None);
        out["observer_callbacks"] = json!(*obs.ivars().lock().unwrap());
        out["not_called"] = json!(["present", "presentPickerUsingContentStyle:", "presentPickerForStream:", "presentPickerForStream:usingContentStyle:"]);
    }
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
