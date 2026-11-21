use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use sc_core::journal::{
    DeviceChange, DeviceKindJ, GapKind, HealthEntry, JournalEvent, JournalRead,
};
use sc_core::project::{
    ColorInfo, DeviceInfo, Extra, GapRecord, PauseRecord, ProjectState, ProjectV1, Resolution,
    ScreenSegment, Speed, Track, TrackHealth, TrackKind, VideoInfo,
};
use sc_core::sessionlog::Level;
use sc_core::{ClockKind, project::Clip, project::SystemAudioMode};
use sc_geometry::{Ratio, SizePx};
use sc_platform::Decoder;
use serde_json::{Value, json};

pub(super) struct Rebuilt {
    pub project: ProjectV1,
    pub dropped_segments: Vec<String>,
    pub notes: Vec<String>,
    pub log: Vec<(Level, String)>,
}

#[derive(Default)]
struct Notes {
    notes: Vec<String>,
    log: Vec<(Level, String)>,
}

impl Notes {
    fn add(&mut self, level: Level, msg: String) {
        self.log.push((level, msg.clone()));
        self.notes.push(msg);
    }
}

struct StartEv {
    start_host_ns: i64,
    clock: ClockKind,
    fps_requested: u32,
    resolution: String,
    system_audio: SystemAudioMode,
}

#[derive(Default)]
struct Folded {
    seg_files: BTreeMap<String, String>,
    prepared: Vec<(String, String)>,
    opened: BTreeSet<String>,
    first_open_content: Option<SizePx>,
    pauses: Vec<PauseRecord>,
    stop_host_ns: Option<i64>,
    health: BTreeMap<String, HealthEntry>,
    finished: BTreeMap<String, (Option<i64>, u64)>,
    gaps: Vec<(String, i64, i64)>,
    camera_keys: Vec<sc_core::project::CameraKey>,
}

/// `None` when there is neither a `project.json` nor a journal `start` to build from.
pub(super) fn rebuild(
    dir: &Path,
    project: Option<ProjectV1>,
    journal: &JournalRead,
    decoder: &dyn Decoder,
) -> Option<Rebuilt> {
    let mut n = Notes::default();
    if journal.torn_tail {
        n.add(Level::Warn, "journal: torn last line ignored".to_string());
    }
    if journal.unknown_lines > 0 {
        n.add(
            Level::Warn,
            format!(
                "journal: {} unreadable line(s) ignored",
                journal.unknown_lines
            ),
        );
    }
    let start = journal.events.iter().find_map(|e| match e {
        JournalEvent::Start {
            start_host_ns,
            clock,
            fps_requested,
            resolution,
            system_audio,
        } => Some(StartEv {
            start_host_ns: *start_host_ns,
            clock: *clock,
            fps_requested: *fps_requested,
            resolution: resolution.clone(),
            system_audio: *system_audio,
        }),
        _ => None,
    });
    let mut p = match project {
        Some(mut p) => {
            if p.recording.start_host_ns == 0
                && let Some(s) = &start
            {
                p.recording.start_host_ns = s.start_host_ns;
            }
            p
        }
        None => {
            let s = start.as_ref()?;
            n.add(
                Level::Warn,
                "project.json missing: rebuilt from the journal with the default canvas"
                    .to_string(),
            );
            from_start(dir, s)
        }
    };
    let had_segments = !p.recording.screen.segments.is_empty();
    let f = fold_events(&mut p, journal, &mut n);
    if !f.camera_keys.is_empty() {
        crate::session::set_camera_layout(&mut p, f.camera_keys.clone());
    }

    for (seg, file) in &f.prepared {
        if !f.opened.contains(seg) {
            let exists = if dir.join(file).exists() {
                "file kept"
            } else {
                "no file"
            };
            n.add(
                Level::Info,
                format!("segment {seg}: prepared but never opened, ignored ({file}, {exists})"),
            );
        }
    }
    if !had_segments
        && p.output.canvas.aspect == sc_geometry::Aspect::Original
        && let Some(content) = f.first_open_content
    {
        p.output.canvas.ratio = Ratio::of(content);
    }

    let mut damaged = false;
    let mut dropped: Vec<Value> = Vec::new();
    add_segment_tracks(&mut p, &f.seg_files, &mut n, &mut damaged, &mut dropped);
    fold_into_tracks(&mut p, &f, &mut n);
    probe_tracks(dir, &mut p, decoder, &mut n, &mut damaged, &mut dropped);

    let kept: BTreeSet<String> = p.recording.tracks.iter().map(|t| t.id.clone()).collect();
    let mut dropped_segments = Vec::new();
    p.recording.screen.segments.retain(|s| {
        let keep = kept.contains(&s.track_id);
        if !keep {
            dropped_segments.push(s.id.clone());
        }
        keep
    });
    end_open_segments(&mut p, &mut n);

    for pause in f.pauses {
        if !p
            .recording
            .pauses
            .iter()
            .any(|q| q.start_host_ns == pause.start_host_ns)
        {
            p.recording.pauses.push(pause);
        }
    }
    p.recording.pauses.sort_by_key(|q| q.start_host_ns);
    if p.recording.stop_host_ns.is_none() {
        p.recording.stop_host_ns = Some(f.stop_host_ns.unwrap_or_else(|| stop_from_tracks(&p)));
    }
    if p.edl.clips.is_empty()
        && let Some(d) = p.duration_ns().filter(|d| *d > 0)
    {
        p.edl.clips.push(Clip {
            id: "c0".to_string(),
            src_in_rec_ns: 0,
            src_out_rec_ns: d,
            speed: Speed { num: 1, den: 1 },
            muted: false,
            extra: Extra::new(),
        });
    }
    p.state = if damaged {
        ProjectState::Damaged
    } else {
        ProjectState::Recovered
    };
    p.recording.extra.insert(
        "recovery".to_string(),
        json!({ "dropped": dropped, "notes": n.notes }),
    );
    Some(Rebuilt {
        project: p,
        dropped_segments,
        notes: n.notes,
        log: n.log,
    })
}

