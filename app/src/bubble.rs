//! The camera bubble (M2_PLAN §3.4–3.5, ADR-041): where its window sits, how drags and resizes
//! change the camera key, and the thread that renders it.

use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use sc_core::camera::camera_at;
use sc_core::image::RgbaImage;
use sc_core::project::CameraKey;
use sc_geometry::{
    CAMERA_SIZE_MAX, CAMERA_SIZE_MIN, PointF, RectF, RectPt, SizePx, clamp_center, shape_size,
    snap_center,
};
use sc_platform::DisplayInfo;
use sc_render::{Backend, Compositor, FrameJob, OutputFormat};
use serde::Serialize;

use crate::core::Core;
use crate::dto::SourceSpecDto;
use crate::settings::RecorderSettings;

/// Room around the shape for the shadow and the resize handles.
pub const MARGIN_PT: f64 = 18.0;
const SNAP_PT: f64 = 12.0;
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Geom {
    /// Window frame in global points.
    pub window: RectPt,
    /// Shape inside the window, in window points.
    pub shape: RectPt,
    pub scale: f64,
    /// The bubble's spot on screen is its spot in the video (whole display, Original frame).
    pub maps_output: bool,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    corner: Option<(f64, f64)>,
    start: PointF,
    key_center: PointF,
    key_size: f64,
    shape_at: PointF,
}

#[derive(Debug, Default)]
pub struct BubbleState {
    pub key: Option<CameraKey>,
    pub shown: bool,
    pub preview_active: bool,
    pub floating_at: Option<PointF>,
    pub geom: Option<Geom>,
    pub version: u64,
    pub last_key_sent: Option<Instant>,
    drag: Option<Drag>,
}

pub fn maps_output(s: &RecorderSettings) -> bool {
    matches!(s.source, Some(SourceSpecDto::Display { .. }))
        && s.canvas.aspect == sc_geometry::Aspect::Original
}

pub fn display_for<'a>(
    s: &RecorderSettings,
    displays: &'a [DisplayInfo],
) -> Option<&'a DisplayInfo> {
    let id = match &s.source {
        Some(SourceSpecDto::Display { display_id })
        | Some(SourceSpecDto::Application { display_id, .. })
        | Some(SourceSpecDto::Region { display_id, .. }) => Some(display_id.as_str()),
        _ => None,
    };
    displays
        .iter()
        .find(|d| Some(d.id.0.as_str()) == id)
        .or_else(|| displays.iter().find(|d| d.is_main))
        .or(displays.first())
}

fn shape_pt(key: &CameraKey, display: &DisplayInfo) -> (f64, f64) {
    let short = display.frame_pt.w.min(display.frame_pt.h)
        * key.size_norm.clamp(CAMERA_SIZE_MIN, CAMERA_SIZE_MAX);
    shape_size(short, key.shape.aspect())
}

pub fn geometry(
    key: &CameraKey,
    display: &DisplayInfo,
    maps_output: bool,
    floating_at: Option<PointF>,
) -> Geom {
    let d = display.frame_pt;
    let (w, h) = shape_pt(key, display);
    let at = if maps_output {
        PointF {
            x: d.x + key.center_norm.x * d.w - w / 2.0,
            y: d.y + key.center_norm.y * d.h - h / 2.0,
        }
    } else {
        floating_at.unwrap_or(PointF {
            x: d.x + d.w - w - 3.0 * MARGIN_PT,
            y: d.y + d.h - h - 3.0 * MARGIN_PT,
        })
    };
    Geom {
        window: RectPt {
            x: at.x - MARGIN_PT,
            y: at.y - MARGIN_PT,
            w: w + 2.0 * MARGIN_PT,
            h: h + 2.0 * MARGIN_PT,
        },
        shape: RectPt {
            x: MARGIN_PT,
            y: MARGIN_PT,
            w,
            h,
        },
        scale: display.scale,
        maps_output,
    }
}

