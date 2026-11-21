use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_core::project::ProjectV1;
use sc_geometry::{RectPx, SizePx};
use sc_platform::{DecodedFrame, Decoder, Error, FrameBuffer, Result, VideoReader};

/// Sequential frames across screen segments (ARCHITECTURE §7). Only the active segment keeps a
/// reader open; a request earlier than the previous one restarts from the first segment.
pub struct SegmentFrameSource {
    decoder: Arc<dyn Decoder>,
    segs: Vec<Seg>,
    active: Option<Active>,
    carry: Option<(usize, FrameBuffer)>,
    last_t: Option<i64>,
}

struct Seg {
    id: String,
    start: i64,
    end: Option<i64>,
    frame_px: SizePx,
    content_px: RectPx,
    path: PathBuf,
}

impl Seg {
    fn limit(&self) -> Option<i64> {
        self.end.map(|e| e - self.start)
    }

    fn err(&self, what: &str, e: Error) -> Error {
        Error::Other(format!(
            "sc_decode: {what} segment {} ({}): {e}",
            self.id,
            self.path.display()
        ))
    }
}

struct Active {
    seg: usize,
    reader: Option<Box<dyn VideoReader>>,
    current: Option<FrameBuffer>,
    pending: Option<DecodedFrame>,
}

impl Active {
    fn advance(&mut self, rel: i64, seg: &Seg) -> Result<()> {
        let limit = seg.limit();
        loop {
            let next = match self.pending.take() {
                Some(f) => f,
                None => {
                    let Some(r) = self.reader.as_mut() else {
                        return Ok(());
                    };
                    match r.next_frame().map_err(|e| seg.err("decoding", e))? {
                        Some(f) => f,
                        None => {
                            self.reader = None;
                            return Ok(());
                        }
                    }
                }
            };
            if limit.is_some_and(|l| next.file_ns >= l) {
                self.reader = None;
                return Ok(());
            }
            if next.file_ns > rel {
                self.pending = Some(next);
                return Ok(());
            }
            self.current = Some(next.frame);
        }
    }
}

impl SegmentFrameSource {
    pub fn open(
        project: &ProjectV1,
        dir: &Path,
        decoder: Arc<dyn Decoder>,
    ) -> Result<SegmentFrameSource> {
        let mut segments: Vec<_> = project.recording.screen.segments.iter().collect();
        segments.sort_by_key(|s| s.start_rec_ns);
        let mut segs = Vec::with_capacity(segments.len());
        for (i, s) in segments.iter().enumerate() {
            let track = project
                .recording
                .tracks
                .iter()
                .find(|t| t.id == s.track_id)
                .ok_or_else(|| {
                    Error::NotFound(format!(
                        "sc_decode: track {} of segment {} is not in project.json",
                        s.track_id, s.id
                    ))
                })?;
            let next_start = segments.get(i + 1).map(|n| n.start_rec_ns);
            let end = match (s.end_rec_ns, next_start) {
                (Some(e), Some(n)) => Some(e.min(n)),
                (e, n) => e.or(n),
            };
            segs.push(Seg {
                id: s.id.clone(),
                start: s.start_rec_ns,
                end,
                frame_px: s.frame_px,
                content_px: s.content_px,
                path: dir.join(&track.file),
            });
        }
        Ok(SegmentFrameSource {
            decoder,
            segs,
            active: None,
            carry: None,
            last_t: None,
        })
    }

    /// `None` before any frame exists. `held` marks a frame that is not the active segment's own
    /// at `t` (a gap, before its first frame, a dropped segment); the id and sizes are those of
    /// the segment the frame came from.
    pub fn frame_at(&mut self, t_rec_ns: i64) -> Result<Option<SegmentFrame>> {
        if self.last_t.is_some_and(|l| t_rec_ns < l) {
            self.active = None;
            self.carry = None;
        }
        self.last_t = Some(t_rec_ns);
        let k = self.segs.partition_point(|s| s.start <= t_rec_ns);
        let Some(k) = k.checked_sub(1) else {
            return Ok(None);
        };
        if self.active.as_ref().is_none_or(|a| a.seg != k) {
            self.switch_to(k)?;
        }
        let seg = &self.segs[k];
        let Some(active) = self.active.as_mut() else {
            return Ok(None);
        };
        active.advance(t_rec_ns - seg.start, seg)?;
        let past_end = seg.end.is_some_and(|e| t_rec_ns >= e);
        if let Some(f) = active.current.clone() {
            return Ok(Some(self.shown(k, f, past_end)));
        }
        Ok(self.carry.clone().map(|(j, f)| self.shown(j, f, true)))
    }