fn from_start(dir: &Path, s: &StartEv) -> ProjectV1 {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let name = name
        .strip_suffix(sc_core::folder::PROJECT_SUFFIX)
        .unwrap_or(name);
    let dated = name.len() > 20
        && name.is_char_boundary(20)
        && name.as_bytes()[..19]
            .iter()
            .enumerate()
            .all(|(i, b)| match i {
                4 | 7 => *b == b'-',
                10 => *b == b' ',
                13 | 16 => *b == b'.',
                _ => b.is_ascii_digit(),
            });
    let (created_at, title) = if dated {
        let d = &name[..19];
        (
            format!("{}T{}", &d[..10], d[11..].replace('.', ":")),
            name[20..].to_string(),
        )
    } else {
        (String::new(), name.to_string())
    };
    let mut p = ProjectV1::new_recording(
        sc_core::ids::new_id(),
        title,
        created_at,
        env!("CARGO_PKG_VERSION").to_string(),
        s.clock,
    );
    p.recording.start_host_ns = s.start_host_ns;
    p.recording.screen.fps_requested = s.fps_requested;
    p.recording.screen.resolution =
        serde_json::from_value(Value::String(s.resolution.clone())).unwrap_or(Resolution::Native);
    p.recording.system_audio = s.system_audio;
    p
}

