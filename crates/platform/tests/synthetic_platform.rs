use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_core::time::HostTime;
use sc_geometry::{RectPt, SizePx};
use sc_platform::synthetic::{
    AudioScript, CAMERA_DEVICE_ID, Collector, DiskScript, FrameCode, MIC_DEVICE_ID, Scenario,
    ScreenScript, ScriptedEvent, SctrkDecoder, SctrkWriterFactory, StartFailure, SyntheticPlatform,
    VideoScript, audio_buffer_pts, coded_frame, display_spec, read_code, source_hash,
};
use sc_platform::{
    AudioBuffer, AudioData, AudioTrackKind, AudioWriterSpec, CameraLimit, Decoder, DeviceKind,
    DisplayId, Error, FrameBuffer, PermissionKind, PermissionStatus, Platform, Resolution,
    ScreenStreamConfig, SourceEvent, SourceSpec, StreamTag, VideoCodec, VideoFrame,
    VideoWriterSpec, WriterFactory,
};

const MS: i64 = 1_000_000;
const S: i64 = 1_000_000_000;

fn cfg(fps: u32) -> ScreenStreamConfig {
    ScreenStreamConfig {
        fps,
        resolution: Resolution::Native,
        show_cursor: false,
        show_clicks: false,
        queue_depth: 6,
        exclude: vec![],
    }
}

