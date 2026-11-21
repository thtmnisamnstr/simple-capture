mod common;

use std::sync::Arc;

use common::{MS, TempDir, project, write_audio};
use sc_decode::{Error, TrackAudio};
use sc_platform::synthetic::SctrkDecoder;

fn ramp(n: usize) -> Vec<f32> {
    (0..n).map(|i| (i + 1) as f32 / 10_000.0).collect()
}

fn read_all(a: &mut TrackAudio, channels: usize, frames: usize, chunk: usize) -> Vec<f32> {
    let mut out = Vec::new();
    while out.len() < frames * channels {
        let want = chunk.min(frames - out.len() / channels);
        let mut buf = vec![f32::NAN; want * channels];
        assert_eq!(a.read(&mut buf).expect("read"), want);
        out.extend_from_slice(&buf);
    }
    out
}

fn open(offset_ns: i64, rate: u32, channels: u16) -> (TempDir, TrackAudio) {
    let dir = TempDir::new("audio");
    let mut p = project();
    write_audio(dir.path(), &mut p, "mic", 48_000, 1, offset_ns, &ramp(1000));
    let a = TrackAudio::open(
        &p,
        dir.path(),
        "mic",
        Arc::new(SctrkDecoder),
        rate,
        channels,
    )
    .expect("open");
    (dir, a)
}

#[test]
fn offset_is_padded_with_silence_and_the_end_stays_silent() {
    let (_dir, mut a) = open(10 * MS, 48_000, 1);
    let out = read_all(&mut a, 1, 3000, 333);
    assert!(out[..480].iter().all(|&s| s == 0.0));
    assert_eq!(&out[480..1480], ramp(1000).as_slice());
    assert!(out[1480..].iter().all(|&s| s == 0.0));
    let mut tail = vec![f32::NAN; 4096];
    for _ in 0..3 {
        assert_eq!(a.read(&mut tail).unwrap(), 4096);
        assert!(tail.iter().all(|&s| s == 0.0));
    }
}

#[test]
fn offset_rounds_to_the_nearest_frame() {
    let (_dir, mut a) = open(10 * MS + 10_400, 48_000, 1);
    let out = read_all(&mut a, 1, 600, 600);
    assert!(out[..480].iter().all(|&s| s == 0.0));
    assert_eq!(out[480], 1.0 / 10_000.0);
    let (_dir, mut a) = open(10 * MS + 10_500, 48_000, 1);
    let out = read_all(&mut a, 1, 600, 600);
    assert!(out[..481].iter().all(|&s| s == 0.0));
    assert_eq!(out[481], 1.0 / 10_000.0);
}

#[test]
fn zero_offset_starts_immediately() {
    let (_dir, mut a) = open(0, 48_000, 1);
    let out = read_all(&mut a, 1, 1200, 1);
    assert_eq!(&out[..1000], ramp(1000).as_slice());
    assert!(out[1000..].iter().all(|&s| s == 0.0));
}

#[test]
fn negative_offset_skips_leading_samples() {
    let (_dir, mut a) = open(-5 * MS, 48_000, 1);
    let out = read_all(&mut a, 1, 1000, 77);
    assert_eq!(&out[..760], &ramp(1000)[240..]);
    assert!(out[760..].iter().all(|&s| s == 0.0));
}

#[test]
fn stereo_output_counts_frames() {
    let (_dir, mut a) = open(10 * MS, 48_000, 2);
    let mut odd = vec![f32::NAN; 7];
    assert_eq!(a.read(&mut odd).unwrap(), 3);
    assert!(odd[..6].iter().all(|&s| s == 0.0));
    assert!(odd[6].is_nan());
    let out = read_all(&mut a, 2, 2000, 250);
    let first = 480 - 3;
    assert!(out[..first * 2].iter().all(|&s| s == 0.0));
    let r = ramp(1000);
    for i in 0..1000 {
        assert_eq!(out[(first + i) * 2], r[i]);
        assert_eq!(out[(first + i) * 2 + 1], r[i]);
    }
    assert!(out[(first + 1000) * 2..].iter().all(|&s| s == 0.0));
}

#[test]
fn resampled_offset_is_in_output_frames() {
    let (_dir, mut a) = open(10 * MS, 24_000, 1);
    let out = read_all(&mut a, 1, 1000, 100);
    assert!(out[..240].iter().all(|&s| s == 0.0));
    assert_eq!(out[240], 1.0 / 10_000.0);
    assert!(out[240..740].iter().all(|&s| s > 0.0));
    assert!(out[740..].iter().all(|&s| s == 0.0));
}

#[test]
fn bad_requests_fail_with_context() {
    let dir = TempDir::new("audio-bad");
    let mut p = project();
    write_audio(dir.path(), &mut p, "mic", 48_000, 1, 0, &ramp(10));
    let d = Arc::new(SctrkDecoder);
    match TrackAudio::open(&p, dir.path(), "system_audio", d.clone(), 48_000, 2) {
        Err(Error::NotFound(m)) => assert!(m.contains("system_audio"), "{m}"),
        Err(e) => panic!("unexpected error {e}"),
        Ok(_) => panic!("missing track accepted"),
    }
    assert!(TrackAudio::open(&p, dir.path(), "mic", d.clone(), 0, 2).is_err());
    assert!(TrackAudio::open(&p, dir.path(), "mic", d.clone(), 48_000, 0).is_err());
    std::fs::remove_file(dir.path().join("tracks/mic.sctrk")).unwrap();
    let e = TrackAudio::open(&p, dir.path(), "mic", d, 48_000, 2)
        .err()
        .expect("missing file");
    assert!(e.to_string().contains("mic"), "{e}");
}

#[test]
fn opening_at_a_time_starts_with_the_sample_at_that_time() {
    let dir = TempDir::new("audio-at");
    let mut p = project();
    write_audio(dir.path(), &mut p, "mic", 48_000, 1, 10 * MS, &ramp(1000));
    let at = |from_ms: i64| {
        let mut a = TrackAudio::open_at(
            &p,
            dir.path(),
            "mic",
            Arc::new(SctrkDecoder),
            48_000,
            1,
            from_ms * MS,
        )
        .expect("open_at");
        read_all(&mut a, 1, 200, 64)
    };
    assert_eq!(&at(15)[..100], &ramp(1000)[240..340]);
    let early = at(9);
    assert!(early[..48].iter().all(|&s| s == 0.0));
    assert_eq!(&early[48..148], &ramp(1000)[..100]);
    assert!(at(40).iter().all(|&s| s == 0.0));
}
