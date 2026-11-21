//! The editable part of a project, edit operations that return a new value, and an unlimited
//! undo/redo history (ARCHITECTURE §7). Clips hold recording-time ranges; text items and the
//! playhead live on the edited timeline.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::project::{
    AudioMixEntry, CameraKey, CameraLayout, Canvas, Clip, Edl, Extra, Output, ProjectV1, Speed,
    TextItem,
};
use crate::{Error, Result};

/// Shortest clip an edit may leave behind.
pub const MIN_CLIP_NS: i64 = 10_000_000;
/// Shortest text item an edit may leave behind.
pub const MIN_TEXT_NS: i64 = 100_000_000;

pub const SPEEDS: [Speed; 5] = [
    Speed { num: 1, den: 2 },
    Speed { num: 1, den: 1 },
    Speed { num: 3, den: 2 },
    Speed { num: 2, den: 1 },
    Speed { num: 4, den: 1 },
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    pub edl: Edl,
    pub text: Vec<TextItem>,
    pub camera_layout: CameraLayout,
    pub audio_mix: BTreeMap<String, AudioMixEntry>,
    pub output: Output,
}

impl Doc {
    pub fn of(p: &ProjectV1) -> Doc {
        Doc {
            edl: p.edl.clone(),
            text: p.text.clone(),
            camera_layout: p.camera_layout.clone(),
            audio_mix: p.audio_mix.clone(),
            output: p.output.clone(),
        }
    }

    pub fn apply_to(&self, p: &mut ProjectV1) {
        p.edl = self.edl.clone();
        p.text = self.text.clone();
        p.camera_layout = self.camera_layout.clone();
        p.audio_mix = self.audio_mix.clone();
        p.output = self.output.clone();
    }

    pub fn duration(&self) -> i64 {
        duration(&self.edl)
    }
}

impl Speed {
    pub const ONE: Speed = Speed { num: 1, den: 1 };

    pub fn is_one(self) -> bool {
        self.num == self.den
    }

    pub fn factor(self) -> f64 {
        f64::from(self.num) / f64::from(self.den.max(1))
    }
}

impl Clip {
    pub fn src_len(&self) -> i64 {
        self.src_out_rec_ns - self.src_in_rec_ns
    }

    /// Length on the edited timeline: the source length divided by the speed.
    pub fn tl_len(&self) -> i64 {
        tl_of(self.src_len(), self.speed)
    }

    /// Recording time `offset_tl` into the clip.
    pub fn rec_at(&self, offset_tl: i64) -> i64 {
        let off =
            i128::from(offset_tl) * i128::from(self.speed.num) / i128::from(self.speed.den.max(1));
        (i128::from(self.src_in_rec_ns) + off).min(i128::from(self.src_out_rec_ns)) as i64
    }

    fn fresh(&self) -> Clip {
        Clip {
            id: crate::ids::new_id(),
            ..self.clone()
        }
    }
}

fn tl_of(src_len: i64, speed: Speed) -> i64 {
    (i128::from(src_len) * i128::from(speed.den) / i128::from(speed.num.max(1))) as i64
}

fn src_of(tl_len: i64, speed: Speed) -> i64 {
    (i128::from(tl_len) * i128::from(speed.num) / i128::from(speed.den.max(1))) as i64
}

/// Timeline start of each clip, plus the total as the last entry.
pub fn starts(edl: &Edl) -> Vec<i64> {
    let mut out = Vec::with_capacity(edl.clips.len() + 1);
    let mut t = 0;
    out.push(0);
    for c in &edl.clips {
        t += c.tl_len();
        out.push(t);
    }
    out
}

/// The whole recording as clips that leave out the paused spans (`(start, end)` in recording
/// time); stretches shorter than [`MIN_CLIP_NS`] are dropped.
pub fn edl_between_pauses(duration_ns: i64, pauses_rec: &[(i64, i64)]) -> Edl {
    let mut spans: Vec<(i64, i64)> = pauses_rec
        .iter()
        .map(|(a, b)| ((*a).clamp(0, duration_ns), (*b).clamp(0, duration_ns)))
        .filter(|(a, b)| b > a)
        .collect();
    spans.sort_unstable();
    let mut clips = Vec::new();
    let mut at = 0;
    for (a, b) in spans.into_iter().chain([(duration_ns, duration_ns)]) {
        if a - at >= MIN_CLIP_NS {
            clips.push(Clip {
                id: format!("c{}", clips.len()),
                src_in_rec_ns: at,
                src_out_rec_ns: a,
                speed: Speed::ONE,
                muted: false,
                extra: Extra::new(),
            });
        }
        at = at.max(b);
    }
    Edl {
        clips,
        extra: Extra::new(),
    }
}

