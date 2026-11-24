use super::*;
use crate::project::{CameraOverlay, Clip, Edl, ProjectV1};
use crate::time::ClockKind;

const S: i64 = 1_000_000_000;

fn clip(a: i64, b: i64) -> Clip {
    Clip {
        id: crate::ids::new_id(),
        src_in_rec_ns: a,
        src_out_rec_ns: b,
        speed: Speed::ONE,
        muted: false,
        hidden: false,
        extra: Extra::new(),
    }
}

fn doc(clips: Vec<Clip>) -> Doc {
    let mut p = ProjectV1::new_recording(
        "p".into(),
        "t".into(),
        "now".into(),
        "0".into(),
        ClockKind::MachHost,
    );
    p.edl = Edl {
        clips,
        extra: Extra::new(),
    };
    p.camera_layout.overlays.push(CameraOverlay {
        id: "cam".into(),
        track_id: "camera".into(),
        keyframes: vec![CameraKey::default_placement()],
        extra: Extra::new(),
    });
    Doc::of(&p)
}

const LIM: Limits = Limits { rec_len_ns: 60 * S };

fn ranges(d: &Doc) -> Vec<(i64, i64)> {
    d.edl
        .clips
        .iter()
        .map(|c| (c.src_in_rec_ns, c.src_out_rec_ns))
        .collect()
}

fn spans(d: &Doc) -> Vec<(i64, i64)> {
    let mut v: Vec<_> = d
        .text
        .iter()
        .map(|t| (t.start_tl_ns, t.end_tl_ns))
        .collect();
    v.sort_unstable();
    v
}

fn ids(d: &Doc, idx: &[usize]) -> Vec<String> {
    idx.iter().map(|i| d.edl.clips[*i].id.clone()).collect()
}

#[test]
fn locate_maps_timeline_to_recording_across_cuts_and_speed() {
    let mut d = doc(vec![clip(0, 4 * S), clip(10 * S, 14 * S)]);
    d.edl.clips[1].speed = Speed { num: 2, den: 1 };
    assert_eq!(d.duration(), 6 * S);
    assert_eq!(locate(&d.edl, 3 * S).map(|l| l.rec_ns), Some(3 * S));
    let l = locate(&d.edl, 5 * S).expect("inside");
    assert_eq!((l.index, l.clip_start_tl, l.clip_end_tl), (1, 4 * S, 6 * S));
    assert_eq!(l.rec_ns, 12 * S);
    assert_eq!(locate(&d.edl, 6 * S), None);
    assert_eq!(locate(&d.edl, -1), None);
}

#[test]
fn split_makes_two_clips_and_snaps_near_edges() {
    let d = doc(vec![clip(0, 10 * S)]);
    let d1 = apply(
        &d,
        &EditOp::Split {
            at_tl_ns: 4 * S,
            track: None,
        },
        LIM,
    )
    .expect("split");
    assert_eq!(ranges(&d1), vec![(0, 4 * S), (4 * S, 10 * S)]);
    assert_ne!(d1.edl.clips[0].id, d1.edl.clips[1].id);
    let near = apply(
        &d1,
        &EditOp::Split {
            at_tl_ns: 4 * S + 5_000_000,
            track: None,
        },
        LIM,
    )
    .expect("snap");
    assert_eq!(ranges(&near), ranges(&d1));
}

#[test]
fn ripple_delete_closes_the_gap_and_moves_text() {
    let mut d = doc(vec![clip(0, 10 * S)]);
    d.text = vec![
        TextItem::new(S, 2 * S, "before"),
        TextItem::new(4 * S, 5 * S, "inside"),
        TextItem::new(5 * S, 8 * S, "across"),
        TextItem::new(8 * S, 9 * S, "after"),
    ];
    let d1 = apply(
        &d,
        &EditOp::DeleteRange {
            a_tl_ns: 3 * S,
            b_tl_ns: 6 * S,
        },
        LIM,
    )
    .expect("delete");
    assert_eq!(ranges(&d1), vec![(0, 3 * S), (6 * S, 10 * S)]);
    assert_eq!(d1.duration(), 7 * S);
    assert_eq!(spans(&d1), vec![(S, 2 * S), (3 * S, 5 * S), (5 * S, 6 * S)]);
}

