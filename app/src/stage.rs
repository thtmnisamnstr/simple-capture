//! The Preview window's renderer (ADR-043): the composed output (source in its frame, the
//! letterbox background, the camera) at the window's size in points (1×: a quarter of the pixels
//! to render, read back and hand to WebKit on a Retina display). The source refreshes from a
//! snapshot once a second; the camera is live. A frame is rendered only when something it shows
//! changed, and sent as I420 (what the encoder gets, and 3/8 of the bytes of RGBA).

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
const SIZE_EVERY: Duration = Duration::from_millis(500);

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
    let mut inner: Option<(Instant, SizePx)> = None;
    let mut shown: Option<Shown> = None;
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
            drop(core);
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        if inner.is_none_or(|(at, _)| at.elapsed() >= SIZE_EVERY) {
            inner = core.stage_window_pt().map(|s| (Instant::now(), s));
        }
        let Some((_, window)) = inner else {
            drop(core);
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
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
        let canvas = fit(ratio, window);
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
            match compositor.render(&job, OutputFormat::I420Bt709Limited) {
                Ok(out) => {
                    core.live
                        .publish("stage", canvas.w, canvas.h, crate::live::I420, &out.data);
                    shown = Some(now_showing);
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
