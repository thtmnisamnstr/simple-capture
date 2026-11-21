//! Screen streams through Windows.Graphics.Capture. Each frame is copied (the crop only) into a
//! CPU-readable texture and converted to NV12; a window stream letterboxes into the size it
//! started with when the window is resized.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use sc_core::image::RgbaImage;
use sc_core::project::{RegionMethod, Resolution, SourceDescriptor};
use sc_geometry::{RectPt, SizePx, snap_region_even_px};
use sc_platform::{
    Error, FrameBuffer, FrameSink, HostTime, Result, ScreenStream, ScreenStreamConfig, SourceEvent,
    SourceSpec, StreamTag, VideoFrame, WindowId,
};
use sc_tracks::nv12::{PackedOrder, fit_packed, packed_to_nv12_par};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::{IInspectable, Interface};

use super::enumerate::{self, Monitor};

pub struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    winrt: IDirect3DDevice,
}

pub fn gpu() -> Result<Gpu> {
    let mut last = None;
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        let mut context = None;
        let r = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };
        match (r, device, context) {
            (Ok(()), Some(device), Some(context)) => {
                let dxgi: IDXGIDevice = device.cast().map_err(os)?;
                let insp = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.map_err(os)?;
                let winrt: IDirect3DDevice = insp.cast().map_err(os)?;
                return Ok(Gpu {
                    device,
                    context,
                    winrt,
                });
            }
            (Err(e), _, _) => last = Some(e),
            _ => {}
        }
    }
    Err(Error::Other(format!(
        "no Direct3D 11 device: {}",
        last.map(|e| e.to_string()).unwrap_or_default()
    )))
}

pub fn os(e: windows::core::Error) -> Error {
    Error::Os {
        code: i64::from(e.code().0),
        message: e.message().to_string(),
    }
}

fn interop() -> Result<IGraphicsCaptureItemInterop> {
    windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(os)
}

/// `(x, y, w, h)` in the captured texture's pixels.
type CropPx = (u32, u32, u32, u32);

/// What one stream captures and how it lands in its frames.
pub struct Plan {
    pub item: GraphicsCaptureItem,
    pub crop_px: Option<CropPx>,
    pub out: SizePx,
    pub scale: f64,
    pub descriptor: SourceDescriptor,
    pub is_window: bool,
}

fn even(w: f64, h: f64) -> SizePx {
    SizePx {
        w: ((w.round() as u32) & !1).max(2),
        h: ((h.round() as u32) & !1).max(2),
    }
}

fn out_scale(res: Resolution, scale: f64) -> f64 {
    match res {
        Resolution::Native => 1.0,
        Resolution::OneX => 1.0 / scale.max(1.0),
    }
}

fn crop_plan(m: &Monitor, region_pt: RectPt, res: Resolution) -> Result<(RectPt, CropPx, SizePx)> {
    let snapped = snap_region_even_px(region_pt, m.scale);
    let size = m.size_px();
    let x = ((snapped.x * m.scale).round().max(0.0) as u32).min(size.w.saturating_sub(2));
    let y = ((snapped.y * m.scale).round().max(0.0) as u32).min(size.h.saturating_sub(2));
    let w = ((snapped.w * m.scale).round() as u32)
        .min(size.w - x)
        .max(2)
        & !1;
    let h = ((snapped.h * m.scale).round() as u32)
        .min(size.h - y)
        .max(2)
        & !1;
    let k = out_scale(res, m.scale);
    Ok((
        snapped,
        (x, y, w, h),
        even(f64::from(w) * k, f64::from(h) * k),
    ))
}

