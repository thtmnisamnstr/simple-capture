//! The edited timeline's audio: every audio track read through the clips, at each track's gain,
//! with clip mute and speed (pitch kept by Signalsmith Stretch) and short fades at every cut
//! (ARCHITECTURE §7).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_core::edit::{Doc, starts};
use sc_core::project::{Clip, Edl, ProjectV1, TrackKind};
use sc_decode::TrackAudio;
use sc_platform::{Decoder, Result};
use signalsmith_stretch::Stretch;

pub const CHANNELS: u16 = 2;
pub const CUT_FADE_NS: i64 = 5_000_000;

pub fn audio_track_ids(project: &ProjectV1) -> Vec<String> {
    project
        .recording
        .tracks
        .iter()
        .filter(|t| t.kind.is_audio())
        .map(|t| t.id.clone())
        .collect()
}

fn db_to_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

struct ClipAudio {
    index: usize,
    end_frame: u64,
    readers: Vec<TrackAudio>,
    stretch: Option<Stretched>,
}

struct Stretched {
    s: Stretch,
    ratio: f64,
    owed_in: f64,
    drop_out: usize,
}

/// Which audio a lane plays and through which clips: everything through the screen's clips
/// while the camera is linked; otherwise system and device audio with the screen and the microphone with the
/// camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaneKind {
    All,
    Screen,
    Camera,
}

impl LaneKind {
    fn plays(self, kind: &TrackKind) -> bool {
        match self {
            LaneKind::All => kind.is_audio(),
            LaneKind::Screen => matches!(kind, TrackKind::SystemAudio | TrackKind::DeviceAudio),
            LaneKind::Camera => matches!(kind, TrackKind::Mic),
        }
    }

    fn edl(self, doc: &Doc) -> &Edl {
        match self {
            LaneKind::Camera => doc.camera_clips(),
            _ => &doc.edl,
        }
    }
}

/// Pulls the mixed timeline audio, interleaved stereo f32 at `rate`, from any timeline position.
pub struct TimelineAudio {
    project: Arc<ProjectV1>,
    dir: PathBuf,
    decoder: Arc<dyn Decoder>,
    rate: u32,
    lanes: Vec<Lane>,
    buf: Vec<f32>,
}

impl TimelineAudio {
    pub fn new(
        project: Arc<ProjectV1>,
        dir: &Path,
        decoder: Arc<dyn Decoder>,
        doc: Doc,
        rate: u32,
    ) -> TimelineAudio {
        let mut a = TimelineAudio {
            project,
            dir: dir.to_path_buf(),
            decoder,
            rate: rate.max(1),
            lanes: Vec::new(),
            buf: Vec::new(),
        };
        a.set_doc(doc);
        a
    }

    fn kinds(&self, doc: &Doc) -> Vec<LaneKind> {
        if doc.linked() {
            return vec![LaneKind::All];
        }
        let mic = self
            .project
            .recording
            .tracks
            .iter()
            .any(|t| LaneKind::Camera.plays(&t.kind));
        let mut v = vec![LaneKind::Screen];
        if mic {
            v.push(LaneKind::Camera);
        }
        v
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn frame_of(&self, tl_ns: i64) -> u64 {
        (i128::from(tl_ns.max(0)) * i128::from(self.rate) / 1_000_000_000) as u64
    }

    pub fn ns_of(&self, frame: u64) -> i64 {
        (i128::from(frame) * 1_000_000_000 / i128::from(self.rate)) as i64
    }

    pub fn len_frames(&self) -> u64 {
        self.lanes.first().map_or(0, Lane::len_frames)
    }

    pub fn position(&self) -> u64 {
        self.lanes.first().map_or(0, |l| l.pos)
    }

    pub fn set_doc(&mut self, doc: Doc) {
        let kinds = self.kinds(&doc);
        if kinds != self.lanes.iter().map(|l| l.kind).collect::<Vec<_>>() {
            let pos = self.position();
            self.lanes = kinds
                .into_iter()
                .map(|k| {
                    let mut l = Lane::new(
                        self.project.clone(),
                        &self.dir,
                        self.decoder.clone(),
                        doc.clone(),
                        self.rate,
                        k,
                    );
                    l.pos = pos;
                    l
                })
                .collect();
            return;
        }
        for l in &mut self.lanes {
            l.set_doc(doc.clone());
        }
    }

    pub fn seek(&mut self, tl_ns: i64) {
        for l in &mut self.lanes {
            l.seek(tl_ns);
        }
    }

    /// Fills all of `out` (whole stereo frames) and moves on; silence past the end.
    pub fn read(&mut self, out: &mut [f32]) -> Result<()> {
        let Some((first, rest)) = self.lanes.split_first_mut() else {
            out.fill(0.0);
            return Ok(());
        };
        first.read(out)?;
        for l in rest {
            self.buf.resize(out.len(), 0.0);
            l.read(&mut self.buf)?;
            for (o, s) in out.iter_mut().zip(&self.buf) {
                *o = (*o + s).clamp(-1.0, 1.0);
            }
        }
        Ok(())
    }
}

struct Lane {
    kind: LaneKind,
    project: Arc<ProjectV1>,
    dir: PathBuf,
    decoder: Arc<dyn Decoder>,
    doc: Doc,
    tracks: Vec<(String, f32)>,
    rate: u32,
    starts_frames: Vec<u64>,
    cuts: Vec<u64>,
    pos: u64,
    cur: Option<ClipAudio>,
    scratch: Vec<f32>,
    mix: Vec<f32>,
}

impl Lane {
    fn new(
        project: Arc<ProjectV1>,
        dir: &Path,
        decoder: Arc<dyn Decoder>,
        doc: Doc,
        rate: u32,
        kind: LaneKind,
    ) -> Lane {
        let mut a = Lane {
            kind,
            project,
            dir: dir.to_path_buf(),
            decoder,
            doc: doc.clone(),
            tracks: Vec::new(),
            rate: rate.max(1),
            starts_frames: Vec::new(),
            cuts: Vec::new(),
            pos: 0,
            cur: None,
            scratch: Vec::new(),
            mix: Vec::new(),
        };
        a.set_doc(doc);
        a
    }

