//! The edited timeline's pictures: the screen segment frame, the camera frame and its placement
//! at any timeline time, ready for the compositor. The editor preview, thumbnails and every
//! export use this, so they show the same thing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_core::camera::{CameraPlacement, camera_at};
use sc_core::edit::{Doc, locate_in, starts};
use sc_core::image::RgbaImage;
use sc_core::project::Fit;
use sc_core::project::ProjectV1;
use sc_decode::{SegmentFrame, SegmentFrameSource, TrackFrameSource};
use sc_geometry::{
    PointF, Ratio, RectF, RectPx, SizePx, camera_rects, contain_fit, cover_src_rect,
};
use sc_platform::{Decoder, Error, FrameBuffer, Result};
use sc_render::{FrameJob, Redact, ScreenLayer, TextLayer};
use sc_text::{BackgroundCache, TextEngine, TextFrame};

pub struct TimelineVideo {
    source: SegmentFrameSource,
    camera: Option<(TrackFrameSource, String)>,
    doc: Doc,
    starts: Vec<i64>,
    camera_starts: Vec<i64>,
    backgrounds: BackgroundCache,
    project_dir: PathBuf,
    segment_sizes: Vec<SizePx>,
    text: Option<TextEngine>,
    text_failed: bool,
    clicks: Vec<Click>,
    screen_rects: std::collections::HashMap<String, sc_geometry::RectPt>,
}

/// A button press from the recording's pointer log, in global points.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Click {
    t_rec_ns: i64,
    x: f64,
    y: f64,
}

/// How long a click's ring shows after the press.
pub const CLICK_SHOW_NS: i64 = 450_000_000;

/// A click ring on the canvas: centre in canvas pixels and how far through its fade it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClickMark {
    pub x: f32,
    pub y: f32,
    pub age: f32,
}

pub struct Composed {
    pub rec_ns: i64,
    /// Recording time of the camera picture; `None` in a gap on an unlinked camera track.
    pub camera_rec_ns: Option<i64>,
    pub screen: Option<SegmentFrame>,
    pub camera: Option<(FrameBuffer, CameraPlacement)>,
    pub text: Vec<TextFrame>,
    pub redact: Vec<Redact>,
    pub fit: Fit,
    pub focus: PointF,
    pub clicks: Vec<ClickMark>,
    canvas_short: u32,
}

impl TimelineVideo {
    pub fn open(
        project: &ProjectV1,
        dir: &Path,
        decoder: Arc<dyn Decoder>,
        doc: Doc,
    ) -> Result<TimelineVideo> {
        let camera = match doc.camera_layout.overlays.first() {
            Some(o) => TrackFrameSource::open(project, dir, decoder.clone(), &o.track_id)?
                .map(|src| (src, o.id.clone())),
            None => None,
        };
        Ok(TimelineVideo {
            source: SegmentFrameSource::open(project, dir, decoder)?,
            camera,
            starts: starts(&doc.edl),
            camera_starts: starts(doc.camera_clips()),
            doc,
            backgrounds: BackgroundCache::new(2),
            project_dir: dir.to_path_buf(),
            segment_sizes: project
                .recording
                .screen
                .segments
                .iter()
                .map(|s| SizePx {
                    w: s.content_px.w,
                    h: s.content_px.h,
                })
                .collect(),
            text: None,
            text_failed: false,
            clicks: load_clicks(project, dir),
            screen_rects: project
                .recording
                .screen
                .segments
                .iter()
                .filter_map(|s| {
                    let r = s.extra.get("screen_pt")?;
                    Some((s.id.clone(), serde_json::from_value(r.clone()).ok()?))
                })
                .collect(),
        })
    }

    pub fn set_doc(&mut self, doc: Doc) {
        self.starts = starts(&doc.edl);
        self.camera_starts = starts(doc.camera_clips());
        self.doc = doc;
    }

    pub fn duration(&self) -> i64 {
        *self.starts.last().unwrap_or(&0)
    }

    pub fn canvas_ratio(&self) -> Ratio {
        let c = &self.doc.output.canvas;
        let reference = sc_geometry::reference_canvas_size(c.aspect, c.ratio, &self.segment_sizes);
        c.aspect.ratio().unwrap_or(Ratio::of(reference))
    }

