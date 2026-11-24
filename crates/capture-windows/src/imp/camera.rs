//! Cameras through a Media Foundation source reader that decodes and converts to NV12.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Once, mpsc};
use std::time::Duration;

use sc_geometry::SizePx;
use sc_platform::{
    CameraLimit, CaptureDevice, CpuNv12Frame, DeviceKind, DeviceStream, Error, FrameBuffer,
    FrameSink, Result, SourceEvent, StreamTag, VideoFrame,
};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, IMFMediaSource, IMFMediaType, IMFSourceReader, MF_API_VERSION,
    MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SDK_VERSION, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR,
    MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromMediaSource,
    MFEnumDeviceSources, MFMediaType_Video, MFSTARTUP_FULL, MFStartup, MFVideoFormat_NV12,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::core::{GUID, PWSTR};

use super::sys::host_now;
use super::wgc::os;

fn startup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Err(e) = MFStartup((MF_SDK_VERSION << 16) | MF_API_VERSION, MFSTARTUP_FULL) {
            tracing::warn!("MFStartup: {e}");
        }
    });
}

fn string(a: &IMFActivate, key: &GUID) -> Option<String> {
    let mut p = PWSTR::null();
    let mut len = 0u32;
    unsafe {
        a.GetAllocatedString(key, &mut p, &mut len).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

fn activates() -> Result<Vec<IMFActivate>> {
    startup();
    unsafe {
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1).map_err(os)?;
        let attrs = attrs.ok_or_else(|| Error::Other("MFCreateAttributes".into()))?;
        attrs
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(os)?;
        let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attrs, &mut list, &mut count).map_err(os)?;
        let mut out = Vec::new();
        if !list.is_null() {
            for i in 0..count as usize {
                if let Some(a) = (*list.add(i)).take() {
                    out.push(a);
                }
            }
            CoTaskMemFree(Some(list as *const _));
        }
        Ok(out)
    }
}

pub fn devices() -> Result<Vec<CaptureDevice>> {
    Ok(activates()?
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            let id = string(a, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK)?;
            Some(CaptureDevice {
                name: string(a, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
                    .unwrap_or_else(|| format!("Camera {}", i + 1)),
                id,
                model_id: None,
                is_default: i == 0,
                kind_hint: None,
            })
        })
        .collect())
}

fn unpack(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, v as u32)
}

const STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

fn choose(reader: &IMFSourceReader, limit: CameraLimit) -> Option<(IMFMediaType, SizePx, f64)> {
    let mut best: Option<(IMFMediaType, SizePx, f64)> = None;
    for i in 0.. {
        let Ok(t) = (unsafe { reader.GetNativeMediaType(STREAM, i) }) else {
            break;
        };
        let Ok(size) = (unsafe { t.GetUINT64(&MF_MT_FRAME_SIZE) }) else {
            continue;
        };
        let (w, h) = unpack(size);
        let fps = unsafe { t.GetUINT64(&MF_MT_FRAME_RATE) }
            .map(|r| {
                let (n, d) = unpack(r);
                f64::from(n) / f64::from(d.max(1))
            })
            .unwrap_or(30.0);
        if w > limit.max.w || h > limit.max.h || fps < 14.0 {
            continue;
        }
        let score = |s: SizePx, f: f64| {
            (
                u64::from(s.w) * u64::from(s.h),
                (f.min(f64::from(limit.max_fps)) * 100.0) as u64,
            )
        };
        let cand = SizePx { w, h };
        if best
            .as_ref()
            .is_none_or(|(_, bs, bf)| score(cand, fps) > score(*bs, *bf))
        {
            best = Some((t, cand, fps));
        }
    }
    best
}

