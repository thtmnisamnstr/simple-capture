//! The About window's licenses and the Diagnostics window's self-test.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde_json::{Value, json};

use crate::core::{Core, Reply};

/// The bundle's `licenses` resource folder (staged in app/gen/licenses), else `None` in a
/// development build, which reads the repository's own files.
fn licenses_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path()
        .resource_dir()
        .ok()
        .map(|r| r.join("licenses"))
        .filter(|d| d.is_dir())
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

struct Entry {
    title: &'static str,
    license: &'static str,
    note: &'static str,
    files: Vec<PathBuf>,
}

fn files_in(dir: &Path, filter: impl Fn(&str) -> bool + Copy) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            v.extend(files_in(&p, filter));
        } else if p.file_name().and_then(|n| n.to_str()).is_some_and(filter) {
            v.push(p);
        }
    }
    v.sort();
    v
}

fn entries(bundled: Option<PathBuf>) -> Vec<Entry> {
    let (app_license, helpers, notices) = match &bundled {
        Some(d) => (
            d.join("LICENSE"),
            d.join("helpers"),
            Some(d.join("THIRD_PARTY_RUST.html")),
        ),
        None => (
            repo().join("LICENSE"),
            repo()
                .join("third_party/out")
                .join(sc_helpers::platform_dir_name())
                .join("licenses"),
            None,
        ),
    };
    let fonts = sc_text::fonts::default_fonts_dir();
    let mut out = vec![Entry {
        title: "Simple Capture",
        license: "MIT",
        note: "This app. It has no networking code: nothing you record leaves this computer.",
        files: [app_license].into_iter().filter(|p| p.is_file()).collect(),
    }];
    out.push(Entry {
        title: "FFmpeg export helper",
        license: "GPL-2.0-or-later",
        note: "A separate program the app runs to encode exports, built with x264, x265, \
               SVT-AV1, libvpx, Opus and Vorbis, and with networking turned off. Its source \
               code is published with each release.",
        files: files_in(&helpers, |n| !n.starts_with("gifski")),
    });
    let gifski = {
        let mut f = files_in(&helpers, |n| n.starts_with("gifski"));
        if f.is_empty() {
            let dev = repo()
                .join("third_party/out")
                .join(sc_helpers::platform_dir_name())
                .join("gifski.LICENSE");
            if dev.is_file() {
                f.push(dev);
            }
        }
        f
    };
    out.push(Entry {
        title: "gifski GIF helper",
        license: "AGPL-3.0-or-later",
        note: "A separate program the app runs to make GIFs. Its source code is published \
               with each release.",
        files: gifski,
    });
    out.push(Entry {
        title: "Fonts for text overlays",
        license: "OFL-1.1, Apache-2.0",
        note: "Inter, Montserrat, Oswald, Playfair Display, Bebas Neue, Abril Fatface, Pacifico, \
               Caveat, Baloo 2, Tilt Neon, Sedgwick Ave Display, Courier Prime, Special Elite \
               and Noto Color Emoji.",
        files: files_in(&fonts, |n| n.starts_with("LICENSE")),
    });
    out.push(Entry {
        title: "Rust libraries",
        license: "MIT, Apache-2.0, BSD and others",
        note: "Every library compiled into the app, with its license text.",
        files: notices.into_iter().filter(|p| p.is_file()).collect(),
    });
    out
}

fn allowed(bundled: Option<PathBuf>, path: &Path) -> bool {
    entries(bundled)
        .iter()
        .flat_map(|e| e.files.iter())
        .any(|f| f == path)
}

impl Core {
    pub fn app_action(self: &Arc<Self>, action: &str, args: &Value) -> Reply {
        match action {
            "about" => Ok(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "licenses": entries(licenses_dir(&self.app)).iter().map(|e| json!({
                    "title": e.title,
                    "license": e.license,
                    "note": e.note,
                    "files": e.files.iter().map(|f| json!({
                        "name": f.file_name().map(|n| n.to_string_lossy().into_owned()),
                        "path": f,
                        "html": f.extension().is_some_and(|x| x == "html"),
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })),
            "license_text" => {
                let path = PathBuf::from(
                    args.get("path")
                        .and_then(Value::as_str)
                        .ok_or("missing argument \"path\"")?,
                );
                if !allowed(licenses_dir(&self.app), &path) {
                    return Err("not a license file".into());
                }
                std::fs::read_to_string(&path)
                    .map(Value::String)
                    .map_err(|e| e.to_string())
            }
            "diagnostics_run" => self.diagnostics_run(),
            other => Err(format!("unknown app action {other:?}")),
        }
    }

    fn diagnostics_run(self: &Arc<Self>) -> Reply {
        if self.phase().is_recording() {
            return Err("Stop the recording first.".into());
        }
        if self.diagnostics_busy.swap(true, Ordering::SeqCst) {
            return Err("The self-test is already running.".into());
        }
        let core = self.clone();
        let spawned = std::thread::Builder::new()
            .name("self-test".into())
            .spawn(move || {
                let o = sc_diagnostics::run(core.platform.clone(), &mut |l| {
                    core.emit("diagnostics-line", json!({ "line": l }));
                });
                let report = sc_diagnostics::write_report(
                    &o,
                    &core.data_dir.join("self-test"),
                    &*core.platform,
                );
                core.emit(
                    "diagnostics-done",
                    json!({
                        "passed": o.passed,
                        "failed": o.failed,
                        "skipped": o.skipped,
                        "text": o.text,
                        "report": report.as_ref().ok(),
                        "error": report.as_ref().err().map(ToString::to_string),
                    }),
                );
                core.diagnostics_busy.store(false, Ordering::SeqCst);
            });
        if let Err(e) = spawned {
            self.diagnostics_busy.store(false, Ordering::SeqCst);
            return Err(e.to_string());
        }
        Ok(Value::Null)
    }
}