    fn frame_of(&self, tl_ns: i64) -> u64 {
        (i128::from(tl_ns.max(0)) * i128::from(self.rate) / 1_000_000_000) as u64
    }

    fn ns_of(&self, frame: u64) -> i64 {
        (i128::from(frame) * 1_000_000_000 / i128::from(self.rate)) as i64
    }

    fn len_frames(&self) -> u64 {
        *self.starts_frames.last().unwrap_or(&0)
    }

    fn set_doc(&mut self, doc: Doc) {
        let kind = self.kind;
        self.tracks = self
            .project
            .recording
            .tracks
            .iter()
            .filter(|t| kind.plays(&t.kind))
            .map(|t| t.id.clone())
            .map(|id| {
                let gain = match doc.audio_mix.get(&id) {
                    Some(m) if m.muted => 0.0,
                    Some(m) => db_to_gain(m.gain_db),
                    None => 1.0,
                };
                (id, gain)
            })
            .collect();
        let edl = kind.edl(&doc);
        self.starts_frames = starts(edl).into_iter().map(|t| self.frame_of(t)).collect();
        self.cuts = edl
            .clips
            .windows(2)
            .enumerate()
            .filter(|(_, w)| !continues(&w[0], &w[1]))
            .map(|(i, _)| self.starts_frames[i + 1])
            .collect();
        self.doc = doc;
        self.cur = None;
    }

    fn seek(&mut self, tl_ns: i64) {
        self.pos = self.frame_of(tl_ns);
        self.cur = None;
    }

