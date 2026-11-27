//! The self-test (SPEC Library and safety) behind `simple-capture --self-test` and the app's
//! Diagnostics window: what this machine offers, then a synthetic record → edit → export, and a
//! short real capture when the screen grant is already there. The report (text and JSON, home
//! folder redacted) holds no recordings and is meant to be sent back.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sc_core::edit::{EditOp, Limits, apply};
use sc_core::project::{ProjectV1, TextItem, Via};
use sc_export::plan::ExportRequest;
use sc_export::settings::{ExportSettings, Format, SizeChoice};
use sc_export::{ExportJob, ExportState};
use sc_geometry::SizePx;
use sc_platform::synthetic::{Scenario, SyntheticPlatform};
use sc_platform::{
    PermissionKind, PermissionStatus, Platform, Resolution, SourceSpec, SystemAudioMode, VideoCodec,
};
use sc_record::{
    HealthSnapshot, Notice, RecordRequest, Session, SessionObserver, SessionState, StopReason,
    Tuning,
};
use serde_json::{Value, json};

const S: i64 = 1_000_000_000;

struct Report<'a> {
    sections: serde_json::Map<String, Value>,
    steps: Vec<Value>,
    text: Vec<String>,
    out: &'a mut dyn FnMut(&str),
}

impl Report<'_> {
    fn line(&mut self, s: impl Into<String>) {
        let s = redact(&s.into());
        (self.out)(&s);
        self.text.push(s);
    }

    fn section(&mut self, name: &str, v: Value) {
        self.sections.insert(name.into(), v);
    }

    fn step<T>(
        &mut self,
        name: &str,
        f: impl FnOnce() -> Result<(T, String), String>,
    ) -> Option<T> {
        let t0 = Instant::now();
        let r = f();
        let ms = t0.elapsed().as_millis() as u64;
        let (ok, detail, out) = match r {
            Ok((v, d)) => (true, d, Some(v)),
            Err(e) => (false, e, None),
        };
        self.line(format!(
            "  {} {name} ({ms} ms){}",
            if ok { "PASS" } else { "FAIL" },
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        ));
        self.steps
            .push(json!({ "step": name, "ok": ok, "ms": ms, "detail": redact(&detail) }));
        out
    }

    fn skip(&mut self, name: &str, why: &str) {
        self.line(format!("  SKIP {name}: {why}"));
        self.steps
            .push(json!({ "step": name, "ok": Value::Null, "detail": why }));
    }
}

pub fn redact(s: &str) -> String {
    let mut homes: Vec<String> = ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|h| h.to_string_lossy().to_string())
        .collect();
    // A sandboxed app's $HOME is its container, while the paths it reports are under the real
    // home folder.
    if cfg!(target_os = "macos")
        && let Some(u) = std::env::var_os("USER")
    {
        homes.push(format!("/Users/{}", u.to_string_lossy()));
    }
    homes.sort_by_key(|h| std::cmp::Reverse(h.len()));
    let mut out = s.to_string();
    for h in homes.iter().filter(|h| h.len() > 3) {
        out = out.replace(h.as_str(), "~");
    }
    out
}

fn helpers(r: &mut Report) -> Option<PathBuf> {
    let mut v = serde_json::Map::new();
    let mut ffmpeg = None;
    let kinds = [
        sc_helpers::HelperKind::Ffmpeg,
        sc_helpers::HelperKind::Ffprobe,
        sc_helpers::HelperKind::Gifski,
    ];
    for kind in kinds
        .into_iter()
        .filter(|k| !cfg!(target_os = "macos") || *k != sc_helpers::HelperKind::Gifski)
    {
        let line = match sc_helpers::locate(kind) {
            Ok(p) => {
                let version = sc_helpers::helper_info(&p)
                    .map(|i| {
                        let net = if i.network_disabled {
                            ", network off"
                        } else {
                            ""
                        };
                        format!("{}{net}", i.version_line)
                    })
                    .unwrap_or_else(|e| format!("unreadable: {e}"));
                if kind == sc_helpers::HelperKind::Ffmpeg {
                    ffmpeg = Some(p.clone());
                }
                v.insert(
                    kind.name().into(),
                    json!({ "path": redact(&p.display().to_string()), "version": version }),
                );
                format!("{} {version}", kind.name())
            }
            Err(e) => {
                v.insert(kind.name().into(), json!({ "error": e.to_string() }));
                format!("{} missing", kind.name())
            }
        };
        r.line(format!("  {line}"));
    }
    r.section("helpers", Value::Object(v));
    ffmpeg
}