#[test]
fn deleting_clips_removes_them_in_any_order() {
    let d = doc(vec![clip(0, S), clip(S, 2 * S), clip(2 * S, 3 * S)]);
    let gone = ids(&d, &[2, 0]);
    let d1 = apply(&d, &EditOp::DeleteClips { ids: gone }, LIM).expect("delete");
    assert_eq!(ranges(&d1), vec![(S, 2 * S)]);
}

#[test]
fn cut_and_paste_moves_a_piece_with_its_text() {
    let mut d = doc(vec![clip(0, 10 * S)]);
    d.text = vec![TextItem::new(2 * S + S / 2, 3 * S, "moves")];
    let cb = copy_range(&d, 2 * S, 4 * S);
    assert_eq!(cb.len(), 2 * S);
    assert_eq!(cb.text.len(), 1);
    let cut = apply(
        &d,
        &EditOp::DeleteRange {
            a_tl_ns: 2 * S,
            b_tl_ns: 4 * S,
        },
        LIM,
    )
    .expect("cut");
    assert!(cut.text.is_empty());
    let pasted = apply(
        &cut,
        &EditOp::Paste {
            at_tl_ns: 6 * S,
            clipboard: cb,
        },
        LIM,
    )
    .expect("paste");
    assert_eq!(
        ranges(&pasted),
        vec![(0, 2 * S), (4 * S, 8 * S), (2 * S, 4 * S), (8 * S, 10 * S)]
    );
    assert_eq!(spans(&pasted), vec![(6 * S + S / 2, 7 * S)]);
}

#[test]
fn paste_shifts_text_that_starts_at_or_after_the_playhead() {
    let mut d = doc(vec![clip(0, 10 * S)]);
    d.text = vec![
        TextItem::new(S, 3 * S, "a"),
        TextItem::new(5 * S, 6 * S, "b"),
    ];
    let cb = copy_range(&d, 7 * S, 9 * S);
    let d1 = apply(
        &d,
        &EditOp::Paste {
            at_tl_ns: 2 * S,
            clipboard: cb,
        },
        LIM,
    )
    .expect("paste");
    assert_eq!(spans(&d1), vec![(S, 3 * S), (7 * S, 8 * S)]);
    assert_eq!(d1.duration(), 12 * S);
}

#[test]
fn duplicate_inserts_a_copy_after_the_clip() {
    let d = doc(vec![clip(0, 2 * S), clip(5 * S, 6 * S)]);
    let d1 = apply(&d, &EditOp::Duplicate { ids: ids(&d, &[0]) }, LIM).expect("dup");
    assert_eq!(ranges(&d1), vec![(0, 2 * S), (0, 2 * S), (5 * S, 6 * S)]);
    let all: std::collections::HashSet<_> = d1.edl.clips.iter().map(|c| &c.id).collect();
    assert_eq!(all.len(), 3);
}

#[test]
fn the_last_clip_cannot_be_deleted() {
    let d = doc(vec![clip(0, S)]);
    let all = EditOp::DeleteRange {
        a_tl_ns: 0,
        b_tl_ns: S,
    };
    assert!(apply(&d, &all, LIM).is_err());
    assert!(apply(&d, &EditOp::DeleteClips { ids: ids(&d, &[0]) }, LIM).is_err());
}

#[test]
fn move_clip_reorders() {
    let d = doc(vec![clip(0, S), clip(S, 2 * S), clip(2 * S, 3 * S)]);
    let id = d.edl.clips[0].id.clone();
    let d1 = apply(&d, &EditOp::MoveClip { id, to_index: 2 }, LIM).expect("move");
    assert_eq!(ranges(&d1), vec![(S, 2 * S), (2 * S, 3 * S), (0, S)]);
}

