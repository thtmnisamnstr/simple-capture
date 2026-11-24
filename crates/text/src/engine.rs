//! One renderer thread's text: font library, laid-out items cached until they change, and the
//! layer for each visible item at a timeline time.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use sc_core::Result;
use sc_core::image::RgbaImage;
use sc_core::project::TextItem;
use sc_geometry::SizePx;

use crate::anim::evaluate;
use crate::fonts::FontLibrary;
use crate::item::{PreparedText, Reveal, prepare};

/// One text item placed on the canvas.
#[derive(Debug, Clone)]
pub struct TextFrame {
    pub id: String,
    pub image: Arc<RgbaImage>,
    /// Identifies `image`'s pixels, so a renderer can keep its upload.
    pub key: u64,
    /// Image pixel to canvas pixel: `x' = m0·x + m2·y + m4`, `y' = m1·x + m3·y + m5`.
    pub to_canvas: [f32; 6],
    pub opacity: f32,
    /// The text block's corners on the canvas (for selection handles), clockwise from top-left.
    pub quad: [(f32, f32); 4],
    pub unresolved: usize,
}

pub struct TextEngine {
    fonts: FontLibrary,
    prepared: HashMap<u64, Arc<PreparedText>>,
    composed: HashMap<(u64, Reveal), Arc<RgbaImage>>,
}

const KEEP: usize = 64;
/// Composed bitmaps kept: a typewriter reveal makes one per character, each the size of the text.
const KEEP_COMPOSED: usize = 32;

impl TextEngine {
    pub fn new() -> Result<TextEngine> {
        Ok(TextEngine {
            fonts: FontLibrary::new()?,
            prepared: HashMap::new(),
            composed: HashMap::new(),
        })
    }

    fn key(item: &TextItem, canvas: SizePx) -> u64 {
        let mut h = DefaultHasher::new();
        item.content.hash(&mut h);
        serde_json::to_string(&item.style)
            .unwrap_or_default()
            .hash(&mut h);
        canvas.w.hash(&mut h);
        canvas.h.hash(&mut h);
        item.transform.scale.to_bits().hash(&mut h);
        h.finish()
    }

    pub fn prepare(&mut self, item: &TextItem, canvas: SizePx) -> Result<(u64, Arc<PreparedText>)> {
        let key = Self::key(item, canvas);
        if let Some(p) = self.prepared.get(&key) {
            return Ok((key, p.clone()));
        }
        if self.prepared.len() >= KEEP {
            self.prepared.clear();
            self.composed.clear();
        }
        let p = Arc::new(prepare(
            &mut self.fonts,
            &item.content,
            &item.style,
            canvas,
            item.transform.scale.clamp(0.05, 20.0) as f32,
        )?);
        self.prepared.insert(key, p.clone());
        Ok((key, p))
    }

    /// Every item visible at `tl_ns`, bottom layer first.
    pub fn frames(
        &mut self,
        items: &[TextItem],
        tl_ns: i64,
        canvas: SizePx,
    ) -> Result<Vec<TextFrame>> {
        let mut visible: Vec<&TextItem> = items
            .iter()
            .filter(|t| t.start_tl_ns <= tl_ns && tl_ns < t.end_tl_ns)
            .collect();
        visible.sort_by_key(|t| t.layer);
        let mut out = Vec::with_capacity(visible.len());
        for item in visible {
            if let Some(f) = self.frame(item, tl_ns, canvas)? {
                out.push(f);
            }
        }
        Ok(out)
    }

    pub fn frame(
        &mut self,
        item: &TextItem,
        tl_ns: i64,
        canvas: SizePx,
    ) -> Result<Option<TextFrame>> {
        let (key, p) = self.prepare(item, canvas)?;
        let len = (item.end_tl_ns - item.start_tl_ns).max(1);
        let a = evaluate(
            &item.anim,
            tl_ns - item.start_tl_ns,
            len,
            p.cluster_count(),
            p.word_count(),
        );
        let opacity = a.opacity * item.style.opacity.clamp(0.0, 1.0) as f32;
        if opacity <= 0.0 || a.scale <= 0.001 {
            return Ok(None);
        }
        let reveal = a.reveal;
        let image = match self.composed.get(&(key, reveal)) {
            Some(i) => i.clone(),
            None => {
                let i = Arc::new(p.compose(reveal));
                if self.composed.len() >= KEEP_COMPOSED {
                    self.composed.clear();
                }
                self.composed.insert((key, reveal), i.clone());
                i
            }
        };
        let mut h = DefaultHasher::new();
        key.hash(&mut h);
        reveal.hash(&mut h);
        let image_key = h.finish();
        let unit = canvas.w.min(canvas.h) as f32 / 1080.0;
        let cx = item.transform.center_norm.x as f32 * canvas.w as f32 + a.offset.0 * unit;
        let cy = item.transform.center_norm.y as f32 * canvas.h as f32 + a.offset.1 * unit;
        let th = (item.transform.rotation_deg as f32 + a.rotation_deg).to_radians();
        let (s, c) = th.sin_cos();
        let (ma, mb) = (a.scale * c, a.scale * s);
        let (ax, ay) = p.anchor;
        let to_canvas = [
            ma,
            mb,
            -mb,
            ma,
            cx - ma * ax + mb * ay,
            cy - mb * ax - ma * ay,
        ];
        let half = (p.block.0 * p.scale / 2.0, p.block.1 * p.scale / 2.0);
        let corner = |dx: f32, dy: f32| {
            let (x, y) = (ax + dx, ay + dy);
            (
                to_canvas[0] * x + to_canvas[2] * y + to_canvas[4],
                to_canvas[1] * x + to_canvas[3] * y + to_canvas[5],
            )
        };
        Ok(Some(TextFrame {
            id: item.id.clone(),
            image,
            key: image_key,
            to_canvas,
            opacity,
            quad: [
                corner(-half.0, -half.1),
                corner(half.0, -half.1),
                corner(half.0, half.1),
                corner(-half.0, half.1),
            ],
            unresolved: p.unresolved,
        }))
    }
}