pub fn plan(spec: &SourceSpec, cfg: &ScreenStreamConfig) -> Result<Plan> {
    match spec {
        SourceSpec::Display { display } => {
            let m = enumerate::monitor(display)?;
            let item: GraphicsCaptureItem =
                unsafe { interop()?.CreateForMonitor(m.hmon) }.map_err(os)?;
            let k = out_scale(cfg.resolution, m.scale);
            let px = m.size_px();
            Ok(Plan {
                item,
                crop_px: None,
                out: even(f64::from(px.w) * k, f64::from(px.h) * k),
                scale: m.scale * k,
                descriptor: SourceDescriptor::Display {
                    display_id: display.0.clone(),
                    display_name: None,
                    scale: m.scale * k,
                    frame_pt: Some(m.frame_pt()),
                },
                is_window: false,
            })
        }
        SourceSpec::Region { display, region_pt } => {
            let m = enumerate::monitor(display)?;
            let item: GraphicsCaptureItem =
                unsafe { interop()?.CreateForMonitor(m.hmon) }.map_err(os)?;
            let (snapped, crop, out) = crop_plan(&m, *region_pt, cfg.resolution)?;
            let scale = m.scale * out_scale(cfg.resolution, m.scale);
            Ok(Plan {
                item,
                crop_px: Some(crop),
                out,
                scale,
                descriptor: SourceDescriptor::Region {
                    display_id: display.0.clone(),
                    scale,
                    region_pt: snapped,
                    method: RegionMethod::Crop,
                },
                is_window: false,
            })
        }
        SourceSpec::Application { display, app } => {
            let m = enumerate::monitor(display)?;
            let item: GraphicsCaptureItem =
                unsafe { interop()?.CreateForMonitor(m.hmon) }.map_err(os)?;
            let r = m.rect_px;
            let mut acc: Option<(i32, i32, i32, i32)> = None;
            for w in enumerate::app_window_rects(app.pid, &cfg.exclude) {
                let (x0, y0) = (w.left.max(r.left), w.top.max(r.top));
                let (x1, y1) = (w.right.min(r.right), w.bottom.min(r.bottom));
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                acc = Some(match acc {
                    None => (x0, y0, x1, y1),
                    Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
                });
            }
            let scale = m.scale * out_scale(cfg.resolution, m.scale);
            let name = enumerate::process_path(app.pid as u32).and_then(|p| {
                std::path::Path::new(&p)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            });
            let full = m.size_px();
            let crop = acc.and_then(|(x0, y0, x1, y1)| {
                let pt = RectPt {
                    x: f64::from(x0 - r.left) / m.scale,
                    y: f64::from(y0 - r.top) / m.scale,
                    w: f64::from(x1 - x0) / m.scale,
                    h: f64::from(y1 - y0) / m.scale,
                };
                ((x1 - x0) < full.w as i32 || (y1 - y0) < full.h as i32).then_some(pt)
            });
            let (crop_px, out, crop_pt) = match crop {
                Some(pt) => {
                    let (snapped, c, out) = crop_plan(&m, pt, cfg.resolution)?;
                    (Some(c), out, Some(snapped))
                }
                None => {
                    let k = out_scale(cfg.resolution, m.scale);
                    (
                        None,
                        even(f64::from(full.w) * k, f64::from(full.h) * k),
                        None,
                    )
                }
            };
            Ok(Plan {
                item,
                crop_px,
                out,
                scale,
                descriptor: SourceDescriptor::Application {
                    display_id: display.0.clone(),
                    app_bundle_id: app.bundle_id.clone(),
                    app_pid: Some(app.pid),
                    app_name: name,
                    scale,
                    crop_pt,
                },
                is_window: false,
            })
        }
        SourceSpec::Window { window } => {
            let hwnd = enumerate::hwnd_of(*window);
            let item: GraphicsCaptureItem =
                unsafe { interop()?.CreateForWindow(hwnd) }.map_err(os)?;
            let size = item.Size().map_err(os)?;
            let monitors = enumerate::monitors()?;
            let info = enumerate::window_info(hwnd, &monitors, 0);
            let m_scale = info
                .as_ref()
                .and_then(|i| i.display.as_ref())
                .and_then(|d| monitors.iter().find(|m| m.id == d.0))
                .map_or(1.0, |m| m.scale);
            let k = out_scale(cfg.resolution, m_scale);
            Ok(Plan {
                item,
                crop_px: None,
                out: even(f64::from(size.Width) * k, f64::from(size.Height) * k),
                scale: m_scale * k,
                descriptor: SourceDescriptor::Window {
                    window_id: window.0,
                    app_bundle_id: info.as_ref().and_then(|i| i.app.bundle_id.clone()),
                    app_name: info.as_ref().map(|i| i.app_name.clone()),
                    window_title: info.as_ref().map(|i| i.title.clone()),
                    scale: m_scale * k,
                },
                is_window: true,
            })
        }
        SourceSpec::Picked(_) => Err(Error::Unsupported("the system picker")),
        SourceSpec::Camera { .. } => Err(Error::Unsupported(
            "camera sources go through the camera, not the screen",
        )),
    }
}