#[test]
fn trim_start_and_end_ripple_text() {
    let mut d = doc(vec![clip(2 * S, 8 * S), clip(20 * S, 22 * S)]);
    d.text = vec![TextItem::new(7 * S, 8 * S, "late")];
    let id = d.edl.clips[0].id.clone();
    let d1 = apply(
        &d,
        &EditOp::Trim {
            clip_id: id.clone(),
            edge: Edge::Start,
            rec_ns: 3 * S,
        },
        LIM,
    )
    .expect("trim start");
    assert_eq!(ranges(&d1)[0], (3 * S, 8 * S));
    assert_eq!(spans(&d1), vec![(6 * S, 7 * S)]);
    let d2 = apply(
        &d1,
        &EditOp::Trim {
            clip_id: id.clone(),
            edge: Edge::End,
            rec_ns: 10 * S,
        },
        LIM,
    )
    .expect("trim end out");
    assert_eq!(ranges(&d2)[0], (3 * S, 10 * S));
    assert_eq!(spans(&d2), vec![(8 * S, 9 * S)]);
    let d3 = apply(
        &d2,
        &EditOp::Trim {
            clip_id: id,
            edge: Edge::Start,
            rec_ns: -5 * S,
        },
        LIM,
    )
    .expect("clamp");
    assert_eq!(ranges(&d3)[0], (0, 10 * S));
    let past_end = apply(
        &d3,
        &EditOp::Trim {
            clip_id: d3.edl.clips[1].id.clone(),
            edge: Edge::End,
            rec_ns: 99 * S,
        },
        LIM,
    )
    .expect("clamp end");
    assert_eq!(ranges(&past_end)[1], (20 * S, LIM.rec_len_ns));
}

#[test]
fn speed_changes_length_and_retimes_text() {
    let mut d = doc(vec![clip(0, 4 * S), clip(4 * S, 8 * S)]);
    d.text = vec![
        TextItem::new(2 * S, 3 * S, "in"),
        TextItem::new(6 * S, 7 * S, "after"),
    ];
    let d1 = apply(
        &d,
        &EditOp::SetSpeed {
            ids: ids(&d, &[0]),
            speed: Speed { num: 2, den: 1 },
        },
        LIM,
    )
    .expect("speed");
    assert_eq!(d1.duration(), 6 * S);
    assert_eq!(spans(&d1), vec![(S, S + S / 2), (4 * S, 5 * S)]);
    let half = apply(
        &d,
        &EditOp::SetSpeed {
            ids: ids(&d, &[1]),
            speed: Speed { num: 1, den: 2 },
        },
        LIM,
    )
    .expect("slow");
    assert_eq!(half.duration(), 12 * S);
    assert_eq!(locate(&half.edl, 6 * S).map(|l| l.rec_ns), Some(5 * S));
    assert!(
        apply(
            &d,
            &EditOp::SetSpeed {
                ids: ids(&d, &[1]),
                speed: Speed { num: 0, den: 1 },
            },
            LIM
        )
        .is_err()
    );
}

#[test]
fn camera_keys_are_set_deleted_and_flattened() {
    let d = doc(vec![clip(0, 10 * S)]);
    let mut k = CameraKey::default_placement();
    k.t_rec_ns = 5 * S;
    k.size_norm = 0.5;
    let d1 = apply(
        &d,
        &EditOp::SetCameraKey {
            overlay_id: "cam".into(),
            key: k.clone(),
        },
        LIM,
    )
    .expect("key");
    assert_eq!(d1.camera_layout.overlays[0].keyframes.len(), 2);
    let d2 = apply(
        &d1,
        &EditOp::DeleteCameraKey {
            overlay_id: "cam".into(),
            t_rec_ns: 5 * S,
        },
        LIM,
    )
    .expect("delete key");
    assert_eq!(d2.camera_layout.overlays[0].keyframes.len(), 1);
    let flat = apply(
        &d1,
        &EditOp::SetCameraStatic {
            overlay_id: "cam".into(),
            key: k,
        },
        LIM,
    )
    .expect("static");
    let keys = &flat.camera_layout.overlays[0].keyframes;
    assert_eq!(
        (keys.len(), keys[0].t_rec_ns, keys[0].size_norm),
        (1, 0, 0.5)
    );
    let last = apply(
        &flat,
        &EditOp::DeleteCameraKey {
            overlay_id: "cam".into(),
            t_rec_ns: 0,
        },
        LIM,
    )
    .expect("keeps one");
    assert_eq!(last.camera_layout.overlays[0].keyframes.len(), 1);
}

