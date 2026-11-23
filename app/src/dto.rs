//! Command and event DTOs (plan §5.2), mirrored by ui/src/lib/types.ts (snake_case), and their
//! conversions to and from the crate types.

use std::path::PathBuf;

use sc_core::project::{SystemAudioMode, Via};
use sc_export::{ExportProgress, ExportSize, ExportState};
use sc_geometry::{RectPt, SizeClass};
use sc_platform::{
    AppRef, AudioTrackKind, Capabilities, DeviceKind, DisplayId, PermissionStatus, PickedSource,
    Resolution, SourceSpec, WindowId,
};
use sc_record::recovery::{Outcome, RecoveryReport};
use sc_record::{HealthSnapshot, Notice, RecordRequest, SessionState, Tuning};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::settings::{RecorderSettings, ResolutionDto, SystemAudioDto};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceSpecDto {
    Display {
        display_id: String,
    },
    Window {
        window_id: u32,
    },
    Application {
        display_id: String,
        pid: i32,
        bundle_id: Option<String>,
    },
    Region {
        display_id: String,
        region_pt: RectPt,
    },
    Picked {
        pick_id: String,
        region_pt: Option<RectPt>,
    },
    /// Camera only: the camera is the main picture.
    Camera {
        device_id: String,
    },
    /// A plugged-in iPhone's or iPad's screen as the main picture.
    Device {
        device_id: String,
    },
}

