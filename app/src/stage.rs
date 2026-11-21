//! The Preview window's renderer (ADR-043): the composed output (source in its frame, the
//! letterbox background, the camera). The source refreshes from a snapshot once a second; the
//! camera is live. A frame is rendered only when something it shows changed.
//!
//! On macOS it draws at the display's pixel size into a `CAMetalLayer` under the page (ADR-007),
//! and the page only learns the frame size. Elsewhere, or with `SC_PREVIEW=webview`, it renders
//! at the window's size in points and sends I420 frames to the page ("webview frames").

use std::path::Path;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use sc_core::camera::camera_at;
use sc_core::image::RgbaImage;
use sc_core::project::Background;
use sc_geometry::{Ratio, RectPx, SizePx, camera_rects, contain_fit, even_round};
use sc_platform::FrameBuffer;
use sc_render::{Backend, Compositor, FrameJob, OutputFormat, ScreenLayer};

use crate::core::Core;
use crate::still::rgba_to_nv12;

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const IDLE_POLL: Duration = Duration::from_millis(8);
const SNAPSHOT_EVERY: Duration = Duration::from_millis(1000);
const SIZE_EVERY: Duration = Duration::from_millis(250);
const OFFSCREEN_POLL: Duration = Duration::from_millis(100);
#[cfg(target_os = "macos")]
const CLEAR: [u8; 3] = [0x11, 0x11, 0x11];

/// The largest even-sized `ratio` rectangle inside `inner`.
pub fn fit(ratio: Ratio, inner: SizePx) -> SizePx {
    let r = ratio.as_f64();
    let (w, h) = (f64::from(inner.w), f64::from(inner.h));
    let (fw, fh) = if w / h > r { (h * r, h) } else { (w, w / r) };
    let even_cap = |v: u32| (v & !1).max(2);
    SizePx {
        w: even_round(fw).min(even_cap(inner.w)),
        h: even_round(fh).min(even_cap(inner.h)),
    }
}

pub struct StageInputs {
    pub source: Option<sc_platform::SourceSpec>,
    pub aspect: sc_geometry::Aspect,
    pub background: Background,
    pub key: Option<sc_core::project::CameraKey>,
}

struct Snap {
    at: Instant,
    frame: Option<FrameBuffer>,
    size: Option<SizePx>,
}

#[derive(PartialEq)]
struct Shown {
    camera: Option<sc_core::time::HostTime>,
    key: Option<sc_core::project::CameraKey>,
    background: u64,
    snapshot: Option<Instant>,
    canvas: SizePx,
    drawable: SizePx,
}

enum Sink {
    Webview,
    #[cfg(target_os = "macos")]
    Native {
        surface: sc_render::NativeSurface,
        top_inset: f64,
        told: Option<SizePx>,
    },
}

impl Sink {
    fn open(core: &Core, compositor: &Compositor, scale: f64) -> Sink {
        #[cfg(target_os = "macos")]
        if std::env::var("SC_PREVIEW").as_deref() != Ok("webview") {
            let surface = core.stage_layer(scale).and_then(|(layer, top_inset)| {
                // SAFETY: the layer lives as long as the preview window, which is never destroyed.
                let surface = unsafe { compositor.metal_layer_surface(layer) };
                surface.map(|s| (s, top_inset)).map_err(|e| e.to_string())
            });
            match surface {
                Ok((surface, top_inset)) => {
                    return Sink::Native {
                        surface,
                        top_inset,
                        told: None,
                    };
                }
                Err(e) => tracing::warn!("stage: no native layer, using webview frames: {e}"),
            }
        }
        let _ = (core, compositor, scale);
        Sink::Webview
    }

    fn native(&self) -> bool {
        !matches!(self, Sink::Webview)
    }

    /// Native layers draw the page's part of the window at the display's pixel size, webview
    /// frames at the window's point size.
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    fn drawable(&self, window_pt: SizePx, scale: f64) -> SizePx {
        match self {
            Sink::Webview => window_pt,
            #[cfg(target_os = "macos")]
            Sink::Native { top_inset, .. } => {
                let px = |v: f64| ((v * scale).round() as u32).max(2);
                SizePx {
                    w: px(f64::from(window_pt.w)),
                    h: px(f64::from(window_pt.h) - top_inset),
                }
            }
        }
    }

    fn forget_page(&mut self) {
        #[cfg(target_os = "macos")]
        if let Sink::Native { told, .. } = self {
            *told = None;
        }
    }

    /// `Ok(false)`: not on screen, nothing drawn.
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    fn show(
        &mut self,
        core: &Core,
        compositor: &mut Compositor,
        job: &FrameJob,
        drawable: SizePx,
    ) -> sc_render::Result<bool> {
        let canvas = job.canvas;
        match self {
            Sink::Webview => {
                let out = compositor.render(job, OutputFormat::I420Bt709Limited)?;
                core.live
                    .publish("stage", canvas.w, canvas.h, crate::live::I420, &out.data);
                Ok(true)
            }
            #[cfg(target_os = "macos")]
            Sink::Native { surface, told, .. } => {
                let at = ((drawable.w - canvas.w) / 2, (drawable.h - canvas.h) / 2);
                let shown = compositor.present(job, surface, drawable, at, CLEAR)?;
                if shown && *told != Some(canvas) {
                    core.live
                        .publish("stage", canvas.w, canvas.h, crate::live::NATIVE, &[]);
                    *told = Some(canvas);
                }
                Ok(shown)
            }
        }
    }
}

