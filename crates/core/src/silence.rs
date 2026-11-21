//! Pauses in a recording's sound, found from loudness windows in recording time and placed on
//! the edited timeline (which may repeat, reorder or speed up the recording).

use crate::edit::{starts, tl_of};
use crate::project::Edl;

/// What counts as a pause: quieter than `threshold_db` for at least `min_ns`. `pad_ns` of quiet
/// is kept on each side so speech does not start or stop abruptly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuietRule {
    pub threshold_db: f32,
    pub min_ns: i64,
    pub pad_ns: i64,
}

/// Timeline ranges shorter than this are left alone.
pub const MIN_CUT_NS: i64 = 40_000_000;

/// Recording-time spans of the pauses in `levels_db` (one loudness value per `window_ns`). The
/// quiet before the first sound and after the last is removed whole; quiet that lasts to the end
/// ends at `i64::MAX`.
pub fn quiet_spans(levels_db: &[f32], window_ns: i64, rule: QuietRule) -> Vec<(i64, i64)> {
    let n = levels_db.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if levels_db[i] >= rule.threshold_db {
            i += 1;
            continue;
        }
        let first = i;
        while i < n && levels_db[i] < rule.threshold_db {
            i += 1;
        }
        let (a, b) = (first as i64 * window_ns, i as i64 * window_ns);
        if b - a < rule.min_ns {
            continue;
        }
        let a = if first == 0 { a } else { a + rule.pad_ns };
        let b = if i == n { i64::MAX } else { b - rule.pad_ns };
        if b > a {
            out.push((a, b));
        }
    }
    out
}

/// Where recording-time `spans` fall on the timeline of `edl`: sorted, touching ranges merged,
/// ranges under [`MIN_CUT_NS`] dropped.
pub fn on_timeline(edl: &Edl, spans: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let st = starts(edl);
    let mut found = Vec::new();
    for (k, c) in edl.clips.iter().enumerate() {
        for &(a, b) in spans {
            let (lo, hi) = (a.max(c.src_in_rec_ns), b.min(c.src_out_rec_ns));
            if hi > lo {
                found.push((
                    st[k] + tl_of(lo - c.src_in_rec_ns, c.speed),
                    st[k] + tl_of(hi - c.src_in_rec_ns, c.speed),
                ));
            }
        }
    }
    found.sort_unstable();
    let mut out: Vec<(i64, i64)> = Vec::with_capacity(found.len());
    for (a, b) in found {
        match out.last_mut() {
            Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out.retain(|&(a, b)| b - a >= MIN_CUT_NS);
    out
}

/// A starting threshold between the room's quiet and the speech: a third of the way up from the
/// quietest sixth of the windows to the loudest tenth, in dB, within -60..-20 dB.
pub fn suggested_threshold(levels_db: &[f32]) -> f32 {
    let mut v: Vec<f32> = levels_db
        .iter()
        .copied()
        .filter(|x| x.is_finite())
        .collect();
    if v.is_empty() {
        return -45.0;
    }
    v.sort_unstable_by(f32::total_cmp);
    let at = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize];
    let (floor, speech) = (at(0.15), at(0.9));
    let t = if speech - floor < 10.0 {
        floor + 6.0
    } else {
        floor + (speech - floor) / 3.0
    };
    t.clamp(-60.0, -20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Clip, Speed};

    const W: i64 = 10_000_000;
    const MS: i64 = 1_000_000;

    fn clip(a_ms: i64, b_ms: i64, speed: Speed) -> Clip {
        Clip {
            id: format!("{a_ms}"),
            src_in_rec_ns: a_ms * MS,
            src_out_rec_ns: b_ms * MS,
            speed,
            muted: false,
            extra: Default::default(),
        }
    }

    fn levels(pattern: &[(f32, usize)]) -> Vec<f32> {
        pattern
            .iter()
            .flat_map(|&(db, n)| std::iter::repeat_n(db, n))
            .collect()
    }

    #[test]
    fn pauses_need_the_minimum_length_and_keep_padding_inside_speech() {
        let rule = QuietRule {
            threshold_db: -40.0,
            min_ns: 500 * MS,
            pad_ns: 100 * MS,
        };
        let l = levels(&[
            (-70.0, 60),
            (-12.0, 100),
            (-55.0, 30),
            (-15.0, 50),
            (-60.0, 80),
            (-10.0, 20),
            (-65.0, 60),
        ]);
        assert_eq!(
            quiet_spans(&l, W, rule),
            vec![(0, 500 * MS), (2500 * MS, 3100 * MS), (3500 * MS, i64::MAX)]
        );
    }

    #[test]
    fn spans_follow_clips_through_cuts_speed_and_order() {
        let edl = Edl {
            clips: vec![
                clip(5000, 8000, Speed::ONE),
                clip(0, 4000, Speed { num: 2, den: 1 }),
            ],
            extra: Default::default(),
        };
        let spans = [(1000 * MS, 2000 * MS), (3990 * MS, 6000 * MS)];
        assert_eq!(
            on_timeline(&edl, &spans),
            vec![(0, 1000 * MS), (3500 * MS, 4000 * MS)]
        );
        let touching = [(7000 * MS, i64::MAX), (0, 10 * MS), (10 * MS, 400 * MS)];
        assert_eq!(on_timeline(&edl, &touching), vec![(2000 * MS, 3200 * MS)]);
    }

    #[test]
    fn suggestion_sits_between_room_tone_and_speech() {
        let talk = levels(&[(-58.0, 400), (-14.0, 600)]);
        let t = suggested_threshold(&talk);
        assert!((-46.0..=-42.0).contains(&t), "{t}");
        assert_eq!(suggested_threshold(&levels(&[(-80.0, 100)])), -60.0);
        assert_eq!(suggested_threshold(&[]), -45.0);
    }
}