fn machine(r: &mut Report, p: &dyn Platform) {
    r.line(format!(
        "simple-capture {} on {} {} ({})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        p.name()
    ));
    r.section(
        "app",
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "platform": p.name(),
            "clock": format!("{:?}", p.clock_kind()),
            "capabilities": serde_json::to_value(p.capabilities()).unwrap_or(Value::Null),
        }),
    );
    r.line("Capture");
    let displays = p.screen().displays();
    match &displays {
        Ok(d) => {
            r.line(format!("  {} display(s)", d.len()));
            for d in d {
                r.line(format!(
                    "    {} {}x{} px, scale {}",
                    d.name, d.size_px.w, d.size_px.h, d.scale
                ));
            }
        }
        Err(e) => r.line(format!("  displays: {e}")),
    }
    let perms: serde_json::Map<String, Value> = [
        PermissionKind::Screen,
        PermissionKind::Camera,
        PermissionKind::Microphone,
    ]
    .iter()
    .map(|k| {
        (
            format!("{k:?}").to_lowercase(),
            json!(format!("{:?}", p.permissions().status(*k)).to_lowercase()),
        )
    })
    .collect();
    r.line(format!("  permissions {}", Value::Object(perms.clone())));
    let names = |d: sc_platform::Result<Vec<sc_platform::CaptureDevice>>| match d {
        Ok(d) => json!(d.iter().map(|d| d.name.clone()).collect::<Vec<_>>()),
        Err(e) => json!({ "error": e.to_string() }),
    };
    let mics = names(p.mic().devices());
    let cams = names(p.camera().devices());
    r.line(format!("  microphones {mics}"));
    r.line(format!("  cameras {cams}"));
    let sources = if p.permissions().status(PermissionKind::Screen) == PermissionStatus::Granted {
        match p.screen().list(&[]) {
            Ok(l) => json!({ "windows": l.windows.len(), "apps": l.apps.len() }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    } else {
        json!("not listed: no screen grant")
    };
    r.section(
        "capture",
        json!({
            "displays": displays.map(|d| serde_json::to_value(d).unwrap_or(Value::Null))
                .unwrap_or_else(|e| json!({ "error": e.to_string() })),
            "sources": sources,
            "permissions": perms,
            "microphones": mics,
            "cameras": cams,
        }),
    );
}

fn encoders(r: &mut Report, p: &dyn Platform, ffmpeg: Option<&Path>) {
    r.line("Encoders");
    let mut v = serde_json::Map::new();
    if let Some(ff) = ffmpeg {
        let raw: Vec<Value> = sc_tracks::encoders::survey(ff)
            .into_iter()
            .map(|(n, ok)| json!([n, ok]))
            .collect();
        r.line(format!("  helper raw-track encoders {}", json!(raw)));
        v.insert("helper_raw_tracks".into(), json!(raw));
        let export: Vec<Value> = sc_export::probe::VIDEO_ENCODERS
            .iter()
            .chain(sc_export::probe::AUDIO_ENCODERS.iter())
            .filter(|n| {
                cfg!(target_os = "macos") || !(n.ends_with("_videotoolbox") || **n == "aac_at")
            })
            .map(|n| json!([n, sc_export::probe::encoder_works(ff, n)]))
            .collect();
        r.line(format!("  export encoders {}", json!(export)));
        v.insert("export".into(), json!(export));
    }
    let bench = p
        .writers()
        .encode_benchmark(SizePx { w: 1920, h: 1080 }, 30, 60, VideoCodec::Hevc);
    let bench = match bench {
        Ok(fps) => {
            r.line(format!(
                "  recording encoder ({} tracks): {fps:.0} fps at 1080p",
                p.writers().extension()
            ));
            json!({ "fps_1080p": fps, "container": p.writers().extension() })
        }
        Err(e) => {
            r.line(format!("  recording encoder: {e}"));
            json!({ "error": e.to_string() })
        }
    };
    v.insert("recording".into(), bench);
    r.section("encoders", Value::Object(v));
}

struct Silent;

impl SessionObserver for Silent {
    fn on_state(&self, _: &SessionState) {}
    fn on_health(&self, _: &HealthSnapshot) {}
    fn on_notice(&self, _: &Notice) {}
}

fn main_display(p: &dyn Platform) -> Result<SourceSpec, String> {
    let displays = p.screen().displays().map_err(|e| e.to_string())?;
    let main = displays
        .iter()
        .find(|d| d.is_main)
        .or(displays.first())
        .ok_or("no display")?;
    Ok(SourceSpec::Display {
        display: main.id.clone(),
    })
}

fn record(
    p: Arc<dyn Platform>,
    source: SourceSpec,
    projects: &Path,
    len_ns: i64,
) -> Result<PathBuf, String> {
    let canvas = ProjectV1::new_recording(
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        p.clock_kind(),
    )
    .output
    .canvas;
    let req = RecordRequest {
        projects_dir: projects.to_path_buf(),
        title: "Self-test".into(),
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
        duration_ns: Some(len_ns),
        camera_key: None,
        tuning: Tuning::default(),
    };
    let session = Session::start(p, req, Arc::new(Silent)).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_nanos((len_ns + 300_000_000) as u64));
    let summary = session
        .stop(StopReason::Duration)
        .map_err(|e| e.to_string())?;
    Ok(summary.project_dir)
}