pub fn render_loop(core: Weak<Core>) {
    let mut compositor = match Compositor::new(Backend::Auto) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("stage renderer: {e}");
            return;
        }
    };
    let mut snap: Option<Snap> = None;
    let mut bg: Option<(Background, SizePx, RgbaImage)> = None;
    let mut bg_key = 0u64;
    let mut inner: Option<(Instant, SizePx, f64)> = None;
    let mut shown: Option<Shown> = None;
    let mut sink: Option<Sink> = None;
    loop {
        let t0 = Instant::now();
        let Some(core) = core.upgrade() else { return };
        if core.exiting() {
            return;
        }
        let Some(inputs) = core.stage_inputs() else {
            snap = None;
            inner = None;
            shown = None;
            if let Some(sink) = &mut sink {
                sink.forget_page();
            }
            drop(core);
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        if inner.is_none_or(|(at, _, _)| at.elapsed() >= SIZE_EVERY) {
            inner = core
                .stage_window()
                .map(|(pt, scale)| (Instant::now(), pt, scale));
        }
        let Some((_, window, scale)) = inner else {
            drop(core);
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        let sink = sink.get_or_insert_with(|| Sink::open(&core, &compositor, scale));
        let drawable = sink.drawable(window, scale);
        if snap
            .as_ref()
            .is_none_or(|s| s.at.elapsed() >= SNAPSHOT_EVERY)
        {
            let img = inputs.source.as_ref().and_then(|spec| {
                core.platform
                    .screen()
                    .snapshot(
                        spec,
                        SizePx {
                            w: window.w.min(1280),
                            h: window.h.min(1280),
                        },
                    )
                    .map_err(|e| tracing::debug!("stage snapshot: {e}"))
                    .ok()
            });
            snap = Some(Snap {
                at: Instant::now(),
                size: img.as_ref().map(|i| i.size),
                frame: img.map(|i| FrameBuffer(Arc::new(rgba_to_nv12(&i)))),
            });
        }
        let content = snap.as_ref().and_then(|s| s.size);
        let ratio = inputs
            .aspect
            .ratio()
            .or(content.map(Ratio::of))
            .unwrap_or(Ratio::new(16, 9));
        let canvas = fit(ratio, drawable);
        if bg
            .as_ref()
            .is_none_or(|(b, s, _)| *b != inputs.background || *s != canvas)
        {
            match sc_text::rasterize_background(&inputs.background, canvas, Path::new("/")) {
                Ok(img) => {
                    bg = Some((inputs.background.clone(), canvas, img));
                    bg_key += 1;
                }
                Err(e) => tracing::warn!("stage background: {e}"),
            }
        }
        let cam_frame = core.camera.latest();
        let now_showing = Shown {
            camera: cam_frame.as_ref().map(|f| f.pts),
            key: inputs.key.clone(),
            background: bg_key,
            snapshot: snap.as_ref().map(|s| s.at),
            canvas,
            drawable,
        };
        if shown.as_ref() == Some(&now_showing) {
            drop(core);
            std::thread::sleep(IDLE_POLL);
            continue;
        }
        if let Some((_, _, background)) = &bg {
            let screen = snap.as_ref().and_then(|s| s.frame.as_ref()).map(|f| {
                let size = f.0.size();
                ScreenLayer {
                    frame: f,
                    src: RectPx {
                        x: 0,
                        y: 0,
                        w: size.w,
                        h: size.h,
                    },
                    dst: contain_fit(canvas, size),
                }
            });
            let placement = inputs
                .key
                .as_ref()
                .and_then(|k| camera_at(std::slice::from_ref(k), 0));
            let camera = cam_frame.as_ref().zip(placement.as_ref()).map(|(f, p)| {
                let rects = camera_rects(canvas, f.buffer.0.size(), p.shape.aspect(), &p.pose);
                sc_render::camera_layer(p, &f.buffer, rects.dst)
            });
            let job = FrameJob {
                background_key: Some(bg_key),
                canvas,
                background,
                screen,
                camera,
            };
            match sink.show(&core, &mut compositor, &job, drawable) {
                Ok(true) => {
                    shown = Some(now_showing);
                    core.stage_presented(sink.native());
                }
                Ok(false) => {
                    drop(core);
                    std::thread::sleep(OFFSCREEN_POLL);
                    continue;
                }
                Err(e) => tracing::warn!("stage render: {e}"),
            }
        }
        drop(core);
        std::thread::sleep(FRAME_INTERVAL.saturating_sub(t0.elapsed()));
    }
}

pub fn spawn(core: &Arc<Core>) {
    let weak = Arc::downgrade(core);
    if let Err(e) = std::thread::Builder::new()
        .name("sc-stage".into())
        .spawn(move || render_loop(weak))
    {
        tracing::warn!("stage renderer thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_the_frame_shape_inside_the_window() {
        assert_eq!(
            fit(Ratio::new(16, 9), SizePx { w: 960, h: 960 }),
            SizePx { w: 960, h: 540 }
        );
        assert_eq!(
            fit(Ratio::new(9, 16), SizePx { w: 960, h: 600 }),
            SizePx { w: 338, h: 600 }
        );
        assert_eq!(
            fit(Ratio::new(1, 1), SizePx { w: 3, h: 3 }),
            SizePx { w: 2, h: 2 }
        );
    }
}