fn fold_events(p: &mut ProjectV1, journal: &JournalRead, n: &mut Notes) -> Folded {
    let mut f = Folded::default();
    let mut open_pause: Option<i64> = None;
    for ev in &journal.events {
        match ev {
            JournalEvent::SegmentPrepare { seg, file, .. } => {
                f.prepared.push((seg.clone(), file.clone()));
            }
            JournalEvent::SegmentOpen {
                seg,
                track_id,
                start_rec_ns,
                file,
                via,
                reason,
                source,
                frame_px,
                content_px,
            } => {
                f.opened.insert(seg.clone());
                f.seg_files.insert(track_id.clone(), file.clone());
                if f.first_open_content.is_none() {
                    f.first_open_content = Some(SizePx {
                        w: content_px.w,
                        h: content_px.h,
                    });
                }
                let segments = &mut p.recording.screen.segments;
                if !segments.iter().any(|s| &s.id == seg) {
                    segments.push(ScreenSegment {
                        id: seg.clone(),
                        track_id: track_id.clone(),
                        start_rec_ns: *start_rec_ns,
                        end_rec_ns: None,
                        start_reason: *reason,
                        end_reason: None,
                        via: *via,
                        source: source.clone(),
                        frame_px: *frame_px,
                        content_px: *content_px,
                        extra: Extra::new(),
                    });
                }
            }
            JournalEvent::SegmentClose {
                seg,
                end_rec_ns,
                reason,
                ..
            } => match p
                .recording
                .screen
                .segments
                .iter_mut()
                .find(|s| &s.id == seg)
            {
                Some(s) => {
                    if s.end_rec_ns.is_none() {
                        s.end_rec_ns = Some(*end_rec_ns);
                        s.end_reason = Some(*reason);
                    }
                }
                None => n.add(
                    Level::Warn,
                    format!("segment {seg}: segment_close without segment_open, ignored"),
                ),
            },
            JournalEvent::Pause { host_ns } => open_pause = Some(*host_ns),
            JournalEvent::Resume { host_ns } => {
                if let Some(start) = open_pause.take() {
                    f.pauses.push(PauseRecord {
                        start_host_ns: start,
                        end_host_ns: (*host_ns).max(start),
                        extra: Extra::new(),
                    });
                }
            }
            JournalEvent::AudioGap {
                track,
                kind: GapKind::Gap,
                at_rec_ns,
                dur_ns,
            } => f.gaps.push((track.clone(), *at_rec_ns, *dur_ns)),
            JournalEvent::Device {
                track_id,
                kind,
                change,
                name,
                unique_id,
                ..
            } => match change {
                DeviceChange::Connected => {
                    if let Some(t) = track_id
                        .as_ref()
                        .and_then(|id| p.recording.tracks.iter_mut().find(|t| &t.id == id))
                        && t.device.is_none()
                    {
                        t.device = Some(DeviceInfo {
                            name: name.clone(),
                            unique_id: unique_id.clone(),
                            model_id: None,
                            format: None,
                            extra: Extra::new(),
                        });
                    }
                }
                DeviceChange::Lost | DeviceChange::Disconnected => n.add(
                    Level::Warn,
                    format!(
                        "{} {name:?} ({}) was lost during the recording",
                        match kind {
                            DeviceKindJ::Camera => "camera",
                            DeviceKindJ::Mic => "mic",
                        },
                        track_id.as_deref().unwrap_or("no track")
                    ),
                ),
            },
            JournalEvent::TrackFinished {
                track_id,
                duration_ns,
                samples,
            } => {
                f.finished
                    .insert(track_id.clone(), (*duration_ns, *samples));
            }
            JournalEvent::Health { tracks, .. } => {
                for h in tracks {
                    f.health.insert(h.track_id.clone(), h.clone());
                }
            }
            JournalEvent::Stop { stop_host_ns, .. } => f.stop_host_ns = Some(*stop_host_ns),
            JournalEvent::CameraKeys { keys } => f.camera_keys.extend(keys.iter().cloned()),
            _ => {}
        }
    }
    if let Some(start) = open_pause {
        n.add(
            Level::Warn,
            format!("pause at host {start} has no resume, ignored"),
        );
    }
    f
}