#[test]
fn history_undoes_redoes_and_coalesces() {
    let d = doc(vec![clip(0, 10 * S)]);
    let mut h = History::new(d.clone());
    assert!(
        h.apply(
            &EditOp::Split {
                at_tl_ns: 5 * S,
                track: None
            },
            LIM,
            None
        )
        .expect("split")
    );
    let after_split = h.doc().clone();
    for g in [-6.0, -3.0, -1.0] {
        h.apply(
            &EditOp::SetTrackMix {
                track_id: "mic".into(),
                gain_db: g,
                muted: false,
            },
            LIM,
            Some("mic-gain"),
        )
        .expect("gain");
    }
    assert_eq!(h.doc().audio_mix["mic"].gain_db, -1.0);
    assert!(h.undo());
    assert_eq!(h.doc(), &after_split);
    assert!(h.undo());
    assert_eq!(h.doc(), &d);
    assert!(!h.undo());
    assert!(h.redo());
    assert!(h.redo());
    assert_eq!(h.doc().audio_mix["mic"].gain_db, -1.0);
    assert!(!h.redo());
    assert!(
        !h.apply(
            &EditOp::Split {
                at_tl_ns: 0,
                track: None
            },
            LIM,
            None
        )
        .expect("no-op")
    );
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    fn time(&mut self, d: &Doc) -> i64 {
        self.below(d.duration().max(1) as u64) as i64
    }
}

fn random_op(r: &mut Rng, d: &Doc, clipboard: &Clipboard) -> EditOp {
    let n = d.edl.clips.len().max(1) as u64;
    let some_id = |r: &mut Rng| {
        d.edl.clips[r.below(n) as usize % d.edl.clips.len().max(1)]
            .id
            .clone()
    };
    match r.below(11) {
        0 | 1 => EditOp::Split {
            at_tl_ns: r.time(d),
            track: None,
        },
        2 => {
            let a = r.time(d);
            EditOp::DeleteRange {
                a_tl_ns: a,
                b_tl_ns: a + r.below(3 * S as u64) as i64,
            }
        }
        3 if d.edl.clips.len() > 1 => EditOp::DeleteClips {
            ids: vec![some_id(r)],
        },
        4 => EditOp::Paste {
            at_tl_ns: r.time(d),
            clipboard: clipboard.clone(),
        },
        5 => EditOp::Duplicate {
            ids: vec![some_id(r)],
        },
        6 => EditOp::MoveClip {
            id: some_id(r),
            to_index: r.below(n + 1) as usize,
        },
        7 => EditOp::SetSpeed {
            ids: vec![some_id(r)],
            speed: SPEEDS[r.below(SPEEDS.len() as u64) as usize],
        },
        8 => EditOp::Trim {
            clip_id: some_id(r),
            edge: if r.below(2) == 0 {
                Edge::Start
            } else {
                Edge::End
            },
            rec_ns: r.below(LIM.rec_len_ns as u64) as i64,
        },
        9 => EditOp::SetMuted {
            ids: vec![some_id(r)],
            muted: r.below(2) == 0,
        },
        _ => {
            let s = r.time(d);
            EditOp::UpsertText {
                item: TextItem::new(s, s + S, "t"),
            }
        }
    }
}

fn check_invariants(d: &Doc) {
    for c in &d.edl.clips {
        assert!(c.src_in_rec_ns >= 0, "{c:?}");
        assert!(c.src_out_rec_ns <= LIM.rec_len_ns, "{c:?}");
        assert!(c.tl_len() > 0, "{c:?}");
        assert!(c.speed.num > 0 && c.speed.den > 0);
    }
    let ids: std::collections::HashSet<_> = d.edl.clips.iter().map(|c| &c.id).collect();
    assert_eq!(ids.len(), d.edl.clips.len(), "clip ids are unique");
    for t in &d.text {
        assert!(t.end_tl_ns - t.start_tl_ns >= MIN_TEXT_NS, "{t:?}");
    }
    let st = starts(&d.edl);
    for w in st.windows(2) {
        assert!(w[0] <= w[1]);
    }
}