fn edit(dir: &Path) -> Result<((), String), String> {
    let mut project = ProjectV1::load(dir).map_err(|e| e.to_string())?;
    let len = project.duration_ns().ok_or("no duration")?;
    let lim = Limits { rec_len_ns: len };
    let doc = sc_player::timeline_doc(&project);
    let doc = apply(
        &doc,
        &EditOp::DeleteRange {
            a_tl_ns: S,
            b_tl_ns: 2 * S,
        },
        lim,
    )
    .map_err(|e| e.to_string())?;
    let item = TextItem::new(0, S, "SELF-TEST");
    let doc = apply(&doc, &EditOp::UpsertText { item }, lim).map_err(|e| e.to_string())?;
    doc.apply_to(&mut project);
    project.save(dir).map_err(|e| e.to_string())?;
    Ok((
        (),
        format!(
            "{} clips, {:.2} s after cutting 1 s, one text item",
            doc.edl.clips.len(),
            sc_core::edit::duration(&doc.edl) as f64 / S as f64
        ),
    ))
}

struct Quiet;

impl sc_export::ExportObserver for Quiet {
    fn on_state(&self, _: &ExportState) {}
    fn on_progress(&self, _: &sc_export::ExportProgress) {}
}

fn export(
    platform: Arc<dyn Platform>,
    dir: &Path,
    out: PathBuf,
    settings: ExportSettings,
) -> Result<PathBuf, String> {
    let job = ExportJob::start_with(
        platform,
        ExportRequest {
            project_dir: dir.to_path_buf(),
            settings,
            out_path: Some(out),
            canvas: None,
        },
        Arc::new(Quiet),
    )
    .map_err(|e| e.to_string())?;
    job.wait().map_err(|e| e.to_string())
}

struct Inspected {
    summary: String,
    seconds: Option<f64>,
}

