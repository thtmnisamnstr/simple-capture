mod common;

use std::sync::Arc;

use common::{MS, SegSpec, TempDir, every, identify, project, write_segments};
use sc_decode::{Error, SegmentFrameSource, cfr_frame_times};
use sc_geometry::SizePx;
use sc_platform::synthetic::SctrkDecoder;

#[derive(Debug, PartialEq)]
struct Expect {
    seg: u32,
    frame_ms: i64,
    held: bool,
}

fn last_frame(s: &SegSpec, upto_rel_ns: i64) -> Option<i64> {
    let limit = s.end_ms.map_or(i64::MAX, |e| (e - s.start_ms) * MS);
    s.frames_ms
        .iter()
        .copied()
        .filter(|&f| f * MS <= upto_rel_ns && f * MS < limit)
        .max()
}

fn oracle(specs: &[SegSpec], t: i64) -> Option<Expect> {
    let listed: Vec<&SegSpec> = specs.iter().filter(|s| s.listed).collect();
    let k = listed.iter().rposition(|s| s.start_ms * MS <= t)?;
    let s = listed[k];
    if let Some(f) = last_frame(s, t - s.start_ms * MS) {
        let past_end = s.end_ms.is_some_and(|e| t >= e * MS);
        return Some(Expect {
            seg: s.n,
            frame_ms: f,
            held: past_end,
        });
    }
    listed[..k].iter().rev().find_map(|p| {
        last_frame(p, i64::MAX).map(|f| Expect {
            seg: p.n,
            frame_ms: f,
            held: true,
        })
    })
}

fn source(dir: &TempDir, specs: &[SegSpec]) -> SegmentFrameSource {
    let mut p = project();
    write_segments(dir.path(), &mut p, specs);
    SegmentFrameSource::open(&p, dir.path(), Arc::new(SctrkDecoder)).expect("open")
}

fn check(src: &mut SegmentFrameSource, specs: &[SegSpec], t: i64) -> Option<Expect> {
    let got = src.frame_at(t).expect("frame_at");
    let want = oracle(specs, t);
    let got_e = got.as_ref().map(|f| {
        let (seg, frame_ms) = identify(&f.frame);
        let spec = specs.iter().find(|s| s.n == seg).expect("spec");
        assert_eq!(f.segment_id, spec.id(), "t={t}");
        assert_eq!(f.frame_px, spec.size, "t={t}");
        assert_eq!(f.content_px, spec.content(), "t={t}");
        Expect {
            seg,
            frame_ms,
            held: f.held,
        }
    });
    assert_eq!(got_e, want, "t={t}");
    got_e
}

fn ids(f: &sc_decode::SegmentFrame) -> (&str, (u32, i64), bool) {
    (f.segment_id.as_str(), identify(&f.frame), f.held)
}

fn three_with_gap() -> Vec<SegSpec> {
    let mut s1 = SegSpec::new(1, 1000, 2000, every(40, 0, 1000));
    s1.size = SizePx { w: 256, h: 160 };
    let mut s2 = SegSpec::new(2, 2500, 3500, every(50, 20, 1000));
    s2.size = SizePx { w: 480, h: 270 };
    vec![SegSpec::new(0, 0, 1000, every(100, 0, 1000)), s1, s2]
}

#[test]
fn cfr_times_round_and_stop_before_duration() {
    let t: Vec<i64> = cfr_frame_times(1_000_000_000, 30).collect();
    assert_eq!(t.len(), 30);
    assert_eq!(&t[..4], &[0, 33_333_333, 66_666_667, 100_000_000]);
    assert_eq!(t[29], 966_666_667);
    let t60: Vec<i64> = cfr_frame_times(100_000_000, 60).collect();
    assert_eq!(
        t60,
        vec![
            0, 16_666_667, 33_333_333, 50_000_000, 66_666_667, 83_333_333
        ]
    );
    assert_eq!(cfr_frame_times(1_000_000_001, 30).count(), 31);
    assert_eq!(cfr_frame_times(0, 30).count(), 0);
    assert_eq!(cfr_frame_times(-5, 30).count(), 0);
    assert_eq!(cfr_frame_times(1_000_000_000, 0).count(), 0);
    let long: Vec<i64> = cfr_frame_times(3_600 * 1_000_000_000, 60).collect();
    assert_eq!(long.len(), 216_000);
    assert!(long.windows(2).all(|w| w[1] > w[0]));
}