fn open(
    device_id: &str,
    limit: CameraLimit,
) -> Result<(IMFSourceReader, IMFMediaSource, SizePx, String)> {
    let a = activates()?
        .into_iter()
        .find(|a| {
            string(a, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK).as_deref()
                == Some(device_id)
        })
        .ok_or_else(|| Error::NotFound(format!("camera {device_id}")))?;
    let name = string(&a, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME).unwrap_or_default();
    unsafe {
        let source: IMFMediaSource = a.ActivateObject().map_err(os)?;
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1).map_err(os)?;
        let attrs = attrs.ok_or_else(|| Error::Other("MFCreateAttributes".into()))?;
        attrs
            .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
            .map_err(os)?;
        let reader = MFCreateSourceReaderFromMediaSource(&source, &attrs).map_err(os)?;
        let (native, size, fps) = choose(&reader, limit)
            .ok_or_else(|| Error::Other(format!("{name}: no format within the limit")))?;
        reader
            .SetCurrentMediaType(STREAM, None, &native)
            .map_err(os)?;
        let nv12 = MFCreateMediaType().map_err(os)?;
        nv12.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(os)?;
        nv12.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
            .map_err(os)?;
        nv12.SetUINT64(
            &MF_MT_FRAME_SIZE,
            (u64::from(size.w) << 32) | u64::from(size.h),
        )
        .map_err(os)?;
        reader
            .SetCurrentMediaType(STREAM, None, &nv12)
            .map_err(os)?;
        Ok((
            reader,
            source,
            size,
            format!("{name} {}x{} {fps:.0} fps NV12", size.w, size.h),
        ))
    }
}

fn frame_from(
    sample: &windows::Win32::Media::MediaFoundation::IMFSample,
    size: SizePx,
) -> Option<CpuNv12Frame> {
    unsafe {
        let buf = sample.ConvertToContiguousBuffer().ok()?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len = 0u32;
        buf.Lock(&mut ptr, None, Some(&mut len)).ok()?;
        let data = std::slice::from_raw_parts(ptr, len as usize);
        let (w, h) = (size.w as usize, size.h as usize);
        let rows = h + h.div_ceil(2);
        let stride = (len as usize / rows).max(w);
        let f = CpuNv12Frame::from_padded(data, size, stride, h);
        let _ = buf.Unlock();
        Some(f)
    }
}

pub struct CameraStream {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    format: String,
}

impl DeviceStream for CameraStream {
    fn format(&self) -> String {
        self.format.clone()
    }

    fn stop(mut self: Box<Self>) -> Result<()> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        Ok(())
    }
}

static NEXT_TAG: AtomicU64 = AtomicU64::new(1 << 48);

pub fn start(
    device_id: &str,
    limit: CameraLimit,
    sink: Arc<dyn FrameSink>,
) -> Result<CameraStream> {
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Result<String>>();
    let id = device_id.to_string();
    let flag = stop.clone();
    let tag = StreamTag(NEXT_TAG.fetch_add(1, Ordering::Relaxed));
    let thread = std::thread::Builder::new()
        .name("camera".into())
        .spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            startup();
            let (reader, source, size, format) = match open(&id, limit) {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            let _ = tx.send(Ok(format));
            while !flag.load(Ordering::SeqCst) {
                let mut flags = 0u32;
                let mut sample = None;
                let r = unsafe {
                    reader.ReadSample(STREAM, 0, None, Some(&mut flags), None, Some(&mut sample))
                };
                if r.is_err()
                    || flags
                        & (MF_SOURCE_READERF_ERROR.0 as u32
                            | MF_SOURCE_READERF_ENDOFSTREAM.0 as u32)
                        != 0
                {
                    if !flag.load(Ordering::SeqCst) {
                        sink.on_event(SourceEvent::DeviceLost {
                            kind: DeviceKind::Camera,
                            device_id: id.clone(),
                        });
                    }
                    break;
                }
                let Some(sample) = sample else { continue };
                if let Some(f) = frame_from(&sample, size) {
                    sink.on_camera(VideoFrame {
                        stream: tag,
                        pts: host_now(),
                        size,
                        content_rect_pt: None,
                        content_scale: None,
                        buffer: FrameBuffer(Arc::new(f)),
                    });
                }
            }
            unsafe {
                let _ = source.Shutdown();
            }
        })?;
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(format)) => Ok(CameraStream {
            stop,
            thread: Some(thread),
            format,
        }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            Err(Error::Timeout("the camera did not start".into()))
        }
    }
}