pub fn duration(edl: &Edl) -> i64 {
    edl.clips.iter().map(Clip::tl_len).sum()
}

/// Where timeline time `tl` falls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Located {
    pub index: usize,
    pub clip_start_tl: i64,
    pub clip_end_tl: i64,
    pub rec_ns: i64,
}

/// The clip under `tl` (`0 ≤ tl < duration`), found by binary search over the clip starts.
pub fn locate(edl: &Edl, tl: i64) -> Option<Located> {
    locate_in(edl, &starts(edl), tl)
}

pub fn locate_in(edl: &Edl, starts: &[i64], tl: i64) -> Option<Located> {
    if tl < 0 || edl.clips.is_empty() || tl >= *starts.last()? {
        return None;
    }
    let index = starts.partition_point(|s| *s <= tl).saturating_sub(1);
    let clip = edl.clips.get(index)?;
    let start = starts[index];
    Some(Located {
        index,
        clip_start_tl: start,
        clip_end_tl: starts[index + 1],
        rec_ns: clip.rec_at(tl - start),
    })
}

/// Clips and text lifted out of the timeline by copy or cut. Text times are relative to the
/// start of the copied piece.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Clipboard {
    pub clips: Vec<Clip>,
    pub text: Vec<TextItem>,
}

impl Clipboard {
    pub fn len(&self) -> i64 {
        self.clips.iter().map(Clip::tl_len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Start,
    End,
}

/// One user edit, as sent by the editor UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum EditOp {
    Split {
        at_tl_ns: i64,
    },
    Trim {
        clip_id: String,
        edge: Edge,
        rec_ns: i64,
    },
    DeleteRange {
        a_tl_ns: i64,
        b_tl_ns: i64,
    },
    DeleteClips {
        ids: Vec<String>,
    },
    Paste {
        at_tl_ns: i64,
        clipboard: Clipboard,
    },
    Duplicate {
        ids: Vec<String>,
    },
    MoveClip {
        id: String,
        to_index: usize,
    },
    SetSpeed {
        ids: Vec<String>,
        speed: Speed,
    },
    SetMuted {
        ids: Vec<String>,
        muted: bool,
    },
    SetCameraKey {
        overlay_id: String,
        key: CameraKey,
    },
    DeleteCameraKey {
        overlay_id: String,
        t_rec_ns: i64,
    },
    /// One fixed placement, `key` at time 0, for the whole recording.
    SetCameraStatic {
        overlay_id: String,
        key: CameraKey,
    },
    SetTrackMix {
        track_id: String,
        gain_db: f64,
        muted: bool,
    },
    SetCanvas {
        canvas: Canvas,
    },
    UpsertText {
        item: TextItem,
    },
    DeleteText {
        ids: Vec<String>,
    },
}

/// Fixed facts the edits are checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub rec_len_ns: i64,
}

