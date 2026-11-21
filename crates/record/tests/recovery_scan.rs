use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sc_core::ClockKind;
use sc_core::folder::ProjectLock;
use sc_core::journal::{
    DeviceChange, DeviceKindJ, GapKind, HealthEntry, JournalEvent, JournalWriter, StopReason,
};
use sc_core::project::{
    AudioInfo, AudioMixEntry, EndReason, Extra, ProjectState, ProjectV1, Resolution,
    SourceDescriptor, StartReason, SystemAudioMode, Track, TrackHealth, TrackKind, Via,
};
use sc_geometry::{Ratio, RectPx, SizePx};
use sc_platform::synthetic::{FrameCode, SctrkDecoder, SctrkWriterFactory, coded_frame};
use sc_platform::{
    AudioBuffer, AudioData, AudioWriterSpec, Decoder, FrameBuffer, HostTime, StreamTag,
    TrackWriter, VideoCodec, VideoFrame, VideoWriterSpec, WriterFactory,
};
use sc_record::recovery::{Outcome, recover_project, scan};

const START: i64 = 1_000_000_000_000;
const FRAME_NS: i64 = 1_000_000_000 / 30;
const SEC: i64 = 1_000_000_000;
const FRAME: SizePx = SizePx { w: 64, h: 36 };
const CONTENT: RectPx = RectPx {
    x: 2,
    y: 0,
    w: 60,
    h: 36,
};
const FOLDER: &str = "2026-11-19 10.00.00 Test.sc";

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> TempRoot {
        let p = std::env::temp_dir().join(format!(
            "sc-recovery-{name}-{}-{}",
            std::process::id(),
            sc_core::ids::new_id()
        ));
        fs::create_dir_all(&p).expect("create temp root");
        TempRoot(p)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Fx {
    dir: PathBuf,
    j: JournalWriter,
}

impl Fx {
    fn new(root: &Path) -> Fx {
        Fx::named(root, FOLDER)
    }

    fn named(root: &Path, folder: &str) -> Fx {
        let dir = root.join(folder);
        fs::create_dir_all(dir.join("tracks")).expect("create project dir");
        let mut p = ProjectV1::new_recording(
            "00000000-0000-4000-8000-000000000000".to_string(),
            "Test".to_string(),
            "2026-11-19T10:00:00+00:00".to_string(),
            "0.0.0-test".to_string(),
            ClockKind::MachHost,
        );
        p.recording.start_host_ns = START;
        p.recording.tracks.push(mic_track());
        p.audio_mix.insert(
            "mic".to_string(),
            AudioMixEntry {
                gain_db: 0.0,
                muted: false,
                extra: Extra::new(),
            },
        );
        p.save(&dir).expect("save project");
        let mut fx = Fx {
            j: JournalWriter::create(&dir).expect("journal"),
            dir,
        };
        fx.ev(start_ev());
        fx
    }

    fn ev(&mut self, ev: JournalEvent) {
        self.j.append(&ev).expect("append journal");
    }

    fn open(&mut self, idx: u32, start_rec_ns: i64) {
        self.ev(JournalEvent::SegmentOpen {
            seg: format!("s{idx}"),
            track_id: format!("screen-{idx}"),
            start_rec_ns,
            file: seg_file(idx),
            via: Via::OwnChooser,
            reason: if idx == 0 {
                StartReason::RecordStart
            } else {
                StartReason::UserSwitch
            },
            source: SourceDescriptor::Display {
                display_id: "1".to_string(),
                display_name: None,
                scale: 2.0,
                frame_pt: None,
            },
            frame_px: FRAME,
            content_px: CONTENT,
        });
    }

    fn close(&mut self, idx: u32, end_rec_ns: i64) {
        self.ev(JournalEvent::SegmentClose {
            seg: format!("s{idx}"),
            end_rec_ns,
            reason: EndReason::UserSwitch,
            trailing_dropped: 0,
        });
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    fn project(&self) -> ProjectV1 {
        ProjectV1::load(&self.dir).expect("load project")
    }
}

fn start_ev() -> JournalEvent {
    JournalEvent::Start {
        start_host_ns: START,
        clock: ClockKind::MachHost,
        fps_requested: 60,
        resolution: "1x".to_string(),
        system_audio: SystemAudioMode::Off,
    }
}

fn seg_file(idx: u32) -> String {
    format!("tracks/screen-{idx:03}.sctrk")
}

fn mic_track() -> Track {
    Track {
        id: "mic".to_string(),
        kind: TrackKind::Mic,
        segment_id: None,
        file: "tracks/mic.sctrk".to_string(),
        container: "sctrk".to_string(),
        codec: "sctrk".to_string(),
        first_sample_offset_ns: 2_000_000,
        duration_ns: None,
        video: None,
        audio: Some(AudioInfo {
            sample_rate: 48_000,
            channels: 1,
            extra: Extra::new(),
        }),
        device: None,
        health: TrackHealth {
            frames: 0,
            dropped: 0,
            writer_not_ready: 0,
            gaps: Vec::new(),
            extra: Extra::new(),
        },
        fragment_interval_s: 1.0,
        extra: Extra::new(),
    }
}

fn write_video(path: &Path, frames: u32, finish: Option<i64>) {
    let spec = VideoWriterSpec {
        size: FRAME,
        fps: 30,
        codec: VideoCodec::Hevc,
        keyframe_interval_s: 1.0,
        fragment_interval_s: 1.0,
        bits_per_pixel: 0.1,
    };
    let mut w = SctrkWriterFactory.video(path, &spec).expect("video writer");
    for i in 0..frames {
        let f = coded_frame(
            FRAME,
            FrameCode {
                source_hash: 7,
                index: i,
                pts_ms: i,
            },
            false,
        );
        let frame = VideoFrame {
            stream: StreamTag(1),
            pts: HostTime(START + i64::from(i) * FRAME_NS),
            size: FRAME,
            content_rect_pt: None,
            content_scale: None,
            buffer: FrameBuffer(Arc::new(f)),
        };
        w.append_video(&frame, i64::from(i) * FRAME_NS)
            .expect("append video");
    }
    finish_or_crash(w, finish);
}

fn write_audio(path: &Path, tenths: u32) {
    let spec = AudioWriterSpec {
        sample_rate: 48_000,
        channels: 1,
        bitrate: 128_000,
        fragment_interval_s: 1.0,
    };
    let mut w = SctrkWriterFactory.audio(path, &spec).expect("audio writer");
    for i in 0..tenths {
        let buf = AudioBuffer {
            pts: HostTime(START + i64::from(i) * SEC / 10),
            sample_rate: 48_000,
            channels: 1,
            frames: 4_800,
            data: AudioData::F32Interleaved(Arc::from(vec![0.0f32; 4_800])),
        };
        w.append_audio(&buf, i64::from(i) * SEC / 10, 0)
            .expect("append audio");
    }
    finish_or_crash(w, None);
}

fn finish_or_crash(w: Box<dyn TrackWriter>, finish: Option<i64>) {
    match finish {
        Some(end) => {
            w.finish(Some(end)).expect("finish");
        }
        None => drop(w),
    }
}

fn probe_dur(path: &Path) -> i64 {
    SctrkDecoder
        .probe(path)
        .expect("probe")
        .duration_ns
        .expect("duration")
}

fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).expect("read dir") {
            let p = e.expect("entry").path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p
                    .strip_prefix(base)
                    .expect("prefix")
                    .to_string_lossy()
                    .into_owned();
                out.insert(rel, fs::read(&p).expect("read file"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("mkdir");
    for e in fs::read_dir(from).expect("read dir") {
        let p = e.expect("entry").path();
        let dst = to.join(p.file_name().expect("name"));
        if p.is_dir() {
            copy_dir(&p, &dst);
        } else {
            fs::copy(&p, &dst).expect("copy");
        }
    }
}

fn garbage(path: &Path) {
    fs::write(path, b"\x00\x01this is not a track file at all\xff").expect("write garbage");
}

fn session_log(dir: &Path) -> String {
    fs::read_to_string(dir.join("session.log")).unwrap_or_default()
}

fn killed_in_s1(root: &Path) -> Fx {
    let mut fx = Fx::new(root);
    fx.open(0, 0);
    write_video(&fx.path(&seg_file(0)), 30, Some(SEC));
    fx.ev(JournalEvent::SwitchBegin {
        host_ns: START + SEC - 100_000_000,
        to: SourceDescriptor::Window {
            window_id: 42,
            app_bundle_id: None,
            app_name: None,
            window_title: None,
            scale: 2.0,
        },
        via: Via::OwnChooser,
    });
    fx.ev(JournalEvent::SegmentPrepare {
        seg: "s1".to_string(),
        file: seg_file(1),
        host_ns: START + SEC - 90_000_000,
    });
    fx.open(1, SEC);
    fx.close(0, SEC);
    fx.ev(JournalEvent::TrackFinished {
        track_id: "screen-0".to_string(),
        duration_ns: Some(SEC),
        samples: 30,
    });
    write_video(&fx.path(&seg_file(1)), 15, None);
    write_audio(&fx.path("tracks/mic.sctrk"), 20);
    fx.ev(JournalEvent::Device {
        host_ns: START,
        track_id: Some("mic".to_string()),
        kind: DeviceKindJ::Mic,
        change: DeviceChange::Connected,
        name: "Synthetic Mic".to_string(),
        unique_id: "mic-1".to_string(),
    });
    fx.ev(JournalEvent::AudioGap {
        track: "mic".to_string(),
        kind: GapKind::Gap,
        at_rec_ns: 500_000_000,
        dur_ns: 20_000_000,
    });
    fx.ev(JournalEvent::AudioGap {
        track: "mic".to_string(),
        kind: GapKind::Overlap,
        at_rec_ns: 700_000_000,
        dur_ns: 3_000_000,
    });
    fx.ev(JournalEvent::Health {
        host_ns: START + SEC + 300_000_000,
        rec_ns: SEC + 300_000_000,
        tracks: vec![HealthEntry {
            track_id: "screen-1".to_string(),
            frames: 9,
            dropped: 2,
            writer_not_ready: 1,
            gaps: 0,
        }],
    });
    fx
}

#[test]
fn missing_segment_close_ends_at_measured_duration_and_folds_the_journal() {
    let root = TempRoot::new("close");
    let fx = killed_in_s1(&root.0);
    let before = snapshot(&fx.dir);

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Recovered, "{r:?}");
    assert_eq!(r.segments, 2);
    assert!(r.dropped_segments.is_empty());

    let p = fx.project();
    assert_eq!(p.state, ProjectState::Recovered);
    let segs = &p.recording.screen.segments;
    assert_eq!(segs[0].id, "s0");
    assert_eq!(segs[0].end_rec_ns, Some(SEC));
    assert_eq!(segs[0].end_reason, Some(EndReason::UserSwitch));
    let s1_dur = probe_dur(&fx.path(&seg_file(1)));
    assert_eq!(s1_dur, 15 * FRAME_NS);
    assert_eq!(segs[1].id, "s1");
    assert_eq!(segs[1].end_rec_ns, Some(SEC + s1_dur));
    assert_eq!(segs[1].end_reason, Some(EndReason::Recovered));
    assert_eq!(segs[1].start_reason, StartReason::UserSwitch);
    assert_eq!(segs[1].content_px, CONTENT);
    assert_eq!(p.output.canvas.ratio, Ratio::new(5, 3));

    let track = |id: &str| {
        p.recording
            .tracks
            .iter()
            .find(|t| t.id == id)
            .unwrap_or_else(|| panic!("track {id}"))
    };
    let s0 = track("screen-0");
    assert_eq!((s0.duration_ns, s0.health.frames), (Some(SEC), 30));
    assert_eq!(s0.segment_id.as_deref(), Some("s0"));
    let s1 = track("screen-1");
    assert_eq!(s1.kind, TrackKind::Screen);
    assert_eq!(s1.file, seg_file(1));
    assert_eq!(s1.container, "sctrk");
    assert_eq!(s1.first_sample_offset_ns, SEC);
    assert_eq!(s1.duration_ns, Some(s1_dur));
    assert_eq!(
        (
            s1.health.frames,
            s1.health.dropped,
            s1.health.writer_not_ready
        ),
        (9, 2, 1)
    );
    assert_eq!(s1.video.as_ref().map(|v| v.size_px), Some(FRAME));
    let mic = track("mic");
    let mic_dur = probe_dur(&fx.path("tracks/mic.sctrk"));
    assert_eq!(mic_dur, 2 * SEC);
    assert_eq!(mic.duration_ns, Some(mic_dur));
    assert_eq!(mic.health.gaps.len(), 1);
    assert_eq!(
        (mic.health.gaps[0].at_rec_ns, mic.health.gaps[0].dur_ns),
        (500_000_000, 20_000_000)
    );
    assert_eq!(
        mic.device.as_ref().map(|d| d.unique_id.as_str()),
        Some("mic-1")
    );

    let rec_end = (2_000_000 + mic_dur).max(SEC + s1_dur);
    assert_eq!(p.recording.stop_host_ns, Some(START + rec_end));
    assert_eq!(p.duration_ns(), Some(rec_end));
    assert_eq!(p.edl.clips.len(), 1);
    assert_eq!(
        (p.edl.clips[0].src_in_rec_ns, p.edl.clips[0].src_out_rec_ns),
        (0, rec_end)
    );

    let after = snapshot(&fx.dir);
    for (name, bytes) in &before {
        if name != "project.json" {
            assert_eq!(after.get(name), Some(bytes), "{name} changed");
        }
    }
    assert!(!after.contains_key(".lock"), "lock left behind");
    assert!(session_log(&fx.dir).contains("recovery: state recovered with 2 segment(s)"));
}

#[test]
fn torn_journal_tail_is_ignored() {
    let root = TempRoot::new("torn");
    let mut fx = Fx::new(&root.0);
    fx.open(0, 0);
    write_video(&fx.path(&seg_file(0)), 10, None);
    write_audio(&fx.path("tracks/mic.sctrk"), 3);
    fs::OpenOptions::new()
        .append(true)
        .open(fx.dir.join("journal.jsonl"))
        .and_then(|mut f| f.write_all(br#"{"ev":"segment_close","seg":"s0","end_"#))
        .expect("tear journal");

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Recovered, "{r:?}");
    assert!(r.notes.iter().any(|n| n.contains("torn")), "{:?}", r.notes);
    let p = fx.project();
    let s0 = &p.recording.screen.segments[0];
    assert_eq!(s0.end_rec_ns, Some(10 * FRAME_NS));
    assert_eq!(s0.end_reason, Some(EndReason::Recovered));
}

#[test]
fn prepared_never_opened_segment_is_ignored_and_its_file_kept() {
    let root = TempRoot::new("prepared");
    let mut fx = Fx::new(&root.0);
    fx.open(0, 0);
    write_video(&fx.path(&seg_file(0)), 10, None);
    fx.ev(JournalEvent::SegmentPrepare {
        seg: "s1".to_string(),
        file: seg_file(1),
        host_ns: START + 300_000_000,
    });
    write_video(&fx.path(&seg_file(1)), 0, None);
    write_audio(&fx.path("tracks/mic.sctrk"), 3);
    fx.ev(JournalEvent::Stop {
        stop_host_ns: START + 400_000_000,
        reason: StopReason::User,
    });
    let prepared = fs::read(fx.path(&seg_file(1))).expect("read prepared");

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Recovered, "{r:?}");
    assert_eq!(r.segments, 1);
    assert!(r.dropped_segments.is_empty());
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("s1") && n.contains("prepared but never opened")),
        "{:?}",
        r.notes
    );
    let p = fx.project();
    assert!(p.recording.tracks.iter().all(|t| t.id != "screen-1"));
    assert_eq!(p.recording.stop_host_ns, Some(START + 400_000_000));
    assert_eq!(fs::read(fx.path(&seg_file(1))).expect("read"), prepared);
}

