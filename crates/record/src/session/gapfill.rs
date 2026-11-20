//! Audio gap filler (ADR-035): file time comes from the cumulative sample count anchored at the
//! first buffer's host time. Late buffers get silence in front, early ones lose their overlapping
//! leading samples, so device clock skew is corrected in small steps.

use sc_core::journal::GapKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    pub kind: GapKind,
    pub at_host_ns: i64,
    pub dur_ns: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub silence_frames: u32,
    pub silence_file_ns: i64,
    /// File time of the buffer's first sample; writing starts `skip_frames` later.
    pub buffer_file_ns: i64,
    pub skip_frames: u32,
    pub drop: bool,
    pub gap: Option<Gap>,
    pts_ns: i64,
    frames: u32,
    advance: u64,
}

#[derive(Debug, Clone)]
pub struct GapFiller {
    rate: u32,
    threshold_ns: i64,
    anchor: Option<i64>,
    written: u64,
    last_host_end: Option<i64>,
}

pub fn frames_to_ns(frames: u64, rate: u32) -> i64 {
    let r = i128::from(rate.max(1));
    ((i128::from(frames) * 1_000_000_000 + r / 2) / r) as i64
}

pub fn ns_to_frames(ns: i64, rate: u32) -> u64 {
    let ns = i128::from(ns.max(0));
    ((ns * i128::from(rate) + 500_000_000) / 1_000_000_000) as u64
}

impl GapFiller {
    pub fn new(rate: u32, threshold_ns: i64) -> GapFiller {
        GapFiller {
            rate: rate.max(1),
            threshold_ns: threshold_ns.max(0),
            anchor: None,
            written: 0,
            last_host_end: None,
        }
    }

    pub fn anchor(&self) -> Option<i64> {
        self.anchor
    }

    pub fn written_frames(&self) -> u64 {
        self.written
    }

    pub fn expected_host(&self) -> Option<i64> {
        self.anchor
            .map(|a| a + frames_to_ns(self.written, self.rate))
    }

    /// Count-based end minus host-time end of the last committed buffer.
    pub fn drift_ns(&self) -> i64 {
        match (self.expected_host(), self.last_host_end) {
            (Some(e), Some(h)) => e - h,
            _ => 0,
        }
    }

    pub fn plan(&self, pts_ns: i64, frames: u32) -> Step {
        let mut s = Step {
            silence_frames: 0,
            silence_file_ns: 0,
            buffer_file_ns: 0,
            skip_frames: 0,
            drop: false,
            gap: None,
            pts_ns,
            frames,
            advance: u64::from(frames),
        };
        let Some(expected) = self.expected_host() else {
            return s;
        };
        let pos = self.written;
        let diff = pts_ns - expected;
        if diff > self.threshold_ns {
            let silence = ns_to_frames(diff, self.rate).clamp(1, u64::from(u32::MAX)) as u32;
            s.silence_frames = silence;
            s.silence_file_ns = frames_to_ns(pos, self.rate);
            s.buffer_file_ns = frames_to_ns(pos + u64::from(silence), self.rate);
            s.gap = Some(Gap {
                kind: GapKind::Gap,
                at_host_ns: expected,
                dur_ns: frames_to_ns(u64::from(silence), self.rate),
            });
        } else if diff < -self.threshold_ns {
            let overlap = ns_to_frames(-diff, self.rate);
            if overlap >= u64::from(frames) || overlap > pos {
                s.drop = true;
                s.advance = 0;
                s.gap = Some(Gap {
                    kind: GapKind::Overlap,
                    at_host_ns: pts_ns,
                    dur_ns: frames_to_ns(u64::from(frames), self.rate),
                });
            } else {
                let skip = overlap as u32;
                s.skip_frames = skip;
                s.buffer_file_ns = frames_to_ns(pos - overlap, self.rate);
                s.advance = u64::from(frames - skip);
                s.gap = Some(Gap {
                    kind: GapKind::Overlap,
                    at_host_ns: pts_ns,
                    dur_ns: frames_to_ns(overlap, self.rate),
                });
            }
        } else {
            s.buffer_file_ns = frames_to_ns(pos, self.rate);
        }
        s
    }

    pub fn commit_silence(&mut self, s: &Step) {
        self.written += u64::from(s.silence_frames);
    }

    /// Also for a buffer that `plan` dropped.
    pub fn commit_buffer(&mut self, s: &Step) {
        if self.anchor.is_none() {
            self.anchor = Some(s.pts_ns);
        }
        self.written += s.advance;
        let end = s.pts_ns + frames_to_ns(u64::from(s.frames), self.rate);
        self.last_host_end = Some(self.last_host_end.map_or(end, |e| e.max(end)));
    }

