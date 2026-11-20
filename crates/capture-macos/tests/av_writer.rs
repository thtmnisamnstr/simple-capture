//! Non-TCC AVFoundation tests: writers, readers and probe on synthetic media (plan wp-mac-av).
#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_core::time::HostTime;
use sc_geometry::SizePx;
use sc_platform::synthetic::{FrameCode, coded_frame, read_code};
use sc_platform::{
    Append, AudioBuffer, AudioData, AudioWriterSpec, FrameBuffer, StreamTag, TrackWriter,
    VideoCodec, VideoFrame, VideoWriterSpec,
};

const DEFAULT_BPP: f64 = sc_capture_macos::av::DEFAULT_BITS_PER_PIXEL;
const KILL_CHILD_ENV: &str = "SC_AV_KILL_CHILD_DIR";

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("av_writer")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn allow_no_hw() -> bool {
    std::env::var("SC_ALLOW_NO_HW_ENCODER").ok().as_deref() == Some("1")
}

fn video_spec(size: SizePx, fps: u32) -> VideoWriterSpec {
    VideoWriterSpec {
        size,
        fps,
        codec: VideoCodec::Hevc,
        keyframe_interval_s: 1.0,
        fragment_interval_s: 1.0,
        bits_per_pixel: DEFAULT_BPP,
    }
}

fn audio_spec() -> AudioWriterSpec {
    AudioWriterSpec {
        sample_rate: 48_000,
        channels: 2,
        bitrate: 160_000,
        fragment_interval_s: 1.0,
    }
}

/// `None` (after printing SKIPPED) when the VM has no hardware HEVC encoder and CI allows it.
fn video_writer(path: &Path, spec: &VideoWriterSpec) -> Option<Box<dyn TrackWriter>> {
    match sc_capture_macos::writers().video(path, spec) {
        Ok(w) => Some(w),
        Err(e) if allow_no_hw() => {
            println!("SKIPPED: no usable HEVC encoder (SC_ALLOW_NO_HW_ENCODER=1): {e}");
            None
        }
        Err(e) => panic!("video writer: {e}"),
    }
}

fn append_retry(w: &mut dyn TrackWriter, mut f: impl FnMut(&mut dyn TrackWriter) -> Append) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while f(w) == Append::NotReady {
        assert!(Instant::now() < deadline, "writer stayed NotReady for 10 s");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn frame(size: SizePx, i: u32, file_ns: i64) -> VideoFrame {
    let code = FrameCode {
        source_hash: 0x5ca1_ab1e,
        index: i,
        pts_ms: (file_ns / 1_000_000) as u32,
    };
    VideoFrame {
        stream: StreamTag(1),
        pts: HostTime(file_ns),
        size,
        content_rect_pt: None,
        content_scale: None,
        buffer: FrameBuffer(Arc::new(coded_frame(size, code, false))),
    }
}

fn write_coded_video(path: &Path, size: SizePx, fps: u32, n: u32) -> Option<i64> {
    let mut w = video_writer(path, &video_spec(size, fps))?;
    assert!(path.exists(), "file must exist right after video()");
    let step = 1_000_000_000 / i64::from(fps);
    for i in 0..n {
        let ns = i64::from(i) * step;
        let f = frame(size, i, ns);
        append_retry(w.as_mut(), |w| {
            w.append_video(&f, ns).expect("append_video")
        });
    }
    let fin = w.finish(None).expect("finish");
    assert_eq!(fin.samples, u64::from(n));
    assert_eq!(fin.duration_ns, Some(i64::from(n) * step));
    Some(step)
}

#[test]
fn video_round_trip_keeps_codes_pts_and_size() {
    let dir = tmp("video");
    let path = dir.join("screen.mov");
    let size = SizePx { w: 640, h: 480 };
    let n = 45;
    let Some(step) = write_coded_video(&path, size, 30, n) else {
        return;
    };
    let dec = sc_capture_macos::decoder();
    let mut r = dec.open_video(&path).expect("open_video");
    assert_eq!(r.size(), size);
    let mut i = 0u32;
    let mut worst_pts = 0i64;
    while let Some(d) = r.next_frame().expect("next_frame") {
        let expected = i64::from(i) * step;
        worst_pts = worst_pts.max((d.file_ns - expected).abs());
        assert_eq!(d.frame.0.size(), size);
        let mut code = None;
        d.frame
            .0
            .with_nv12(&mut |v| code = read_code(&v))
            .expect("with_nv12");
        let code = code.unwrap_or_else(|| panic!("frame {i}: code unreadable after HEVC"));
        assert_eq!(code.index, i, "frame order");
        assert_eq!(code.source_hash, 0x5ca1_ab1e);
        i += 1;
    }
    assert_eq!(i, n, "decoded frame count");
    assert!(worst_pts <= 1_000_000, "worst PTS error {worst_pts} ns");
    let p = dec.probe(&path).expect("probe");
    assert!(p.readable, "{p:?}");
    assert_eq!(p.samples, u64::from(n));
    assert_eq!(p.size, Some(size));
    let probed = p.duration_ns.expect("probe duration");
    assert!(
        (probed - i64::from(n) * step).abs() <= 1_000_000,
        "probe duration {probed}"
    );
    println!("video round trip: {n} frames, worst pts error {worst_pts} ns, probe {p:?}");
    let _ = std::fs::remove_dir_all(dir);
}

fn ffprobe() -> Option<PathBuf> {
    let required = std::env::var("SC_REQUIRE_HELPERS").ok().as_deref() == Some("1");
    let found = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|d| d.join("third_party/out/macos-arm64/ffprobe"))
        .find(|p| p.exists());
    if found.is_some() {
        found
    } else {
        assert!(
            !required,
            "SC_REQUIRE_HELPERS=1 but third_party/out/macos-arm64/ffprobe is missing"
        );
        println!("SKIPPED: ffprobe not built (just helpers)");
        None
    }
}