#[test]
fn garbage_segment_is_dropped_and_the_project_damaged() {
    let root = TempRoot::new("garbage");
    let mut fx = Fx::new(&root.0);
    fx.open(0, 0);
    write_video(&fx.path(&seg_file(0)), 10, Some(10 * FRAME_NS));
    fx.open(1, 10 * FRAME_NS);
    fx.close(0, 10 * FRAME_NS);
    garbage(&fx.path(&seg_file(1)));
    write_audio(&fx.path("tracks/mic.sctrk"), 5);
    let bad = fs::read(fx.path(&seg_file(1))).expect("read");

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Damaged, "{r:?}");
    assert_eq!(r.dropped_segments, vec!["s1".to_string()]);
    assert_eq!(r.segments, 1);
    let p = fx.project();
    assert_eq!(p.state, ProjectState::Damaged);
    assert_eq!(p.recording.screen.segments.len(), 1);
    assert!(p.recording.tracks.iter().all(|t| t.id != "screen-1"));
    let dropped = &p.recording.extra["recovery"]["dropped"];
    assert_eq!(dropped[0]["segment_id"], "s1");
    assert_eq!(dropped[0]["file"], seg_file(1).as_str());
    assert_eq!(fs::read(fx.path(&seg_file(1))).expect("read"), bad);
    let log = session_log(&fx.dir);
    assert!(
        log.contains("segment s1") && log.contains("will not open"),
        "{log}"
    );
    assert!(log.contains("ERROR recovery: state damaged"), "{log}");
}