fn add_segment_tracks(
    p: &mut ProjectV1,
    seg_files: &BTreeMap<String, String>,
    n: &mut Notes,
    damaged: &mut bool,
    dropped: &mut Vec<Value>,
) {
    let template = p
        .recording
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Screen)
        .cloned();
    let fps = p.recording.screen.fps_requested;
    let mut missing = Vec::new();
    for seg in &p.recording.screen.segments {
        if p.recording.tracks.iter().any(|t| t.id == seg.track_id) {
            continue;
        }
        match seg_files.get(&seg.track_id) {
            Some(file) => {
                let t = screen_track(template.as_ref(), seg, file, fps);
                p.recording.tracks.push(t);
            }
            None => missing.push((seg.id.clone(), seg.track_id.clone())),
        }
    }
    for (seg, track) in missing {
        *damaged = true;
        n.add(
            Level::Error,
            format!("segment {seg}: no file recorded for track {track}, dropped"),
        );
        dropped.push(json!({ "segment_id": seg, "track_id": track, "reason": "no track file" }));
    }
}

fn screen_track(template: Option<&Track>, seg: &ScreenSegment, file: &str, fps: u32) -> Track {
    let sctrk = Path::new(file)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("sctrk"));
    let (container, codec) = match template {
        Some(t) => (t.container.clone(), t.codec.clone()),
        None if sctrk => ("sctrk".to_string(), "sctrk".to_string()),
        None if Path::new(file)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mkv")) =>
        {
            ("matroska".to_string(), "hevc_or_h264".to_string())
        }
        None => ("mov_fragmented".to_string(), "hevc".to_string()),
    };
    let video = match template.and_then(|t| t.video.clone()) {
        Some(v) => VideoInfo {
            size_px: seg.frame_px,
            ..v
        },
        None => VideoInfo {
            size_px: seg.frame_px,
            nominal_fps: fps,
            vfr: true,
            keyframe_interval_s: 1.0,
            color: ColorInfo {
                primaries: "bt709".to_string(),
                transfer: "bt709".to_string(),
                matrix: "bt709".to_string(),
                range: "limited".to_string(),
                extra: Extra::new(),
            },
            extra: Extra::new(),
        },
    };
    Track {
        id: seg.track_id.clone(),
        kind: TrackKind::Screen,
        segment_id: Some(seg.id.clone()),
        file: file.to_string(),
        container,
        codec,
        first_sample_offset_ns: seg.start_rec_ns,
        duration_ns: None,
        video: Some(video),
        audio: None,
        device: None,
        health: TrackHealth {
            frames: 0,
            dropped: 0,
            writer_not_ready: 0,
            gaps: Vec::new(),
            extra: Extra::new(),
        },
        fragment_interval_s: template.map_or(1.0, |t| t.fragment_interval_s),
        extra: Extra::new(),
    }
}

fn fold_into_tracks(p: &mut ProjectV1, f: &Folded, n: &mut Notes) {
    for t in &mut p.recording.tracks {
        if let Some(h) = f.health.get(&t.id) {
            t.health.frames = t.health.frames.max(h.frames);
            t.health.dropped = t.health.dropped.max(h.dropped);
            t.health.writer_not_ready = t.health.writer_not_ready.max(h.writer_not_ready);
        }
        if let Some((duration, samples)) = f.finished.get(&t.id) {
            if t.duration_ns.is_none() {
                t.duration_ns = *duration;
            }
            t.health.frames = t.health.frames.max(*samples);
        }
    }
    for (track, at, dur) in &f.gaps {
        match p.recording.tracks.iter_mut().find(|t| &t.id == track) {
            Some(t) => {
                if !t.health.gaps.iter().any(|g| g.at_rec_ns == *at) {
                    t.health.gaps.push(GapRecord {
                        at_rec_ns: *at,
                        dur_ns: *dur,
                        extra: Extra::new(),
                    });
                }
            }
            None => n.add(
                Level::Warn,
                format!("audio gap for unknown track {track} ignored"),
            ),
        }
    }
    for t in &mut p.recording.tracks {
        t.health.gaps.sort_by_key(|g| g.at_rec_ns);
    }
}