impl SourceSpecDto {
    /// `picked` looks up a system-picker result by its id.
    pub fn to_spec(
        &self,
        picked: impl Fn(&str) -> Option<PickedSource>,
    ) -> Result<SourceSpec, String> {
        Ok(match self {
            SourceSpecDto::Display { display_id } => SourceSpec::Display {
                display: DisplayId(display_id.clone()),
            },
            SourceSpecDto::Window { window_id } => SourceSpec::Window {
                window: WindowId(*window_id),
            },
            SourceSpecDto::Application {
                display_id,
                pid,
                bundle_id,
            } => SourceSpec::Application {
                display: DisplayId(display_id.clone()),
                app: AppRef {
                    pid: *pid,
                    bundle_id: bundle_id.clone(),
                },
            },
            SourceSpecDto::Region {
                display_id,
                region_pt,
            } => {
                if !(region_pt.w > 0.0 && region_pt.h > 0.0) {
                    return Err(format!("region has no area: {region_pt:?}"));
                }
                SourceSpec::Region {
                    display: DisplayId(display_id.clone()),
                    region_pt: *region_pt,
                }
            }
            SourceSpecDto::Picked { pick_id, region_pt } => {
                let mut p = picked(pick_id)
                    .ok_or_else(|| format!("picked source {pick_id:?} is no longer available"))?;
                if region_pt.is_some() {
                    p.region_pt = *region_pt;
                }
                SourceSpec::Picked(p)
            }
            SourceSpecDto::Camera { device_id } => {
                if device_id.is_empty() {
                    return Err("choose a camera to record camera only".into());
                }
                SourceSpec::Camera {
                    device: device_id.clone(),
                }
            }
            SourceSpecDto::Device { device_id } => SourceSpec::Device {
                device: device_id.clone(),
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RecordRequestDto {
    pub source: SourceSpecDto,
    pub via: Via,
    #[serde(default)]
    pub title: Option<String>,
    pub settings: RecorderSettings,
}

/// The parts of a `RecordRequest` that come from the app rather than the UI.
pub struct RequestEnv {
    pub projects_dir: PathBuf,
    pub exclude: Vec<WindowId>,
    pub default_canvas: sc_core::project::Canvas,
}

pub fn record_request(
    dto: &RecordRequestDto,
    source: SourceSpec,
    env: RequestEnv,
) -> Result<RecordRequest, String> {
    let s = &dto.settings;
    s.validate()?;
    let camera_only = matches!(source, SourceSpec::Camera { .. });
    let mut canvas = env.default_canvas;
    canvas.aspect = s.canvas.aspect;
    if let Some(r) = s.canvas.aspect.ratio() {
        canvas.ratio = r;
    }
    canvas.background = s.canvas.background.clone();
    let title = dto
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("Recording")
        .to_string();
    Ok(RecordRequest {
        projects_dir: env.projects_dir,
        title,
        source,
        via: dto.via,
        fps: s.fps,
        resolution: match s.resolution {
            ResolutionDto::Native => Resolution::Native,
            ResolutionDto::OneX => Resolution::OneX,
        },
        show_cursor: s.cursor,
        click_highlight: s.click_highlight,
        camera: s
            .camera
            .clone()
            .filter(|c| !c.is_empty())
            .filter(|_| !camera_only),
        mic: s.mic.clone().filter(|m| !m.is_empty()),
        system_audio: match s.system_audio {
            SystemAudioDto::Off => SystemAudioMode::Off,
            SystemAudioDto::All => SystemAudioMode::All,
            SystemAudioDto::App => SystemAudioMode::App,
        },
        canvas,
        background_image: s.canvas.background_image.as_ref().map(PathBuf::from),
        exclude: env.exclude,
        duration_ns: None,
        camera_key: Some(s.camera_key.clone()),
        tuning: Tuning::default(),
    })
}

pub fn permission_text(s: PermissionStatus) -> &'static str {
    match s {
        PermissionStatus::Granted => "granted",
        PermissionStatus::Denied => "denied",
        PermissionStatus::NotDetermined => "not_determined",
        PermissionStatus::Restricted => "restricted",
        PermissionStatus::Unknown => "unknown",
    }
}

pub fn permission_kind(s: &str) -> Result<sc_platform::PermissionKind, String> {
    use sc_platform::PermissionKind as K;
    match s {
        "screen" => Ok(K::Screen),
        "camera" => Ok(K::Camera),
        "microphone" | "mic" => Ok(K::Microphone),
        _ => Err(format!("unknown permission kind {s:?}")),
    }
}

pub fn recording_state(s: &SessionState, elapsed_ns: i64) -> Value {
    match s {
        SessionState::Preparing => json!({"state": "preparing"}),
        SessionState::Recording => json!({"state": "recording", "elapsed_ns": elapsed_ns}),
        SessionState::Switching => json!({"state": "switching", "elapsed_ns": elapsed_ns}),
        SessionState::Stopping => json!({"state": "stopping"}),
        SessionState::Finalized { project_dir } => {
            json!({"state": "finalized", "project_dir": project_dir.to_string_lossy()})
        }
        SessionState::Failed { error } => json!({"state": "failed", "error": error}),
    }
}

pub fn idle_state() -> Value {
    json!({"state": "idle"})
}

fn track_kind(k: AudioTrackKind) -> &'static str {
    match k {
        AudioTrackKind::Mic => "mic",
        AudioTrackKind::SystemAudio => "system_audio",
    }
}

fn device_kind(k: DeviceKind) -> &'static str {
    match k {
        DeviceKind::Camera => "camera",
        DeviceKind::Mic => "mic",
    }
}

pub fn notice(n: &Notice) -> Value {
    match n {
        Notice::SwitchFailed { error, restored } => {
            json!({"kind": "switch_failed", "error": error, "restored": restored})
        }
        Notice::SourceLost { segment } => json!({"kind": "source_lost", "segment": segment}),
        Notice::DeviceLost { kind, name } => {
            json!({"kind": "device_lost", "device": device_kind(*kind), "name": name})
        }
        Notice::DiskLow { bytes_left } => json!({"kind": "disk_low", "bytes_left": bytes_left}),
        Notice::AudioGap { track, dur_ns } => {
            json!({"kind": "audio_gap", "track": track_kind(*track), "dur_ns": dur_ns})
        }
        Notice::FramesDropping { track, pct } => {
            json!({"kind": "frames_dropping", "track": track, "pct": pct})
        }
        Notice::StoppedForSleep => json!({"kind": "stopped_for_sleep"}),
        Notice::StoppedForDisk => json!({"kind": "stopped_for_disk"}),
        Notice::Paused { paused } => json!({"kind": "paused", "paused": paused}),
    }
}

pub fn hotkey_unavailable(error: &str) -> Value {
    json!({"kind": "hotkey_unavailable", "error": error})
}

pub fn health(h: &HealthSnapshot) -> Value {
    let tracks: Vec<Value> = h
        .tracks
        .iter()
        .map(|t| {
            json!({
                "track_id": t.track_id, "frames": t.frames, "dropped": t.dropped,
                "writer_not_ready": t.writer_not_ready, "gaps": t.gaps, "overlaps": t.overlaps,
                "fps": t.fps, "drift_ns": t.drift_ns,
            })
        })
        .collect();
    json!({"rec_ns": h.rec_ns, "tracks": tracks, "free_disk_bytes": h.free_disk_bytes})
}

pub fn export_size(s: &str) -> Result<ExportSize, String> {
    Ok(match s {
        "reference" => ExportSize::Reference,
        "2160" => ExportSize::Class(SizeClass::S2160),
        "1440" => ExportSize::Class(SizeClass::S1440),
        "1080" => ExportSize::Class(SizeClass::S1080),
        "720" => ExportSize::Class(SizeClass::S720),
        "480" => ExportSize::Class(SizeClass::S480),
        _ => return Err(format!("unknown export size {s:?}")),
    })
}

pub fn export_state(id: &str, s: &ExportState) -> Value {
    match s {
        ExportState::Preparing => json!({"id": id, "state": "preparing"}),
        ExportState::Rendering => json!({"id": id, "state": "rendering"}),
        ExportState::Finalizing => json!({"id": id, "state": "finalizing"}),
        ExportState::Canceled => json!({"id": id, "state": "canceled"}),
        ExportState::Done { path } => {
            json!({"id": id, "state": "done", "path": path.to_string_lossy()})
        }
        ExportState::Failed { error } => json!({"id": id, "state": "failed", "error": error}),
    }
}

pub fn export_progress(id: &str, p: &ExportProgress) -> Value {
    json!({"id": id, "frames_done": p.frames_done, "frames_total": p.frames_total,
           "fps": p.fps, "eta_s": p.eta_s})
}

pub fn recovery(r: &RecoveryReport) -> Value {
    let outcome = match r.outcome {
        Outcome::Recovered => "recovered",
        Outcome::Damaged => "damaged",
        Outcome::AlreadyFinal => "already_final",
        Outcome::Locked => "locked",
        Outcome::NotAProject => "not_a_project",
    };
    json!({"dir": r.dir.to_string_lossy(), "outcome": outcome, "segments": r.segments,
           "dropped_segments": r.dropped_segments, "notes": r.notes})
}

/// `{display_id, can_60fps_native, can_60fps_1x}` per display.
pub struct DisplayProbeDto {
    pub display_id: String,
    pub can_60fps_native: bool,
    pub can_60fps_1x: bool,
}

pub fn capabilities(c: &Capabilities, displays: &[DisplayProbeDto]) -> Value {
    let mut v = serde_json::to_value(c).unwrap_or_else(|_| json!({}));
    let list: Vec<Value> = displays
        .iter()
        .map(|d| {
            json!({"display_id": d.display_id, "can_60fps_native": d.can_60fps_native,
                   "can_60fps_1x": d.can_60fps_1x})
        })
        .collect();
    if let Some(o) = v.as_object_mut() {
        o.insert("displays".into(), Value::Array(list));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::ClockKind;
    use sc_core::project::{Background, ProjectV1, Rgba};
    use sc_geometry::{Aspect, Ratio};
    use sc_platform::{ExclusionKind, SourceDescriptor};
    use std::sync::Arc;

    fn default_canvas() -> sc_core::project::Canvas {
        ProjectV1::new_recording(
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            ClockKind::MachHost,
        )
        .output
        .canvas
    }

    #[test]
    fn source_specs_parse_from_ui_json() {
        let cases = [
            r#"{"kind":"display","display_id":"69733382"}"#,
            r#"{"kind":"window","window_id":4242}"#,
            r#"{"kind":"application","display_id":"1","pid":77,"bundle_id":null}"#,
            r#"{"kind":"region","display_id":"1","region_pt":{"x":1.5,"y":2.0,"w":300.0,"h":200.0}}"#,
            r#"{"kind":"picked","pick_id":"p1","region_pt":null}"#,
        ];
        for c in cases {
            let dto: SourceSpecDto = serde_json::from_str(c).expect(c);
            let back = serde_json::to_value(&dto).expect("encode");
            assert_eq!(back, serde_json::from_str::<Value>(c).expect("json"), "{c}");
        }
    }

    #[test]
    fn source_spec_conversions() {
        let none = |_: &str| None;
        let d = SourceSpecDto::Display {
            display_id: "5".into(),
        };
        assert!(matches!(d.to_spec(none), Ok(SourceSpec::Display { display }) if display.0 == "5"));
        let w = SourceSpecDto::Window { window_id: 9 };
        assert!(matches!(w.to_spec(none), Ok(SourceSpec::Window { window }) if window.0 == 9));
        let a = SourceSpecDto::Application {
            display_id: "5".into(),
            pid: 12,
            bundle_id: Some("com.x".into()),
        };
        match a.to_spec(none) {
            Ok(SourceSpec::Application { display, app }) => {
                assert_eq!(display.0, "5");
                assert_eq!(app.pid, 12);
                assert_eq!(app.bundle_id.as_deref(), Some("com.x"));
            }
            other => panic!("{other:?}"),
        }
        let bad = SourceSpecDto::Region {
            display_id: "5".into(),
            region_pt: RectPt::default(),
        };
        assert!(bad.to_spec(none).is_err());
        let p = SourceSpecDto::Picked {
            pick_id: "gone".into(),
            region_pt: None,
        };
        assert!(p.to_spec(none).is_err());
        let picked = PickedSource {
            descriptor: SourceDescriptor::Display {
                display_id: "5".into(),
                display_name: None,
                scale: 2.0,
                frame_pt: None,
            },
            handle: Arc::new(()),
            region_pt: None,
        };
        let region = RectPt {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 50.0,
        };
        let p = SourceSpecDto::Picked {
            pick_id: "p".into(),
            region_pt: Some(region),
        };
        match p.to_spec(|id| (id == "p").then(|| picked.clone())) {
            Ok(SourceSpec::Picked(ps)) => assert_eq!(ps.region_pt, Some(region)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn record_request_from_dto() {
        let json = r##"{"source":{"kind":"display","display_id":"1"},"via":"own_chooser","title":"  ",
            "settings":{"source":null,"canvas":{"aspect":"4:3","background":{"kind":"solid","color":[1,1,1,1]},
            "background_image":"/x/bg.png"},"camera":"","mic":"mic-1","system_audio":"app","fps":60,
            "resolution":"1x","cursor":false,"click_highlight":true}}"##;
        let dto: RecordRequestDto = serde_json::from_str(json).expect("parse");
        let src = dto.source.to_spec(|_| None).expect("spec");
        let req = record_request(
            &dto,
            src,
            RequestEnv {
                projects_dir: PathBuf::from("/p"),
                exclude: vec![WindowId(3), WindowId(4)],
                default_canvas: default_canvas(),
            },
        )
        .expect("request");
        assert_eq!(req.title, "Recording");
        assert_eq!(req.fps, 60);
        assert_eq!(req.resolution, Resolution::OneX);
        assert!(!req.show_cursor && req.click_highlight);
        assert_eq!(req.camera, None);
        assert_eq!(req.mic.as_deref(), Some("mic-1"));
        assert_eq!(req.system_audio, SystemAudioMode::App);
        assert_eq!(req.canvas.aspect, Aspect::R4x3);
        assert_eq!(req.canvas.ratio, Ratio::new(4, 3));
        assert_eq!(
            req.canvas.background,
            Background::Solid {
                color: Rgba([1.0, 1.0, 1.0, 1.0])
            }
        );
        assert_eq!(req.background_image, Some(PathBuf::from("/x/bg.png")));
        assert_eq!(req.exclude, vec![WindowId(3), WindowId(4)]);
        assert_eq!(req.via, Via::OwnChooser);
    }

    #[test]
    fn camera_only_records_the_camera_as_the_main_picture_without_an_overlay() {
        let json = r##"{"source":{"kind":"camera","device_id":"cam-1"},"via":"own_chooser","title":null,
            "settings":{"source":null,"canvas":{"aspect":"original","background":{"kind":"solid","color":[1,1,1,1]}},
            "camera":"cam-1","mic":null,"system_audio":"all","fps":30,"resolution":"native","cursor":true,
            "click_highlight":false}}"##;
        let dto: RecordRequestDto = serde_json::from_str(json).expect("parse");
        let src = dto.source.to_spec(|_| None).expect("spec");
        assert_eq!(
            src,
            SourceSpec::Camera {
                device: "cam-1".into()
            }
        );
        let env = RequestEnv {
            projects_dir: PathBuf::from("/p"),
            exclude: vec![],
            default_canvas: default_canvas(),
        };
        let req = record_request(&dto, src, env).expect("request");
        assert_eq!(req.camera, None);
        assert_eq!(req.system_audio, SystemAudioMode::All);
        let empty = SourceSpecDto::Camera {
            device_id: String::new(),
        };
        assert!(empty.to_spec(|_| None).is_err());
    }

    #[test]
    fn event_payloads() {
        assert_eq!(
            recording_state(&SessionState::Recording, 5),
            json!({"state":"recording","elapsed_ns":5})
        );
        assert_eq!(
            recording_state(
                &SessionState::Finalized {
                    project_dir: PathBuf::from("/a/b.sc")
                },
                0
            ),
            json!({"state":"finalized","project_dir":"/a/b.sc"})
        );
        assert_eq!(
            notice(&Notice::AudioGap {
                track: AudioTrackKind::SystemAudio,
                dur_ns: 7
            }),
            json!({"kind":"audio_gap","track":"system_audio","dur_ns":7})
        );
        assert_eq!(
            notice(&Notice::DeviceLost {
                kind: DeviceKind::Camera,
                name: "c".into()
            })["device"],
            "camera"
        );
        assert_eq!(
            export_state(
                "j",
                &ExportState::Done {
                    path: PathBuf::from("/o.mp4")
                }
            ),
            json!({"id":"j","state":"done","path":"/o.mp4"})
        );
        assert!(matches!(
            export_size("1080"),
            Ok(ExportSize::Class(SizeClass::S1080))
        ));
        assert!(export_size("999").is_err());
        assert_eq!(
            permission_text(PermissionStatus::NotDetermined),
            "not_determined"
        );
        assert!(permission_kind("microphone").is_ok() && permission_kind("x").is_err());
        let caps = Capabilities {
            system_picker: true,
            application_source: true,
            app_audio: true,
            show_clicks: false,
            mic_modes: true,
            switch_source: true,
            exclusion: ExclusionKind::PerWindow,
        };
        let v = capabilities(
            &caps,
            &[DisplayProbeDto {
                display_id: "1".into(),
                can_60fps_native: true,
                can_60fps_1x: false,
            }],
        );
        assert_eq!(v["exclusion"], "per_window");
        assert_eq!(v["displays"][0]["can_60fps_native"], true);
        let r = RecoveryReport {
            dir: PathBuf::from("/d"),
            outcome: Outcome::AlreadyFinal,
            segments: 2,
            dropped_segments: vec![],
            notes: vec!["n".into()],
        };
        assert_eq!(recovery(&r)["outcome"], "already_final");
    }
}
