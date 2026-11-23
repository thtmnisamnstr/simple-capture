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
use sc_geometry::{Ratio, RectPx, SizePx, camera_rects, even_round};
use sc_platform::FrameBuffer;
use sc_render::{Backend, Compositor, FrameJob, ScreenLayer};

use crate::core::Core;
use crate::preview::StageFrame;
use crate::sink::Sink;
use crate::still::rgba_to_nv12;

/// At most 60 draws a second: a 30 fps cap beat against the camera's own 30 fps and skipped
/// about one frame in five; between new frames the loop waits on the camera instead.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const IDLE_WAIT: Duration = Duration::from_millis(30);
const SNAPSHOT_EVERY: Duration = Duration::from_millis(1000);
const OFFSCREEN_POLL: Duration = Duration::from_millis(100);
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
    pub fit: sc_core::project::Fit,
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
    device: Option<sc_core::time::HostTime>,
    key: Option<sc_core::project::CameraKey>,
    background: u64,
    snapshot: Option<Instant>,
    canvas: SizePx,
    drawable: SizePx,
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
            shown = None;
            if let Some(sink) = &mut sink {
                sink.forget_page();
            }
            drop(core);
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        let sink = sink.get_or_insert_with(|| {
            Sink::open(&compositor, "stage", || {
                #[cfg(target_os = "macos")]
                return core.stage_layer();
                #[cfg(not(target_os = "macos"))]
                Err("no native layer".into())
            })
        });
        let Some(page) = core.stage_page() else {
            drop(core);
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        let drawable = sink.drawable(page);
        let camera_only = matches!(inputs.source, Some(sc_platform::SourceSpec::Camera { .. }));
        let device = matches!(inputs.source, Some(sc_platform::SourceSpec::Device { .. }));
        let device_seen = core.screens.frames_seen();
        let device_frame = device.then(|| core.screens.latest()).flatten();
        let window = SizePx {
            w: page.w.round() as u32,
            h: page.h.round() as u32,
        };
        if snap
            .as_ref()
            .is_none_or(|s| s.at.elapsed() >= SNAPSHOT_EVERY)
        {
            let img = inputs
                .source
                .as_ref()
                .filter(|_| !camera_only && !device)
                .and_then(|spec| {
                    core.platform
                        .screen()
                        .snapshot(
                            spec,
                            SizePx {
                                w: drawable.w.max(window.w).min(2560),
                                h: drawable.h.max(window.h).min(2560),
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
        let cam_seen = core.camera.frames_seen();
        let cam_frame = core.camera.latest();
        let content = if camera_only {
            cam_frame.as_ref().map(|f| f.buffer.0.size())
        } else if device {
            device_frame.as_ref().map(|f| f.buffer.0.size())
        } else {
            snap.as_ref().and_then(|s| s.size)
        };
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
        let now_showing = Shown {
            camera: cam_frame.as_ref().map(|f| f.pts),
            device: device_frame.as_ref().map(|f| f.pts),
            key: inputs.key.clone(),
            background: bg_key,
            snapshot: snap.as_ref().map(|s| s.at),
            canvas,
            drawable,
        };
        if shown.as_ref() == Some(&now_showing) {
            let (feed, seen) = if device {
                (core.screens.clone(), device_seen)
            } else {
                (core.camera.clone(), cam_seen)
            };
            drop(core);
            feed.wait_newer(seen, IDLE_WAIT);
            continue;
        }
        if let Some((_, _, background)) = &bg {
            let main = if camera_only {
                cam_frame.as_ref().map(|f| &f.buffer)
            } else if device {
                device_frame.as_ref().map(|f| &f.buffer)
            } else {
                snap.as_ref().and_then(|s| s.frame.as_ref())
            };
            let screen = main.map(|f| {
                let size = f.0.size();
                let (src, dst) = sc_player::video::place(
                    inputs.fit,
                    canvas,
                    RectPx {
                        x: 0,
                        y: 0,
                        w: size.w,
                        h: size.h,
                    },
                );
                ScreenLayer {
                    frame: f,
                    src,
                    dst,
                    redact: &[],
                }
            });
            let placement = inputs
                .key
                .as_ref()
                .filter(|_| !camera_only)
                .and_then(|k| camera_at(std::slice::from_ref(k), 0));
            let mut camera_dst = None;
            let camera = cam_frame.as_ref().zip(placement.as_ref()).map(|(f, p)| {
                let rects = camera_rects(canvas, f.buffer.0.size(), p.shape.aspect(), &p.pose);
                camera_dst = Some(rects.dst);
                sc_render::camera_layer(p, &f.buffer, rects.dst)
            });
            let job = FrameJob {
                text: &[],
                background_key: Some(bg_key),
                canvas,
                background,
                screen,
                camera,
            };
            match sink.show(&core, &mut compositor, &job, drawable, None, CLEAR) {
                Ok(true) => {
                    shown = Some(now_showing);
                    core.stage_presented(StageFrame {
                        native: sink.native(),
                        drawable,
                        canvas,
                        camera: camera_dst,
                    });
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