struct State {
    gpu: Gpu,
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    content: SizeInt32,
    last_ns: Option<i64>,
}

/// The immediate context and staging texture are only touched under the stream's mutex, one
/// frame at a time; the D3D11 device itself is free-threaded.
struct Shared(State);

// SAFETY: see `Shared`: every access goes through the `Mutex` that owns it.
unsafe impl Send for Shared {}

type OnFrame = Box<dyn Fn(i64, Vec<u8>, u32, u32, usize) + Send>;

const FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;

fn grab(
    pool: &Direct3D11CaptureFramePool,
    st: &mut State,
    crop: Option<CropPx>,
    min_interval_ns: i64,
    deliver: &OnFrame,
) -> windows::core::Result<()> {
    let frame = pool.TryGetNextFrame()?;
    let t_ns = frame.SystemRelativeTime()?.Duration * 100;
    let content = frame.ContentSize()?;
    if content.Width != st.content.Width || content.Height != st.content.Height {
        st.content = content;
        pool.Recreate(&st.gpu.winrt, FORMAT, 2, content)?;
    }
    if st.last_ns.is_some_and(|l| t_ns - l < min_interval_ns) {
        return Ok(());
    }
    st.last_ns = Some(t_ns);
    let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
    let tex: ID3D11Texture2D = unsafe { access.GetInterface()? };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { tex.GetDesc(&mut desc) };
    let avail_w = desc.Width.min(content.Width.max(1) as u32);
    let avail_h = desc.Height.min(content.Height.max(1) as u32);
    let (sx, sy, sw, sh) = match crop {
        Some((x, y, w, h)) => {
            let x = x.min(avail_w.saturating_sub(1));
            let y = y.min(avail_h.saturating_sub(1));
            (x, y, w.min(avail_w - x), h.min(avail_h - y))
        }
        None => (0, 0, avail_w, avail_h),
    };
    if sw == 0 || sh == 0 {
        return Ok(());
    }
    if st
        .staging
        .as_ref()
        .is_none_or(|(_, w, h)| (*w, *h) != (sw, sh))
    {
        let sd = D3D11_TEXTURE2D_DESC {
            Width: sw,
            Height: sh,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut t = None;
        unsafe { st.gpu.device.CreateTexture2D(&sd, None, Some(&mut t))? };
        st.staging = t.map(|t| (t, sw, sh));
    }
    let Some((staging, _, _)) = st.staging.as_ref() else {
        return Ok(());
    };
    let bx = D3D11_BOX {
        left: sx,
        top: sy,
        front: 0,
        right: sx + sw,
        bottom: sy + sh,
        back: 1,
    };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        st.gpu
            .context
            .CopySubresourceRegion(staging, 0, 0, 0, 0, &tex, 0, Some(&bx));
        st.gpu
            .context
            .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
    }
    let pitch = mapped.RowPitch as usize;
    let bytes =
        unsafe { std::slice::from_raw_parts(mapped.pData as *const u8, pitch * sh as usize) }
            .to_vec();
    unsafe { st.gpu.context.Unmap(staging, 0) };
    deliver(t_ns, bytes, sw, sh, pitch);
    Ok(())
}

pub struct WgcStream {
    tag: StreamTag,
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
}

impl ScreenStream for WgcStream {
    fn tag(&self) -> StreamTag {
        self.tag
    }

    fn update_exclusions(&self, _exclude: &[WindowId]) -> Result<()> {
        Ok(())
    }

    fn stop(self: Box<Self>) -> Result<()> {
        let _ = self.session.Close();
        let _ = self.pool.Close();
        Ok(())
    }
}

