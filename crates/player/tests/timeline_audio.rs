#[path = "../../decode/tests/common/mod.rs"]
mod common;

use std::sync::Arc;

use common::{MS, TempDir, project, write_audio};
use sc_core::edit::Doc;
use sc_core::project::{AudioMixEntry, Clip, Extra, ProjectV1, Speed};
use sc_platform::synthetic::SctrkDecoder;
use sc_player::audio::TimelineAudio;

const RATE: u32 = 48_000;

fn clip(a_ms: i64, b_ms: i64) -> Clip {
    Clip {
        id: sc_core::ids::new_id(),
        src_in_rec_ns: a_ms * MS,
        src_out_rec_ns: b_ms * MS,
        speed: Speed::ONE,
        muted: false,
        hidden: false,
        extra: Extra::new(),
    }
}

/// Sample `i` of the mic track is `i / 1e6`: the value says where it came from.
fn setup(len_ms: i64) -> (TempDir, ProjectV1) {
    let dir = TempDir::new("tl-audio");
    let mut p = project();
    let n = (len_ms * 48) as usize;
    let ramp: Vec<f32> = (0..n).map(|i| i as f32 / 1e6).collect();
    write_audio(dir.path(), &mut p, "mic", RATE, 1, 0, &ramp);
    (dir, p)
}

fn read(a: &mut TimelineAudio, frames: usize) -> Vec<f32> {
    let mut out = vec![f32::NAN; frames * 2];
    a.read(&mut out).expect("read");
    out.chunks_exact(2).map(|f| f[0]).collect()
}

fn timeline(dir: &TempDir, p: &ProjectV1, clips: Vec<Clip>) -> TimelineAudio {
    let mut doc = Doc::of(p);
    doc.edl.clips = clips;
    TimelineAudio::new(
        Arc::new(p.clone()),
        dir.path(),
        Arc::new(SctrkDecoder),
        doc,
        RATE,
    )
}

fn src_index(v: f32) -> i64 {
    (f64::from(v) * 1e6).round() as i64
}

#[test]
fn clips_play_their_source_ranges_in_order() {
    let (dir, p) = setup(1000);
    let mut a = timeline(&dir, &p, vec![clip(500, 600), clip(100, 200)]);
    let out = read(&mut a, 9600);
    assert_eq!(src_index(out[1000]), 500 * 48 + 1000);
    assert_eq!(src_index(out[4800 + 1000]), 100 * 48 + 1000);
    assert!(out[4799].abs() < out[4700].abs(), "fade out before the cut");
    assert_eq!(out[4800 - 120..4800 + 120].len(), 240);
    let tail = read(&mut a, 100);
    assert!(tail.iter().all(|s| *s == 0.0), "silence past the end");
}

#[test]
fn contiguous_clips_have_no_fade() {
    let (dir, p) = setup(1000);
    let mut a = timeline(&dir, &p, vec![clip(100, 200), clip(200, 300)]);
    let out = read(&mut a, 9600);
    for i in [4790, 4799, 4800, 4810] {
        assert_eq!(src_index(out[i]), 100 * 48 + i as i64, "sample {i}");
    }
}

#[test]
fn seeking_lands_on_the_right_sample() {
    let (dir, p) = setup(1000);
    let mut a = timeline(&dir, &p, vec![clip(500, 600), clip(100, 300)]);
    a.seek(150 * MS);
    let out = read(&mut a, 10);
    assert_eq!(src_index(out[0]), 150 * 48);
}

#[test]
fn gain_mute_and_clip_mute_apply() {
    let (dir, mut p) = setup(500);
    p.audio_mix.insert(
        "mic".into(),
        AudioMixEntry {
            gain_db: -6.0,
            muted: false,
            extra: Extra::new(),
        },
    );
    let mut a = timeline(&dir, &p, vec![clip(100, 200)]);
    let out = read(&mut a, 2000);
    let want = (100 * 48 + 1000) as f32 / 1e6 * 10f32.powf(-6.0 / 20.0);
    assert!((out[1000] - want).abs() < 1e-6);
    let mut muted = clip(100, 200);
    muted.muted = true;
    let mut a = timeline(&dir, &p, vec![muted]);
    assert!(read(&mut a, 2000).iter().all(|s| *s == 0.0));
}

fn tone_with_clicks(len_ms: i64, clicks_ms: &[i64]) -> Vec<f32> {
    let n = (len_ms * 48) as usize;
    let mut v: Vec<f32> = (0..n)
        .map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 300.0 / 48_000.0).sin() * 0.05)
        .collect();
    for c in clicks_ms {
        let i = (*c * 48) as usize;
        for k in 0..48 {
            v[i + k] = 0.9;
        }
    }
    v
}

fn loud_spots(v: &[f32]) -> Vec<i64> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < v.len() {
        if v[i].abs() > 0.4 {
            out.push(i as i64 / 48);
            i += 48 * 50;
        } else {
            i += 1;
        }
    }
    out
}

#[test]
fn double_speed_keeps_events_in_time() {
    let dir = TempDir::new("tl-speed");
    let mut p = project();
    write_audio(
        dir.path(),
        &mut p,
        "mic",
        RATE,
        1,
        0,
        &tone_with_clicks(4000, &[1000, 2000, 3000]),
    );
    let mut fast = clip(0, 4000);
    fast.speed = Speed { num: 2, den: 1 };
    let mut a = timeline(&dir, &p, vec![fast]);
    let out = read(&mut a, 2000 * 48);
    let spots = loud_spots(&out);
    assert_eq!(spots.len(), 3, "{spots:?}");
    for (got, want) in spots.iter().zip([500, 1000, 1500]) {
        assert!(
            (got - want).abs() <= 25,
            "click at {got} ms, want {want} ms ({spots:?})"
        );
    }
}

#[test]
fn an_unlinked_mic_follows_the_camera_clips() {
    let (dir, p) = setup(1000);
    let mut doc = Doc::of(&p);
    doc.edl.clips = vec![clip(100, 200), clip(200, 300)];
    let mut gap = clip(0, 100);
    gap.hidden = true;
    doc.camera_edl = Some(sc_core::project::Edl {
        clips: vec![clip(500, 600), gap],
        extra: Extra::new(),
    });
    let mut a = TimelineAudio::new(
        Arc::new(p.clone()),
        dir.path(),
        Arc::new(SctrkDecoder),
        doc,
        RATE,
    );
    assert_eq!(a.len_frames(), 9600);
    let out = read(&mut a, 9600);
    assert_eq!(src_index(out[1000]), 500 * 48 + 1000);
    assert!(
        out[5000..].iter().all(|s| *s == 0.0),
        "the camera gap is silent"
    );
}