#[test]
fn unreadable_first_segment_is_dropped_and_later_segments_kept() {
    let root = TempRoot::new("first");
    let mut fx = Fx::new(&root.0);
    fx.open(0, 0);
    garbage(&fx.path(&seg_file(0)));
    fx.open(1, SEC);
    fx.close(0, SEC);
    write_video(&fx.path(&seg_file(1)), 10, None);
    write_audio(&fx.path("tracks/mic.sctrk"), 5);

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Damaged, "{r:?}");
    assert_eq!(r.dropped_segments, vec!["s0".to_string()]);
    let p = fx.project();
    let segs = &p.recording.screen.segments;
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].id, "s1");
    assert_eq!(segs[0].start_rec_ns, SEC);
    assert_eq!(segs[0].end_rec_ns, Some(SEC + 10 * FRAME_NS));
    assert_eq!(p.recording.stop_host_ns, Some(START + SEC + 10 * FRAME_NS));
    assert!(fx.path(&seg_file(0)).exists());
}

#[test]
fn open_segment_is_cut_at_the_next_segment_start() {
    let root = TempRoot::new("cut");
    let mut fx = Fx::new(&root.0);
    fx.open(0, 0);
    write_video(&fx.path(&seg_file(0)), 30, None);
    fx.open(1, 900_000_000);
    write_video(&fx.path(&seg_file(1)), 6, None);
    write_audio(&fx.path("tracks/mic.sctrk"), 5);

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Recovered, "{r:?}");
    let p = fx.project();
    let segs = &p.recording.screen.segments;
    assert_eq!(segs[0].end_rec_ns, Some(900_000_000));
    assert_eq!(segs[0].end_reason, Some(EndReason::Recovered));
    assert_eq!(segs[1].end_rec_ns, Some(900_000_000 + 6 * FRAME_NS));
}

