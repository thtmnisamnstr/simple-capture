use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sc_core::journal::{GapKind, JournalEvent, StopReason, SwitchNext, read_journal};
use sc_core::project::{
    EndReason, ProjectState, ProjectV1, SourceDescriptor, StartReason, TrackKind, Via,
};
use sc_geometry::{Aspect, Ratio, RectPt, SizePx};
use sc_platform::synthetic::{
    AudioScript, DiskScript, Scenario, ScreenScript, ScriptedEvent, StartFailure,
    SyntheticPlatform, VideoScript, display_spec, read_code, source_hash,
};
use sc_platform::{
    AppRef, AudioTrackKind, DeviceKind, DisplayId, Error as PError, PermissionKind,
    PermissionStatus, Platform, Resolution, SourceSpec, SystemAudioMode, WindowId,
};
use sc_record::recovery::{Outcome, recover_project};
use sc_record::{
    Error, HealthSnapshot, Notice, RecordRequest, Session, SessionObserver, SessionState, Tuning,
};

const MS: i64 = 1_000_000;

#[derive(Default)]
struct Obs {
    states: Mutex<Vec<SessionState>>,
    notices: Mutex<Vec<Notice>>,
    health: Mutex<Vec<HealthSnapshot>>,
    notices_at_end: Mutex<Option<Vec<Notice>>>,
}

impl SessionObserver for Obs {
    fn on_state(&self, s: &SessionState) {
        if matches!(
            s,
            SessionState::Finalized { .. } | SessionState::Failed { .. }
        ) {
            self.notices_at_end
                .lock()
                .expect("lock")
                .get_or_insert_with(|| self.notices());
        }
        self.states.lock().expect("lock").push(s.clone());
    }
    fn on_health(&self, h: &HealthSnapshot) {
        self.health.lock().expect("lock").push(h.clone());
    }
    fn on_notice(&self, n: &Notice) {
        self.notices.lock().expect("lock").push(n.clone());
    }
}

impl Obs {
    fn notices(&self) -> Vec<Notice> {
        self.notices.lock().expect("lock").clone()
    }

    fn notices_at_end(&self) -> Vec<Notice> {
        let get = || self.notices_at_end.lock().expect("lock").clone();
        assert!(wait_for(Duration::from_secs(3), || get().is_some()));
        get().expect("terminal state")
    }
}

fn wait_for(timeout: Duration, f: impl Fn() -> bool) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    f()
}

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("session_synthetic")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn request(dir: &Path, source: SourceSpec) -> RecordRequest {
    let canvas = ProjectV1::new_recording(
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        sc_core::ClockKind::ClockMonotonic,
    )
    .output
    .canvas;
    RecordRequest {
        projects_dir: dir.to_path_buf(),
        title: "Test".into(),
        source,
        via: Via::OwnChooser,
        fps: 30,
        resolution: Resolution::Native,
        show_cursor: true,
        click_highlight: false,
        camera: None,
        mic: None,
        system_audio: SystemAudioMode::Off,
        canvas,
        background_image: None,
        exclude: Vec::new(),
        duration_ns: None,
        camera_key: None,
        tuning: Tuning::default(),
    }
}

fn size(w: u32, h: u32) -> SizePx {
    SizePx { w, h }
}

fn mic() -> AudioScript {
    AudioScript {
        rate: 48_000,
        channels: 1,
        buffer_frames: 480,
        beeps_ns: vec![],
        gaps: vec![],
        overlaps: vec![],
        clock_ppm: 0.0,
    }
}

fn sleep_host(p: &dyn Platform, ns: i64) {
    std::thread::sleep(p.host_duration(ns));
}

fn journal(dir: &Path) -> Vec<JournalEvent> {
    read_journal(dir).expect("journal").events
}

fn source_key(d: &SourceDescriptor) -> String {
    match d {
        SourceDescriptor::Display { display_name, .. } => display_name.clone().unwrap_or_default(),
        SourceDescriptor::Window { window_title, .. } => window_title.clone().unwrap_or_default(),
        SourceDescriptor::Application { app_name, .. } => app_name.clone().unwrap_or_default(),
        SourceDescriptor::Region { display_id, .. } => {
            let name = display_id.trim_start_matches("synthetic:");
            format!("{}/region", name.split(':').next().unwrap_or(name))
        }
    }
}

/// Segments are sorted, non-overlapping and tile; every decoded frame lies inside its segment and
/// carries its source's code; frames whose code is not older than their position are on time.
fn check_segments(p: &ProjectV1, dir: &Path, decoder: &dyn sc_platform::Decoder) -> usize {
    let segs = &p.recording.screen.segments;
    let start = p.recording.start_host_ns;
    let mut total = 0;
    for (i, s) in segs.iter().enumerate() {
        let end = s.end_rec_ns.expect("segment end");
        assert!(end > s.start_rec_ns, "{} is empty", s.id);
        if let Some(n) = segs.get(i + 1) {
            assert!(n.start_rec_ns >= end, "{} overlaps {}", s.id, n.id);
        }
        let t = p
            .recording
            .tracks
            .iter()
            .find(|t| t.segment_id.as_ref() == Some(&s.id))
            .expect("segment track");
        assert_eq!(t.first_sample_offset_ns, s.start_rec_ns);
        assert_eq!(t.duration_ns, Some(end - s.start_rec_ns));
        let probe = decoder.probe(&dir.join(&t.file)).expect("probe");
        assert!(probe.readable, "{}: {:?}", t.file, probe.error);
        let want = source_hash(&source_key(&s.source));
        let mut r = decoder.open_video(&dir.join(&t.file)).expect("open");
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
            let code = code.expect("frame code");
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
            "{}: last frame does not reach the end",
            s.id
        );
        total += frames;
    }
    total
}

fn platform(s: Scenario) -> Arc<SyntheticPlatform> {
    SyntheticPlatform::new(s)
}

fn as_dyn(p: &Arc<SyntheticPlatform>) -> Arc<dyn Platform> {
    p.clone()
}