#[test]
fn random_edits_keep_invariants_and_undo_back_to_every_state() {
    for seed in 1..=40u64 {
        let mut r = Rng(0x9E37_79B9_7F4A_7C15 ^ seed);
        let start = doc(vec![clip(0, 30 * S)]);
        let mut h = History::new(start.clone());
        let mut seen = vec![start];
        let mut cb = copy_range(h.doc(), 0, 2 * S);
        for _ in 0..60 {
            if r.below(5) == 0 && !h.doc().edl.clips.is_empty() {
                let a = r.time(h.doc());
                cb = copy_range(h.doc(), a, a + S);
            }
            let op = random_op(&mut r, h.doc(), &cb);
            if let Ok(true) = h.apply(&op, LIM, None) {
                check_invariants(h.doc());
                seen.push(h.doc().clone());
            }
        }
        for want in seen.iter().rev().skip(1) {
            assert!(h.undo());
            assert_eq!(h.doc(), want);
        }
        assert!(!h.undo());
        for want in seen.iter().skip(1) {
            assert!(h.redo());
            assert_eq!(h.doc(), want);
        }
    }
}

#[test]
fn every_timeline_instant_maps_inside_its_clip() {
    let mut r = Rng(7);
    let mut d = doc(vec![clip(0, 30 * S)]);
    let cb = copy_range(&d, 0, 3 * S);
    for _ in 0..40 {
        let op = random_op(&mut r, &d, &cb);
        if let Ok(next) = apply(&d, &op, LIM) {
            d = next;
        }
    }
    let st = starts(&d.edl);
    for k in 0..500 {
        let t = d.duration() * k / 500;
        let l = locate_in(&d.edl, &st, t).expect("inside");
        let c = &d.edl.clips[l.index];
        assert!(l.clip_start_tl <= t && t < l.clip_end_tl);
        assert!(c.src_in_rec_ns <= l.rec_ns && l.rec_ns <= c.src_out_rec_ns);
    }
}

#[test]
fn pauses_become_cuts() {
    let s = 1_000_000_000;
    let edl = edl_between_pauses(
        10 * s,
        &[(2 * s, 3 * s), (6 * s, 6 * s + 5_000_000), (9 * s, 12 * s)],
    );
    let spans: Vec<(i64, i64)> = edl
        .clips
        .iter()
        .map(|c| (c.src_in_rec_ns, c.src_out_rec_ns))
        .collect();
    assert_eq!(
        spans,
        vec![(0, 2 * s), (3 * s, 6 * s), (6 * s + 5_000_000, 9 * s)]
    );
    assert_eq!(duration(&edl), 8 * s - 5_000_000);
    assert_eq!(edl_between_pauses(4 * s, &[]).clips.len(), 1);
}

#[test]
fn delete_ranges_removes_several_spans_as_one_edit() {
    let mut d = doc(vec![clip(0, 10 * S)]);
    d.text = vec![TextItem::new(8 * S, 9 * S, "late")];
    let op = EditOp::DeleteRanges {
        ranges: vec![
            (6 * S, 7 * S),
            (S, 2 * S),
            (3 * S, 5 * S),
            (4 * S, 5 * S + S / 2),
        ],
    };
    let d1 = apply(&d, &op, LIM).expect("delete");
    assert_eq!(
        ranges(&d1),
        vec![(0, S), (2 * S, 3 * S), (11 * S / 2, 6 * S), (7 * S, 10 * S)]
    );
    assert_eq!(d1.duration(), 5 * S + S / 2);
    assert_eq!(spans(&d1), vec![(7 * S / 2, 9 * S / 2)]);
    let all = EditOp::DeleteRanges {
        ranges: vec![(0, 5 * S), (5 * S, 10 * S)],
    };
    assert!(apply(&d, &all, LIM).is_err());
}

fn blur(id: &str, a: i64, b: i64) -> crate::project::Redaction {
    crate::project::Redaction {
        id: id.into(),
        start_rec_ns: a,
        end_rec_ns: b,
        rect_norm: crate::project::RectNorm {
            x: 0.1,
            y: 0.1,
            w: 0.2,
            h: 0.1,
        },
        style: crate::project::RedactStyle::Pixelate,
        extra: Default::default(),
    }
}

