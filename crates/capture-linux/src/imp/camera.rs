//! Cameras through V4L2: NV12 or YUYV formats, or MJPEG decoded by GStreamer when a camera
//! offers no raw mode within the limit.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use sc_geometry::SizePx;
use sc_platform::{
    CameraLimit, CaptureDevice, CpuNv12Frame, DeviceKind, DeviceStream, Error, FrameBuffer,
    FrameSink, Result, SourceEvent, StreamTag, VideoFrame,
};
use v4l::buffer::Type;
use v4l::format::FourCC;
use v4l::frameinterval::FrameIntervalEnum;
use v4l::framesize::FrameSizeEnum;
use v4l::io::traits::CaptureStream;
use v4l::prelude::*;
use v4l::video::Capture;

use super::sys::host_now;

const YUYV: [u8; 4] = *b"YUYV";
const NV12: [u8; 4] = *b"NV12";
const MJPG: [u8; 4] = *b"MJPG";

fn is_capture(path: &std::path::Path) -> bool {
    Device::with_path(path)
        .and_then(|d| d.query_caps())
        .is_ok_and(|c| {
            c.capabilities
                .contains(v4l::capability::Flags::VIDEO_CAPTURE)
        })
}

pub fn devices() -> Result<Vec<CaptureDevice>> {
    let mut nodes = v4l::context::enum_devices();
    nodes.sort_by_key(|n| n.index());
    Ok(nodes
        .into_iter()
        .filter(|n| is_capture(n.path()))
        .enumerate()
        .map(|(i, n)| CaptureDevice {
            id: n.path().display().to_string(),
            name: n.name().unwrap_or_else(|| format!("Camera {}", i + 1)),
            model_id: None,
            is_default: i == 0,
            kind_hint: None,
        })
        .collect())
}

fn best_fps(dev: &Device, fourcc: FourCC, w: u32, h: u32) -> Option<f64> {
    dev.enum_frameintervals(fourcc, w, h)
        .ok()?
        .into_iter()
        .filter_map(|i| match i.interval {
            FrameIntervalEnum::Discrete(f) if f.numerator > 0 => {
                Some(f64::from(f.denominator) / f64::from(f.numerator))
            }
            _ => None,
        })
        .reduce(f64::max)
}

fn choose(dev: &Device, limit: CameraLimit) -> Option<(FourCC, SizePx)> {
    choose_from(dev, limit, &[NV12, YUYV]).or_else(|| choose_from(dev, limit, &[MJPG]))
}

fn choose_from(dev: &Device, limit: CameraLimit, codes: &[[u8; 4]]) -> Option<(FourCC, SizePx)> {
    let mut best: Option<(FourCC, SizePx, u64)> = None;
    for &code in codes {
        let fourcc = FourCC::new(&code);
        for fs in dev.enum_framesizes(fourcc).unwrap_or_default() {
            let sizes: Vec<(u32, u32)> = match fs.size {
                FrameSizeEnum::Discrete(d) => vec![(d.width, d.height)],
                FrameSizeEnum::Stepwise(s) => {
                    vec![(s.max_width.min(limit.max.w), s.max_height.min(limit.max.h))]
                }
            };
            for (w, h) in sizes {
                if w > limit.max.w || h > limit.max.h || w < 2 || h < 2 {
                    continue;
                }
                if best_fps(dev, fourcc, w, h).is_some_and(|f| f < 14.0) {
                    continue;
                }
                let score = u64::from(w) * u64::from(h);
                if best.as_ref().is_none_or(|b| score > b.2) {
                    best = Some((fourcc, SizePx { w, h }, score));
                }
            }
        }
    }
    best.map(|(f, s, _)| (f, s))
}

