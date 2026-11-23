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
}

pub struct Composed {
    pub tl_ns: i64,
    pub rec_ns: i64,
    /// Recording time of the camera picture; `None` in a gap on an unlinked camera track.
    pub camera_rec_ns: Option<i64>,
    pub screen: Option<SegmentFrame>,
    pub camera: Option<(FrameBuffer, CameraPlacement)>,
    pub text: Vec<TextFrame>,
    pub redact: Vec<Redact>,
    pub fit: Fit,
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
        })
    }

    pub fn set_doc(&mut self, doc: Doc) {
        self.starts = starts(&doc.edl);
        self.camera_starts = starts(doc.camera_clips());
        self.doc = doc;
    }

    pub fn doc(&self) -> &Doc {
        &self.doc
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
        let fit = self.doc.output.canvas.fit;
        let redact = match screen.as_ref() {
            Some(f) if screen_shown && f.content_px.w > 0 && f.content_px.h > 0 => recrop(
                &redactions_at(&self.doc, rec),
                f.content_px,
                place(fit, canvas, f.content_px).0,
            ),
            _ => Vec::new(),
        };
        Ok(Composed {
            tl_ns: tl,
            rec_ns: rec,
            camera_rec_ns: camera_rec,
            screen,
            camera,
            text,
            redact,
            fit,
        })
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

    pub fn text_engine(&mut self) -> Option<&mut TextEngine> {
        self.text.as_mut()
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

/// Source crop and canvas destination of a main picture: all of it fitted inside, or the
/// centre cropped to fill the whole canvas.
pub fn place(fit: Fit, canvas: SizePx, content: RectPx) -> (RectPx, RectPx) {
    let size = SizePx {
        w: content.w,
        h: content.h,
    };
    match fit {
        Fit::Contain => (content, contain_fit(canvas, size)),
        Fit::Cover => {
            let c = cover_src_rect(size, canvas, PointF { x: 0.5, y: 0.5 });
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
    pub fn text_layers(&self) -> Vec<TextLayer<'_>> {
        self.text
            .iter()
            .map(|t| TextLayer {
                image: &t.image,
                key: t.key,
                to_canvas: t.to_canvas,
                opacity: t.opacity,
            })
            .collect()
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
                let (src, dst) = place(self.fit, canvas, f.content_px);
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
                let (src, dst) = place(self.fit, canvas, f.content_px);
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
        let (src, dst) = place(Fit::Cover, canvas, rect(0, 0, 1920, 1080));
        assert_eq!(dst, rect(0, 0, 1080, 1920));
        assert_eq!(src, rect(656, 0, 608, 1080));
        let (src, dst) = place(Fit::Contain, canvas, rect(0, 0, 1920, 1080));
        assert_eq!(src, rect(0, 0, 1920, 1080));
        assert_eq!(dst.w, 1080);
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