fn code_of(f: &VideoFrame) -> Option<FrameCode> {
    let mut code = None;
    f.buffer
        .0
        .with_nv12(&mut |v| code = read_code(&v))
        .expect("cpu frame");
    code
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "sc-platform-synth-{name}-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

fn small() -> SizePx {
    SizePx { w: 320, h: 180 }
}

fn mic_script() -> AudioScript {
    AudioScript {
        rate: 48_000,
        channels: 2,
        buffer_frames: 480,
        beeps_ns: vec![],
        gaps: vec![],
        overlaps: vec![],
        clock_ppm: 0.0,
    }
}

#[test]
fn code_round_trips_through_sctrk() {
    let dir = temp_dir("code");
    let path = dir.join("screen.sctrk");
    let size = SizePx { w: 333, h: 201 };
    let w = SctrkWriterFactory;
    let mut tw = w
        .video(
            &path,
            &VideoWriterSpec {
                size,
                fps: 30,
                codec: VideoCodec::Hevc,
                keyframe_interval_s: 2.0,
                fragment_interval_s: 1.0,
                bits_per_pixel: 0.1,
            },
        )
        .unwrap();
    assert!(path.exists(), "file exists right after video()");
    let codes: Vec<FrameCode> = (0..10)
        .map(|i| FrameCode {
            source_hash: source_hash("a"),
            index: i,
            pts_ms: 1000 + i * 33,
        })
        .collect();
    for (i, c) in codes.iter().enumerate() {
        let frame = VideoFrame {
            stream: StreamTag(1),
            pts: HostTime(0),
            size,
            content_rect_pt: None,
            content_scale: None,
            buffer: FrameBuffer(Arc::new(coded_frame(size, *c, false))),
        };
        tw.append_video(&frame, i as i64 * 33 * MS).unwrap();
    }
    let done = tw.finish(Some(330 * MS)).unwrap();
    assert_eq!(done.samples, 10);
    assert_eq!(done.duration_ns, Some(330 * MS));

    let d = SctrkDecoder;
    let mut r = d.open_video(&path).unwrap();
    assert_eq!(r.size(), size);
    let mut n = 0;
    while let Some(f) = r.next_frame().unwrap() {
        let mut got = None;
        f.frame.0.with_nv12(&mut |v| got = read_code(&v)).unwrap();
        assert_eq!(got, Some(codes[n]));
        assert_eq!(f.file_ns, n as i64 * 33 * MS);
        n += 1;
    }
    assert_eq!(n, 10);
    let p = d.probe(&path).unwrap();
    assert!(p.readable);
    assert_eq!(
        (p.samples, p.duration_ns, p.size),
        (10, Some(330 * MS), Some(size))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn torn_file_probe_ignores_the_tail() {
    let dir = temp_dir("torn");
    let path = dir.join("screen.sctrk");
    let size = small();
    let mut tw = SctrkWriterFactory
        .video(
            &path,
            &VideoWriterSpec {
                size,
                fps: 30,
                codec: VideoCodec::Hevc,
                keyframe_interval_s: 2.0,
                fragment_interval_s: 1.0,
                bits_per_pixel: 0.1,
            },
        )
        .unwrap();
    for i in 0..5u32 {
        let frame = VideoFrame {
            stream: StreamTag(1),
            pts: HostTime(0),
            size,
            content_rect_pt: None,
            content_scale: None,
            buffer: FrameBuffer(Arc::new(coded_frame(
                size,
                FrameCode {
                    source_hash: 7,
                    index: i,
                    pts_ms: i,
                },
                false,
            ))),
        };
        tw.append_video(&frame, i64::from(i) * 100 * MS).unwrap();
    }
    drop(tw);
    let len = std::fs::metadata(&path).unwrap().len();
    let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    f.set_len(len - 7).unwrap();
    drop(f);

    let d = SctrkDecoder;
    let p = d.probe(&path).unwrap();
    assert!(p.readable);
    assert_eq!(p.samples, 4);
    assert_eq!(p.duration_ns, Some(300 * MS + S / 30));
    let mut r = d.open_video(&path).unwrap();
    let mut n = 0;
    while r.next_frame().unwrap().is_some() {
        n += 1;
    }
    assert_eq!(n, 4);

    let garbage = dir.join("garbage.sctrk");
    std::fs::write(&garbage, b"not a track at all").unwrap();
    let g = d.probe(&garbage).unwrap();
    assert!(!g.readable && g.error.is_some());
    assert!(matches!(
        d.probe(&dir.join("missing.sctrk")),
        Err(Error::Io(_))
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn audio_round_trips_with_silence_and_skip() {
    let dir = temp_dir("audio");
    let path = dir.join("mic.sctrk");
    let mut tw = SctrkWriterFactory
        .audio(
            &path,
            &AudioWriterSpec {
                sample_rate: 48_000,
                channels: 2,
                bitrate: 128_000,
                fragment_interval_s: 1.0,
            },
        )
        .unwrap();
    let buf = |v: f32| AudioBuffer {
        pts: HostTime(0),
        sample_rate: 48_000,
        channels: 2,
        frames: 480,
        data: AudioData::F32Interleaved(vec![v; 960].into()),
    };
    tw.append_audio(&buf(0.25), 0, 0).unwrap();
    tw.append_silence(10 * MS, 480).unwrap();
    tw.append_audio(&buf(0.5), 20 * MS - 5 * MS, 240).unwrap();
    let done = tw.finish(None).unwrap();
    assert_eq!(done.samples, 480 + 480 + 240);
    assert_eq!(done.duration_ns, Some(25 * MS));

    let d = SctrkDecoder;
    let p = d.probe(&path).unwrap();
    assert_eq!((p.samples, p.duration_ns), (1200, Some(25 * MS)));
    let mut r = d.open_audio(&path, 48_000, 1).unwrap();
    assert_eq!(r.first_sample_file_ns(), 0);
    let mut out = vec![0f32; 4000];
    let n = r.read(&mut out).unwrap();
    assert_eq!(n, 1200);
    assert!(out[..480].iter().all(|&s| s == 0.25));
    assert!(out[480..960].iter().all(|&s| s == 0.0));
    assert!(out[960..1200].iter().all(|&s| s == 0.5));
    assert_eq!(r.read(&mut out).unwrap(), 0);
    let mut r = d.open_audio(&path, 24_000, 2).unwrap();
    let mut out = vec![0f32; 4000];
    assert_eq!(r.read(&mut out).unwrap(), 1200);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn time_scale_ten_delivers_ten_seconds_of_30fps_in_about_one_second() {
    let p = SyntheticPlatform::new(Scenario::new().time_scale(10.0).screen(
        "a",
        ScreenScript {
            size: small(),
            ..Default::default()
        },
    ));
    let sink = Collector::new();
    let t0 = Instant::now();
    let h0 = p.host_now();
    let s = p
        .screen()
        .start(
            StreamTag(1),
            &display_spec("a", None),
            &cfg(30),
            sink.clone(),
        )
        .unwrap();
    assert!(sink.wait_until(Duration::from_secs(10), |c| c.screen().len() >= 300));
    let real = t0.elapsed();
    s.stop().unwrap();
    let frames = sink.screen();
    let span = frames[299].pts.0 - frames[0].pts.0;
    assert!((span - 299 * S / 30).abs() <= 1, "span {span}");
    for w in frames.windows(2) {
        let d = w[1].pts.0 - w[0].pts.0;
        assert!((33_333_332..=33_333_334).contains(&d), "delta {d}");
    }
    let first = frames[0].pts.0 - h0.0;
    assert!(
        (30 * MS..40 * MS).contains(&first),
        "first frame at {first}"
    );
    assert!(
        real > Duration::from_millis(900) && real < Duration::from_millis(3000),
        "10 s at 10x took {real:?}"
    );
    let host = p.host_now().0 - h0.0;
    let real_now = t0.elapsed().as_nanos() as i64;
    assert!(
        (host - real_now * 10).abs() < 50 * MS,
        "host {host} real {real_now}"
    );
}

#[test]
fn trailing_frames_after_a_new_stream() {
    let p = SyntheticPlatform::new(
        Scenario::new()
            .time_scale(5.0)
            .screen(
                "old",
                ScreenScript {
                    size: small(),
                    trailing_after_new_stream: 2,
                    ..Default::default()
                },
            )
            .screen(
                "new",
                ScreenScript {
                    size: SizePx { w: 256, h: 144 },
                    ..Default::default()
                },
            ),
    );
    let sink = Collector::new();
    let old = p
        .screen()
        .start(
            StreamTag(1),
            &display_spec("old", None),
            &cfg(30),
            sink.clone(),
        )
        .unwrap();
    assert!(sink.wait_until(Duration::from_secs(5), |c| c.screen().len() >= 5));
    let new = p
        .screen()
        .start(
            StreamTag(2),
            &display_spec("new", None),
            &cfg(30),
            sink.clone(),
        )
        .unwrap();
    assert_eq!(p.active_screen_streams(), 2);
    assert!(sink.wait_until(Duration::from_secs(5), |c| {
        c.screen()
            .iter()
            .filter(|f| f.stream == StreamTag(2))
            .count()
            >= 20
    }));
    let frames = sink.screen();
    let cut = frames
        .iter()
        .position(|f| f.stream == StreamTag(2))
        .unwrap();
    let t1 = frames[cut].pts;
    assert!(
        frames[..cut]
            .iter()
            .all(|f| f.stream == StreamTag(1) && f.pts < t1)
    );
    let trailing: Vec<_> = frames[cut..]
        .iter()
        .filter(|f| f.stream == StreamTag(1))
        .collect();
    assert_eq!(trailing.len(), 2);
    assert!(trailing.iter().all(|f| f.pts >= t1));
    assert_eq!(
        code_of(trailing[0]).unwrap().source_hash,
        source_hash("old")
    );
    assert_eq!(
        code_of(&frames[cut]).unwrap().source_hash,
        source_hash("new")
    );
    assert!(frames.windows(2).all(|w| w[0].pts <= w[1].pts), "PTS order");
    old.stop().unwrap();
    assert_eq!(p.active_screen_streams(), 1);
    new.stop().unwrap();
    assert_eq!(p.active_screen_streams(), 0);
}

#[test]
fn stream_fails_only_while_another_runs() {
    let p = SyntheticPlatform::new(
        Scenario::new()
            .time_scale(5.0)
            .screen(
                "b",
                ScreenScript {
                    size: small(),
                    start_fails: StartFailure::WhileOtherStreamRuns,
                    ..Default::default()
                },
            )
            .screen(
                "never",
                ScreenScript {
                    start_fails: StartFailure::Always,
                    ..Default::default()
                },
            ),
    );
    let sink = Collector::new();
    let b = display_spec("b", None);
    let a = p
        .screen()
        .start(
            StreamTag(1),
            &display_spec("a", Some(small())),
            &cfg(30),
            sink.clone(),
        )
        .unwrap();
    assert!(matches!(
        p.screen().start(StreamTag(2), &b, &cfg(30), sink.clone()),
        Err(Error::Busy(_))
    ));
    assert_eq!(p.active_screen_streams(), 1);
    a.stop().unwrap();
    let s = p
        .screen()
        .start(StreamTag(3), &b, &cfg(30), sink.clone())
        .unwrap();
    assert!(sink.wait_until(Duration::from_secs(5), |c| {
        c.screen().iter().any(|f| f.stream == StreamTag(3))
    }));
    s.stop().unwrap();
    assert!(matches!(
        p.screen()
            .start(StreamTag(4), &display_spec("never", None), &cfg(30), sink),
        Err(Error::Os { .. })
    ));
}

#[test]
fn injected_gap_and_overlap_move_audio_pts() {
    let script = AudioScript {
        gaps: vec![(500 * MS, 40 * MS)],
        overlaps: vec![(1000 * MS, 20 * MS)],
        ..mic_script()
    };
    let p = SyntheticPlatform::new(Scenario::new().time_scale(20.0).mic(Some(script)));
    let sink = Collector::new();
    let m = p.mic().start(MIC_DEVICE_ID, sink.clone()).unwrap();
    assert!(sink.wait_until(Duration::from_secs(5), |c| c.audio().len() >= 150));
    m.stop().unwrap();
    let audio = sink.audio();
    let pts: Vec<i64> = audio
        .iter()
        .map(|(_, a)| a.pts.0 - audio[0].1.pts.0)
        .collect();
    assert!(audio.iter().all(|(k, _)| *k == AudioTrackKind::Mic));
    for (k, w) in pts.windows(2).enumerate() {
        let expected = match k + 1 {
            50 => 10 * MS + 40 * MS,
            100 => 10 * MS - 20 * MS,
            _ => 10 * MS,
        };
        assert_eq!(w[1] - w[0], expected, "buffer {}", k + 1);
    }
}

#[test]
fn clock_ppm_moves_pts_over_sixty_seconds() {
    let script = AudioScript {
        rate: 48_000,
        channels: 1,
        buffer_frames: 1024,
        clock_ppm: 100.0,
        ..mic_script()
    };
    let p = SyntheticPlatform::new(Scenario::new().time_scale(60.0).mic(Some(script.clone())));
    let sink = Collector::new();
    let m = p.mic().start(MIC_DEVICE_ID, sink.clone()).unwrap();
    let buffers = 60 * 48_000 / 1024 + 1;
    assert!(sink.wait_until(Duration::from_secs(20), |c| c.audio().len() >= buffers));
    m.stop().unwrap();
    let audio = sink.audio();
    let first = audio[0].1.pts.0;
    let k = buffers - 1;
    let nominal = (k as f64 * 1024.0 * 1e9 / 48_000.0).round() as i64;
    let drift = audio[k].1.pts.0 - first - nominal;
    let expected = (nominal as f64 * 100e-6).round() as i64;
    assert!(
        (drift - expected).abs() <= 1,
        "drift {drift} expected {expected}"
    );
    assert!((5_900_000..6_100_000).contains(&drift));
    assert_eq!(audio_buffer_pts(&script, first, k as u64), audio[k].1.pts.0);
}

#[test]
fn beeps_land_at_their_scripted_time() {
    let script = AudioScript {
        rate: 48_000,
        channels: 1,
        buffer_frames: 480,
        beeps_ns: vec![200 * MS],
        ..mic_script()
    };
    let p = SyntheticPlatform::new(Scenario::new().time_scale(10.0).mic(Some(script)));
    let sink = Collector::new();
    let m = p.mic().start(MIC_DEVICE_ID, sink.clone()).unwrap();
    assert!(sink.wait_until(Duration::from_secs(5), |c| c.audio().len() >= 40));
    m.stop().unwrap();
    let audio = sink.audio();
    let start = audio[0].1.pts.0;
    let mut first_loud = None;
    'outer: for (_, a) in &audio {
        let AudioData::F32Interleaved(d) = &a.data else {
            panic!("f32 audio")
        };
        for (i, s) in d.iter().enumerate() {
            if s.abs() > 0.1 {
                first_loud = Some(a.pts.0 - start + i as i64 * S / 48_000);
                break 'outer;
            }
        }
    }
    let at = first_loud.expect("a beep");
    assert!((200 * MS..201 * MS).contains(&at), "beep at {at}");
}

#[test]
fn idle_lost_and_first_frame_size() {
    let p = SyntheticPlatform::new(
        Scenario::new()
            .time_scale(10.0)
            .screen(
                "idle",
                ScreenScript {
                    size: small(),
                    idle: vec![(300 * MS, 500 * MS)],
                    lost_after_ns: Some(1200 * MS),
                    first_frame_size: Some(SizePx { w: 640, h: 360 }),
                    ..Default::default()
                },
            )
            .screen(
                "frozen",
                ScreenScript {
                    size: small(),
                    idle_from_start: true,
                    ..Default::default()
                },
            ),
    );
    let spec = display_spec("idle", None);
    let (_, described) = p.screen().describe(&spec, &cfg(30)).unwrap();
    assert_eq!(described, small());
    let sink = Collector::new();
    let h0 = p.host_now().0;
    let s = p
        .screen()
        .start(StreamTag(9), &spec, &cfg(30), sink.clone())
        .unwrap();
    let f = p
        .screen()
        .start(
            StreamTag(10),
            &display_spec("frozen", None),
            &cfg(30),
            sink.clone(),
        )
        .unwrap();
    assert!(sink.wait_until(Duration::from_secs(5), |c| !c.events().is_empty()));
    std::thread::sleep(Duration::from_millis(100));
    let frames = sink.screen();
    assert!(
        frames.iter().all(|f| f.stream == StreamTag(9)),
        "frozen sent a frame"
    );
    assert!(frames.iter().all(|f| f.size == SizePx { w: 640, h: 360 }));
    assert!(frames.iter().all(|f| f.buffer.0.size() == f.size));
    for f in &frames {
        let off = f.pts.0 - h0;
        assert!(
            !(310 * MS..800 * MS).contains(&off),
            "frame in idle span at {off}"
        );
        assert!(off < 1200 * MS + 5 * MS);
    }
    let gap = frames
        .windows(2)
        .map(|w| w[1].pts.0 - w[0].pts.0)
        .max()
        .unwrap();
    assert!(gap >= 490 * MS, "idle gap {gap}");
    assert_eq!(
        sink.events(),
        vec![SourceEvent::SourceLost {
            stream: StreamTag(9),
            reason: "synthetic: source lost (scripted)".into()
        }]
    );
    s.stop().unwrap();
    f.stop().unwrap();
}

#[test]
fn permissions_disk_devices_and_events() {
    let p = SyntheticPlatform::new(
        Scenario::new()
            .permissions(
                PermissionStatus::Granted,
                PermissionStatus::Denied,
                PermissionStatus::Granted,
            )
            .camera(Some(VideoScript {
                size: SizePx { w: 3840, h: 2160 },
                fps: 60,
            }))
            .mic(Some(mic_script()))
            .disk(DiskScript::Linear {
                start: 10_000_000_000,
                bytes_per_s: -1_000_000_000,
            })
            .at(150 * MS, ScriptedEvent::DeviceLost(DeviceKind::Mic))
            .at(50 * MS, ScriptedEvent::OwnWindowsChanged)
            .at(600 * MS, ScriptedEvent::DevicesChanged(DeviceKind::Mic))
            .at(700 * MS, ScriptedEvent::WillSleep)
            .at(710 * MS, ScriptedEvent::DidWake),
    );
    assert_eq!(
        p.permissions().status(PermissionKind::Camera),
        PermissionStatus::Denied
    );
    assert!(matches!(
        p.camera()
            .start(CAMERA_DEVICE_ID, CameraLimit::default(), Collector::new()),
        Err(Error::PermissionDenied(PermissionKind::Camera))
    ));
    let disk0 = p.free_disk_bytes(std::path::Path::new("/")).unwrap();
    assert!(disk0 <= 10_000_000_000 && disk0 > 9_000_000_000);

    let events = Collector::new();
    let mic = Collector::new();
    let w = p.watch_system(events.clone()).unwrap();
    let m = p.mic().start(MIC_DEVICE_ID, mic.clone()).unwrap();
    assert!(events.wait_until(Duration::from_millis(400), |c| c.events().len() >= 2));
    assert!(p.mic().devices().unwrap().is_empty(), "mic lost");
    let mic_count = mic.audio().len();
    assert!(events.wait_until(Duration::from_secs(5), |c| c.events().len() == 5));
    assert_eq!(mic.audio().len(), mic_count, "a lost mic delivers nothing");
    assert_eq!(
        events.events(),
        vec![
            SourceEvent::OwnWindowsChanged,
            SourceEvent::DeviceLost {
                kind: DeviceKind::Mic,
                device_id: MIC_DEVICE_ID.into()
            },
            SourceEvent::DevicesChanged {
                kind: DeviceKind::Mic
            },
            SourceEvent::SystemWillSleep,
            SourceEvent::SystemDidWake,
        ]
    );
    assert_eq!(
        p.mic().devices().unwrap().len(),
        1,
        "mic back after DevicesChanged"
    );
    m.stop().unwrap();
    w.stop().unwrap();
    let disk1 = p.free_disk_bytes(std::path::Path::new("/")).unwrap();
    assert!(disk1 < disk0);
}

#[test]
fn camera_is_coded_and_limited() {
    let p = SyntheticPlatform::new(Scenario::new().time_scale(10.0).camera(Some(VideoScript {
        size: SizePx { w: 3840, h: 2160 },
        fps: 60,
    })));
    let sink = Collector::new();
    let c = p
        .camera()
        .start(CAMERA_DEVICE_ID, CameraLimit::default(), sink.clone())
        .unwrap();
    assert!(c.format().contains("1920x1080 @ 30"));
    assert!(sink.wait_until(Duration::from_secs(5), |c| c.camera().len() >= 3));
    c.stop().unwrap();
    let f = &sink.camera()[0];
    assert_eq!(f.size, SizePx { w: 1920, h: 1080 });
    assert_eq!(code_of(f).unwrap().source_hash, source_hash("camera"));
}

#[test]
fn sources_regions_windows_and_listing() {
    let p = SyntheticPlatform::new(
        Scenario::new()
            .screen("main", ScreenScript::default())
            .screen(
                "window:42",
                ScreenScript {
                    size: SizePx { w: 800, h: 600 },
                    ..Default::default()
                },
            )
            .screen("app:77", ScreenScript::default()),
    );
    let list = p.screen().list(&[]).unwrap();
    assert_eq!(list.displays.len(), 1);
    assert_eq!(list.displays[0].id, DisplayId("synthetic:main".into()));
    assert_eq!(list.windows.len(), 1);
    assert_eq!(list.apps.len(), 1);
    assert!(
        p.screen()
            .list(&[sc_platform::WindowId(42)])
            .unwrap()
            .windows
            .is_empty()
    );
    let (_, size) = p
        .screen()
        .describe(
            &SourceSpec::Region {
                display: DisplayId("synthetic:main".into()),
                region_pt: RectPt {
                    x: 10.0,
                    y: 10.0,
                    w: 301.0,
                    h: 157.0,
                },
            },
            &cfg(30),
        )
        .unwrap();
    assert_eq!(size, SizePx { w: 301, h: 157 });
    let (_, size) = p
        .screen()
        .describe(
            &SourceSpec::Window {
                window: sc_platform::WindowId(42),
            },
            &cfg(30),
        )
        .unwrap();
    assert_eq!(size, SizePx { w: 800, h: 600 });
    p.screen()
        .describe(&display_spec("fresh", None), &cfg(30))
        .unwrap();
    assert_eq!(p.screen().displays().unwrap().len(), 2, "created on demand");
    let img = p
        .screen()
        .snapshot(&display_spec("main", None), SizePx { w: 320, h: 320 })
        .unwrap();
    assert_eq!(img.size, SizePx { w: 320, h: 180 });
}