#[test]
fn records_every_track_and_finalizes() {
    let dir = tmp("all_tracks");
    let p = platform(
        Scenario::new()
            .screen(
                "main",
                ScreenScript {
                    size: size(320, 180),
                    ..Default::default()
                },
            )
            .camera(Some(VideoScript {
                size: size(320, 240),
                fps: 30,
            }))
            .mic(Some(mic()))
            .system_audio(Some(AudioScript {
                channels: 2,
                ..mic()
            })),
    );
    let mut req = request(&dir, display_spec("main", None));
    req.camera = Some(sc_platform::synthetic::CAMERA_DEVICE_ID.into());
    req.mic = Some(sc_platform::synthetic::MIC_DEVICE_ID.into());
    req.system_audio = SystemAudioMode::All;
    let obs = Arc::new(Obs::default());
    let s = Session::start(as_dyn(&p), req, obs.clone()).expect("start");
    let pdir = s.project_dir().to_path_buf();
    assert_eq!(s.state(), SessionState::Recording);
    assert!(pdir.join(".lock").exists());
    sleep_host(&*p, 1500 * MS);
    let live = recover_project(&pdir, &*p.decoder()).expect("recover");
    assert_eq!(
        live.outcome,
        Outcome::Locked,
        "the lock is held while recording"
    );
    assert!(matches!(s.pause(), Err(Error::Unsupported(_))));
    assert!(matches!(s.resume(), Err(Error::Unsupported(_))));
    let sum = s.stop(StopReason::User).expect("stop");
    assert!(!pdir.join(".lock").exists());
    assert_eq!(sum.segments, 1);
    assert!(sum.duration_ns >= 1500 * MS);
    assert_eq!(p.active_screen_streams(), 0);

    let proj = ProjectV1::load(&pdir).expect("load");
    assert_eq!(proj.state, ProjectState::Finalized);
    let dur = proj.duration_ns().expect("duration");
    assert_eq!(dur, sum.duration_ns);
    assert_eq!(proj.edl.clips.len(), 1);
    assert_eq!(proj.edl.clips[0].src_out_rec_ns, dur);
    let s0 = &proj.recording.screen.segments[0];
    assert_eq!((s0.start_rec_ns, s0.end_rec_ns), (0, Some(dur)));
    assert_eq!(s0.end_reason, Some(EndReason::RecordStop));
    assert_eq!(s0.start_reason, StartReason::RecordStart);
    let frames = check_segments(&proj, &pdir, &*p.decoder());
    assert!(
        frames as i64 >= dur / (34 * MS),
        "{frames} screen frames in {dur} ns"
    );

    let kinds: Vec<_> = proj.recording.tracks.iter().map(|t| t.kind).collect();
    for k in [
        TrackKind::Screen,
        TrackKind::Camera,
        TrackKind::Mic,
        TrackKind::SystemAudio,
    ] {
        assert!(kinds.contains(&k), "{k:?} missing");
    }
    for t in proj
        .recording
        .tracks
        .iter()
        .filter(|t| t.kind != TrackKind::Screen)
    {
        assert!(
            t.first_sample_offset_ns >= 0 && t.first_sample_offset_ns < 60 * MS,
            "{} offset {}",
            t.id,
            t.first_sample_offset_ns
        );
        let d = t.duration_ns.expect("track duration");
        assert!(
            (d + t.first_sample_offset_ns - dur).abs() < 80 * MS,
            "{} ends at {} of {dur}",
            t.id,
            d + t.first_sample_offset_ns
        );
    }
    for id in ["mic", "system_audio"] {
        let t = proj.recording.tracks.iter().find(|t| t.id == id).expect(id);
        let a = t.audio.as_ref().expect("audio info");
        let mut r = p
            .decoder()
            .open_audio(&pdir.join(&t.file), a.sample_rate, a.channels)
            .expect("open audio");
        let mut buf = vec![0f32; 4800 * usize::from(a.channels)];
        let mut n = 0usize;
        loop {
            let got = r.read(&mut buf).expect("read");
            if got == 0 {
                break;
            }
            n += got / usize::from(a.channels);
        }
        let expect = (t.duration_ns.expect("d") as f64 * 48e-6) as usize;
        assert!(
            n.abs_diff(expect) <= 1,
            "{id}: {n} samples, {expect} expected"
        );
        assert!(t.health.gaps.is_empty());
        assert!(proj.audio_mix.contains_key(id));
    }
    let cam = proj
        .recording
        .tracks
        .iter()
        .find(|t| t.id == "camera")
        .expect("camera");
    assert_eq!(
        cam.device.as_ref().map(|d| d.name.as_str()),
        Some("Synthetic Camera")
    );
    let ev = journal(&pdir);
    assert!(matches!(ev.first(), Some(JournalEvent::Start { .. })));
    assert!(matches!(
        ev.last(),
        Some(JournalEvent::Stop {
            reason: StopReason::User,
            ..
        })
    ));
    assert!(ev.iter().any(|e| matches!(e, JournalEvent::Health { .. })));
    assert!(!obs.health.lock().expect("lock").is_empty());
    let log = std::fs::read_to_string(pdir.join("session.log")).expect("log");
    assert!(log.contains("segment_open") && log.contains("finalized"));
    let again = recover_project(&pdir, &*p.decoder()).expect("recover");
    assert_eq!(again.outcome, Outcome::AlreadyFinal);
}