    /// The frame at `tl_ns` on a `canvas`; past the end, the last frame.
    pub fn compose(&mut self, tl_ns: i64, canvas: SizePx) -> Result<Composed> {
        let end = self.duration();
        if end <= 0 {
            return Err(Error::Other("sc_player: the timeline is empty".into()));
        }
        let tl = tl_ns.clamp(0, end - 1);
        let loc = locate_in(&self.doc.edl, &self.starts, tl)
            .ok_or_else(|| Error::Other(format!("sc_player: no clip at {tl} ns")))?;
        let rec = loc.rec_ns;
        let screen_shown = !self.doc.edl.clips[loc.index].hidden;
        let mut screen = if screen_shown {
            self.source.frame_at(rec)?
        } else {
            None
        };
        let camera_rec = match &self.doc.camera_edl {
            None => Some(rec),
            Some(cam) => locate_in(cam, &self.camera_starts, tl)
                .filter(|l| !cam.clips[l.index].hidden)
                .map(|l| l.rec_ns),
        };
        let mut camera = None;
        if let (Some((src, overlay)), Some(cam_rec)) = (self.camera.as_mut(), camera_rec)
            && let Some(f) = src.frame_at(cam_rec)?
        {
            let placement = self
                .doc
                .camera_layout
                .overlays
                .iter()
                .find(|o| &o.id == overlay)
                .and_then(|o| camera_at(&o.keyframes, cam_rec));
            if screen_shown {
                camera = placement.map(|p| (f, p));
            } else {
                screen = Some(full_frame(f));
            }
        }
        let text = self.text_at(tl_ns.clamp(0, end), canvas);
        let (fit, focus) = framing(&self.doc, screen.as_ref().map(|f| f.segment_id.as_str()));
        let clicks = match screen.as_ref() {
            Some(f) if screen_shown && !self.clicks.is_empty() => {
                self.click_marks(rec, f, fit, focus, canvas)
            }
            _ => Vec::new(),
        };
        let redact = match screen.as_ref() {
            Some(f) if screen_shown && f.content_px.w > 0 && f.content_px.h > 0 => recrop(
                &redactions_at(&self.doc, rec),
                f.content_px,
                place(fit, focus, canvas, f.content_px).0,
            ),
            _ => Vec::new(),
        };
        Ok(Composed {
            rec_ns: rec,
            camera_rec_ns: camera_rec,
            screen,
            camera,
            text,
            redact,
            fit,
            focus,
            clicks,
            canvas_short: canvas.w.min(canvas.h),
        })
    }

    /// Rings for the presses of the last [`CLICK_SHOW_NS`] that fall inside this picture.
    fn click_marks(
        &self,
        rec: i64,
        f: &SegmentFrame,
        fit: Fit,
        focus: PointF,
        canvas: SizePx,
    ) -> Vec<ClickMark> {
        let Some(on) = self.screen_rects.get(&f.segment_id) else {
            return Vec::new();
        };
        if on.w <= 0.0 || on.h <= 0.0 || f.content_px.w == 0 || f.content_px.h == 0 {
            return Vec::new();
        }
        let (src, dst) = place(fit, focus, canvas, f.content_px);
        let from = self
            .clicks
            .partition_point(|c| c.t_rec_ns < rec - CLICK_SHOW_NS);
        self.clicks[from..]
            .iter()
            .take_while(|c| c.t_rec_ns <= rec)
            .filter_map(|c| {
                let (x, y) = click_on_canvas((c.x, c.y), *on, f.content_px, src, dst)?;
                Some(ClickMark {
                    x,
                    y,
                    age: ((rec - c.t_rec_ns) as f64 / CLICK_SHOW_NS as f64) as f32,
                })
            })
            .collect()
    }

    fn text_at(&mut self, tl: i64, canvas: SizePx) -> Vec<TextFrame> {
        if self.doc.text.is_empty() || self.text_failed {
            return Vec::new();
        }
        if self.text.is_none() {
            match TextEngine::new() {
                Ok(e) => self.text = Some(e),
                Err(e) => {
                    tracing::warn!("sc_player: no text engine, text is not drawn: {e}");
                    self.text_failed = true;
                    return Vec::new();
                }
            }
        }
        let Some(engine) = self.text.as_mut() else {
            return Vec::new();
        };
        engine
            .frames(&self.doc.text, tl, canvas)
            .unwrap_or_else(|e| {
                tracing::warn!("sc_player: text: {e}");
                Vec::new()
            })
    }

    pub fn background(&mut self, canvas: SizePx) -> Result<Arc<RgbaImage>> {
        self.backgrounds
            .get(
                &self.doc.output.canvas.background,
                canvas,
                &self.project_dir,
            )
            .map_err(|e| Error::Other(format!("sc_player: background: {e}")))
    }
}