#[test]
fn empty_new_segment_is_dropped_without_damage_and_pauses_fold() {
    let root = TempRoot::new("empty");
    let mut fx = Fx::new(&root.0);
    fx.open(0, 0);
    write_video(&fx.path(&seg_file(0)), 30, None);
    fx.ev(JournalEvent::Pause {
        host_ns: START + 200_000_000,
    });
    fx.ev(JournalEvent::Resume {
        host_ns: START + 500_000_000,
    });
    fx.open(1, SEC);
    write_video(&fx.path(&seg_file(1)), 0, None);
    write_audio(&fx.path("tracks/mic.sctrk"), 5);

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Recovered, "{r:?}");
    assert_eq!(r.dropped_segments, vec!["s1".to_string()]);
    let p = fx.project();
    assert_eq!(p.recording.screen.segments.len(), 1);
    assert_eq!(
        p.recording.screen.segments[0].end_rec_ns,
        Some(30 * FRAME_NS)
    );
    assert_eq!(p.recording.pauses.len(), 1);
    assert_eq!(
        (
            p.recording.pauses[0].start_host_ns,
            p.recording.pauses[0].end_host_ns
        ),
        (START + 200_000_000, START + 500_000_000)
    );
    assert_eq!(p.recording.stop_host_ns, Some(START + 30 * FRAME_NS));
    assert_eq!(p.duration_ns(), Some(30 * FRAME_NS));
    let spans: Vec<(i64, i64)> = p
        .edl
        .clips
        .iter()
        .map(|c| (c.src_in_rec_ns, c.src_out_rec_ns))
        .collect();
    assert_eq!(
        spans,
        vec![(0, 200_000_000), (500_000_000, 30 * FRAME_NS)],
        "the pause is cut out of the timeline"
    );
    assert!(fx.path(&seg_file(1)).exists());
}