#[test]
fn three_segments_with_gap_follow_the_frame_rule() {
    let dir = TempDir::new("three");
    let specs = three_with_gap();
    let mut src = source(&dir, &specs);
    let (mut held, mut segs_seen) = (0, [0u32; 3]);
    for t in cfr_frame_times(3_500 * MS, 30) {
        let e = check(&mut src, &specs, t).expect("a frame at every t");
        segs_seen[e.seg as usize] += 1;
        held += usize::from(e.held);
    }
    assert!(segs_seen.iter().all(|&n| n > 0), "{segs_seen:?}");
    assert_eq!(held, 15 + 1);
}

#[test]
fn gap_holds_the_previous_segment_last_frame() {
    let dir = TempDir::new("gap");
    let specs = three_with_gap();
    let mut src = source(&dir, &specs);
    for t in [1_999 * MS, 2_000 * MS, 2_250 * MS, 2_499 * MS] {
        let f = src.frame_at(t).unwrap().unwrap();
        assert_eq!(ids(&f), ("s1", (1, 960), t >= 2_000 * MS), "t={t}");
    }
    let f = src.frame_at(2_510 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s1", (1, 960), true));
    let f = src.frame_at(2_520 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s2", (2, 20), false));
}

#[test]
fn idle_periods_hold_under_cfr() {
    let dir = TempDir::new("idle");
    let specs = vec![SegSpec::new(0, 0, 3000, vec![0, 100, 1500, 1600, 2900])];
    let mut src = source(&dir, &specs);
    let mut frames = Vec::new();
    for t in cfr_frame_times(3_000 * MS, 30) {
        let e = check(&mut src, &specs, t).unwrap();
        assert!(!e.held, "t={t}");
        frames.push(e.frame_ms);
    }
    assert_eq!(frames.len(), 90);
    assert_eq!(frames.iter().filter(|&&f| f == 100).count(), 42);
    assert_eq!(frames.iter().filter(|&&f| f == 1600).count(), 39);
    assert_eq!(frames[89], 2900);
}

#[test]
fn boundary_picks_the_new_segment_exactly_at_start() {
    let dir = TempDir::new("boundary");
    let specs = vec![
        SegSpec::new(0, 0, 1000, every(33, 0, 1000)),
        SegSpec::new(1, 1000, 2000, every(33, 0, 1000)),
        SegSpec::new(2, 2000, 3000, every(33, 50, 1000)),
    ];
    let mut src = source(&dir, &specs);
    let f = src.frame_at(1_000 * MS - 1).unwrap().unwrap();
    assert_eq!(ids(&f), ("s0", (0, 990), false));
    let f = src.frame_at(1_000 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s1", (1, 0), false));
    let f = src.frame_at(2_000 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s1", (1, 990), true));
    let f = src.frame_at(2_050 * MS - 1).unwrap().unwrap();
    assert_eq!(ids(&f), ("s1", (1, 990), true));
    let f = src.frame_at(2_050 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s2", (2, 50), false));
}

#[test]
fn dropped_segment_holds_and_dropped_first_segment_shows_nothing() {
    let dir = TempDir::new("dropped");
    let mut specs = three_with_gap();
    specs[1].listed = false;
    let mut src = source(&dir, &specs);
    for t in cfr_frame_times(3_500 * MS, 30) {
        check(&mut src, &specs, t).unwrap();
    }
    let f = src.frame_at(3_500 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s2", (2, 970), true));
    let dir_b = TempDir::new("dropped-b");
    let mut src = source(&dir_b, &specs);
    let f = src.frame_at(1_500 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s0", (0, 900), true));

    let dir = TempDir::new("dropped-first");
    let mut specs = three_with_gap();
    specs[0].listed = false;
    let mut src = source(&dir, &specs);
    assert!(src.frame_at(0).unwrap().is_none());
    assert!(src.frame_at(999 * MS).unwrap().is_none());
    let f = src.frame_at(1_000 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s1", (1, 0), false));
}

#[test]
fn frames_past_the_segment_end_are_not_shown() {
    let dir = TempDir::new("past-end");
    let specs = vec![
        SegSpec::new(0, 0, 1000, vec![0, 500, 1000, 1100]),
        SegSpec::new(1, 2000, 3000, vec![0]),
    ];
    let mut src = source(&dir, &specs);
    for t in [900, 1000, 1200, 1999, 2000] {
        check(&mut src, &specs, t * MS).unwrap();
    }
    let dir_b = TempDir::new("past-end-b");
    let mut src = source(&dir_b, &specs);
    let f = src.frame_at(1_200 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s0", (0, 500), true));
}

#[test]
fn before_any_frame_is_none() {
    let dir = TempDir::new("none");
    let specs = vec![SegSpec::new(0, 100, 1000, vec![50, 100])];
    let mut src = source(&dir, &specs);
    assert!(src.frame_at(0).unwrap().is_none());
    assert!(src.frame_at(149 * MS).unwrap().is_none());
    let f = src.frame_at(150 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s0", (0, 50), false));

    let empty = SegmentFrameSource::open(&project(), dir.path(), Arc::new(SctrkDecoder))
        .unwrap()
        .frame_at(0)
        .unwrap();
    assert!(empty.is_none());
}

#[test]
fn jumps_across_segments_and_backwards_requests() {
    let dir = TempDir::new("jumps");
    let specs = three_with_gap();
    let mut src = source(&dir, &specs);
    for t in [50, 2_505, 3_100, 100, 1_500, 1_499, 2_200, 0, 3_499] {
        check(&mut src, &specs, t * MS).unwrap();
    }
    let dir_b = TempDir::new("jumps-b");
    let mut src = source(&dir_b, &specs);
    let f = src.frame_at(2_510 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s1", (1, 960), true));
}

#[test]
fn empty_segment_holds_the_one_before_it() {
    let dir = TempDir::new("empty-seg");
    let specs = vec![
        SegSpec::new(0, 0, 1000, every(100, 0, 1000)),
        SegSpec::new(1, 1000, 2000, Vec::new()),
        SegSpec::new(2, 2000, 3000, every(100, 100, 1000)),
    ];
    let mut src = source(&dir, &specs);
    for t in cfr_frame_times(3_000 * MS, 30) {
        check(&mut src, &specs, t).unwrap();
    }
    let dir_b = TempDir::new("empty-seg-b");
    let mut src = source(&dir_b, &specs);
    let f = src.frame_at(2_050 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s0", (0, 900), true));
}

#[test]
fn open_segment_without_end_runs_to_the_next_start() {
    let dir = TempDir::new("open-end");
    let mut specs = vec![
        SegSpec::new(0, 0, 1000, vec![0, 500, 1200]),
        SegSpec::new(1, 1000, 0, vec![0, 3000]),
    ];
    specs[0].end_ms = None;
    specs[1].end_ms = None;
    let mut src = source(&dir, &specs);
    let f = src.frame_at(999 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s0", (0, 500), false));
    let f = src.frame_at(9_000 * MS).unwrap().unwrap();
    assert_eq!(ids(&f), ("s1", (1, 3000), false));
}

#[test]
fn errors_name_the_segment() {
    let dir = TempDir::new("errors");
    let mut p = project();
    write_segments(dir.path(), &mut p, &three_with_gap());
    std::fs::remove_file(dir.path().join("tracks/screen-001.sctrk")).unwrap();
    let mut src = SegmentFrameSource::open(&p, dir.path(), Arc::new(SctrkDecoder)).unwrap();
    assert!(src.frame_at(500 * MS).unwrap().is_some());
    let e = src.frame_at(1_500 * MS).expect_err("missing file");
    assert!(e.to_string().contains("segment s1"), "{e}");

    p.recording.tracks.retain(|t| t.id != "screen-2");
    match SegmentFrameSource::open(&p, dir.path(), Arc::new(SctrkDecoder)) {
        Err(Error::NotFound(m)) => assert!(m.contains("screen-2"), "{m}"),
        Err(e) => panic!("unexpected error {e}"),
        Ok(_) => panic!("missing track accepted"),
    }
}
