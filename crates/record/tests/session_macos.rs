//! macOS non-TCC: the switch scenario on the synthetic platform with real AVAssetWriter files,
//! decoded back with AVAssetReader. Prints the cut sequence timings.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sc_core::journal::{JournalEvent, read_journal};
use sc_core::project::{ProjectV1, SourceDescriptor, Via};
use sc_geometry::SizePx;
use sc_platform::synthetic::{
    AudioScript, Scenario, SyntheticPlatform, VideoScript, display_spec, read_code, source_hash,
};
use sc_platform::{Decoder, Platform, Resolution, SystemAudioMode};
use sc_record::{
    HealthSnapshot, Notice, RecordRequest, Session, SessionObserver, SessionState, StopReason,
    Tuning,
};

const MS: i64 = 1_000_000;

#[derive(Default)]
struct Obs {
    states: Mutex<Vec<(Instant, SessionState)>>,
    notices: Mutex<Vec<Notice>>,
}

impl SessionObserver for Obs {
    fn on_state(&self, s: &SessionState) {
        self.states
            .lock()
            .expect("lock")
            .push((Instant::now(), s.clone()));
    }
    fn on_health(&self, _: &HealthSnapshot) {}
    fn on_notice(&self, n: &Notice) {
        self.notices.lock().expect("lock").push(n.clone());
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("session_macos")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn size(w: u32, h: u32) -> SizePx {
    SizePx { w, h }
}

fn source_name(d: &SourceDescriptor) -> String {
    match d {
        SourceDescriptor::Display { display_name, .. } => display_name.clone().unwrap_or_default(),
        other => panic!("unexpected source {other:?}"),
    }
}

/// `false` (after printing SKIPPED) when this machine has no usable HEVC encoder and
/// `SC_ALLOW_NO_HW_ENCODER=1`.
fn encoder_available(dir: &Path) -> bool {
    let spec = sc_platform::VideoWriterSpec {
        size: size(640, 360),
        fps: 30,
        codec: sc_platform::VideoCodec::Hevc,
        keyframe_interval_s: 1.0,
        fragment_interval_s: 1.0,
        bits_per_pixel: 0.05,
    };
    match sc_capture_macos::writers().video(&dir.join("probe.mov"), &spec) {
        Ok(w) => {
            let _ = w.finish(None);
            let _ = std::fs::remove_file(dir.join("probe.mov"));
            true
        }
        Err(e) if std::env::var("SC_ALLOW_NO_HW_ENCODER").ok().as_deref() == Some("1") => {
            println!("SKIPPED: no usable HEVC encoder (SC_ALLOW_NO_HW_ENCODER=1): {e}");
            false
        }
        Err(e) => panic!("HEVC writer: {e}"),
    }
}

fn check_segments(p: &ProjectV1, dir: &Path, decoder: &dyn Decoder) -> usize {
    let segs = &p.recording.screen.segments;
    let start = p.recording.start_host_ns;
    let mut total = 0;
    for (i, s) in segs.iter().enumerate() {
        let end = s.end_rec_ns.expect("segment end");
        assert!(end > s.start_rec_ns, "{} is empty", s.id);
        if let Some(n) = segs.get(i + 1) {
            assert_eq!(n.start_rec_ns, end, "{} and {} tile", s.id, n.id);
        }
        let t = p
            .recording
            .tracks
            .iter()
            .find(|t| t.segment_id.as_ref() == Some(&s.id))
            .expect("segment track");
        assert!(t.file.ends_with(".mov"), "{}", t.file);
        let path = dir.join(&t.file);
        let probe = decoder.probe(&path).expect("probe");
        assert!(probe.readable, "{}: {:?}", t.file, probe.error);
        let want = source_hash(&source_name(&s.source));
        let mut r = decoder.open_video(&path).expect("open");
        assert_eq!(r.size(), s.frame_px);
        let (mut frames, mut last) = (0, -1);
        while let Some(f) = r.next_frame().expect("frame") {
            let rec = s.start_rec_ns + f.file_ns;
            assert!(
                rec >= s.start_rec_ns && rec < end,
                "{}: frame at {rec} outside [{}, {end})",
                s.id,
                s.start_rec_ns
            );
            assert!(f.file_ns > last);
            last = f.file_ns;
            let mut code = None;
            f.frame
                .0
                .with_nv12(&mut |v| code = read_code(&v))
                .expect("nv12");
            let code = code.unwrap_or_else(|| panic!("{}: unreadable frame code", s.id));
            assert_eq!(
                code.source_hash, want,
                "{}: frame from another source",
                s.id
            );
            let code_rec = i64::from(code.pts_ms) * MS - start;
            assert!(code_rec <= rec + MS, "{}: frame from the future", s.id);
            frames += 1;
        }
        assert!(frames >= 1, "{} has no frames", s.id);
        assert!(
            last >= end - s.start_rec_ns - 40 * MS,
            "{}: last frame at {last} does not reach the end {}",
            s.id,
            end - s.start_rec_ns
        );
        total += frames;
    }
    total
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

#[test]
fn switch_every_500ms_with_real_writers() {
    let dir = tmp("switch_real");
    if !encoder_available(&dir) {
        return;
    }
    let mic = AudioScript {
        rate: 48_000,
        channels: 1,
        buffer_frames: 480,
        beeps_ns: vec![],
        gaps: vec![],
        overlaps: vec![],
        clock_ppm: 0.0,
    };
    let p = SyntheticPlatform::with_writers(
        Scenario::new()
            .camera(Some(VideoScript {
                size: size(640, 480),
                fps: 30,
            }))
            .mic(Some(mic)),
        sc_capture_macos::writers(),
        sc_capture_macos::decoder(),
    );
    let specs = [
        display_spec("a", Some(size(640, 360))),
        display_spec("b", Some(size(512, 288))),
        display_spec("c", Some(size(800, 448))),
    ];
    let canvas = ProjectV1::new_recording(
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        sc_core::ClockKind::ClockMonotonic,
    )
    .output
    .canvas;
    let req = RecordRequest {
        projects_dir: dir.clone(),
        title: "Real writers".into(),
        source: specs[0].clone(),
        via: Via::OwnChooser,
        fps: 30,
        resolution: Resolution::Native,
        show_cursor: true,
        click_highlight: false,
        camera: Some(sc_platform::synthetic::CAMERA_DEVICE_ID.into()),
        mic: Some(sc_platform::synthetic::MIC_DEVICE_ID.into()),
        system_audio: SystemAudioMode::Off,
        canvas,
        background_image: None,
        exclude: Vec::new(),
        tuning: Tuning::default(),
    };
    let obs = Arc::new(Obs::default());
    let platform: Arc<dyn Platform> = p.clone();
    let s = Session::start(platform, req, obs.clone()).expect("start");
    let pdir = s.project_dir().to_path_buf();
    let mut calls = Vec::new();
    for i in 1..12 {
        std::thread::sleep(Duration::from_millis(500));
        calls.push(Instant::now());
        s.switch_source(specs[i % 3].clone(), Via::OwnChooser)
            .expect("switch");
    }
    std::thread::sleep(Duration::from_millis(500));
    let t_stop = Instant::now();
    let sum = s.stop(StopReason::User).expect("stop");
    let stop_ms = t_stop.elapsed().as_secs_f64() * 1e3;
    assert_eq!(p.active_screen_streams(), 0);
    assert_eq!(
        sum.segments,
        12,
        "notices: {:?}",
        obs.notices.lock().expect("lock")
    );

    let proj = ProjectV1::load(&pdir).expect("load");
    let segs = &proj.recording.screen.segments;
    for (i, sg) in segs.iter().enumerate() {
        assert_eq!(
            sg.frame_px,
            [size(640, 360), size(512, 288), size(800, 448)][i % 3]
        );
    }
    let decoder = sc_capture_macos::decoder();
    let frames = check_segments(&proj, &pdir, &*decoder);
    for id in ["camera", "mic"] {
        let t = proj
            .recording
            .tracks
            .iter()
            .find(|t| t.id == id)
            .unwrap_or_else(|| panic!("{id} track"));
        let probe = decoder.probe(&pdir.join(&t.file)).expect("probe");
        assert!(probe.readable, "{id}: {:?}", probe.error);
    }

    let ev = read_journal(&pdir).expect("journal").events;
    let start = proj.recording.start_host_ns;
    let (mut prep, mut cut, mut total, mut trailing) = (vec![], vec![], vec![], vec![]);
    let mut begin = None;
    let mut prepared = None;
    for e in &ev {
        match e {
            JournalEvent::SwitchBegin { host_ns, .. } => begin = Some(*host_ns),
            JournalEvent::SegmentPrepare { host_ns, .. } => prepared = Some(*host_ns),
            JournalEvent::SegmentOpen { start_rec_ns, .. } if *start_rec_ns > 0 => {
                let (b, pr) = (begin.expect("begin"), prepared.expect("prepare"));
                let c = start + start_rec_ns;
                prep.push((pr - b) as f64 / 1e6);
                cut.push((c - pr) as f64 / 1e6);
                total.push((c - b) as f64 / 1e6);
            }
            JournalEvent::SegmentClose {
                trailing_dropped, ..
            } => trailing.push(f64::from(*trailing_dropped)),
            _ => {}
        }
    }
    let states = obs.states.lock().expect("lock").clone();
    let mut settle: Vec<f64> = calls
        .iter()
        .filter_map(|c| {
            states
                .iter()
                .find(|(t, s)| t > c && *s == SessionState::Recording)
                .map(|(t, _)| (*t - *c).as_secs_f64() * 1e3)
        })
        .collect();
    assert_eq!(settle.len(), 11);
    println!(
        "cut timings over {} switches (ms, median / max): switch_begin->segment_prepare {:.1} / {:.1}; \
         segment_prepare->cut {:.1} / {:.1}; switch_begin->cut {:.1} / {:.1}; \
         switch call->Recording {:.1} / {:.1}; trailing dropped per close max {}; \
         stop() {stop_ms:.1} ms; {frames} screen frames decoded",
        prep.len(),
        pct(&mut prep, 0.5),
        pct(&mut prep, 1.0),
        pct(&mut cut, 0.5),
        pct(&mut cut, 1.0),
        pct(&mut total, 0.5),
        pct(&mut total, 1.0),
        pct(&mut settle, 0.5),
        pct(&mut settle, 1.0),
        pct(&mut trailing, 1.0),
    );
    assert_eq!(prep.len(), 11);
}