#[test]
fn unreadable_mic_track_is_dropped_and_the_project_damaged() {
    let root = TempRoot::new("mic");
    let mut fx = Fx::new(&root.0);
    fx.open(0, 0);
    write_video(&fx.path(&seg_file(0)), 10, None);
    garbage(&fx.path("tracks/mic.sctrk"));

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Damaged, "{r:?}");
    assert!(r.dropped_segments.is_empty());
    let p = fx.project();
    assert!(p.recording.tracks.iter().all(|t| t.id != "mic"));
    assert!(!p.audio_mix.contains_key("mic"));
    assert_eq!(p.recording.screen.segments.len(), 1);
}

#[test]
fn already_final_project_is_left_untouched() {
    let root = TempRoot::new("final");
    let fx = killed_in_s1(&root.0);
    let mut p = fx.project();
    p.state = ProjectState::Finalized;
    p.save(&fx.dir).expect("save");
    let before = snapshot(&fx.dir);

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::AlreadyFinal);
    assert_eq!(snapshot(&fx.dir), before);
}

#[test]
fn project_locked_by_a_live_process_is_left_untouched() {
    let root = TempRoot::new("locked");
    let fx = killed_in_s1(&root.0);
    let lock = ProjectLock::acquire(&fx.dir).expect("lock");
    let before = snapshot(&fx.dir);

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Locked);
    assert_eq!(snapshot(&fx.dir), before);
    drop(lock);

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Recovered);
}