#[test]
fn switch_every_500ms_across_three_sizes() {
    let dir = tmp("switch_500");
    let p = platform(
        Scenario::new()
            .time_scale(2.0)
            .camera(Some(VideoScript {
                size: size(160, 120),
                fps: 30,
            }))
            .mic(Some(mic())),
    );
    let specs = [
        display_spec("a", Some(size(320, 180))),
        display_spec("b", Some(size(256, 144))),
        display_spec("c", Some(size(400, 224))),
    ];
    let mut req = request(&dir, specs[0].clone());
    req.camera = Some(sc_platform::synthetic::CAMERA_DEVICE_ID.into());
    req.mic = Some(sc_platform::synthetic::MIC_DEVICE_ID.into());
    let obs = Arc::new(Obs::default());
    let s = Session::start(as_dyn(&p), req, obs.clone()).expect("start");
    let pdir = s.project_dir().to_path_buf();
    let recordings = || {
        obs.states
            .lock()
            .expect("lock")
            .iter()
            .filter(|st| matches!(st, SessionState::Recording))
            .count()
    };
    for i in 1..12 {
        sleep_host(&*p, 500 * MS);
        s.switch_source(specs[i % 3].clone(), Via::OwnChooser)
            .expect("switch");
        assert!(
            wait_for(Duration::from_secs(10), || recordings() > i),
            "switch {i} never cut over; states: {:?}",
            obs.states.lock().expect("lock")
        );
    }
    sleep_host(&*p, 500 * MS);
    let sum = s.stop(StopReason::User).expect("stop");
    assert_eq!(p.active_screen_streams(), 0);
    assert_eq!(sum.segments, 12, "notices: {:?}", obs.notices());
    let proj = ProjectV1::load(&pdir).expect("load");
    let segs = &proj.recording.screen.segments;
    for (i, sg) in segs.iter().enumerate() {
        let want = [size(320, 180), size(256, 144), size(400, 224)][i % 3];
        assert_eq!(sg.frame_px, want);
        if let Some(n) = segs.get(i + 1) {
            assert_eq!(sg.end_rec_ns, Some(n.start_rec_ns), "segments tile");
            assert_eq!(sg.end_reason, Some(EndReason::UserSwitch));
            assert_eq!(n.start_reason, StartReason::UserSwitch);
        }
    }
    check_segments(&proj, &pdir, &*p.decoder());

    let ev = journal(&pdir);
    let pos = |f: &dyn Fn(&JournalEvent) -> bool| ev.iter().position(f);
    for sg in segs {
        let prep = pos(&|e| matches!(e, JournalEvent::SegmentPrepare { seg, .. } if *seg == sg.id))
            .expect("prepare");
        let open = pos(&|e| matches!(e, JournalEvent::SegmentOpen { seg, .. } if *seg == sg.id))
            .expect("open");
        let close = pos(&|e| matches!(e, JournalEvent::SegmentClose { seg, .. } if *seg == sg.id))
            .expect("close");
        assert!(
            prep < open && open < close,
            "{}: prepare {prep}, open {open}, close {close}",
            sg.id
        );
    }
    for (a, b) in segs.iter().zip(segs.iter().skip(1)) {
        let begin = pos(&|e| matches!(e, JournalEvent::SwitchBegin { .. })).expect("begin");
        assert!(begin < ev.len());
        let close_a = ev.iter().find_map(|e| match e {
            JournalEvent::SegmentClose {
                seg, end_rec_ns, ..
            } if *seg == a.id => Some(*end_rec_ns),
            _ => None,
        });
        assert_eq!(close_a, Some(b.start_rec_ns), "segment_close is at the cut");
    }
    assert!(
        !ev.iter()
            .any(|e| matches!(e, JournalEvent::AudioGap { .. })),
        "switches never touch audio"
    );

    let cam = proj
        .recording
        .tracks
        .iter()
        .find(|t| t.id == "camera")
        .expect("camera");
    let mut r = p
        .decoder()
        .open_video(&pdir.join(&cam.file))
        .expect("camera");
    // Switches must not interrupt the camera. A loaded CI runner can starve the camera writer,
    // and frames it drops are counted; every hole must be such a counted drop.
    let counted = cam.health.dropped + cam.health.writer_not_ready;
    let frame_ns = 1e9 / 30.0;
    let mut prev: Option<i64> = None;
    let mut n = 0u64;
    let mut missing = 0u64;
    while let Some(f) = r.next_frame().expect("frame") {
        if let Some(pv) = prev {
            missing += ((f.file_ns - pv) as f64 / frame_ns).round().max(1.0) as u64 - 1;
        }
        prev = Some(f.file_ns);
        n += 1;
    }
    assert!(
        missing <= counted,
        "camera holes of {missing} frames, {counted} counted as dropped"
    );
    assert!(counted <= n / 5, "camera dropped {counted} of {n}");
    let d = proj.duration_ns().expect("duration");
    assert!(
        (n + counted) as i64 >= d / (34 * MS) - 2,
        "camera {n} frames (+{counted} dropped) over {d} ns"
    );
}

#[test]
fn trailing_old_frames_are_dropped_and_counted() {
    let dir = tmp("trailing");
    let p = platform(Scenario::new().screen(
        "old",
        ScreenScript {
            trailing_after_new_stream: 2,
            ..Default::default()
        },
    ));
    let s = Session::start(
        as_dyn(&p),
        request(&dir, display_spec("old", Some(size(320, 180)))),
        Arc::new(Obs::default()),
    )
    .expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 400 * MS);
    s.switch_source(display_spec("new", Some(size(320, 180))), Via::OwnChooser)
        .expect("switch");
    assert!(wait_for(Duration::from_secs(3), || s.state()
        == SessionState::Recording
        && ProjectV1::load(&pdir).is_ok_and(|p| p
            .recording
            .screen
            .segments
            .len()
            == 2)));
    sleep_host(&*p, 300 * MS);
    s.stop(StopReason::User).expect("stop");
    let trailing = journal(&pdir).iter().find_map(|e| match e {
        JournalEvent::SegmentClose {
            seg,
            trailing_dropped,
            ..
        } if seg == "s0" => Some(*trailing_dropped),
        _ => None,
    });
    assert!(
        matches!(trailing, Some(1..=2)),
        "trailing_dropped {trailing:?}: the old stream runs until its first frame past the cut"
    );
    assert_eq!(p.active_screen_streams(), 0);
    let proj = ProjectV1::load(&pdir).expect("load");
    check_segments(&proj, &pdir, &*p.decoder());
}

#[test]
fn old_stream_without_trailing_frames_stops_after_the_drain() {
    let dir = tmp("no_trailing");
    let p = platform(Scenario::new().screen(
        "old",
        ScreenScript {
            trailing_after_new_stream: 0,
            ..Default::default()
        },
    ));
    let s = Session::start(
        as_dyn(&p),
        request(&dir, display_spec("old", Some(size(320, 180)))),
        Arc::new(Obs::default()),
    )
    .expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 300 * MS);
    s.switch_source(display_spec("new", Some(size(320, 180))), Via::OwnChooser)
        .expect("switch");
    assert!(wait_for(Duration::from_secs(3), || s.state()
        == SessionState::Recording
        && ProjectV1::load(&pdir).is_ok_and(|p| p
            .recording
            .screen
            .segments
            .len()
            == 2)));
    assert_eq!(p.active_screen_streams(), 1);
    s.stop(StopReason::User).expect("stop");
    let trailing = journal(&pdir).iter().find_map(|e| match e {
        JournalEvent::SegmentClose {
            seg,
            trailing_dropped,
            ..
        } if seg == "s0" => Some(*trailing_dropped),
        _ => None,
    });
    assert_eq!(trailing, Some(0));
}

fn switch_failures(next: &[JournalEvent]) -> Vec<SwitchNext> {
    next.iter()
        .filter_map(|e| match e {
            JournalEvent::SwitchFailed { next, .. } => Some(*next),
            _ => None,
        })
        .collect()
}