fn probe_tracks(
    dir: &Path,
    p: &mut ProjectV1,
    decoder: &dyn Decoder,
    n: &mut Notes,
    damaged: &mut bool,
    dropped: &mut Vec<Value>,
) {
    let mut dropped_ids = Vec::new();
    let mut kept = Vec::new();
    for mut t in std::mem::take(&mut p.recording.tracks) {
        let path = dir.join(&t.file);
        let what = match &t.segment_id {
            Some(seg) => format!("segment {seg} ({})", t.file),
            None => format!("track {} ({})", t.id, t.file),
        };
        if std::fs::metadata(&path).is_ok_and(|m| m.len() == 0) {
            n.add(
                Level::Warn,
                format!("{what} is empty (nothing written before the crash), dropped (file kept)"),
            );
            dropped.push(drop_entry(&t, "empty file"));
            dropped_ids.push(t.id);
            continue;
        }
        let probe = match decoder.probe(&path) {
            Ok(pr) if pr.readable => Ok(pr),
            Ok(pr) => Err(pr.error.unwrap_or_else(|| "unreadable".to_string())),
            Err(e) => Err(e.to_string()),
        };
        match probe {
            Err(e) => {
                *damaged = true;
                n.add(
                    Level::Error,
                    format!("{what} will not open, dropped (file kept): {e}"),
                );
                dropped.push(drop_entry(&t, &e));
                dropped_ids.push(t.id);
            }
            Ok(pr) if t.kind == TrackKind::Screen && pr.samples == 0 => {
                n.add(
                    Level::Warn,
                    format!("{what} has no frames, dropped (file kept)"),
                );
                dropped.push(drop_entry(&t, "no frames"));
                dropped_ids.push(t.id);
            }
            Ok(pr) => {
                if t.duration_ns.is_none() {
                    t.duration_ns = pr.duration_ns;
                }
                if t.health.frames == 0 {
                    t.health.frames = pr.samples;
                }
                kept.push(t);
            }
        }
    }
    p.recording.tracks = kept;
    for id in &dropped_ids {
        p.audio_mix.remove(id);
    }
    p.camera_layout
        .overlays
        .retain(|o| !dropped_ids.contains(&o.track_id));
}

fn drop_entry(t: &Track, reason: &str) -> Value {
    let mut v = json!({ "track_id": t.id, "file": t.file, "reason": reason });
    if let Some(seg) = &t.segment_id {
        v["segment_id"] = json!(seg);
    }
    v
}

fn end_open_segments(p: &mut ProjectV1, n: &mut Notes) {
    let durations: BTreeMap<String, Option<i64>> = p
        .recording
        .tracks
        .iter()
        .map(|t| (t.id.clone(), t.duration_ns))
        .collect();
    let segments = &mut p.recording.screen.segments;
    segments.sort_by_key(|s| s.start_rec_ns);
    for i in 0..segments.len() {
        if segments[i].end_rec_ns.is_some() {
            continue;
        }
        let next_start = segments.get(i + 1).map(|s| s.start_rec_ns);
        let s = &mut segments[i];
        let measured = durations.get(&s.track_id).copied().flatten().unwrap_or(0);
        let mut end = s.start_rec_ns + measured.max(0);
        let mut how = format!("start + measured duration {measured} ns");
        if let Some(next) = next_start
            && end > next
        {
            end = next;
            how.push_str(", cut at the next segment's start");
        }
        s.end_rec_ns = Some(end.max(s.start_rec_ns));
        s.end_reason = Some(sc_core::project::EndReason::Recovered);
        n.add(
            Level::Info,
            format!("segment {}: no segment_close, ends at {end} ({how})", s.id),
        );
    }
}

fn stop_from_tracks(p: &ProjectV1) -> i64 {
    let seg_end = p
        .recording
        .screen
        .segments
        .iter()
        .filter_map(|s| s.end_rec_ns)
        .max();
    let track_end = p
        .recording
        .tracks
        .iter()
        .filter(|t| t.kind != TrackKind::Screen)
        .filter_map(|t| t.duration_ns.map(|d| t.first_sample_offset_ns + d))
        .max();
    let rec_end = seg_end.max(track_end).unwrap_or(0).max(0);
    let mut host = p.recording.start_host_ns + rec_end;
    for pause in &p.recording.pauses {
        if pause.start_host_ns <= host {
            host += pause.end_host_ns - pause.start_host_ns;
        }
    }
    host
}