impl BubbleState {
    /// `corner` is the dragged handle's direction (±1, ±1); `None` moves the bubble.
    pub fn drag_begin(&mut self, corner: Option<(f64, f64)>, at: PointF) {
        let (Some(key), Some(geom)) = (&self.key, self.geom) else {
            return;
        };
        self.drag = Some(Drag {
            corner,
            start: at,
            key_center: key.center_norm,
            key_size: key.size_norm,
            shape_at: PointF {
                x: geom.window.x + MARGIN_PT,
                y: geom.window.y + MARGIN_PT,
            },
        });
    }

    /// Applies the drag to the key, or for a floating bubble that is only moved, to its own spot.
    /// `true` when the key changed.
    pub fn drag_to(&mut self, at: PointF, display: &DisplayInfo, maps_output: bool) -> bool {
        let (Some(drag), Some(key)) = (self.drag, self.key.as_mut()) else {
            return false;
        };
        let (dx, dy) = (at.x - drag.start.x, at.y - drag.start.y);
        let d = display.frame_pt;
        let canvas = display.size_px;
        let aspect = key.shape.aspect();
        match drag.corner {
            None if maps_output => {
                let c = PointF {
                    x: drag.key_center.x + dx / d.w,
                    y: drag.key_center.y + dy / d.h,
                };
                key.center_norm =
                    snap_center(canvas, aspect, key.size_norm, c, SNAP_PT * display.scale);
                true
            }
            None => {
                self.floating_at = Some(PointF {
                    x: drag.shape_at.x + dx,
                    y: drag.shape_at.y + dy,
                });
                false
            }
            Some((sx, sy)) => {
                let (w0, h0) = shape_size(d.w.min(d.h) * drag.key_size, aspect);
                let scale = (1.0 + dx * sx / w0 + dy * sy / h0).max(0.05);
                key.size_norm = (drag.key_size * scale).clamp(CAMERA_SIZE_MIN, CAMERA_SIZE_MAX);
                if maps_output {
                    key.center_norm = clamp_center(canvas, aspect, key.size_norm, key.center_norm);
                }
                true
            }
        }
    }

    pub fn drag_end(&mut self) {
        self.drag = None;
    }
}

/// Renders the bubble while it is shown: the newest camera frame through the compositor, at the
/// window's device-pixel size, published as live stream `bubble`.
pub fn render_loop(core: Weak<Core>) {
    let mut compositor = match Compositor::new(Backend::Auto) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("bubble renderer: {e}");
            return;
        }
    };
    let mut last: Option<(i64, u64)> = None;
    let mut clear: Option<RgbaImage> = None;
    loop {
        let t0 = Instant::now();
        let Some(core) = core.upgrade() else { return };
        if core.exiting() {
            return;
        }
        let (key, geom, version) = {
            let b = lock(&core.bubble);
            (b.key.clone(), b.geom.filter(|_| b.shown), b.version)
        };
        let frame = core.camera.latest();
        if let (Some(key), Some(geom), Some(frame)) = (key, geom, frame) {
            let stamp = (frame.pts.0, version);
            if last != Some(stamp) {
                last = Some(stamp);
                let canvas = SizePx {
                    w: (geom.window.w * geom.scale).round().max(2.0) as u32,
                    h: (geom.window.h * geom.scale).round().max(2.0) as u32,
                };
                let bg = match clear.take() {
                    Some(b) if b.size == canvas => b,
                    _ => RgbaImage {
                        size: canvas,
                        stride: canvas.w as usize * 4,
                        data: vec![0; canvas.w as usize * canvas.h as usize * 4],
                    },
                };
                let mut shown = key;
                shown.visible = true;
                if let Some(p) = camera_at(&[shown], 0) {
                    let dst = RectF {
                        x: geom.shape.x * geom.scale,
                        y: geom.shape.y * geom.scale,
                        w: geom.shape.w * geom.scale,
                        h: geom.shape.h * geom.scale,
                    };
                    let job = FrameJob {
                        canvas,
                        background: &bg,
                        screen: None,
                        camera: Some(sc_render::camera_layer(&p, &frame.buffer, dst)),
                    };
                    match compositor.render(&job, OutputFormat::Rgba8) {
                        Ok(out) => core.live.publish("bubble", canvas.w, canvas.h, &out.data),
                        Err(e) => tracing::warn!("bubble render: {e}"),
                    }
                }
                clear = Some(bg);
            }
        }
        drop(core);
        std::thread::sleep(FRAME_INTERVAL.saturating_sub(t0.elapsed()));
    }
}