#[test]
fn start_failing_while_old_runs_retries_stop_then_start() {
    let dir = tmp("retry");
    let p = platform(Scenario::new().screen(
        "shy",
        ScreenScript {
            start_fails: StartFailure::WhileOtherStreamRuns,
            ..Default::default()
        },
    ));
    let obs = Arc::new(Obs::default());
    let s = Session::start(
        as_dyn(&p),
        request(&dir, display_spec("a", Some(size(320, 180)))),
        obs.clone(),
    )
    .expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 400 * MS);
    s.switch_source(display_spec("shy", Some(size(320, 180))), Via::OwnChooser)
        .expect("switch");
    assert!(wait_for(Duration::from_secs(3), || ProjectV1::load(&pdir)
        .is_ok_and(|p| p.recording.screen.segments.len() == 2)));
    assert_eq!(p.active_screen_streams(), 1);
    sleep_host(&*p, 300 * MS);
    s.stop(StopReason::User).expect("stop");
    let ev = journal(&pdir);
    assert_eq!(switch_failures(&ev), vec![SwitchNext::RetryStopThenStart]);
    let proj = ProjectV1::load(&pdir).expect("load");
    check_segments(&proj, &pdir, &*p.decoder());
    let segs = &proj.recording.screen.segments;
    assert_eq!(
        segs[0].end_rec_ns,
        Some(segs[1].start_rec_ns),
        "the hold covers the stop-then-start gap"
    );
    let t0 = proj
        .recording
        .tracks
        .iter()
        .find(|t| t.id == "screen-0")
        .expect("t0");
    let mut r = p.decoder().open_video(&pdir.join(&t0.file)).expect("open");
    let mut last_real = 0;
    let start = proj.recording.start_host_ns;
    while let Some(f) = r.next_frame().expect("frame") {
        let mut code = None;
        f.frame
            .0
            .with_nv12(&mut |v| code = read_code(&v))
            .expect("nv12");
        last_real = last_real.max(i64::from(code.expect("code").pts_ms) * MS - start);
    }
    let gap = segs[1].start_rec_ns - last_real;
    println!("stop-then-start gap: {} ms", gap / MS);
    assert!(
        gap <= 30 * MS + 34 * MS + 20 * MS + ci_slack_ns(),
        "gap {} ms exceeds stop + start latency",
        gap / MS
    );
    assert!(
        !obs.notices()
            .iter()
            .any(|n| matches!(n, Notice::SwitchFailed { .. }))
    );
}

#[test]
fn retry_failing_restores_the_old_source() {
    let dir = tmp("restore");
    let p = platform(Scenario::new().screen(
        "broken",
        ScreenScript {
            start_fails: StartFailure::Always,
            ..Default::default()
        },
    ));
    let obs = Arc::new(Obs::default());
    let s = Session::start(
        as_dyn(&p),
        request(&dir, display_spec("a", Some(size(320, 180)))),
        obs.clone(),
    )
    .expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 400 * MS);
    s.switch_source(display_spec("broken", None), Via::OwnChooser)
        .expect("switch");
    assert!(wait_for(Duration::from_secs(3), || obs
        .notices()
        .iter()
        .any(|n| matches!(
            n,
            Notice::SwitchFailed { restored: true, .. }
        ))));
    assert_eq!(p.active_screen_streams(), 1);
    sleep_host(&*p, 300 * MS);
    s.stop(StopReason::User).expect("stop");
    let ev = journal(&pdir);
    assert_eq!(
        switch_failures(&ev),
        vec![SwitchNext::RetryStopThenStart, SwitchNext::RestoreOld]
    );
    let proj = ProjectV1::load(&pdir).expect("load");
    let ids: Vec<_> = proj
        .recording
        .screen
        .segments
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(ids, ["s0", "s2"]);
    assert!(
        !pdir.join("tracks/screen-001.sctrk").exists(),
        "prepared file of the failed switch removed"
    );
    assert!(
        proj.recording
            .screen
            .segments
            .iter()
            .all(|s| source_key(&s.source) == "a")
    );
    check_segments(&proj, &pdir, &*p.decoder());
}

/// Regression (ADR-039 review): a restored application segment recorded the crop from when the old
/// segment started, not the one `describe()` returned for the restored stream.
#[test]
fn restored_application_segment_records_the_current_crop() {
    let dir = tmp("restore_crop");
    let crop = |x: f64| RectPt {
        x,
        y: 10.0,
        w: 320.0,
        h: 180.0,
    };
    let p = platform(
        Scenario::new()
            .screen(
                "app:42",
                ScreenScript {
                    size: size(320, 180),
                    describe_crops: vec![crop(10.0), crop(200.0)],
                    ..Default::default()
                },
            )
            .screen(
                "broken",
                ScreenScript {
                    start_fails: StartFailure::Always,
                    ..Default::default()
                },
            ),
    );
    let obs = Arc::new(Obs::default());
    let spec = SourceSpec::Application {
        display: DisplayId("synthetic:main".into()),
        app: AppRef {
            pid: 42,
            bundle_id: None,
        },
    };
    let s = Session::start(as_dyn(&p), request(&dir, spec), obs.clone()).expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 300 * MS);
    s.switch_source(display_spec("broken", None), Via::OwnChooser)
        .expect("switch");
    assert!(wait_for(Duration::from_secs(3), || obs
        .notices()
        .iter()
        .any(|n| matches!(
            n,
            Notice::SwitchFailed { restored: true, .. }
        ))));
    sleep_host(&*p, 300 * MS);
    s.stop(StopReason::User).expect("stop");
    let crops = |segs: Vec<&SourceDescriptor>| -> Vec<Option<RectPt>> {
        segs.into_iter()
            .map(|d| match d {
                SourceDescriptor::Application { crop_pt, .. } => *crop_pt,
                other => panic!("not an application: {other:?}"),
            })
            .collect()
    };
    let proj = ProjectV1::load(&pdir).expect("load");
    let segs = &proj.recording.screen.segments;
    assert_eq!(
        segs.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["s0", "s2"]
    );
    assert_eq!(
        crops(segs.iter().map(|s| &s.source).collect()),
        vec![Some(crop(10.0)), Some(crop(200.0))]
    );
    let ev = journal(&pdir);
    let opened = ev
        .iter()
        .filter_map(|e| match e {
            JournalEvent::SegmentOpen { source, .. } => Some(source),
            _ => None,
        })
        .collect();
    assert_eq!(crops(opened), vec![Some(crop(10.0)), Some(crop(200.0))]);
    check_segments(&proj, &pdir, &*p.decoder());
}

#[test]
fn idle_new_source_keeps_old_after_first_frame_timeout() {
    let dir = tmp("keep_old");
    let p = platform(Scenario::new().screen(
        "frozen",
        ScreenScript {
            idle_from_start: true,
            ..Default::default()
        },
    ));
    let obs = Arc::new(Obs::default());
    let mut req = request(&dir, display_spec("a", Some(size(320, 180))));
    req.tuning.switch_first_frame_timeout_ms = 300;
    let s = Session::start(as_dyn(&p), req, obs.clone()).expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 300 * MS);
    s.switch_source(display_spec("frozen", None), Via::OwnChooser)
        .expect("switch");
    assert!(wait_for(Duration::from_secs(3), || obs
        .notices()
        .iter()
        .any(|n| matches!(
            n,
            Notice::SwitchFailed { restored: true, .. }
        ))));
    assert_eq!(
        p.active_screen_streams(),
        1,
        "never two streams after a switch settles"
    );
    assert_eq!(s.state(), SessionState::Recording);
    sleep_host(&*p, 200 * MS);
    s.stop(StopReason::User).expect("stop");
    assert_eq!(switch_failures(&journal(&pdir)), vec![SwitchNext::KeepOld]);
    assert!(!pdir.join("tracks/screen-001.sctrk").exists());
    let proj = ProjectV1::load(&pdir).expect("load");
    assert_eq!(proj.recording.screen.segments.len(), 1);
    check_segments(&proj, &pdir, &*p.decoder());
}