    fn read(&mut self, out: &mut [f32]) -> Result<()> {
        let ch = usize::from(CHANNELS);
        let frames = out.len() / ch;
        let mut done = 0;
        while done < frames {
            if self.cur.as_ref().is_none_or(|c| self.pos >= c.end_frame) {
                self.enter_clip_at_pos()?;
            }
            let Some(end) = self.cur.as_ref().map(|c| c.end_frame) else {
                out[done * ch..frames * ch].fill(0.0);
                self.pos += (frames - done) as u64;
                break;
            };
            let n = ((end - self.pos) as usize).min(frames - done);
            self.render_clip(&mut out[done * ch..(done + n) * ch])?;
            self.apply_fades(&mut out[done * ch..(done + n) * ch], self.pos);
            self.pos += n as u64;
            done += n;
        }
        for s in &mut out[..frames * ch] {
            *s = if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        Ok(())
    }

    fn edl(&self) -> &Edl {
        self.kind.edl(&self.doc)
    }

    fn enter_clip_at_pos(&mut self) -> Result<()> {
        let st = &self.starts_frames;
        let total = *st.last().unwrap_or(&0);
        if self.pos >= total || self.edl().clips.is_empty() {
            self.cur = None;
            return Ok(());
        }
        let index = st.partition_point(|s| *s <= self.pos).saturating_sub(1);
        let clip = self.edl().clips[index].clone();
        let into_ns = self.ns_of(self.pos - st[index]);
        let rec = clip.rec_at(into_ns);
        let end_frame = st[index + 1];
        let keep = self.cur.take().filter(|c| {
            c.index + 1 == index
                && c.stretch.is_none()
                && into_ns == 0
                && continues(&self.edl().clips[c.index], &clip)
        });
        let readers = match keep {
            Some(c) => c.readers,
            None => self.open_readers(rec)?,
        };
        let mut cur = ClipAudio {
            index,
            end_frame,
            readers,
            stretch: None,
        };
        if !clip.speed.is_one() {
            let mut s = Stretch::preset_default(u32::from(CHANNELS), self.rate);
            let ratio = clip.speed.factor();
            let mut pre_in = vec![0f32; s.input_latency() * usize::from(CHANNELS)];
            self.mix_sources(&mut cur, &mut pre_in)?;
            s.seek(&pre_in, ratio);
            cur.stretch = Some(Stretched {
                drop_out: s.output_latency(),
                s,
                ratio,
                owed_in: 0.0,
            });
        }
        self.cur = Some(cur);
        Ok(())
    }

    fn open_readers(&self, rec: i64) -> Result<Vec<TrackAudio>> {
        self.tracks
            .iter()
            .map(|(id, _)| {
                TrackAudio::open_at(
                    &self.project,
                    &self.dir,
                    id,
                    self.decoder.clone(),
                    self.rate,
                    CHANNELS,
                    rec,
                )
            })
            .collect()
    }

    /// Source audio (all tracks at their gains) into `out`, advancing the readers.
    fn mix_sources(&mut self, cur: &mut ClipAudio, out: &mut [f32]) -> Result<()> {
        out.fill(0.0);
        self.scratch.resize(out.len(), 0.0);
        let muted = {
            let c = &self.edl().clips[cur.index];
            c.muted || c.hidden
        };
        for (r, (_, gain)) in cur.readers.iter_mut().zip(&self.tracks) {
            r.read(&mut self.scratch[..out.len()])?;
            if muted || *gain == 0.0 {
                continue;
            }
            for (o, s) in out.iter_mut().zip(&self.scratch) {
                *o += s * gain;
            }
        }
        Ok(())
    }

    fn render_clip(&mut self, out: &mut [f32]) -> Result<()> {
        let Some(mut cur) = self.cur.take() else {
            out.fill(0.0);
            return Ok(());
        };
        let r = match cur.stretch.take() {
            None => self.mix_sources(&mut cur, out),
            Some(mut st) => {
                let r = self.stretch_into(&mut cur, &mut st, out);
                cur.stretch = Some(st);
                r
            }
        };
        self.cur = Some(cur);
        r
    }

    /// The stretcher's first `output_latency` frames come before the clip's first sample, so
    /// they are produced and dropped.
    fn stretch_into(
        &mut self,
        cur: &mut ClipAudio,
        st: &mut Stretched,
        out: &mut [f32],
    ) -> Result<()> {
        let ch = usize::from(CHANNELS);
        let mut want = out.len() / ch;
        let mut filled = 0;
        let mut sink = Vec::new();
        while want > 0 {
            let dropping = st.drop_out.min(4096);
            let n_out = if dropping > 0 { dropping } else { want };
            st.owed_in += n_out as f64 * st.ratio;
            let n_in = st.owed_in.floor() as usize;
            st.owed_in -= n_in as f64;
            let mut input = std::mem::take(&mut self.mix);
            input.resize(n_in * ch, 0.0);
            self.mix_sources(cur, &mut input)?;
            if dropping > 0 {
                sink.resize(n_out * ch, 0.0);
                st.s.process(&input, &mut sink);
                st.drop_out -= dropping;
            } else {
                st.s.process(&input, &mut out[filled * ch..(filled + n_out) * ch]);
                filled += n_out;
                want -= n_out;
            }
            self.mix = input;
        }
        Ok(())
    }

    fn apply_fades(&self, out: &mut [f32], from: u64) {
        if self.cuts.is_empty() {
            return;
        }
        let ch = usize::from(CHANNELS);
        let f = (i128::from(CUT_FADE_NS) * i128::from(self.rate) / 1_000_000_000) as f64;
        if f < 1.0 {
            return;
        }
        let n = (out.len() / ch) as u64;
        let to = from + n;
        let k = self.cuts.partition_point(|c| (*c as f64) + f < from as f64);
        for &cut in &self.cuts[k..] {
            if (cut as f64) - f > to as f64 {
                break;
            }
            for i in 0..n {
                let p = from + i;
                let d = if p < cut {
                    (cut - p) as f64 - 0.5
                } else {
                    (p - cut) as f64 + 0.5
                };
                if d >= f {
                    continue;
                }
                let g = (0.5 - 0.5 * (std::f64::consts::PI * d / f).cos()) as f32;
                for c in 0..ch {
                    out[i as usize * ch + c] *= g;
                }
            }
        }
    }
}

/// `b` picks up exactly where `a` stops, at normal speed: there is no cut to hear.
fn continues(a: &Clip, b: &Clip) -> bool {
    a.src_out_rec_ns == b.src_in_rec_ns && a.speed == b.speed && a.speed.is_one()
}
