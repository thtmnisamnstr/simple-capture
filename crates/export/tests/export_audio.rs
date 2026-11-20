//! The export mix: sample budget, 0 dB sum, mono upmix, clamp, offsets.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::*;
use sc_core::project::TrackKind;
use sc_export::audio::{mix_samples, mix_to_wav};
use sc_platform::synthetic::SctrkDecoder;

fn read_wav_f32(path: &Path) -> (u16, u32, Vec<f32>) {
    let b = std::fs::read(path).expect("read wav");
    assert_eq!(&b[0..4], b"RIFF");
    let mut i = 12;
    let (mut ch, mut rate) = (0u16, 0u32);
    loop {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = &b[i + 8..i + 8 + len];
        if id == b"fmt " {
            assert_eq!(u16::from_le_bytes([body[0], body[1]]), 3, "IEEE float");
            ch = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
        }
        if id == b"data" {
            let s = body
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            return (ch, rate, s);
        }
        i += 8 + len + (len & 1);
    }
}

#[test]
fn sample_budget() {
    assert_eq!(mix_samples(30, 30), 48_000);
    assert_eq!(mix_samples(24, 25), 50_000);
    assert_eq!(mix_samples(60, 7), 5_600);
    assert_eq!(mix_samples(29, 10), 16_551);
    for n in 0..200u64 {
        assert_eq!(mix_samples(30, n + 1) - mix_samples(30, n), 1600);
    }
}

#[test]
fn mix_sums_upmixes_clamps_and_offsets() {
    let t = TempDir::new("mix");
    let mut p = new_project(t.path(), "Mix", 2000);
    p.segment(Seg::new(0, 2000, 64, 36));
    let mono = vec![0.8f32; 48_000];
    let stereo: Vec<f32> = (0..96_000)
        .map(|i| if i % 2 == 0 { 0.5 } else { -0.25 })
        .collect();
    p.audio("mic", TrackKind::Mic, 48_000, 1, 100, &mono);
    p.audio("sys", TrackKind::SystemAudio, 48_000, 2, 0, &stereo);
    let dir = p.save();
    for (fps, frames) in [(24u32, 25u64), (30, 31), (29, 17)] {
        let wav = t.path().join(format!("mix-{fps}.wav"));
        let had =
            mix_to_wav(&p.project, &dir, Arc::new(SctrkDecoder), fps, frames, &wav).expect("mix");
        assert!(had);
        let (ch, rate, s) = read_wav_f32(&wav);
        assert_eq!((ch, rate), (2, 48_000));
        assert_eq!(
            s.len() as u64,
            2 * (frames * 48_000 / u64::from(fps)),
            "fps {fps}"
        );
        assert_eq!(s.len() as u64 / 2, mix_samples(fps, frames));
        let at = |n: usize| (s[2 * n], s[2 * n + 1]);
        assert_eq!(at(0), (0.5, -0.25));
        assert_eq!(at(4_799), (0.5, -0.25));
        let (l, r) = at(4_801);
        assert_eq!(l, 1.0, "0.8 + 0.5 clamps to 1");
        assert!(
            (r - 0.55).abs() < 1e-6,
            "mono upmixed to both channels: {r}"
        );
        if s.len() / 2 > 52_000 {
            assert_eq!(at(49_000), (0.8, 0.8), "after the system track ends");
            assert_eq!(at(s.len() / 2 - 1), (0.0, 0.0));
        }
    }
}

#[test]
fn no_audio_tracks_means_no_wav() {
    let t = TempDir::new("noaudio");
    let mut p = new_project(t.path(), "Quiet", 500);
    p.segment(Seg::new(0, 500, 64, 36));
    let dir = p.save();
    let wav = t.path().join("none.wav");
    let had = mix_to_wav(&p.project, &dir, Arc::new(SctrkDecoder), 30, 15, &wav).expect("mix");
    assert!(!had);
    assert!(!wav.exists());
}