#[test]
fn source_lost_then_switch() {
    let dir = tmp("lost");
    let p = platform(Scenario::new().screen(
        "lost",
        ScreenScript {
            lost_after_ns: Some(600 * MS),
            size: size(320, 180),
            ..Default::default()
        },
    ));
    let obs = Arc::new(Obs::default());
    let s = Session::start(
        as_dyn(&p),
        request(&dir, display_spec("lost", None)),
        obs.clone(),
    )
    .expect("start");
    let pdir = s.project_dir().to_path_buf();
    assert!(wait_for(Duration::from_secs(10), || obs
        .notices()
        .iter()
        .any(|n| matches!(n, Notice::SourceLost { .. }))));
    assert_eq!(p.active_screen_streams(), 0);
    assert_eq!(
        s.state(),
        SessionState::Recording,
        "audio and camera continue"
    );
    sleep_host(&*p, 300 * MS);
    s.switch_source(display_spec("b", Some(size(256, 144))), Via::OwnChooser)
        .expect("switch");
    assert!(wait_for(Duration::from_secs(3), || ProjectV1::load(&pdir)
        .is_ok_and(|p| p.recording.screen.segments.len() == 2)));
    sleep_host(&*p, 300 * MS);
    s.stop(StopReason::User).expect("stop");
    let proj = ProjectV1::load(&pdir).expect("load");
    let segs = &proj.recording.screen.segments;
    assert_eq!(segs[0].end_reason, Some(EndReason::SourceLost));
    assert!(
        segs[1].start_rec_ns >= segs[0].end_rec_ns.expect("end") + 200 * MS,
        "a held gap between the segments"
    );
    assert_eq!(segs[1].end_reason, Some(EndReason::RecordStop));
    check_segments(&proj, &pdir, &*p.decoder());
    assert!(
        journal(&pdir)
            .iter()
            .any(|e| matches!(e, JournalEvent::SourceLost { seg, .. } if seg == "s0"))
    );
}

#[test]
fn device_lost_finishes_that_track_and_continues() {
    let dir = tmp("device_lost");
    let p = platform(
        Scenario::new()
            .mic(Some(mic()))
            .camera(Some(VideoScript {
                size: size(160, 120),
                fps: 30,
            }))
            .at(500 * MS, ScriptedEvent::DeviceLost(DeviceKind::Mic)),
    );
    let obs = Arc::new(Obs::default());
    let mut req = request(&dir, display_spec("a", Some(size(320, 180))));
    req.mic = Some(sc_platform::synthetic::MIC_DEVICE_ID.into());
    req.camera = Some(sc_platform::synthetic::CAMERA_DEVICE_ID.into());
    let s = Session::start(as_dyn(&p), req, obs.clone()).expect("start");
    let pdir = s.project_dir().to_path_buf();
    assert!(wait_for(Duration::from_secs(3), || obs
        .notices()
        .iter()
        .any(|n| matches!(
            n,
            Notice::DeviceLost {
                kind: DeviceKind::Mic,
                ..
            }
        ))));
    assert_eq!(s.state(), SessionState::Recording);
    sleep_host(&*p, 600 * MS);
    s.stop(StopReason::User).expect("stop");
    let ev = journal(&pdir);
    let lost = ev
        .iter()
        .position(|e| {
            matches!(
                e,
                JournalEvent::Device {
                    change: sc_core::journal::DeviceChange::Lost,
                    ..
                }
            )
        })
        .expect("device lost");
    let fin = ev
        .iter()
        .position(
            |e| matches!(e, JournalEvent::TrackFinished { track_id, .. } if track_id == "mic"),
        )
        .expect("mic finished");
    let stop = ev
        .iter()
        .position(|e| matches!(e, JournalEvent::Stop { .. }))
        .expect("stop");
    assert!(lost < fin && fin < stop);
    let proj = ProjectV1::load(&pdir).expect("load");
    let d = proj.duration_ns().expect("d");
    let mic_t = proj
        .recording
        .tracks
        .iter()
        .find(|t| t.id == "mic")
        .expect("mic");
    let cam_t = proj
        .recording
        .tracks
        .iter()
        .find(|t| t.id == "camera")
        .expect("camera");
    assert!(mic_t.duration_ns.expect("mic d") < d - 400 * MS);
    assert!(cam_t.duration_ns.expect("cam d") > d - 100 * MS);
}

#[test]
fn sleep_stops_and_finalizes() {
    let dir = tmp("sleep");
    let p = platform(Scenario::new().at(600 * MS, ScriptedEvent::WillSleep));
    let obs = Arc::new(Obs::default());
    let s = Session::start(
        as_dyn(&p),
        request(&dir, display_spec("a", Some(size(320, 180)))),
        obs.clone(),
    )
    .expect("start");
    let pdir = s.project_dir().to_path_buf();
    assert!(wait_for(Duration::from_secs(3), || matches!(
        s.state(),
        SessionState::Finalized { .. }
    )));
    assert!(obs.notices_at_end().contains(&Notice::StoppedForSleep));
    assert!(
        s.switch_source(display_spec("b", None), Via::OwnChooser)
            .is_err()
    );
    let sum = s
        .stop(StopReason::User)
        .expect("summary of the self-stopped recording");
    assert_eq!(sum.project_dir, pdir);
    assert!(matches!(
        journal(&pdir).last(),
        Some(JournalEvent::Stop {
            reason: StopReason::Sleep,
            ..
        })
    ));
    assert_eq!(
        ProjectV1::load(&pdir).expect("load").state,
        ProjectState::Finalized
    );
    assert!(!pdir.join(".lock").exists());
}

#[test]
fn low_disk_warns_then_stops() {
    let dir = tmp("disk");
    let gb = 1_000_000_000u64;
    let p = platform(Scenario::new().disk(DiskScript::Linear {
        start: 25 * gb / 10,
        bytes_per_s: -(gb as i64),
    }));
    let obs = Arc::new(Obs::default());
    let mut req = request(&dir, display_spec("a", Some(size(320, 180))));
    req.tuning.disk_warn_bytes = 2 * gb;
    req.tuning.disk_stop_bytes = gb;
    let s = Session::start(as_dyn(&p), req, obs.clone()).expect("start");
    let pdir = s.project_dir().to_path_buf();
    assert!(wait_for(Duration::from_secs(5), || matches!(
        s.state(),
        SessionState::Finalized { .. }
    )));
    let n = obs.notices_at_end();
    let low = n
        .iter()
        .position(|x| matches!(x, Notice::DiskLow { .. }))
        .expect("disk low");
    let stopped = n
        .iter()
        .position(|x| *x == Notice::StoppedForDisk)
        .expect("stopped");
    assert!(low < stopped);
    assert!(matches!(
        journal(&pdir).last(),
        Some(JournalEvent::Stop {
            reason: StopReason::Disk,
            ..
        })
    ));
    s.stop(StopReason::User).expect("summary");
}