#[test]
fn blur_boxes_keep_their_recording_time_through_edits() {
    let d = doc(vec![clip(0, 10 * S)]);
    let d = apply(
        &d,
        &EditOp::UpsertRedaction {
            item: blur("b", 6 * S, 8 * S),
        },
        LIM,
    )
    .expect("add");
    let ops = [
        EditOp::DeleteRange {
            a_tl_ns: S,
            b_tl_ns: 3 * S,
        },
        EditOp::Split {
            at_tl_ns: 5 * S,
            track: None,
        },
    ];
    let mut e = d.clone();
    for op in &ops {
        e = apply(&e, op, LIM).expect("edit");
    }
    let id = e.edl.clips[1].id.clone();
    let e = apply(&e, &EditOp::MoveClip { id, to_index: 0 }, LIM).expect("move");
    assert_eq!(
        (e.redactions[0].start_rec_ns, e.redactions[0].end_rec_ns),
        (6 * S, 8 * S)
    );
    let gone = apply(
        &e,
        &EditOp::DeleteRedactions {
            ids: vec!["b".into()],
        },
        LIM,
    )
    .expect("remove");
    assert!(gone.redactions.is_empty());
}

#[test]
fn blur_boxes_are_checked_and_capped_at_eight_at_once() {
    let d = doc(vec![clip(0, 10 * S)]);
    let mut flat = blur("f", 0, S);
    flat.rect_norm.w = 0.0;
    assert!(apply(&d, &EditOp::UpsertRedaction { item: flat }, LIM).is_err());
    assert!(
        apply(
            &d,
            &EditOp::UpsertRedaction {
                item: blur("s", S, S + S / 20)
            },
            LIM
        )
        .is_err()
    );
    let mut wide = blur("w", -S, 90 * S);
    wide.rect_norm = crate::project::RectNorm {
        x: 0.8,
        y: -0.5,
        w: 5.0,
        h: 0.9,
    };
    let c = apply(&d, &EditOp::UpsertRedaction { item: wide }, LIM).expect("clamped");
    let r = &c.redactions[0];
    assert_eq!((r.start_rec_ns, r.end_rec_ns), (0, 60 * S));
    assert!((r.rect_norm.x - 0.8).abs() < 1e-9 && (r.rect_norm.w - 0.2).abs() < 1e-9);
    assert_eq!(r.rect_norm.y, 0.0);
    let mut e = d.clone();
    for k in 0..8 {
        e = apply(
            &e,
            &EditOp::UpsertRedaction {
                item: blur(&format!("b{k}"), k * S / 10, 5 * S),
            },
            LIM,
        )
        .expect("fits");
    }
    assert!(
        apply(
            &e,
            &EditOp::UpsertRedaction {
                item: blur("ninth", 2 * S, 3 * S)
            },
            LIM
        )
        .is_err()
    );
    assert!(
        apply(
            &e,
            &EditOp::UpsertRedaction {
                item: blur("later", 5 * S, 6 * S)
            },
            LIM
        )
        .is_ok()
    );
    let moved = blur("b0", 0, 4 * S);
    assert!(
        apply(&e, &EditOp::UpsertRedaction { item: moved }, LIM).is_ok(),
        "editing one of the eight"
    );
}

fn shown(edl: &Edl) -> Vec<(i64, i64, i64)> {
    let st = starts(edl);
    edl.clips
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.hidden)
        .map(|(i, c)| (st[i], st[i + 1], c.src_in_rec_ns))
        .collect()
}

fn unlinked() -> Doc {
    let d = doc(vec![clip(0, 4 * S), clip(4 * S, 10 * S)]);
    apply(&d, &EditOp::SetLinked { linked: false }, LIM).expect("unlink")
}