#[test]
fn video_tags_are_hvc1_bt709_limited() {
    let Some(ffprobe) = ffprobe() else {
        return;
    };
    let dir = tmp("tags");
    let path = dir.join("screen.mov");
    if write_coded_video(&path, SizePx { w: 320, h: 240 }, 30, 10).is_none() {
        return;
    }
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,codec_tag_string,color_range,color_space,color_transfer,color_primaries,width,height",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(&path)
        .output()
        .expect("run ffprobe");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    println!("ffprobe:\n{text}");
    for want in [
        "codec_name=hevc",
        "codec_tag_string=hvc1",
        "color_range=tv",
        "color_space=bt709",
        "color_transfer=bt709",
        "color_primaries=bt709",
        "width=320",
        "height=240",
    ] {
        assert!(
            text.lines().any(|l| l == want),
            "missing {want} in:\n{text}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

fn tone(frames: u32, start_frame: u64, hz: f64, channels: u16) -> Arc<[f32]> {
    let ch = usize::from(channels);
    let mut v = Vec::with_capacity(frames as usize * ch);
    for i in 0..u64::from(frames) {
        let t = (start_frame + i) as f64 / 48_000.0;
        let s = (0.4 * (2.0 * std::f64::consts::PI * hz * t).sin()) as f32;
        v.extend(std::iter::repeat_n(s, ch));
    }
    v.into()
}

fn audio_buf(data: Arc<[f32]>, frames: u32, host_ns: i64) -> AudioBuffer {
    AudioBuffer {
        pts: HostTime(host_ns),
        sample_rate: 48_000,
        channels: 2,
        frames,
        data: AudioData::F32Interleaved(data),
    }
}

fn frames_ns(frames: u64) -> i64 {
    (frames as f64 * 1e9 / 48_000.0).round() as i64
}

fn decode_all(path: &Path) -> (i64, Vec<f32>) {
    let mut r = sc_capture_macos::decoder()
        .open_audio(path, 48_000, 2)
        .expect("open_audio");
    let first = r.first_sample_file_ns();
    let mut all = Vec::new();
    let mut buf = vec![0f32; 4096];
    loop {
        let n = r.read(&mut buf).expect("read");
        if n == 0 {
            break;
        }
        all.extend_from_slice(&buf[..n]);
    }
    (first, all)
}

fn rms(s: &[f32]) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    (s.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / s.len() as f64).sqrt()
}

#[test]
fn audio_gap_filled_with_silence_keeps_host_span() {
    let dir = tmp("gap");
    let path = dir.join("mic.mov");
    let mut w = sc_capture_macos::writers()
        .audio(&path, &audio_spec())
        .expect("audio writer");
    assert!(path.exists(), "file must exist right after audio()");
    let buf_frames = 1024u32;
    let mut frame_pos = 0u64;
    let mut host_ns = 0i64;
    for _ in 0..47 {
        let d = tone(buf_frames, frame_pos, 440.0, 2);
        let a = audio_buf(d, buf_frames, host_ns);
        append_retry(w.as_mut(), |w| {
            w.append_audio(&a, host_ns, 0).expect("append_audio")
        });
        frame_pos += u64::from(buf_frames);
        host_ns = frames_ns(frame_pos);
    }
    let gap_start = host_ns;
    let gap_frames = 9600u32;
    append_retry(w.as_mut(), |w| {
        w.append_silence(gap_start, gap_frames).expect("silence")
    });
    frame_pos += u64::from(gap_frames);
    host_ns = frames_ns(frame_pos);
    assert_eq!(host_ns - gap_start, 200_000_000);
    for _ in 0..47 {
        let d = tone(buf_frames, frame_pos, 440.0, 2);
        let a = audio_buf(d, buf_frames, host_ns);
        append_retry(w.as_mut(), |w| {
            w.append_audio(&a, host_ns, 0).expect("append_audio")
        });
        frame_pos += u64::from(buf_frames);
        host_ns = frames_ns(frame_pos);
    }
    let fin = w.finish(None).expect("finish");
    assert_eq!(fin.samples, frame_pos);
    let (first, all) = decode_all(&path);
    let decoded_ns = frames_ns(all.len() as u64 / 2);
    println!(
        "gap test: host span {host_ns} ns, decoded {decoded_ns} ns, first sample {first} ns, finished {fin:?}"
    );
    assert!(first.abs() <= 1_000_000, "first sample at {first} ns");
    assert!(
        (decoded_ns - host_ns).abs() <= 1_000_000,
        "decoded {decoded_ns} vs host span {host_ns}"
    );
    let at = |ns: i64| (ns as f64 * 48_000.0 / 1e9) as usize * 2;
    let gap = &all[at(gap_start + 20_000_000)..at(gap_start + 180_000_000)];
    let before = &all[at(gap_start - 300_000_000)..at(gap_start - 100_000_000)];
    let after = &all[at(gap_start + 300_000_000)..at(gap_start + 500_000_000)];
    println!(
        "rms before {:.3} gap {:.5} after {:.3}",
        rms(before),
        rms(gap),
        rms(after)
    );
    assert!(rms(gap) < 0.01, "gap not silent: {}", rms(gap));
    assert!(
        rms(before) > 0.2 && rms(after) > 0.2,
        "tone missing around the gap"
    );
    let p = sc_capture_macos::decoder().probe(&path).expect("probe");
    assert!(p.readable, "{p:?}");
    let probed = p.duration_ns.expect("probe duration");
    assert!(
        (probed - host_ns).abs() <= 10_000_000,
        "probe {probed} vs host span {host_ns}"
    );
    println!("probe {p:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn skip_frames_trims_exactly() {
    let dir = tmp("skip");
    let path = dir.join("system.mov");
    let mut w = sc_capture_macos::writers()
        .audio(&path, &audio_spec())
        .expect("audio writer");
    let buf_frames = 4800u32;
    let overlap = 480u32;
    let mut next_ns = 0i64;
    let mut kept = 0u64;
    for k in 0..10u32 {
        let skip = if k == 5 { overlap } else { 0 };
        let start_ns = next_ns - frames_ns(u64::from(skip));
        let d = tone(buf_frames, kept.saturating_sub(u64::from(skip)), 1000.0, 2);
        let a = audio_buf(d, buf_frames, start_ns);
        append_retry(w.as_mut(), |w| {
            w.append_audio(&a, start_ns, skip).expect("append_audio")
        });
        kept += u64::from(buf_frames - skip);
        next_ns = frames_ns(kept);
    }
    let all_skip = audio_buf(tone(100, 0, 1000.0, 2), 100, next_ns);
    assert_eq!(
        w.append_audio(&all_skip, next_ns, 100).expect("skip all"),
        Append::Written
    );
    let fin = w.finish(None).expect("finish");
    assert_eq!(fin.samples, kept);
    assert_eq!(kept, 48_000 - u64::from(overlap));
    let (_, all) = decode_all(&path);
    let decoded = all.len() as u64 / 2;
    println!("skip test: kept {kept} frames, decoded {decoded}, finished {fin:?}");
    assert!(
        decoded.abs_diff(kept) <= 48,
        "decoded {decoded} frames vs {kept} appended"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn resampled_input_keeps_duration() {
    let dir = tmp("resample");
    let path = dir.join("mic.mov");
    let mut w = sc_capture_macos::writers()
        .audio(&path, &audio_spec())
        .expect("audio writer");
    let rate = 24_000u32;
    let buf_frames = 480u32;
    for k in 0..50u32 {
        let ns = i64::from(k) * 20_000_000;
        let a = AudioBuffer {
            pts: HostTime(ns),
            sample_rate: rate,
            channels: 1,
            frames: buf_frames,
            data: AudioData::F32Interleaved(vec![0.1f32; buf_frames as usize].into()),
        };
        append_retry(w.as_mut(), |w| {
            w.append_audio(&a, ns, 0).expect("append 24 kHz mono")
        });
    }
    w.finish(None).expect("finish");
    let (_, all) = decode_all(&path);
    let decoded_ns = frames_ns(all.len() as u64 / 2);
    println!("24 kHz mono into a 48 kHz stereo writer: decoded {decoded_ns} ns of 1 s");
    assert!(
        (decoded_ns - 1_000_000_000).abs() <= 2_000_000,
        "decoded {decoded_ns} ns"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn encode_benchmark_1080p_is_positive() {
    let w = sc_capture_macos::writers();
    match w.encode_benchmark(SizePx { w: 1920, h: 1080 }, 30, 60, VideoCodec::Hevc) {
        Ok(fps) => {
            println!("encode_benchmark 1920x1080 HEVC: {fps:.1} fps");
            assert!(fps > 0.0);
        }
        Err(e) if allow_no_hw() => println!("SKIPPED: encode_benchmark: {e}"),
        Err(e) => panic!("encode_benchmark: {e}"),
    }
}

#[test]
fn probe_missing_and_garbage_files() {
    let dir = tmp("probe");
    let dec = sc_capture_macos::decoder();
    assert!(dec.probe(&dir.join("nope.mov")).is_err());
    let junk = dir.join("junk.mov");
    std::fs::write(&junk, b"not a movie at all").expect("write junk");
    let p = dec.probe(&junk).expect("probe junk");
    assert!(!p.readable, "{p:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Child half of `killed_writer_stays_readable`: writes until killed.
#[test]
fn kill_child_body() {
    let Ok(dir) = std::env::var(KILL_CHILD_ENV) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let size = SizePx { w: 640, h: 360 };
    let mut v = video_writer(&dir.join("screen.mov"), &video_spec(size, 30)).expect("video writer");
    let mut a = sc_capture_macos::writers()
        .audio(&dir.join("mic.mov"), &audio_spec())
        .expect("audio writer");
    let start = Instant::now();
    let mut audio_frames = 0u64;
    for i in 0u32.. {
        let ns = i64::from(i) * 33_333_333;
        let f = frame(size, i, ns);
        append_retry(v.as_mut(), |w| {
            w.append_video(&f, ns).expect("append_video")
        });
        while frames_ns(audio_frames) <= ns {
            let at = frames_ns(audio_frames);
            let b = audio_buf(tone(1024, audio_frames, 440.0, 2), 1024, at);
            append_retry(a.as_mut(), |w| {
                w.append_audio(&b, at, 0).expect("append_audio")
            });
            audio_frames += 1024;
        }
        println!("WROTE {ns}");
        let due = start + Duration::from_nanos(u64::try_from(ns).unwrap_or(0));
        if let Some(d) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(d);
        }
    }
}

#[test]
fn killed_writer_stays_readable() {
    if std::env::var(KILL_CHILD_ENV).is_ok() {
        return;
    }
    let dir = tmp("kill");
    let exe = std::env::current_exe().expect("current_exe");
    let mut child = Command::new(exe)
        .args([
            "--exact",
            "kill_child_body",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(KILL_CHILD_ENV, &dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn child");
    let stdout = child.stdout.take().expect("child stdout");
    let mut written = 0i64;
    let target = 3_500_000_000i64;
    let deadline = Instant::now() + Duration::from_secs(30);
    for line in BufReader::new(stdout).lines() {
        let line = line.expect("read child");
        if line.starts_with("SKIPPED:") {
            println!("{line}");
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        if let Some(ns) = line
            .strip_prefix("WROTE ")
            .and_then(|s| s.trim().parse::<i64>().ok())
        {
            written = ns;
            if ns >= target || Instant::now() > deadline {
                break;
            }
        }
    }
    child.kill().expect("SIGKILL the writer");
    let _ = child.wait();
    assert!(written >= target, "child only wrote {written} ns");
    let dec = sc_capture_macos::decoder();
    for (file, video) in [("screen.mov", true), ("mic.mov", false)] {
        let path = dir.join(file);
        let p = dec.probe(&path).expect("probe");
        println!("killed at {written} ns: {file} probe {p:?}");
        assert!(p.readable, "{file}: {p:?}");
        let dur = p.duration_ns.expect("duration");
        assert!(
            written - dur <= 1_000_000_000,
            "{file}: lost {} ns",
            written - dur
        );
        if video {
            let mut r = dec.open_video(&path).expect("open killed video");
            let mut n = 0u64;
            while let Some(f) = r.next_frame().expect("decode killed video") {
                let mut code = None;
                f.frame
                    .0
                    .with_nv12(&mut |v| code = read_code(&v))
                    .expect("nv12");
                assert_eq!(
                    code.map(|c| c.index as u64),
                    Some(n),
                    "killed video frame order"
                );
                n += 1;
            }
            assert_eq!(n, p.samples, "decoded frames vs probe");
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}