fn yuyv_to_nv12(src: &[u8], stride: usize, size: SizePx) -> CpuNv12Frame {
    let (w, h) = (size.w as usize, size.h as usize);
    let mut f = CpuNv12Frame::black(size, false);
    let uv_stride = f.uv_stride();
    for y in 0..h {
        let row = &src[y * stride..];
        for x in 0..w {
            f.y[y * w + x] = row[x * 2];
        }
    }
    for cy in 0..h.div_ceil(2) {
        let r0 = &src[cy * 2 * stride..];
        let r1 = &src[(cy * 2 + 1).min(h - 1) * stride..];
        for cx in 0..w.div_ceil(2) {
            let o = cx * 4;
            if o + 3 >= w * 2 + 2 {
                break;
            }
            let u = (u16::from(r0[o + 1]) + u16::from(r1[o + 1])) / 2;
            let v = (u16::from(r0[o + 3]) + u16::from(r1[o + 3])) / 2;
            f.uv[cy * uv_stride + cx * 2] = u as u8;
            f.uv[cy * uv_stride + cx * 2 + 1] = v as u8;
        }
    }
    f
}

pub(super) fn nv12_copy(src: &[u8], stride: usize, size: SizePx) -> CpuNv12Frame {
    CpuNv12Frame::from_padded(src, size, stride, size.h as usize)
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
            let opened = (|| -> Result<(Device, FourCC, SizePx, usize)> {
                let dev = Device::with_path(&id)?;
                let (fourcc, size) = choose(&dev, limit).ok_or(Error::Unsupported(
                    "this camera offers no NV12, YUYV or MJPEG mode within the limit",
                ))?;
                let mut fmt = dev.format()?;
                fmt.width = size.w;
                fmt.height = size.h;
                fmt.fourcc = fourcc;
                let got = dev.set_format(&fmt)?;
                let size = SizePx {
                    w: got.width & !1,
                    h: got.height & !1,
                };
                Ok((dev, got.fourcc, size, got.stride as usize))
            })();
            let (dev, fourcc, size, stride) = match opened {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            let mut stream = match MmapStream::with_buffers(&dev, Type::VideoCapture, 4) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(Err(e.into()));
                    return;
                }
            };
            let mut mjpeg = if fourcc == FourCC::new(&MJPG) {
                match super::mjpeg::Mjpeg::new(size) {
                    Ok(m) => Some(m),
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                }
            } else {
                None
            };
            let _ = tx.send(Ok(format!("{id} {}x{} {fourcc}", size.w, size.h)));
            let yuyv = fourcc == FourCC::new(&YUYV);
            while !flag.load(Ordering::SeqCst) {
                let frame = match stream.next() {
                    Ok((buf, meta)) if mjpeg.is_some() => {
                        let used = (meta.bytesused as usize).min(buf.len());
                        let used = if used == 0 { buf.len() } else { used };
                        mjpeg.as_mut().and_then(|m| m.decode(&buf[..used]))
                    }
                    Ok((buf, _)) => {
                        let row = if yuyv {
                            size.w as usize * 2
                        } else {
                            size.w as usize
                        };
                        let stride = stride.max(row);
                        if yuyv {
                            (buf.len() >= stride * size.h as usize)
                                .then(|| yuyv_to_nv12(buf, stride, size))
                        } else {
                            (buf.len() >= stride * (size.h as usize * 3 / 2))
                                .then(|| nv12_copy(buf, stride, size))
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => {
                        if !flag.load(Ordering::SeqCst) {
                            sink.on_event(SourceEvent::DeviceLost {
                                kind: DeviceKind::Camera,
                                device_id: id.clone(),
                            });
                        }
                        break;
                    }
                };
                if let Some(f) = frame {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_becomes_nv12() {
        let size = SizePx { w: 4, h: 2 };
        let row = [100u8, 50, 110, 200, 120, 60, 130, 210];
        let src: Vec<u8> = row.iter().chain(row.iter()).copied().collect();
        let f = yuyv_to_nv12(&src, 8, size);
        assert_eq!(f.y, vec![100, 110, 120, 130, 100, 110, 120, 130]);
        assert_eq!(f.uv, vec![50, 200, 60, 210]);
    }
}