fn cam_ids(d: &Doc) -> Vec<String> {
    d.camera_clips()
        .clips
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

#[test]
fn unlinking_gives_the_camera_its_own_copy_and_linking_drops_it() {
    let d = unlinked();
    let cam = d.camera_edl.as_ref().expect("camera clips");
    assert_eq!(shown(cam), shown(&d.edl));
    assert!(
        cam.clips
            .iter()
            .all(|c| d.edl.clips.iter().all(|s| s.id != c.id))
    );
    let back = apply(&d, &EditOp::SetLinked { linked: true }, LIM).expect("link");
    assert!(back.linked());
    assert_eq!(ranges(&back), ranges(&d));
}

#[test]
fn unlinking_needs_a_camera() {
    let mut d = doc(vec![clip(0, 4 * S)]);
    d.camera_layout.overlays.clear();
    assert!(apply(&d, &EditOp::SetLinked { linked: false }, LIM).is_err());
}

#[test]
fn deleting_a_camera_clip_leaves_a_gap_and_the_screen_alone() {
    let d = unlinked();
    let ids = vec![cam_ids(&d)[0].clone()];
    let d1 = apply(&d, &EditOp::DeleteClips { ids }, LIM).expect("lift");
    assert_eq!(shown(&d1.edl), shown(&d.edl));
    assert_eq!(shown(d1.camera_clips()), vec![(4 * S, 10 * S, 4 * S)]);
    assert_eq!(d1.duration(), 10 * S);
    let gap = vec![cam_ids(&d1)[0].clone()];
    let d2 = apply(&d1, &EditOp::DeleteClips { ids: gap }, LIM).expect("close");
    assert_eq!(shown(d2.camera_clips()), vec![(0, 6 * S, 4 * S)]);
    assert_eq!(shown(&d2.edl), shown(&d.edl));
}

#[test]
fn a_screen_gap_deleted_takes_its_time_out_of_every_track() {
    let mut d = unlinked();
    d.text.push(TextItem::new(6 * S, 7 * S, "t"));
    let d1 = apply(&d, &EditOp::DeleteClips { ids: ids(&d, &[0]) }, LIM).expect("lift");
    assert_eq!(shown(&d1.edl), vec![(4 * S, 10 * S, 4 * S)]);
    assert_eq!(spans(&d1), vec![(6 * S, 7 * S)]);
    let d2 = apply(
        &d1,
        &EditOp::DeleteClips {
            ids: ids(&d1, &[0]),
        },
        LIM,
    )
    .expect("close");
    assert_eq!(shown(&d2.edl), vec![(0, 6 * S, 4 * S)]);
    assert_eq!(shown(d2.camera_clips()), vec![(0, 6 * S, 4 * S)]);
    assert_eq!(spans(&d2), vec![(2 * S, 3 * S)]);
}

#[test]
fn sliding_stops_at_the_neighbours() {
    let d = unlinked();
    let ids = cam_ids(&d);
    let lifted = apply(
        &d,
        &EditOp::DeleteClips {
            ids: vec![ids[0].clone()],
        },
        LIM,
    )
    .expect("lift");
    let later = cam_ids(&lifted)[1].clone();
    let back = apply(
        &lifted,
        &EditOp::SlideClip {
            id: later.clone(),
            by_tl_ns: -1_000 * S,
        },
        LIM,
    )
    .expect("slide");
    assert_eq!(shown(back.camera_clips()), vec![(0, 6 * S, 4 * S)]);
    let fwd = apply(
        &back,
        &EditOp::SlideClip {
            id: later,
            by_tl_ns: 3 * S,
        },
        LIM,
    )
    .expect("slide");
    assert_eq!(shown(fwd.camera_clips()), vec![(3 * S, 9 * S, 4 * S)]);
    assert_eq!(shown(&fwd.edl), shown(&d.edl));
    assert!(
        apply(
            &d,
            &EditOp::SlideClip {
                id: ids[0].clone(),
                by_tl_ns: S
            },
            LIM
        )
        .map(|x| shown(x.camera_clips()))
        .expect("blocked")
            == shown(d.camera_clips())
    );
}

#[test]
fn trimming_an_unlinked_clip_moves_nothing_else() {
    let d = unlinked();
    let first = cam_ids(&d)[0].clone();
    let d1 = apply(
        &d,
        &EditOp::Trim {
            clip_id: first.clone(),
            edge: Edge::End,
            rec_ns: 3 * S,
        },
        LIM,
    )
    .expect("trim");
    assert_eq!(
        shown(d1.camera_clips()),
        vec![(0, 3 * S, 0), (4 * S, 10 * S, 4 * S)]
    );
    let d2 = apply(
        &d1,
        &EditOp::Trim {
            clip_id: first,
            edge: Edge::End,
            rec_ns: 9 * S,
        },
        LIM,
    )
    .expect("trim");
    assert_eq!(shown(d2.camera_clips()), shown(d.camera_clips()));
    assert_eq!(shown(&d2.edl), shown(&d.edl));
}

#[test]
fn a_range_delete_cuts_both_tracks() {
    let d = unlinked();
    let d1 = apply(
        &d,
        &EditOp::DeleteRange {
            a_tl_ns: 2 * S,
            b_tl_ns: 5 * S,
        },
        LIM,
    )
    .expect("cut");
    assert_eq!(shown(&d1.edl), vec![(0, 2 * S, 0), (2 * S, 7 * S, 5 * S)]);
    assert_eq!(shown(d1.camera_clips()), shown(&d1.edl));
}

#[test]
fn splitting_one_track_leaves_the_other() {
    let d = unlinked();
    let d1 = apply(
        &d,
        &EditOp::Split {
            at_tl_ns: 2 * S,
            track: Some(Track::Camera),
        },
        LIM,
    )
    .expect("split");
    assert_eq!(d1.camera_clips().clips.len(), 3);
    assert_eq!(d1.edl.clips.len(), 2);
}

#[test]
fn linking_closes_the_screen_gaps() {
    let d = unlinked();
    let d1 = apply(&d, &EditOp::DeleteClips { ids: ids(&d, &[0]) }, LIM).expect("lift");
    let d2 = apply(&d1, &EditOp::SetLinked { linked: true }, LIM).expect("link");
    assert!(d2.linked());
    assert_eq!(ranges(&d2), vec![(4 * S, 10 * S)]);
}

#[test]
fn pasting_camera_clips_keeps_the_screen_in_step_with_a_gap() {
    let d = unlinked();
    let cb = copy_clips(&d, &[cam_ids(&d)[0].clone()]).expect("copy");
    assert_eq!(cb.track, Some(Track::Camera));
    let d1 = apply(
        &d,
        &EditOp::Paste {
            at_tl_ns: 4 * S,
            clipboard: cb,
        },
        LIM,
    )
    .expect("paste");
    assert_eq!(
        shown(d1.camera_clips()),
        vec![(0, 4 * S, 0), (4 * S, 8 * S, 0), (8 * S, 14 * S, 4 * S)]
    );
    assert_eq!(shown(&d1.edl), vec![(0, 4 * S, 0), (8 * S, 14 * S, 4 * S)]);
}

#[test]
fn unlinked_projects_round_trip_and_old_ones_still_read() {
    let u = unlinked();
    let d = apply(&u, &EditOp::DeleteClips { ids: ids(&u, &[0]) }, LIM).expect("lift");
    let json = serde_json::to_string(&d).expect("ser");
    assert_eq!(serde_json::from_str::<Doc>(&json).expect("de"), d);
    let linked = doc(vec![clip(0, S)]);
    let old = serde_json::to_value(&linked).expect("ser");
    assert!(
        old.get("camera_edl")
            .is_some_and(serde_json::Value::is_null)
    );
    assert!(old["edl"]["clips"][0].get("hidden").is_none());
}

#[test]
fn a_section_gets_its_own_framing_and_can_go_back_to_the_default() {
    use crate::project::{Fit, SegmentFraming};
    let d = doc(vec![clip(0, 4 * S)]);
    let set = |d: &Doc, framing: Option<SegmentFraming>| {
        apply(
            d,
            &EditOp::SetSegmentFraming {
                segment_id: "s1".into(),
                framing,
            },
            LIM,
        )
        .expect("framing")
    };
    let fill = SegmentFraming {
        fit: Fit::Cover,
        focus_norm: sc_geometry::PointF {
            x: 7.0,
            y: f64::NAN,
        },
        extra: Extra::new(),
    };
    let d1 = set(&d, Some(fill));
    let f = &d1.output.segment_framing.as_ref().expect("map")["s1"];
    assert_eq!(
        (f.fit, f.focus_norm.x, f.focus_norm.y),
        (Fit::Cover, 1.0, 0.5)
    );
    let d2 = set(&d1, None);
    assert!(d2.output.segment_framing.is_none());
}