#[test]
fn second_run_is_byte_identical_and_output_is_deterministic() {
    let root = TempRoot::new("idem");
    let fx = killed_in_s1(&root.0);
    let copy = root.0.join("2026-11-19 10.00.00 Copy.sc");
    copy_dir(&fx.dir, &copy);

    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::Recovered);
    let first = snapshot(&fx.dir);
    let r = recover_project(&fx.dir, &SctrkDecoder).expect("recover again");
    assert_eq!(r.outcome, Outcome::AlreadyFinal);
    assert_eq!(r.segments, 2);
    assert_eq!(snapshot(&fx.dir), first);

    recover_project(&copy, &SctrkDecoder).expect("recover copy");
    assert_eq!(
        fs::read(copy.join("project.json")).expect("read"),
        first["project.json"]
    );
}

#[test]
fn scan_recovers_only_projects_in_state_recording() {
    let root = TempRoot::new("scan");
    let a = killed_in_s1(&root.0);
    let b = Fx::named(&root.0, "2026-11-19 11.00.00 Done.sc");
    let mut p = b.project();
    p.state = ProjectState::Finalized;
    p.save(&b.dir).expect("save");
    fs::create_dir_all(root.0.join("notes")).expect("mkdir");
    fs::create_dir_all(root.0.join("empty.sc")).expect("mkdir");
    fs::write(root.0.join("file.sc"), "x").expect("write");
    let b_before = snapshot(&b.dir);

    let reports = scan(&root.0, &SctrkDecoder).expect("scan");
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].dir, a.dir);
    assert_eq!(reports[0].outcome, Outcome::Recovered);
    assert_eq!(snapshot(&b.dir), b_before);
    assert!(scan(&root.0, &SctrkDecoder).expect("rescan").is_empty());

    assert!(
        scan(&root.0.join("missing"), &SctrkDecoder)
            .expect("scan missing")
            .is_empty()
    );
    let r = recover_project(&root.0.join("notes"), &SctrkDecoder).expect("recover");
    assert_eq!(r.outcome, Outcome::NotAProject);
}

#[test]
fn missing_project_json_is_rebuilt_from_the_journal() {
    let root = TempRoot::new("nojson");
    let mut fx = Fx::new(&root.0);
    fs::remove_file(fx.dir.join("project.json")).expect("rm project.json");
    fx.open(0, 0);
    write_video(&fx.path(&seg_file(0)), 10, None);

    let reports = scan(&root.0, &SctrkDecoder).expect("scan");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, Outcome::Recovered, "{reports:?}");
    let p = fx.project();
    assert_eq!(p.title, "Test");
    assert_eq!(p.created_at, "2026-11-19T10:00:00");
    assert_eq!(p.recording.start_host_ns, START);
    assert_eq!(p.recording.screen.fps_requested, 60);
    assert_eq!(p.recording.screen.resolution, Resolution::OneX);
    assert_eq!(p.recording.screen.segments.len(), 1);
    assert_eq!(p.output.canvas.ratio, Ratio::new(5, 3));
    let t = &p.recording.tracks[0];
    assert_eq!((t.container.as_str(), t.codec.as_str()), ("sctrk", "sctrk"));
    assert_eq!(t.video.as_ref().map(|v| v.nominal_fps), Some(60));
    assert_eq!(p.recording.stop_host_ns, Some(START + 10 * FRAME_NS));
}