/// Where a click at global point `at` lands on the canvas: through the section's place on
/// screen (`on`), its content in the frame, and the crop (`src`) drawn into `dst`. `None` when it
/// falls outside the picture.
fn click_on_canvas(
    at: (f64, f64),
    on: sc_geometry::RectPt,
    content: RectPx,
    src: RectPx,
    dst: RectPx,
) -> Option<(f32, f32)> {
    let fx = (at.0 - on.x) / on.w;
    let fy = (at.1 - on.y) / on.h;
    if !(0.0..=1.0).contains(&fx) || !(0.0..=1.0).contains(&fy) || src.w == 0 || src.h == 0 {
        return None;
    }
    let px = f64::from(content.x) + fx * f64::from(content.w);
    let py = f64::from(content.y) + fy * f64::from(content.h);
    let x = f64::from(dst.x) + (px - f64::from(src.x)) * f64::from(dst.w) / f64::from(src.w);
    let y = f64::from(dst.y) + (py - f64::from(src.y)) * f64::from(dst.h) / f64::from(src.h);
    let inside = x >= f64::from(dst.x)
        && y >= f64::from(dst.y)
        && x <= f64::from(dst.x) + f64::from(dst.w)
        && y <= f64::from(dst.y) + f64::from(dst.h);
    inside.then_some((x as f32, y as f32))
}

const RING_PX: u32 = 128;
const RING_KEY: u64 = 0x5c_c11c_4116;

/// A soft yellow ring with a faint fill, drawn at each click.
fn ring_image() -> &'static RgbaImage {
    static RING: std::sync::OnceLock<RgbaImage> = std::sync::OnceLock::new();
    RING.get_or_init(|| {
        let n = RING_PX as usize;
        let mut data = vec![0u8; n * n * 4];
        let c = n as f32 / 2.0;
        let (outer, width) = (c - 2.0, 9.0f32);
        for y in 0..n {
            for x in 0..n {
                let d = ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt();
                let edge = (outer - d).clamp(0.0, 1.0);
                let ring = edge * (d - (outer - width) + 1.0).clamp(0.0, 1.0);
                let fill = edge * 0.22;
                let a = (ring * 0.95 + fill * (1.0 - ring)).clamp(0.0, 1.0);
                let i = (y * n + x) * 4;
                data[i] = (255.0 * a) as u8;
                data[i + 1] = (214.0 * a) as u8;
                data[i + 2] = (10.0 * a) as u8;
                data[i + 3] = (255.0 * a) as u8;
            }
        }
        RgbaImage {
            size: SizePx {
                w: RING_PX,
                h: RING_PX,
            },
            stride: n * 4,
            data,
        }
    })
}