pub fn apply(doc: &Doc, op: &EditOp, lim: Limits) -> Result<Doc> {
    let mut d = doc.clone();
    match op {
        EditOp::Split { at_tl_ns } => {
            split_at(&mut d.edl, *at_tl_ns);
        }
        EditOp::Trim {
            clip_id,
            edge,
            rec_ns,
        } => trim(&mut d, clip_id, *edge, *rec_ns, lim)?,
        EditOp::DeleteRange { a_tl_ns, b_tl_ns } => {
            delete_range(&mut d, (*a_tl_ns).min(*b_tl_ns), (*a_tl_ns).max(*b_tl_ns));
        }
        EditOp::DeleteClips { ids } => delete_clips(&mut d, ids)?,
        EditOp::Paste {
            at_tl_ns,
            clipboard,
        } => paste(&mut d, *at_tl_ns, clipboard),
        EditOp::Duplicate { ids } => {
            let cb = copy_clips(&d, ids)?;
            let at = clip_ranges(&d.edl, ids)?
                .iter()
                .map(|r| r.1)
                .max()
                .unwrap_or(0);
            paste(&mut d, at, &cb);
        }
        EditOp::MoveClip { id, to_index } => {
            let from = index_of(&d.edl, id)?;
            let c = d.edl.clips.remove(from);
            let to = (*to_index).min(d.edl.clips.len());
            d.edl.clips.insert(to, c);
        }
        EditOp::SetSpeed { ids, speed } => set_speed(&mut d, ids, *speed)?,
        EditOp::SetMuted { ids, muted } => {
            for id in ids {
                let i = index_of(&d.edl, id)?;
                d.edl.clips[i].muted = *muted;
            }
        }
        EditOp::SetCameraKey { overlay_id, key } => {
            let keys = keys_mut(&mut d, overlay_id)?;
            match keys.iter().position(|k| k.t_rec_ns == key.t_rec_ns) {
                Some(i) => keys[i] = key.clone(),
                None => {
                    let at = keys.partition_point(|k| k.t_rec_ns < key.t_rec_ns);
                    keys.insert(at, key.clone());
                }
            }
        }
        EditOp::DeleteCameraKey {
            overlay_id,
            t_rec_ns,
        } => {
            let keys = keys_mut(&mut d, overlay_id)?;
            if keys.len() > 1 {
                keys.retain(|k| k.t_rec_ns != *t_rec_ns);
            }
        }
        EditOp::SetCameraStatic { overlay_id, key } => {
            let keys = keys_mut(&mut d, overlay_id)?;
            keys.clear();
            keys.push(CameraKey {
                t_rec_ns: 0,
                ..key.clone()
            });
        }
        EditOp::SetTrackMix {
            track_id,
            gain_db,
            muted,
        } => {
            d.audio_mix.insert(
                track_id.clone(),
                AudioMixEntry {
                    gain_db: gain_db.clamp(-60.0, 12.0),
                    muted: *muted,
                    extra: Extra::new(),
                },
            );
        }
        EditOp::SetCanvas { canvas } => d.output.canvas = canvas.clone(),
        EditOp::UpsertText { item } => {
            if item.end_tl_ns - item.start_tl_ns < MIN_TEXT_NS {
                return Err(Error::Invalid(
                    "a text item must last at least 0.1 s".into(),
                ));
            }
            match d.text.iter().position(|t| t.id == item.id) {
                Some(i) => d.text[i] = item.clone(),
                None => d.text.push(item.clone()),
            }
        }
        EditOp::DeleteText { ids } => d.text.retain(|t| !ids.contains(&t.id)),
    }
    if d.edl.clips.is_empty() && !doc.edl.clips.is_empty() {
        return Err(Error::Invalid(
            "the timeline needs at least one clip".into(),
        ));
    }
    Ok(d)
}

fn keys_mut<'a>(d: &'a mut Doc, overlay_id: &str) -> Result<&'a mut Vec<CameraKey>> {
    d.camera_layout
        .overlays
        .iter_mut()
        .find(|o| o.id == overlay_id)
        .map(|o| &mut o.keyframes)
        .ok_or_else(|| Error::Invalid(format!("no camera overlay {overlay_id}")))
}

fn index_of(edl: &Edl, id: &str) -> Result<usize> {
    edl.clips
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| Error::Invalid(format!("no clip {id}")))
}

/// Timeline ranges of the given clips, in timeline order.
fn clip_ranges(edl: &Edl, ids: &[String]) -> Result<Vec<(i64, i64)>> {
    let st = starts(edl);
    let mut idx = ids
        .iter()
        .map(|id| index_of(edl, id))
        .collect::<Result<Vec<_>>>()?;
    idx.sort_unstable();
    idx.dedup();
    Ok(idx.into_iter().map(|i| (st[i], st[i + 1])).collect())
}

/// Splits the clip under `tl` and returns the index of the clip that starts at `tl` (the
/// clip count when `tl` is at or past the end). Cuts closer than [`MIN_CLIP_NS`] to an
/// existing edge snap to that edge.
pub fn split_at(edl: &mut Edl, tl: i64) -> usize {
    let st = starts(edl);
    let total = *st.last().unwrap_or(&0);
    if tl <= 0 {
        return 0;
    }
    if tl >= total {
        return edl.clips.len();
    }
    let i = st.partition_point(|s| *s <= tl).saturating_sub(1);
    let (s, e) = (st[i], st[i + 1]);
    if tl - s < MIN_CLIP_NS {
        return i;
    }
    if e - tl < MIN_CLIP_NS {
        return i + 1;
    }
    let c = edl.clips[i].clone();
    let cut = c.rec_at(tl - s);
    let mut right = c.fresh();
    right.src_in_rec_ns = cut;
    edl.clips[i].src_out_rec_ns = cut;
    edl.clips.insert(i + 1, right);
    i + 1
}