#[test]
fn not_enough_disk_to_start() {
    let dir = tmp("disk_start");
    let p = platform(Scenario::new().disk(DiskScript::Constant(600_000_000)));
    let r = Session::start(
        as_dyn(&p),
        request(&dir, display_spec("a", None)),
        Arc::new(Obs::default()),
    );
    assert!(matches!(r, Err(Error::State(_))), "{r:?}");
    assert_eq!(std::fs::read_dir(&dir).expect("dir").count(), 0);
}

#[test]
fn denied_screen_permission_fails_fast_without_a_folder() {
    let dir = tmp("denied");
    let p = platform(Scenario::new().permissions(
        PermissionStatus::Denied,
        PermissionStatus::Granted,
        PermissionStatus::Granted,
    ));
    let r = Session::start(
        as_dyn(&p),
        request(&dir, display_spec("a", None)),
        Arc::new(Obs::default()),
    );
    assert!(
        matches!(
            r,
            Err(Error::Platform(PError::PermissionDenied(
                PermissionKind::Screen
            )))
        ),
        "{r:?}"
    );
    assert_eq!(std::fs::read_dir(&dir).expect("dir").count(), 0);
    let p = platform(Scenario::new().mic(Some(mic())).permissions(
        PermissionStatus::Granted,
        PermissionStatus::Granted,
        PermissionStatus::Denied,
    ));
    let mut req = request(&dir, display_spec("a", None));
    req.mic = Some(sc_platform::synthetic::MIC_DEVICE_ID.into());
    let r = Session::start(as_dyn(&p), req, Arc::new(Obs::default()));
    assert!(
        matches!(
            r,
            Err(Error::Platform(PError::PermissionDenied(
                PermissionKind::Microphone
            )))
        ),
        "{r:?}"
    );
    assert_eq!(std::fs::read_dir(&dir).expect("dir").count(), 0);
}

#[test]
fn start_timeout_removes_the_folder() {
    let dir = tmp("start_timeout");
    let p = platform(Scenario::new().screen(
        "frozen",
        ScreenScript {
            idle_from_start: true,
            ..Default::default()
        },
    ));
    let mut req = request(&dir, display_spec("frozen", None));
    req.tuning.start_timeout_ms = 300;
    let r = Session::start(as_dyn(&p), req, Arc::new(Obs::default()));
    assert!(
        matches!(r, Err(Error::Platform(PError::Timeout(_)))),
        "{r:?}"
    );
    assert_eq!(std::fs::read_dir(&dir).expect("dir").count(), 0);
    assert_eq!(p.active_screen_streams(), 0);
}

#[test]
fn original_ratio_comes_from_the_first_frame() {
    let dir = tmp("ratio");
    let p = platform(
        Scenario::new()
            .screen(
                "retina",
                ScreenScript {
                    first_frame_size: Some(size(640, 400)),
                    ..Default::default()
                },
            )
            .screen(
                "odd",
                ScreenScript {
                    first_frame_size: Some(size(288, 160)),
                    ..Default::default()
                },
            ),
    );
    let mut req = request(&dir, display_spec("retina", Some(size(320, 180))));
    req.canvas.aspect = Aspect::Original;
    req.canvas.ratio = Ratio::new(16, 9);
    let s = Session::start(as_dyn(&p), req, Arc::new(Obs::default())).expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 300 * MS);
    s.switch_source(display_spec("odd", Some(size(320, 180))), Via::OwnChooser)
        .expect("switch");
    assert!(wait_for(Duration::from_secs(3), || ProjectV1::load(&pdir)
        .is_ok_and(|p| p.recording.screen.segments.len() == 2)));
    sleep_host(&*p, 300 * MS);
    s.stop(StopReason::User).expect("stop");
    let proj = ProjectV1::load(&pdir).expect("load");
    assert_eq!(proj.output.canvas.ratio, Ratio::new(8, 5));
    assert_eq!(proj.recording.screen.segments[0].frame_px, size(640, 400));
    assert_eq!(proj.recording.screen.segments[1].frame_px, size(288, 160));
    check_segments(&proj, &pdir, &*p.decoder());
    let log = std::fs::read_to_string(pdir.join("session.log")).expect("log");
    assert!(log.contains("writer recreated"));
}

#[test]
fn audio_gaps_are_filled_and_overlaps_trimmed() {
    let dir = tmp("audio_gaps");
    let script = AudioScript {
        gaps: vec![(500 * MS, 40 * MS)],
        overlaps: vec![(900 * MS, 5 * MS)],
        ..mic()
    };
    let p = platform(Scenario::new().mic(Some(script)));
    let obs = Arc::new(Obs::default());
    let mut req = request(&dir, display_spec("a", Some(size(320, 180))));
    req.mic = Some(sc_platform::synthetic::MIC_DEVICE_ID.into());
    let s = Session::start(as_dyn(&p), req, obs.clone()).expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 1500 * MS);
    s.stop(StopReason::User).expect("stop");
    let ev = journal(&pdir);
    let kinds: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            JournalEvent::AudioGap {
                track,
                kind,
                dur_ns,
                ..
            } if track == "mic" => Some((*kind, *dur_ns)),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        vec![(GapKind::Gap, 40 * MS), (GapKind::Overlap, 5 * MS)]
    );
    assert!(obs.notices().iter().any(|n| matches!(n, Notice::AudioGap { track: AudioTrackKind::Mic, dur_ns } if *dur_ns == 40 * MS)));
    let proj = ProjectV1::load(&pdir).expect("load");
    let t = proj
        .recording
        .tracks
        .iter()
        .find(|t| t.id == "mic")
        .expect("mic");
    assert_eq!(t.health.gaps.len(), 1);
    assert_eq!(t.health.gaps[0].dur_ns, 40 * MS);
    let end = t.first_sample_offset_ns + t.duration_ns.expect("d");
    assert!(
        (end - proj.duration_ns().expect("d")).abs() < 30 * MS + ci_slack_ns(),
        "mic ends at {end}"
    );
}