fn open(
    plan: &Plan,
    fps: u32,
    show_cursor: bool,
    deliver: OnFrame,
) -> Result<(GraphicsCaptureSession, Direct3D11CaptureFramePool)> {
    let gpu = gpu()?;
    let size = plan.item.Size().map_err(os)?;
    let pool =
        Direct3D11CaptureFramePool::CreateFreeThreaded(&gpu.winrt, FORMAT, 2, size).map_err(os)?;
    let session = pool.CreateCaptureSession(&plan.item).map_err(os)?;
    let _ = session.SetIsCursorCaptureEnabled(show_cursor);
    let _ = session.SetIsBorderRequired(false);
    let min_interval_ns = 1_000_000_000 / i64::from(fps.max(1));
    let _ = session.SetMinUpdateInterval(windows::Foundation::TimeSpan {
        Duration: min_interval_ns / 100,
    });
    let state = Arc::new(Mutex::new(Shared(State {
        gpu,
        staging: None,
        content: size,
        last_ns: None,
    })));
    let crop = plan.crop_px;
    let handler =
        TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |pool, _| {
            let pool = pool.ok()?;
            let mut st = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Err(e) = grab(pool, &mut st.0, crop, min_interval_ns * 3 / 4, &deliver) {
                tracing::debug!("capture frame: {e}");
            }
            Ok(())
        });
    pool.FrameArrived(&handler).map_err(os)?;
    session.StartCapture().map_err(os)?;
    Ok((session, pool))
}

pub fn start(
    tag: StreamTag,
    plan: Plan,
    cfg: &ScreenStreamConfig,
    sink: Arc<dyn FrameSink>,
) -> Result<WgcStream> {
    let out = plan.out;
    let scale = plan.scale;
    let s = sink.clone();
    let deliver: OnFrame = Box::new(move |t_ns, bytes, w, h, pitch| {
        let src = SizePx { w, h };
        let nv12 = if src == out {
            packed_to_nv12_par(&bytes, pitch, out, PackedOrder::Bgra)
        } else {
            let (fit, _) = fit_packed(&bytes, pitch, src, out);
            packed_to_nv12_par(&fit, out.w as usize * 4, out, PackedOrder::Bgra)
        };
        s.on_screen(VideoFrame {
            stream: tag,
            pts: HostTime(t_ns),
            size: out,
            content_rect_pt: None,
            content_scale: Some(scale),
            buffer: FrameBuffer(Arc::new(nv12)),
        });
    });
    let (session, pool) = open(&plan, cfg.fps, cfg.show_cursor, deliver)?;
    if plan.is_window {
        let lost = sink.clone();
        let _ = plan.item.Closed(
            &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
                lost.on_event(SourceEvent::SourceLost {
                    stream: tag,
                    reason: "the window closed".into(),
                });
                Ok(())
            }),
        );
    }
    Ok(WgcStream { tag, session, pool })
}

pub fn snapshot(plan: Plan, max: SizePx) -> Result<RgbaImage> {
    let (tx, rx) = mpsc::sync_channel::<(Vec<u8>, u32, u32, usize)>(1);
    let deliver: OnFrame = Box::new(move |_, bytes, w, h, pitch| {
        let _ = tx.try_send((bytes, w, h, pitch));
    });
    let (session, pool) = open(&plan, 30, false, deliver)?;
    let got = rx.recv_timeout(Duration::from_secs(3));
    let _ = session.Close();
    let _ = pool.Close();
    let (bytes, w, h, pitch) =
        got.map_err(|_| Error::Timeout("no frame for the snapshot".into()))?;
    let k = (f64::from(max.w) / f64::from(w))
        .min(f64::from(max.h) / f64::from(h))
        .min(1.0);
    let out = SizePx {
        w: ((f64::from(w) * k).round() as u32).max(1),
        h: ((f64::from(h) * k).round() as u32).max(1),
    };
    let (mut fit, _) = fit_packed(&bytes, pitch, SizePx { w, h }, out);
    for px in fit.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    Ok(RgbaImage {
        size: out,
        stride: out.w as usize * 4,
        data: fit,
    })
}