fn inspect(file: &Path) -> Result<Inspected, String> {
    let len = std::fs::metadata(file).map_err(|e| e.to_string())?.len();
    if len == 0 {
        return Err("empty file".into());
    }
    let Ok(ffprobe) = sc_helpers::locate(sc_helpers::HelperKind::Ffprobe) else {
        return Ok(Inspected {
            summary: format!("{len} bytes (no ffprobe to inspect it)"),
            seconds: None,
        });
    };
    let out = sc_helpers::hide_console(&mut Command::new(ffprobe))
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name:format=duration",
            "-of",
            "json",
        ])
        .arg(file)
        .output()
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let codecs: Vec<String> = v["streams"]
        .as_array()
        .map(|s| {
            s.iter()
                .filter_map(|x| x["codec_name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if codecs.is_empty() {
        return Err("no streams".into());
    }
    let seconds = v["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok());
    Ok(Inspected {
        summary: format!(
            "{} {:.2} s, {len} bytes",
            codecs.join("+"),
            seconds.unwrap_or(0.0)
        ),
        seconds,
    })
}

fn synthetic(r: &mut Report, work: &Path) {
    r.line("Synthetic record → edit → export");
    let synth: Arc<dyn Platform> = SyntheticPlatform::new(Scenario::new());
    let source =
        sc_platform::synthetic::display_spec("self-test", Some(SizePx { w: 1280, h: 720 }));
    let Some(dir) = r.step("record 3 s from a synthetic source", || {
        let dir = record(synth.clone(), source, work, 3 * S)?;
        Ok((dir, String::new()))
    }) else {
        return;
    };
    if r.step("cut 1 s and add a text item", || edit(&dir))
        .is_none()
    {
        return;
    }
    r.step("export MP4 (H.264 + AAC, 720p) with the cut", || {
        let st = ExportSettings {
            size: SizeChoice::Class(720),
            ..ExportSettings::for_format(Format::Mp4)
        };
        let f = export(synth.clone(), &dir, work.join("self-test.mp4"), st)?;
        let i = inspect(&f)?;
        match i.seconds {
            Some(s) if (s - 2.0).abs() > 0.1 => Err(format!("expected 2 s, got {s:.2} s")),
            _ => Ok(((), i.summary)),
        }
    });
    r.step("export GIF (240p)", || {
        let st = ExportSettings {
            size: SizeChoice::Class(240),
            ..ExportSettings::for_format(Format::Gif)
        };
        let f = export(synth.clone(), &dir, work.join("self-test.gif"), st)?;
        Ok(((), inspect(&f)?.summary))
    });
}

fn real(r: &mut Report, p: Arc<dyn Platform>, work: &Path) {
    const STEP: &str = "record 2 s of the main display (no camera, mic or audio)";
    r.line("Real capture");
    if p.name() == "linux" {
        r.skip(
            STEP,
            "on Linux the system asks which screen to share; record from the app instead",
        );
        return;
    }
    if p.permissions().status(PermissionKind::Screen) != PermissionStatus::Granted {
        r.skip(
            STEP,
            "no Screen Recording grant for this program (the self-test never asks)",
        );
        return;
    }
    if p.screen().displays().map(|d| d.is_empty()).unwrap_or(true) {
        r.skip(STEP, "no display listed");
        return;
    }
    let real_dir = work.join("real");
    let _ = std::fs::create_dir_all(&real_dir);
    let Some(dir) = r.step(STEP, || {
        let dir = record(p.clone(), main_display(&*p)?, &real_dir, 2 * S)?;
        Ok((dir, String::new()))
    }) else {
        return;
    };
    r.step("decode the recorded screen track", || {
        let project = ProjectV1::load(&dir).map_err(|e| e.to_string())?;
        let track = project
            .recording
            .tracks
            .iter()
            .find(|t| t.kind == sc_core::project::TrackKind::Screen)
            .ok_or("no screen track")?;
        let probe = p
            .decoder()
            .probe(&dir.join(&track.file))
            .map_err(|e| e.to_string())?;
        if !probe.readable {
            return Err(probe.error.unwrap_or_else(|| "unreadable".into()));
        }
        Ok((
            (),
            format!(
                "{} frames, {:.2} s, {:?}",
                probe.samples,
                probe.duration_ns.unwrap_or(0) as f64 / S as f64,
                probe.size
            ),
        ))
    });
    r.step("export it (MP4)", || {
        let st = ExportSettings {
            size: SizeChoice::Class(720),
            ..ExportSettings::for_format(Format::Mp4)
        };
        let f = export(p.clone(), &dir, work.join("real.mp4"), st)?;
        Ok(((), inspect(&f)?.summary))
    });
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(work.join("real.mp4"));
}

pub struct Outcome {
    pub text: String,
    pub json: Value,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// Runs every check against `p` (this machine's platform), passing each report line to `out` as
/// it is written.
pub fn run(p: Arc<dyn Platform>, out: &mut dyn FnMut(&str)) -> Outcome {
    let mut r = Report {
        sections: serde_json::Map::new(),
        steps: Vec::new(),
        text: Vec::new(),
        out,
    };
    machine(&mut r, &*p);
    r.line("Helpers");
    let ffmpeg = helpers(&mut r);
    encoders(&mut r, &*p, ffmpeg.as_deref());
    let work = std::env::temp_dir().join(format!("sc-self-test-{}", std::process::id()));
    match std::fs::create_dir_all(&work) {
        Ok(()) => {
            synthetic(&mut r, &work);
            real(&mut r, p.clone(), &work);
        }
        Err(e) => r.line(format!("no work folder {}: {e}", work.display())),
    }
    let _ = std::fs::remove_dir_all(&work);

    let count = |want: Value| r.steps.iter().filter(|s| s["ok"] == want).count();
    let (passed, failed) = (count(Value::Bool(true)), count(Value::Bool(false)));
    let skipped = r.steps.len() - failed - passed;
    r.line(format!(
        "{passed} passed, {failed} failed, {skipped} skipped"
    ));
    let mut doc = r.sections.clone();
    doc.insert("steps".into(), Value::Array(r.steps.clone()));
    doc.insert(
        "summary".into(),
        json!({ "passed": passed, "failed": failed, "skipped": skipped }),
    );
    Outcome {
        text: r.text.join("\n"),
        json: Value::Object(doc),
        passed,
        failed,
        skipped,
    }
}

/// Writes `self-test-<local time>.txt` and `.json` into `dir`; returns the text file.
pub fn write_report(o: &Outcome, dir: &Path, p: &dyn Platform) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let t = p.local_now();
    let base = dir.join(format!(
        "self-test-{:04}{:02}{:02}-{:02}{:02}{:02}",
        t.y, t.mo, t.d, t.h, t.mi, t.s
    ));
    let txt = base.with_extension("txt");
    std::fs::write(&txt, format!("{}\n", o.text))?;
    std::fs::write(
        base.with_extension("json"),
        redact(&serde_json::to_string_pretty(&o.json).unwrap_or_default()),
    )?;
    Ok(txt)
}