#[test]
fn exclusions_and_app_audio_follow_the_session() {
    let dir = tmp("exclusions");
    let p = platform(
        Scenario::new()
            .system_audio(Some(AudioScript {
                channels: 2,
                ..mic()
            }))
            .at(300 * MS, ScriptedEvent::OwnWindowsChanged),
    );
    let app = AppRef {
        pid: 42,
        bundle_id: None,
    };
    let mut req = request(
        &dir,
        SourceSpec::Application {
            display: DisplayId("synthetic:main".into()),
            app: app.clone(),
        },
    );
    req.system_audio = SystemAudioMode::App;
    req.exclude = vec![WindowId(7)];
    let s = Session::start(as_dyn(&p), req, Arc::new(Obs::default())).expect("start");
    assert!(wait_for(Duration::from_secs(3), || !p
        .exclusion_updates()
        .is_empty()));
    s.set_exclusions(vec![WindowId(7), WindowId(9)])
        .expect("set exclusions");
    let ups = p.exclusion_updates();
    assert_eq!(ups[0].1, vec![WindowId(7)]);
    assert_eq!(ups.last().expect("last").1, vec![WindowId(7), WindowId(9)]);
    s.switch_source(display_spec("b", Some(size(320, 180))), Via::OwnChooser)
        .expect("switch");
    assert!(wait_for(Duration::from_secs(3), || p.followed_apps().len() >= 2));
    s.stop(StopReason::User).expect("stop");
    assert_eq!(p.followed_apps(), vec![Some(app), None]);
}

#[test]
fn discard_removes_the_project() {
    let dir = tmp("discard");
    let p = platform(Scenario::new());
    let s = Session::start(
        as_dyn(&p),
        request(&dir, display_spec("a", Some(size(320, 180)))),
        Arc::new(Obs::default()),
    )
    .expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 200 * MS);
    s.discard().expect("discard");
    assert!(!pdir.exists());
    assert_eq!(p.active_screen_streams(), 0);
}

#[test]
fn dropping_a_session_finalizes_it() {
    let dir = tmp("drop");
    let p = platform(Scenario::new());
    let s = Session::start(
        as_dyn(&p),
        request(&dir, display_spec("a", Some(size(320, 180)))),
        Arc::new(Obs::default()),
    )
    .expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 200 * MS);
    drop(s);
    assert!(matches!(
        journal(&pdir).last(),
        Some(JournalEvent::Stop {
            reason: StopReason::Quit,
            ..
        })
    ));
    assert_eq!(p.active_screen_streams(), 0);
}

