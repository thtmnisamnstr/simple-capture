//! `export`, `validate` and `probe` (feature `export`, the only part of the CLI that links Skia).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sc_export::{
    ExportJob, ExportObserver, ExportProgress, ExportSize, ExportState, QuickExportRequest,
};
use sc_geometry::{SizeClass, SizePx};
use sc_platform::Platform;
use serde_json::json;

use crate::args::{ExportArgs, PlatformChoice, ValidateArgs};
use crate::commands::{CliError, CliResult, EXIT_OK, print_json};
use crate::host;

pub fn size_arg(s: &str) -> Result<ExportSize, CliError> {
    Ok(match s {
        "reference" => ExportSize::Reference,
        "2160" => ExportSize::Class(SizeClass::S2160),
        "1440" => ExportSize::Class(SizeClass::S1440),
        "1080" => ExportSize::Class(SizeClass::S1080),
        "720" => ExportSize::Class(SizeClass::S720),
        "480" => ExportSize::Class(SizeClass::S480),
        other => return Err(CliError::usage(format!("--size: unknown {other:?}"))),
    })
}

fn state_json(s: &ExportState) -> serde_json::Value {
    match s {
        ExportState::Preparing => json!({ "ev": "state", "state": "preparing" }),
        ExportState::Rendering => json!({ "ev": "state", "state": "rendering" }),
        ExportState::Finalizing => json!({ "ev": "state", "state": "finalizing" }),
        ExportState::Done { path } => json!({ "ev": "state", "state": "done", "path": path }),
        ExportState::Canceled => json!({ "ev": "state", "state": "canceled" }),
        ExportState::Failed { error } => {
            json!({ "ev": "state", "state": "failed", "error": error })
        }
    }
}

struct Progress {
    json: bool,
    last_print: Mutex<Option<Instant>>,
    terminal: Mutex<bool>,
}

impl ExportObserver for Progress {
    fn on_state(&self, s: &ExportState) {
        if self.json {
            print_json(&state_json(s));
        } else if let ExportState::Failed { error } = s {
            eprintln!("export failed: {error}");
        }
        if matches!(
            s,
            ExportState::Done { .. } | ExportState::Canceled | ExportState::Failed { .. }
        ) && let Ok(mut t) = self.terminal.lock()
        {
            *t = true;
        }
    }

    fn on_progress(&self, p: &ExportProgress) {
        let Ok(mut last) = self.last_print.lock() else {
            return;
        };
        let done = p.frames_done >= p.frames_total;
        if !done && last.is_some_and(|t| t.elapsed() < Duration::from_millis(500)) {
            return;
        }
        *last = Some(Instant::now());
        if self.json {
            print_json(&json!({
                "ev": "progress",
                "frames_done": p.frames_done,
                "frames_total": p.frames_total,
                "fps": p.fps,
                "eta_s": p.eta_s,
            }));
        } else {
            let pct = if p.frames_total > 0 {
                100.0 * p.frames_done as f64 / p.frames_total as f64
            } else {
                0.0
            };
            eprintln!(
                "export: {}/{} frames ({pct:.0}%)  {:.1} fps{}",
                p.frames_done,
                p.frames_total,
                p.fps,
                p.eta_s
                    .map(|e| format!("  eta {e:.0} s"))
                    .unwrap_or_default()
            );
        }
    }
}

pub fn export(choice: PlatformChoice, json: bool, a: ExportArgs) -> CliResult {
    let platform = host::platform(choice)?;
    let req = QuickExportRequest {
        project_dir: a.project.clone(),
        out_path: a.out,
        size: size_arg(&a.size)?,
        fps: a.fps,
    };
    let obs = Arc::new(Progress {
        json,
        last_print: Mutex::new(None),
        terminal: Mutex::new(false),
    });
    host::install_signal_handlers();
    let job = ExportJob::start(platform, req, obs.clone())
        .map_err(|e| CliError::from(e).context(&format!("export {}", a.project.display())))?;
    let mut canceled = false;
    // `wait` consumes the job, so a signal can only cancel it before we wait.
    while !obs.terminal.lock().map(|t| *t).unwrap_or(true) {
        if host::signalled() && !canceled {
            eprintln!("export: canceling");
            job.cancel();
            canceled = true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let path = job
        .wait()
        .map_err(|e| CliError::from(e).context(&format!("export {}", a.project.display())))?;
    if json {
        print_json(&json!({ "ev": "done", "path": path }));
    } else {
        println!("{}", path.display());
    }
    Ok(EXIT_OK)
}

pub fn validate(json: bool, a: ValidateArgs) -> CliResult {
    let expect = sc_export::validate::Expect {
        size: SizePx {
            w: a.size.0,
            h: a.size.1,
        },
        fps: a.fps,
        duration_ns: (a.duration_s * 1e9).round() as i64,
        audio: a.audio,
        vcodec: a.vcodec,
    };
    let r = sc_export::validate::ffprobe_check(&a.file, &expect)
        .map_err(|e| CliError::from(e).context(&format!("validate {}", a.file.display())))?;
    let v = json!({
        "file": a.file,
        "ok": true,
        "vcodec": r.vcodec,
        "size": r.size,
        "fps": r.fps,
        "duration_ns": r.duration_ns,
        "frames": r.frames,
        "acodec": r.acodec,
        "audio_duration_ns": r.audio_duration_ns,
        "color_primaries": r.color_primaries,
        "color_transfer": r.color_transfer,
        "color_space": r.color_space,
        "color_range": r.color_range,
    });
    if json {
        print_json(&v);
    } else {
        println!(
            "ok: {} {}×{} {:.3} fps {:.3} s{}",
            r.vcodec,
            r.size.w,
            r.size.h,
            r.fps,
            r.duration_ns as f64 / 1e9,
            r.acodec
                .as_deref()
                .map(|c| format!(", audio {c}"))
                .unwrap_or_default()
        );
    }
    Ok(EXIT_OK)
}

pub fn probe(p: &dyn Platform, json: bool, refresh: bool) -> CliResult {
    let displays = p.screen().displays()?;
    let cache_dir = host::data_dir()?;
    if refresh {
        let cached = cache_dir.join("probe-v1.json");
        match std::fs::remove_file(&cached) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(CliError::error(format!(
                    "probe --refresh: remove {}: {e}",
                    cached.display()
                )));
            }
        }
    }
    let probe = sc_export::probe::load_or_run(p, &cache_dir, &displays)?;
    let text = if json {
        serde_json::to_string(&probe)
    } else {
        serde_json::to_string_pretty(&probe)
    }
    .map_err(|e| CliError::error(format!("probe: {e}")))?;
    println!("{text}");
    Ok(EXIT_OK)
}