fn delete_range(d: &mut Doc, a: i64, b: i64) {
    let total = d.duration();
    let (a, b) = (a.clamp(0, total), b.clamp(0, total));
    if b - a <= 0 {
        return;
    }
    let i = split_at(&mut d.edl, a);
    let j = split_at(&mut d.edl, b);
    let removed: i64 = d.edl.clips[i..j].iter().map(Clip::tl_len).sum();
    d.edl.clips.drain(i..j);
    let new_a = starts(&d.edl)[i];
    ripple_remove(&mut d.text, new_a, new_a + removed);
}

fn delete_clips(d: &mut Doc, ids: &[String]) -> Result<()> {
    for (a, b) in clip_ranges(&d.edl, ids)?.into_iter().rev() {
        delete_range(d, a, b);
    }
    Ok(())
}

/// The clips and the text covering `[a, b)`, cut at both ends.
pub fn copy_range(d: &Doc, a: i64, b: i64) -> Clipboard {
    let mut scratch = d.clone();
    let i = split_at(&mut scratch.edl, a);
    let j = split_at(&mut scratch.edl, b);
    let st = starts(&scratch.edl);
    let (a, b) = (st[i], st[j]);
    Clipboard {
        clips: scratch.edl.clips[i..j].iter().map(Clip::fresh).collect(),
        text: text_within(&d.text, a, b, a),
    }
}

/// The given clips in timeline order, with the text that lies inside each of them.
pub fn copy_clips(d: &Doc, ids: &[String]) -> Result<Clipboard> {
    let mut cb = Clipboard::default();
    for (a, b) in clip_ranges(&d.edl, ids)? {
        let off = cb.len();
        let piece = copy_range(d, a, b);
        cb.text.extend(piece.text.into_iter().map(|mut t| {
            t.start_tl_ns += off;
            t.end_tl_ns += off;
            t
        }));
        cb.clips.extend(piece.clips);
    }
    Ok(cb)
}

fn text_within(text: &[TextItem], a: i64, b: i64, origin: i64) -> Vec<TextItem> {
    text.iter()
        .filter(|t| t.start_tl_ns >= a && t.end_tl_ns <= b)
        .map(|t| TextItem {
            id: crate::ids::new_id(),
            start_tl_ns: t.start_tl_ns - origin,
            end_tl_ns: t.end_tl_ns - origin,
            ..t.clone()
        })
        .collect()
}

fn paste(d: &mut Doc, at: i64, cb: &Clipboard) {
    if cb.is_empty() {
        return;
    }
    let at = at.clamp(0, d.duration());
    let i = split_at(&mut d.edl, at);
    let at = starts(&d.edl)[i];
    let len = cb.len();
    for (k, c) in cb.clips.iter().enumerate() {
        d.edl.clips.insert(i + k, c.fresh());
    }
    ripple_insert(&mut d.text, at, len);
    d.text.extend(cb.text.iter().map(|t| TextItem {
        id: crate::ids::new_id(),
        start_tl_ns: t.start_tl_ns + at,
        end_tl_ns: t.end_tl_ns + at,
        ..t.clone()
    }));
}

fn trim(d: &mut Doc, id: &str, edge: Edge, rec_ns: i64, lim: Limits) -> Result<()> {
    let i = index_of(&d.edl, id)?;
    let st = starts(&d.edl);
    let c = d.edl.clips[i].clone();
    match edge {
        Edge::Start => {
            let new_in = rec_ns.clamp(0, c.src_out_rec_ns - MIN_CLIP_NS);
            let delta_tl = tl_of(new_in - c.src_in_rec_ns, c.speed);
            d.edl.clips[i].src_in_rec_ns = new_in;
            if delta_tl > 0 {
                ripple_remove(&mut d.text, st[i], st[i] + delta_tl);
            } else {
                ripple_insert(&mut d.text, st[i], -delta_tl);
            }
        }
        Edge::End => {
            let new_out = rec_ns.clamp(c.src_in_rec_ns + MIN_CLIP_NS, lim.rec_len_ns);
            let new_len = tl_of(new_out - c.src_in_rec_ns, c.speed);
            let end = st[i + 1];
            d.edl.clips[i].src_out_rec_ns = new_out;
            let new_end = st[i] + new_len;
            if new_end < end {
                ripple_remove(&mut d.text, new_end, end);
            } else {
                ripple_insert(&mut d.text, end, new_end - end);
            }
        }
    }
    Ok(())
}