/// Regression (wp-e2e-cli): the session log had no record of the requested settings, so a run
/// with `--no-cursor` could not be checked from its project folder.
#[test]
fn session_log_records_the_config() {
    let dir = tmp("config_log");
    let p = platform(Scenario::new().screen(
        "main",
        ScreenScript {
            size: size(320, 180),
            ..Default::default()
        },
    ));
    let mut req = request(&dir, display_spec("main", None));
    req.show_cursor = false;
    req.resolution = Resolution::OneX;
    let s = Session::start(as_dyn(&p), req, Arc::new(Obs::default())).expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 300 * MS);
    s.stop(StopReason::User).expect("stop");
    let log = std::fs::read_to_string(pdir.join("session.log")).expect("log");
    let line = log
        .lines()
        .find(|l| l.contains(" INFO config {"))
        .unwrap_or_else(|| panic!("no config line in {log}"));
    assert!(line.contains(r#""cursor":false"#), "{line}");
    assert!(line.contains(r#""resolution":"1x""#), "{line}");
    assert!(line.contains(r#""system_audio":"off""#), "{line}");
}

/// Regression (wp-e2e-cli): continuous audio ended where its last buffer ended (system audio
/// 40–60 ms before the stop on the M3 Pro); it now reaches the stop, padded with silence.
#[test]
fn audio_tracks_reach_the_stop() {
    let dir = tmp("audio_tail");
    let p = platform(
        Scenario::new()
            .screen(
                "main",
                ScreenScript {
                    size: size(320, 180),
                    ..Default::default()
                },
            )
            .mic(Some(AudioScript {
                buffer_frames: 4800,
                ..mic()
            })),
    );
    let mut req = request(&dir, display_spec("main", None));
    req.mic = Some(sc_platform::synthetic::MIC_DEVICE_ID.into());
    let s = Session::start(as_dyn(&p), req, Arc::new(Obs::default())).expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 1250 * MS);
    s.stop(StopReason::User).expect("stop");
    let proj = ProjectV1::load(&pdir).expect("load");
    let dur = proj.duration_ns().expect("duration");
    let mic = proj
        .recording
        .tracks
        .iter()
        .find(|t| t.id == "mic")
        .expect("mic track");
    let end = mic.first_sample_offset_ns + mic.duration_ns.expect("mic duration");
    assert!(
        (end - dur).abs() <= MS,
        "mic ends at {end} ns, recording at {dur} ns"
    );
}

/// Invariant behind two live bugs (wp-e2e-cli, `just e2e` steps 4 and 14): s0 started at the
/// first frame's offset instead of 0, and frames arriving after the stop time pushed the last
/// segment past the recording's end. The synthetic platform cannot produce either race, so this
/// only guards the invariant; the e2e run is the reproducer.
#[test]
fn segments_span_exactly_zero_to_the_stop() {
    for i in 0..5 {
        let dir = tmp(&format!("span_{i}"));
        let p = platform(Scenario::new().screen(
            "main",
            ScreenScript {
                size: size(320, 180),
                ..Default::default()
            },
        ));
        let s = Session::start(
            as_dyn(&p),
            request(&dir, display_spec("main", None)),
            Arc::new(Obs::default()),
        )
        .expect("start");
        let pdir = s.project_dir().to_path_buf();
        sleep_host(&*p, (400 + 37 * i) * MS);
        s.stop(StopReason::User).expect("stop");
        let proj = ProjectV1::load(&pdir).expect("load");
        let segs = &proj.recording.screen.segments;
        assert_eq!(segs[0].start_rec_ns, 0, "{segs:?}");
        assert_eq!(
            segs.last().and_then(|s| s.end_rec_ns),
            proj.duration_ns(),
            "{segs:?}"
        );
    }
}

/// Regression (CI cli `synthetic_record_with_switch`): `record --duration 3` exported 3.066 s,
/// because the stop was taken when the late caller got to it, not at the requested length. With
/// `duration_ns` the length is exact however late the stop comes, and the segments still tile it.
#[test]
fn duration_is_exact_when_the_stop_comes_late() {
    let runs: Vec<_> = (0..4i64)
        .map(|i| {
            std::thread::spawn(move || {
                let dir = tmp(&format!("late_stop_{i}"));
                let p = platform(
                    Scenario::new()
                        .screen(
                            "a",
                            ScreenScript {
                                size: size(320, 180),
                                ..Default::default()
                            },
                        )
                        .screen(
                            "b",
                            ScreenScript {
                                size: size(256, 144),
                                ..Default::default()
                            },
                        )
                        .mic(Some(mic())),
                );
                let want = (900 + 37 * i) * MS;
                let mut req = request(&dir, display_spec("a", None));
                req.mic = Some(sc_platform::synthetic::MIC_DEVICE_ID.into());
                req.duration_ns = Some(want);
                let s = Session::start(as_dyn(&p), req, Arc::new(Obs::default())).expect("start");
                let pdir = s.project_dir().to_path_buf();
                sleep_host(&*p, 400 * MS);
                s.switch_source(display_spec("b", None), Via::OwnChooser)
                    .expect("switch");
                sleep_host(&*p, want - 400 * MS + (100 + 150 * i) * MS);
                let sum = s.stop(StopReason::Duration).expect("stop");
                assert_eq!(sum.duration_ns, want);
                let proj = ProjectV1::load(&pdir).expect("load");
                assert_eq!(proj.duration_ns(), Some(want));
                assert_eq!(
                    proj.recording.stop_host_ns,
                    Some(proj.recording.start_host_ns + want)
                );
                let segs = &proj.recording.screen.segments;
                assert_eq!(segs[0].start_rec_ns, 0, "{segs:?}");
                for w in segs.windows(2) {
                    assert_eq!(w[0].end_rec_ns, Some(w[1].start_rec_ns), "{segs:?}");
                }
                assert_eq!(
                    segs.last().and_then(|s| s.end_rec_ns),
                    Some(want),
                    "{segs:?}"
                );
                check_segments(&proj, &pdir, &*p.decoder());
                let mic = proj
                    .recording
                    .tracks
                    .iter()
                    .find(|t| t.id == "mic")
                    .expect("mic track");
                let end = mic.first_sample_offset_ns + mic.duration_ns.expect("mic duration");
                assert!((end - want).abs() <= MS, "mic ends at {end}, want {want}");
            })
        })
        .collect();
    for r in runs {
        r.join().expect("run");
    }
}

/// A stop before the requested length ends the recording at the stop.
#[test]
fn stop_before_the_duration_ends_there() {
    let dir = tmp("early_stop");
    let p = platform(Scenario::new().screen(
        "main",
        ScreenScript {
            size: size(320, 180),
            ..Default::default()
        },
    ));
    let mut req = request(&dir, display_spec("main", None));
    req.duration_ns = Some(10_000 * MS);
    let s = Session::start(as_dyn(&p), req, Arc::new(Obs::default())).expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 500 * MS);
    let sum = s.stop(StopReason::User).expect("stop");
    assert!(
        sum.duration_ns >= 500 * MS && sum.duration_ns < 5_000 * MS,
        "{}",
        sum.duration_ns
    );
    let proj = ProjectV1::load(&pdir).expect("load");
    let segs = &proj.recording.screen.segments;
    assert_eq!(segs.last().and_then(|s| s.end_rec_ns), proj.duration_ns());
    check_segments(&proj, &pdir, &*p.decoder());
}

/// Hosted CI runners are slow and shared: real-time limits get 100 ms more there.
fn ci_slack_ns() -> i64 {
    if std::env::var_os("CI").is_some() {
        100 * MS
    } else {
        0
    }
}

#[test]
fn camera_keys_are_recorded_journaled_and_recovered() {
    let dir = tmp("camera_keys");
    let p = platform(Scenario::new().time_scale(2.0).camera(Some(VideoScript {
        size: size(320, 240),
        fps: 30,
    })));
    let mut req = request(&dir, display_spec("main", None));
    req.camera = Some(sc_platform::synthetic::CAMERA_DEVICE_ID.into());
    let first = sc_core::project::CameraKey::default_placement();
    req.camera_key = Some(first.clone());
    let s = Session::start(as_dyn(&p), req, Arc::new(Obs::default())).expect("start");
    let pdir = s.project_dir().to_path_buf();
    sleep_host(&*p, 300 * MS);
    let mut moved = first.clone();
    moved.center_norm.x = 0.2;
    s.camera_key(moved.clone()).expect("key");
    let mut dragged = moved.clone();
    dragged.center_norm.x = 0.25;
    s.camera_key(dragged.clone()).expect("key");
    sleep_host(&*p, 300 * MS);
    let mut hidden = dragged.clone();
    hidden.visible = false;
    hidden.easing = sc_core::project::Easing::Step;
    s.camera_key(hidden).expect("key");
    sleep_host(&*p, 300 * MS);
    s.stop(StopReason::User).expect("stop");

    let proj = ProjectV1::load(&pdir).expect("load");
    let o = &proj.camera_layout.overlays;
    assert_eq!(o.len(), 1, "{o:?}");
    assert_eq!(o[0].track_id, "camera");
    let keys = &o[0].keyframes;
    assert_eq!(keys.len(), 4);
    assert_eq!(keys[0].t_rec_ns, 0);
    assert!(
        keys[1].t_rec_ns > 0
            && keys[2].t_rec_ns >= keys[1].t_rec_ns
            && keys[3].t_rec_ns > keys[2].t_rec_ns,
        "{keys:?}"
    );
    assert_eq!(keys[1].center_norm.x, 0.2);
    assert_eq!(
        keys[1].easing,
        sc_core::project::Easing::Step,
        "a key after a pause is a jump"
    );
    assert_eq!(keys[2].center_norm.x, 0.25);
    assert_eq!(
        keys[2].easing,
        sc_core::project::Easing::Linear,
        "a key right after another is motion"
    );
    assert!(!keys[3].visible);
    let at = |t| sc_core::camera::camera_at(keys, t);
    let before_move = at(keys[1].t_rec_ns / 2).expect("shown");
    assert_eq!(
        before_move.pose.center.x, first.center_norm.x,
        "the camera holds still until the move"
    );
    assert!(at(keys[1].t_rec_ns).is_some());
    assert!(
        at(keys[3].t_rec_ns).is_none(),
        "hidden from the last key on"
    );

    let journaled: usize = journal(&pdir)
        .iter()
        .map(|e| match e {
            JournalEvent::CameraKeys { keys } => keys.len(),
            _ => 0,
        })
        .sum();
    assert_eq!(journaled, 4, "every key reaches the journal");

    let mut crashed = proj.clone();
    crashed.state = ProjectState::Recording;
    crashed.camera_layout.overlays.clear();
    crashed.save(&pdir).expect("save");
    let r = recover_project(&pdir, &*p.decoder()).expect("recover");
    assert_eq!(r.outcome, Outcome::Recovered, "{r:?}");
    let back = ProjectV1::load(&pdir).expect("load");
    assert_eq!(
        back.camera_layout.overlays[0].keyframes, *keys,
        "rebuilt from the journal"
    );
}