/// Button presses from `cursor.jsonl`, when the recording logged them for our own highlights.
fn load_clicks(project: &ProjectV1, dir: &Path) -> Vec<Click> {
    let own = project.recording.screen.click_highlight
        && project.recording.screen.extra.get("own_clicks") == Some(&serde_json::Value::Bool(true));
    if !own {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(dir.join("cursor.jsonl")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut held = 0u64;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let (Some(t), Some(x), Some(y), Some(b)) = (
            v["t"].as_i64(),
            v["x"].as_f64(),
            v["y"].as_f64(),
            v["b"].as_u64(),
        ) else {
            continue;
        };
        if b & !held != 0 {
            out.push(Click { t_rec_ns: t, x, y });
        }
        held = b;
    }
    out
}

/// The framing of the section `segment`: its own, or the canvas default with a centred crop.
pub fn framing(doc: &Doc, segment: Option<&str>) -> (Fit, PointF) {
    let centre = PointF { x: 0.5, y: 0.5 };
    segment
        .and_then(|id| doc.output.segment_framing.as_ref()?.get(id))
        .map_or((doc.output.canvas.fit, centre), |f| (f.fit, f.focus_norm))
}

/// Source crop and canvas destination of a main picture: all of it fitted inside, or cropped
/// around `focus` (fractions of the picture) to fill the whole canvas.
pub fn place(fit: Fit, focus: PointF, canvas: SizePx, content: RectPx) -> (RectPx, RectPx) {
    let size = SizePx {
        w: content.w,
        h: content.h,
    };
    match fit {
        Fit::Contain => (content, contain_fit(canvas, size)),
        Fit::Cover => {
            let c = cover_src_rect(size, canvas, focus);
            let src = RectPx {
                x: content.x + c.x.round() as i32,
                y: content.y + c.y.round() as i32,
                w: (c.w.round() as u32).clamp(1, content.w),
                h: (c.h.round() as u32).clamp(1, content.h),
            };
            let dst = RectPx {
                x: 0,
                y: 0,
                w: canvas.w,
                h: canvas.h,
            };
            (src, dst)
        }
    }
}

/// Blur boxes given as fractions of `whole`, as fractions of `crop` instead (the renderer takes
/// them as fractions of the drawn source).
fn recrop(redact: &[Redact], whole: RectPx, crop: RectPx) -> Vec<Redact> {
    if crop == whole {
        return redact.to_vec();
    }
    let (ox, oy) = (
        f64::from(crop.x - whole.x) / f64::from(whole.w),
        f64::from(crop.y - whole.y) / f64::from(whole.h),
    );
    let (sx, sy) = (
        f64::from(whole.w) / f64::from(crop.w),
        f64::from(whole.h) / f64::from(crop.h),
    );
    redact
        .iter()
        .map(|r| Redact {
            rect: [
                (r.rect[0] - ox) * sx,
                (r.rect[1] - oy) * sy,
                r.rect[2] * sx,
                r.rect[3] * sy,
            ],
            ..*r
        })
        .collect()
}

/// The camera filling the picture where the screen track has a gap.
fn full_frame(frame: FrameBuffer) -> SegmentFrame {
    let size = frame.0.size();
    SegmentFrame {
        segment_id: "camera".into(),
        frame_px: size,
        content_px: RectPx {
            x: 0,
            y: 0,
            w: size.w,
            h: size.h,
        },
        frame,
        held: false,
    }
}

/// The blur boxes over recording time `rec`, in the order they were added.
pub fn redactions_at(doc: &Doc, rec: i64) -> Vec<Redact> {
    doc.redactions
        .iter()
        .filter(|r| r.start_rec_ns <= rec && rec < r.end_rec_ns)
        .map(|r| Redact {
            rect: [r.rect_norm.x, r.rect_norm.y, r.rect_norm.w, r.rect_norm.h],
            black: r.style == sc_core::project::RedactStyle::Black,
        })
        .collect()
}

impl Composed {
    /// The text, then any click rings on top.
    pub fn text_layers(&self) -> Vec<TextLayer<'_>> {
        let mut layers: Vec<TextLayer<'_>> = self
            .text
            .iter()
            .map(|t| TextLayer {
                image: &t.image,
                key: t.key,
                to_canvas: t.to_canvas,
                opacity: t.opacity,
            })
            .collect();
        if !self.clicks.is_empty() {
            let canvas = self.canvas_short;
            let ring = ring_image();
            for c in &self.clicks {
                let d = canvas as f32 * 0.05 * (0.6 + 0.4 * c.age.min(1.0));
                let s = d / RING_PX as f32;
                layers.push(TextLayer {
                    image: ring,
                    key: RING_KEY,
                    to_canvas: [s, 0.0, 0.0, s, c.x - d / 2.0, c.y - d / 2.0],
                    opacity: (1.0 - c.age).clamp(0.0, 1.0),
                });
            }
        }
        layers
    }

    pub fn job<'a>(
        &'a self,
        canvas: SizePx,
        background: &'a RgbaImage,
        background_key: Option<u64>,
        text: &'a [TextLayer<'a>],
    ) -> FrameJob<'a> {
        let screen = match self.screen.as_ref() {
            Some(f) if f.content_px.w > 0 && f.content_px.h > 0 => {
                let (src, dst) = place(self.fit, self.focus, canvas, f.content_px);
                Some(ScreenLayer {
                    frame: &f.frame,
                    src,
                    dst,
                    redact: &self.redact,
                })
            }
            _ => None,
        };
        let camera = self.camera.as_ref().map(|(f, p)| {
            let rects = camera_rects(canvas, f.0.size(), p.shape.aspect(), &p.pose);
            sc_render::camera_layer(p, f, rects.dst)
        });
        FrameJob {
            canvas,
            background,
            background_key,
            screen,
            camera,
            text,
        }
    }

    /// Where the whole screen picture lands on `canvas`; with Fill it reaches past the edges.
    pub fn screen_dst(&self, canvas: SizePx) -> Option<RectPx> {
        self.screen
            .as_ref()
            .filter(|f| f.content_px.w > 0 && f.content_px.h > 0)
            .map(|f| {
                let (src, dst) = place(self.fit, self.focus, canvas, f.content_px);
                let k = f64::from(dst.w) / f64::from(src.w.max(1));
                RectPx {
                    x: dst.x - (f64::from(src.x - f.content_px.x) * k).round() as i32,
                    y: dst.y - (f64::from(src.y - f.content_px.y) * k).round() as i32,
                    w: (f64::from(f.content_px.w) * k).round() as u32,
                    h: (f64::from(f.content_px.h) * k).round() as u32,
                }
            })
    }

    pub fn camera_dst(&self, canvas: SizePx) -> Option<RectF> {
        self.camera
            .as_ref()
            .map(|(f, p)| camera_rects(canvas, f.0.size(), p.shape.aspect(), &p.pose).dst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(x: i32, y: i32, w: u32, h: u32) -> RectPx {
        RectPx { x, y, w, h }
    }

    #[test]
    fn fill_crops_the_middle_to_the_frame_shape() {
        let canvas = SizePx { w: 1080, h: 1920 };
        let mid = PointF { x: 0.5, y: 0.5 };
        let (src, dst) = place(Fit::Cover, mid, canvas, rect(0, 0, 1920, 1080));
        assert_eq!(dst, rect(0, 0, 1080, 1920));
        assert_eq!(src, rect(656, 0, 608, 1080));
        let (left, _) = place(
            Fit::Cover,
            PointF { x: 0.0, y: 0.5 },
            canvas,
            rect(0, 0, 1920, 1080),
        );
        assert_eq!(left.x, 0);
        let (src, dst) = place(Fit::Contain, mid, canvas, rect(0, 0, 1920, 1080));
        assert_eq!(src, rect(0, 0, 1920, 1080));
        assert_eq!(dst.w, 1080);
    }

    #[test]
    fn a_click_lands_where_it_was_in_the_picture() {
        let on = sc_geometry::RectPt {
            x: 100.0,
            y: 50.0,
            w: 400.0,
            h: 300.0,
        };
        let content = rect(0, 0, 800, 600);
        let mid = PointF { x: 0.5, y: 0.5 };
        let (src, dst) = place(Fit::Contain, mid, SizePx { w: 1600, h: 900 }, content);
        let (x, y) = click_on_canvas((300.0, 200.0), on, content, src, dst).expect("inside");
        assert!(
            (x - 800.0).abs() < 1.0 && (y - 450.0).abs() < 1.0,
            "the centre: {x},{y}"
        );
        assert!(click_on_canvas((90.0, 200.0), on, content, src, dst).is_none());
        let (src, dst) = place(Fit::Cover, mid, SizePx { w: 900, h: 1600 }, content);
        assert!(
            click_on_canvas((110.0, 200.0), on, content, src, dst).is_none(),
            "cropped away by Fill"
        );
    }

    #[test]
    fn presses_come_from_button_down_edges() {
        let dir = std::env::temp_dir().join(format!("sc-clicks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let lines = [
            r#"{"t":0,"x":1,"y":1,"b":0}"#,
            r#"{"t":10,"x":2,"y":2,"b":1}"#,
            r#"{"t":20,"x":3,"y":3,"b":1}"#,
            r#"{"t":30,"x":4,"y":4,"b":0}"#,
            "not json",
            r#"{"t":40,"x":5,"y":5,"b":2}"#,
        ];
        std::fs::write(dir.join("cursor.jsonl"), lines.join("\n")).expect("write");
        let mut p = sc_core::project::ProjectV1::new_recording(
            "p".into(),
            "t".into(),
            "now".into(),
            "0".into(),
            sc_core::ClockKind::MachHost,
        );
        assert!(load_clicks(&p, &dir).is_empty(), "not asked for");
        p.recording.screen.click_highlight = true;
        p.recording
            .screen
            .extra
            .insert("own_clicks".into(), serde_json::Value::Bool(true));
        let c = load_clicks(&p, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            c.iter().map(|c| (c.t_rec_ns, c.x)).collect::<Vec<_>>(),
            vec![(10, 2.0), (40, 5.0)]
        );
    }

    #[test]
    fn blur_boxes_follow_the_crop() {
        let whole = rect(0, 0, 1000, 500);
        let crop = rect(250, 0, 500, 500);
        let r = recrop(
            &[Redact {
                rect: [0.5, 0.2, 0.1, 0.4],
                black: true,
            }],
            whole,
            crop,
        );
        let [x, y, w, h] = r[0].rect;
        assert!((x - 0.5).abs() < 1e-9 && (y - 0.2).abs() < 1e-9);
        assert!((w - 0.2).abs() < 1e-9 && (h - 0.4).abs() < 1e-9);
        assert!(r[0].black);
    }
}