    fn shown(&self, seg: usize, frame: FrameBuffer, held: bool) -> SegmentFrame {
        let s = &self.segs[seg];
        SegmentFrame {
            segment_id: s.id.clone(),
            frame_px: s.frame_px,
            content_px: s.content_px,
            frame,
            held,
        }
    }

    fn switch_to(&mut self, k: usize) -> Result<()> {
        let mut lower = 0;
        match self.active.take() {
            Some(mut a) if a.seg < k => {
                a.advance(i64::MAX, &self.segs[a.seg])?;
                if let Some(f) = a.current {
                    self.carry = Some((a.seg, f));
                }
                lower = a.seg + 1;
            }
            Some(_) => self.carry = None,
            None => lower = self.carry.as_ref().map_or(0, |(j, _)| j + 1),
        }
        for j in (lower..k).rev() {
            let mut skipped = self.open_segment(j)?;
            skipped.advance(i64::MAX, &self.segs[j])?;
            if let Some(f) = skipped.current {
                self.carry = Some((j, f));
                break;
            }
        }
        self.active = Some(self.open_segment(k)?);
        Ok(())
    }

    fn open_segment(&self, j: usize) -> Result<Active> {
        let seg = &self.segs[j];
        let reader = self
            .decoder
            .open_video(&seg.path)
            .map_err(|e| seg.err("opening", e))?;
        let size = reader.size();
        if size != seg.frame_px {
            tracing::warn!(
                segment = %seg.id,
                "decoded size {}x{} differs from frame_px {}x{}",
                size.w,
                size.h,
                seg.frame_px.w,
                seg.frame_px.h
            );
        }
        Ok(Active {
            seg: j,
            reader: Some(reader),
            current: None,
            pending: None,
        })
    }
}

/// Sequential frames of one continuous video track (the camera), at recording time
/// `first_sample_offset_ns + file time`. Holds the newest frame at or before the requested time;
/// a request earlier than the previous one reopens the file.
pub struct TrackFrameSource {
    decoder: Arc<dyn Decoder>,
    path: PathBuf,
    offset: i64,
    reader: Option<Box<dyn VideoReader>>,
    current: Option<FrameBuffer>,
    pending: Option<DecodedFrame>,
    last_t: Option<i64>,
}

impl TrackFrameSource {
    /// `None` when the project has no track `track_id`.
    pub fn open(
        project: &ProjectV1,
        dir: &Path,
        decoder: Arc<dyn Decoder>,
        track_id: &str,
    ) -> Result<Option<TrackFrameSource>> {
        let Some(track) = project.recording.tracks.iter().find(|t| t.id == track_id) else {
            return Ok(None);
        };
        let path = dir.join(&track.file);
        let reader = decoder.open_video(&path)?;
        Ok(Some(TrackFrameSource {
            decoder,
            path,
            offset: track.first_sample_offset_ns,
            reader: Some(reader),
            current: None,
            pending: None,
            last_t: None,
        }))
    }

    pub fn frame_at(&mut self, t_rec_ns: i64) -> Result<Option<FrameBuffer>> {
        if self.last_t.is_some_and(|l| t_rec_ns < l) {
            self.reader = Some(self.decoder.open_video(&self.path)?);
            self.current = None;
            self.pending = None;
        }
        self.last_t = Some(t_rec_ns);
        let rel = t_rec_ns - self.offset;
        loop {
            let next = match self.pending.take() {
                Some(f) => f,
                None => match self.reader.as_mut() {
                    Some(r) => match r.next_frame()? {
                        Some(f) => f,
                        None => {
                            self.reader = None;
                            break;
                        }
                    },
                    None => break,
                },
            };
            if next.file_ns > rel {
                self.pending = Some(next);
                break;
            }
            self.current = Some(next.frame);
        }
        Ok(self
            .current
            .clone()
            .or_else(|| self.pending.as_ref().map(|p| p.frame.clone())))
    }
}

#[derive(Debug, Clone)]
pub struct SegmentFrame {
    pub segment_id: String,
    pub frame_px: SizePx,
    pub content_px: RectPx,
    pub frame: FrameBuffer,
    pub held: bool,
}

/// `round(n·1e9/fps)` for every `n` whose time is before `duration_ns`.
pub fn cfr_frame_times(duration_ns: i64, fps: u32) -> impl Iterator<Item = i64> {
    let fps = i128::from(fps);
    (0i128..)
        .map_while(move |n| {
            if fps == 0 {
                return None;
            }
            i64::try_from((2 * n * 1_000_000_000 + fps) / (2 * fps)).ok()
        })
        .take_while(move |&t| t < duration_ns)
}