    pub fn feed(&mut self, pts_ns: i64, frames: u32) -> Step {
        let s = self.plan(pts_ns, frames);
        self.commit_silence(&s);
        self.commit_buffer(&s);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_platform::synthetic::{AudioScript, audio_buffer_pts};

    const RATE: u32 = 48_000;
    const TH: i64 = 2_000_000;

    fn script(gaps: Vec<(i64, i64)>, overlaps: Vec<(i64, i64)>, ppm: f64) -> AudioScript {
        AudioScript {
            rate: RATE,
            channels: 1,
            buffer_frames: 480,
            beeps_ns: vec![],
            gaps,
            overlaps,
            clock_ppm: ppm,
        }
    }

    /// Returns the steps and the worst |written position − pts| over every kept buffer.
    fn run(s: &AudioScript, n: u64) -> (Vec<Step>, i64, GapFiller) {
        let start = 5_000_000_000;
        let mut g = GapFiller::new(RATE, TH);
        let mut worst = 0i64;
        let mut steps = Vec::new();
        for k in 0..n {
            let pts = audio_buffer_pts(s, start, k);
            let st = g.feed(pts, s.buffer_frames);
            if !st.drop {
                let skip = frames_to_ns(u64::from(st.skip_frames), RATE);
                let at = g.anchor().expect("anchored") + st.buffer_file_ns + skip;
                worst = worst.max((at - (pts + skip)).abs());
            }
            steps.push(st);
        }
        (steps, worst, g)
    }

    #[test]
    fn steady_stream_has_no_gaps() {
        let (steps, worst, g) = run(&script(vec![], vec![], 0.0), 1000);
        assert!(
            steps
                .iter()
                .all(|s| s.gap.is_none() && s.silence_frames == 0)
        );
        assert!(worst <= 1, "worst {worst}");
        assert_eq!(g.written_frames(), 480_000);
        assert!(g.drift_ns().abs() <= 1);
    }

    #[test]
    fn scripted_gap_is_filled_with_silence() {
        let (steps, worst, g) = run(&script(vec![(1_000_000_000, 40_000_000)], vec![], 0.0), 300);
        let gaps: Vec<_> = steps.iter().filter_map(|s| s.gap).collect();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].kind, GapKind::Gap);
        assert_eq!(gaps[0].dur_ns, 40_000_000);
        let filled = steps
            .iter()
            .find(|s| s.silence_frames > 0)
            .expect("silence");
        assert_eq!(filled.silence_frames, 1920);
        assert_eq!(filled.buffer_file_ns - filled.silence_file_ns, 40_000_000);
        assert!(worst <= 1, "worst {worst}");
        assert_eq!(g.written_frames(), 300 * 480 + 1920);
    }

    #[test]
    fn small_jitter_below_threshold_is_ignored() {
        let (steps, worst, _) = run(&script(vec![(500_000_000, 1_500_000)], vec![], 0.0), 200);
        assert!(steps.iter().all(|s| s.gap.is_none()));
        assert!(worst <= 1_500_001, "worst {worst}");
    }

    #[test]
    fn scripted_overlap_is_trimmed() {
        let (steps, worst, g) = run(&script(vec![], vec![(1_000_000_000, 4_000_000)], 0.0), 300);
        let trimmed: Vec<_> = steps.iter().filter(|s| s.skip_frames > 0).collect();
        assert_eq!(trimmed.len(), 1);
        assert_eq!(trimmed[0].skip_frames, 192);
        assert_eq!(trimmed[0].gap.map(|g| g.kind), Some(GapKind::Overlap));
        assert!(worst <= 1, "worst {worst}");
        assert_eq!(g.written_frames(), 300 * 480 - 192);
    }

    #[test]
    fn buffer_inside_the_overlap_is_dropped() {
        let (steps, worst, g) = run(&script(vec![], vec![(1_000_000_000, 25_000_000)], 0.0), 300);
        let dropped = steps.iter().filter(|s| s.drop).count();
        let skipped: u32 = steps.iter().map(|s| s.skip_frames).sum();
        assert_eq!(
            dropped, 2,
            "two whole 10 ms buffers lie inside a 25 ms overlap"
        );
        assert_eq!(skipped, 240);
        assert!(worst <= 1, "worst {worst}");
        assert_eq!(g.written_frames(), 300 * 480 - 1200);
    }

    fn skew(ppm: f64) -> (i64, Vec<i64>) {
        let thirty_minutes_of_10ms_buffers = 30 * 60 * 100;
        let (steps, worst, g) = run(&script(vec![], vec![], ppm), thirty_minutes_of_10ms_buffers);
        assert!(g.drift_ns().abs() <= TH + 1, "final drift {}", g.drift_ns());
        let fixes = steps
            .iter()
            .filter_map(|s| s.gap.map(|g| g.dur_ns))
            .collect();
        (worst, fixes)
    }

    #[test]
    fn skew_plus_100_ppm_over_30_minutes_stays_within_one_frame() {
        let (worst, fixes) = skew(100.0);
        println!(
            "+100 ppm: worst {worst} ns, {} gap fills, first sizes {:?}",
            fixes.len(),
            &fixes[..fixes.len().min(3)]
        );
        assert!(worst <= 33_000_000, "worst {worst}");
        assert!(worst <= TH + 1_000_000, "worst {worst}");
        assert!(!fixes.is_empty());
    }

    #[test]
    fn skew_minus_100_ppm_over_30_minutes_stays_within_one_frame() {
        let (worst, fixes) = skew(-100.0);
        println!(
            "-100 ppm: worst {worst} ns, {} overlap trims, first sizes {:?}",
            fixes.len(),
            &fixes[..fixes.len().min(3)]
        );
        assert!(worst <= 33_000_000, "worst {worst}");
        assert!(worst <= TH + 1_000_000, "worst {worst}");
        assert!(!fixes.is_empty());
    }

    #[test]
    fn frame_ns_round_trip() {
        for f in [0u64, 1, 479, 480, 48_000, 86_400 * 48_000] {
            assert_eq!(ns_to_frames(frames_to_ns(f, RATE), RATE), f);
        }
    }
}