pub fn spawn(core: &Arc<Core>) {
    let weak = Arc::downgrade(core);
    if let Err(e) = std::thread::Builder::new()
        .name("sc-bubble".into())
        .spawn(move || render_loop(weak))
    {
        tracing::warn!("bubble renderer thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::project::CameraShape;
    use sc_platform::DisplayId;

    fn display() -> DisplayInfo {
        DisplayInfo {
            id: DisplayId("1".into()),
            name: "D".into(),
            frame_pt: RectPt {
                x: 0.0,
                y: 0.0,
                w: 1600.0,
                h: 900.0,
            },
            scale: 2.0,
            size_px: SizePx { w: 3200, h: 1800 },
            is_main: true,
        }
    }

    fn key() -> CameraKey {
        CameraKey::new(0, CameraShape::Circle, PointF { x: 0.5, y: 0.5 }, 0.2)
    }

    fn state(maps: bool) -> BubbleState {
        BubbleState {
            key: Some(key()),
            geom: Some(geometry(&key(), &display(), maps, None)),
            ..Default::default()
        }
    }

    #[test]
    fn a_mapped_bubble_sits_where_the_camera_lands_in_the_video() {
        let g = geometry(&key(), &display(), true, None);
        assert_eq!(g.shape.w, 180.0);
        assert_eq!(g.window.x + g.shape.x + g.shape.w / 2.0, 800.0);
        assert_eq!(g.window.y + g.shape.y + g.shape.h / 2.0, 450.0);
        assert_eq!(g.window.w, 180.0 + 2.0 * MARGIN_PT);
    }

    #[test]
    fn dragging_a_mapped_bubble_moves_and_snaps_the_key() {
        let mut b = state(true);
        b.drag_begin(None, PointF { x: 800.0, y: 450.0 });
        assert!(b.drag_to(PointF { x: 900.0, y: 450.0 }, &display(), true));
        let c = b.key.as_ref().expect("key").center_norm;
        assert!(
            (c.x - 0.5625).abs() < 1e-9 && (c.y - 0.5).abs() < 1e-9,
            "{c:?}"
        );
        b.drag_to(
            PointF {
                x: 800.0 + 683.0,
                y: 450.0,
            },
            &display(),
            true,
        );
        let c = b.key.as_ref().expect("key").center_norm;
        let right = (3200.0 - 1800.0 * 0.03 - 180.0) / 3200.0;
        assert!(
            (c.x - right).abs() < 1e-9,
            "snapped to the right margin: {c:?}"
        );
    }

    #[test]
    fn dragging_a_floating_bubble_moves_only_the_bubble() {
        let mut b = state(false);
        b.drag_begin(None, PointF { x: 0.0, y: 0.0 });
        assert!(!b.drag_to(PointF { x: -50.0, y: -20.0 }, &display(), false));
        assert_eq!(b.key.as_ref().expect("key").center_norm, key().center_norm);
        let g0 = geometry(&key(), &display(), false, None);
        assert_eq!(
            b.floating_at.expect("moved").x,
            g0.window.x + MARGIN_PT - 50.0
        );
    }

    #[test]
    fn resizing_from_any_corner_grows_the_shape_and_keeps_it_on_screen() {
        let mut b = state(true);
        b.drag_begin(Some((1.0, 1.0)), PointF { x: 0.0, y: 0.0 });
        assert!(b.drag_to(PointF { x: 90.0, y: 90.0 }, &display(), true));
        assert!((b.key.as_ref().expect("key").size_norm - 0.4).abs() < 1e-9);
        let mut b = state(true);
        b.drag_begin(Some((-1.0, -1.0)), PointF { x: 0.0, y: 0.0 });
        assert!(b.drag_to(PointF { x: 45.0, y: 45.0 }, &display(), true));
        assert!((b.key.as_ref().expect("key").size_norm - 0.1).abs() < 1e-9);
        assert!(b.drag_to(
            PointF {
                x: -100_000.0,
                y: -100_000.0
            },
            &display(),
            true
        ));
        assert_eq!(b.key.as_ref().expect("key").size_norm, CAMERA_SIZE_MAX);
    }
}