fn set_speed(d: &mut Doc, ids: &[String], speed: Speed) -> Result<()> {
    if speed.num == 0 || speed.den == 0 {
        return Err(Error::Invalid("speed must be positive".into()));
    }
    let old = d.edl.clone();
    for id in ids {
        let i = index_of(&d.edl, id)?;
        let c = &mut d.edl.clips[i];
        c.speed = speed;
        if c.tl_len() < MIN_CLIP_NS {
            c.src_out_rec_ns = c.src_in_rec_ns + src_of(MIN_CLIP_NS, speed);
        }
    }
    retime_text(&mut d.text, &old, &d.edl);
    Ok(())
}

/// Moves text along when clips keep their order and identity but change length.
fn retime_text(text: &mut [TextItem], old: &Edl, new: &Edl) {
    let (so, sn) = (starts(old), starts(new));
    let (to, tn) = (*so.last().unwrap_or(&0), *sn.last().unwrap_or(&0));
    let map = |t: i64| -> i64 {
        if t >= to {
            return t - to + tn;
        }
        let i = so.partition_point(|s| *s <= t).saturating_sub(1);
        let (a, b) = (so[i], so[i + 1]);
        let (na, nb) = (sn[i], sn[i + 1]);
        if b == a {
            return na;
        }
        na + ((i128::from(t - a) * i128::from(nb - na)) / i128::from(b - a)) as i64
    };
    for t in text.iter_mut() {
        let (s, e) = (map(t.start_tl_ns), map(t.end_tl_ns));
        t.start_tl_ns = s;
        t.end_tl_ns = e.max(s + MIN_TEXT_NS);
    }
}

/// Removes `[a, b)` from the text track: later items move back, items inside it go, items
/// across an edge are cut.
pub fn ripple_remove(text: &mut Vec<TextItem>, a: i64, b: i64) {
    let len = b - a;
    if len <= 0 {
        return;
    }
    let f = |t: i64| {
        if t <= a {
            t
        } else if t < b {
            a
        } else {
            t - len
        }
    };
    text.retain_mut(|t| {
        let (s, e) = (f(t.start_tl_ns), f(t.end_tl_ns));
        t.start_tl_ns = s;
        t.end_tl_ns = e;
        e - s >= MIN_TEXT_NS
    });
}

/// Opens a gap of `len` at `at`: items that start at or after it move later.
pub fn ripple_insert(text: &mut [TextItem], at: i64, len: i64) {
    if len <= 0 {
        return;
    }
    for t in text.iter_mut().filter(|t| t.start_tl_ns >= at) {
        t.start_tl_ns += len;
        t.end_tl_ns += len;
    }
}

/// Unlimited undo/redo over whole-document snapshots. Consecutive commits with the same
/// coalesce key (one slider drag, one handle drag) become one undo step.
#[derive(Debug, Clone)]
pub struct History {
    doc: Doc,
    undo: Vec<Doc>,
    redo: Vec<Doc>,
    coalesce: Option<String>,
    revision: u64,
}

impl History {
    pub fn new(doc: Doc) -> History {
        History {
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            coalesce: None,
            revision: 0,
        }
    }

    pub fn doc(&self) -> &Doc {
        &self.doc
    }

    /// Bumped on every change, undo and redo included.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn apply(&mut self, op: &EditOp, lim: Limits, coalesce: Option<&str>) -> Result<bool> {
        let next = apply(&self.doc, op, lim)?;
        Ok(self.commit(next, coalesce))
    }

    /// Makes `next` current. Returns false when nothing changed.
    pub fn commit(&mut self, next: Doc, coalesce: Option<&str>) -> bool {
        if next == self.doc {
            return false;
        }
        let merge = coalesce.is_some() && coalesce == self.coalesce.as_deref();
        let prev = std::mem::replace(&mut self.doc, next);
        if !merge {
            self.undo.push(prev);
        }
        self.redo.clear();
        self.coalesce = coalesce.map(str::to_string);
        self.revision += 1;
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.doc, prev));
        self.coalesce = None;
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.doc, next));
        self.coalesce = None;
        self.revision += 1;
        true
    }

    /// Ends coalescing, so the next commit is its own undo step.
    pub fn seal(&mut self) {
        self.coalesce = None;
    }
}

#[cfg(test)]
mod tests;
