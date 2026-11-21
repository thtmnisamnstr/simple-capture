//! The edited timeline's pictures: the screen segment frame, the camera frame and its placement
//! at any timeline time, ready for the compositor. The editor preview, thumbnails and every
//! export use this, so they show the same thing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_core::camera::{CameraPlacement, camera_at};
use sc_core::edit::{Doc, locate_in, starts};
use sc_core::image::RgbaImage;
use sc_core::project::ProjectV1;
use sc_decode::{SegmentFrame, SegmentFrameSource, TrackFrameSource};
use sc_geometry::{Ratio, RectF, RectPx, SizePx, camera_rects, contain_fit};
use sc_platform::{Decoder, Error, FrameBuffer, Result};
use sc_render::{FrameJob, Redact, ScreenLayer, TextLayer};
use sc_text::{BackgroundCache, TextEngine, TextFrame};

pub struct TimelineVideo {
    source: SegmentFrameSource,
    camera: Option<(TrackFrameSource, String)>,
    doc: Doc,
    starts: Vec<i64>,
    backgrounds: BackgroundCache,
    project_dir: PathBuf,
    segment_sizes: Vec<SizePx>,
    text: Option<TextEngine>,
    text_failed: bool,
}

pub struct Composed {
    pub tl_ns: i64,
    pub rec_ns: i64,
    pub screen: Option<SegmentFrame>,
    pub camera: Option<(FrameBuffer, CameraPlacement)>,
    pub text: Vec<TextFrame>,
    pub redact: Vec<Redact>,
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
        let screen = self.source.frame_at(rec)?;
        let mut camera = None;
        if let Some((src, overlay)) = self.camera.as_mut()
            && let Some(o) = self
                .doc
                .camera_layout
                .overlays
                .iter()
                .find(|o| &o.id == overlay)
            && let Some(p) = camera_at(&o.keyframes, rec)
            && let Some(f) = src.frame_at(rec)?
        {
            camera = Some((f, p));
        }
        let text = self.text_at(tl_ns.clamp(0, end), canvas);
        let redact = redactions_at(&self.doc, rec);
        Ok(Composed {
            tl_ns: tl,
            rec_ns: rec,
            screen,
            camera,
            text,
            redact,
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
        let screen = self.screen_dst(canvas).and_then(|dst| {
            self.screen.as_ref().map(|f| ScreenLayer {
                frame: &f.frame,
                src: f.content_px,
                dst,
                redact: &self.redact,
            })
        });
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

    pub fn screen_dst(&self, canvas: SizePx) -> Option<RectPx> {
        self.screen
            .as_ref()
            .filter(|f| f.content_px.w > 0 && f.content_px.h > 0)
            .map(|f| {
                contain_fit(
                    canvas,
                    SizePx {
                        w: f.content_px.w,
                        h: f.content_px.h,
                    },
                )
            })
    }

    pub fn camera_dst(&self, canvas: SizePx) -> Option<RectF> {
        self.camera
            .as_ref()
            .map(|(f, p)| camera_rects(canvas, f.0.size(), p.shape.aspect(), &p.pose).dst)
    }
}
