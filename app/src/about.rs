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

/// The page the About window links to; App Store review asks for an in-app privacy policy link.
const PRIVACY_POLICY: &str =
    "https://github.com/thtmnisamnstr/simple-capture/blob/main/docs/PRIVACY.md";

#[cfg(not(target_os = "macos"))]
const FFMPEG_LICENSE: &str = "GPL-2.0-or-later";
#[cfg(not(target_os = "macos"))]
const FFMPEG_NOTE: &str = "A separate program the app runs to encode exports, built with x264, x265, SVT-AV1, libvpx, \
     Opus and Vorbis, and with networking turned off. Its complete source code is attached to \
     this version's release on GitHub.";

#[cfg(target_os = "macos")]
const FFMPEG_LICENSE: &str = "LGPL-2.1-or-later";
#[cfg(target_os = "macos")]
const FFMPEG_NOTE: &str = "A separate program the app runs to encode exports, built with SVT-AV1, libvpx, Opus and \
     Vorbis, and with networking turned off. Its complete source code is attached to this \
     version's release on GitHub.";

struct Entry {
    title: &'static str,
    license: &'static str,
    note: &'static str,
    files: Vec<PathBuf>,
    source: bool,
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
        source: false,
    }];
    out.push(Entry {
        title: "FFmpeg export helper",
        license: FFMPEG_LICENSE,
        note: FFMPEG_NOTE,
        files: files_in(&helpers, |n| !n.starts_with("gifski")),
        source: true,
    });
    if !cfg!(target_os = "macos") {
        let mut gifski = files_in(&helpers, |n| n.starts_with("gifski"));
        if gifski.is_empty() {
            let dev = repo()
                .join("third_party/out")
                .join(sc_helpers::platform_dir_name())
                .join("gifski.LICENSE");
            if dev.is_file() {
                gifski.push(dev);
            }
        }
        out.push(Entry {
            title: "gifski GIF helper",
            license: "AGPL-3.0-or-later",
            note: "A separate program the app runs to make GIFs. Its source code is published \
                   with each release.",
            files: gifski,
            source: true,
        });
    }
    out.push(Entry {
        title: "Fonts for text overlays",
        license: "OFL-1.1, Apache-2.0",
        note: "Inter, Montserrat, Oswald, Playfair Display, Bebas Neue, Abril Fatface, Pacifico, \
               Caveat, Baloo 2, Tilt Neon, Sedgwick Ave Display, Courier Prime, Special Elite \
               and Noto Color Emoji.",
        files: files_in(&fonts, |n| n.starts_with("LICENSE")),
        source: false,
    });
    out.push(Entry {
        title: "Rust libraries",
        license: "MIT, Apache-2.0, BSD and others",
        note: "Every library compiled into the app, with its license text.",
        files: notices.into_iter().filter(|p| p.is_file()).collect(),
        source: false,
    });
    out
}

fn allowed(bundled: Option<PathBuf>, path: &Path) -> bool {
    entries(bundled)
        .iter()
        .flat_map(|e| e.files.iter())
        .any(|f| f == path)
}

/// One of the license files the About window lists.
pub fn is_license_file(app: &tauri::AppHandle, path: &Path) -> bool {
    allowed(licenses_dir(app), path)
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
                    "source": e.source,
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
                if !is_license_file(&self.app, &path) {
                    return Err("not a license file".into());
                }
                std::fs::read_to_string(&path)
                    .map(Value::String)
                    .map_err(|e| e.to_string())
            }
            "diagnostics_run" => self.diagnostics_run(),
            "pause" => self.pause_recording(),
            "resume" => self.resume_recording(),
            "toggle_pause" => self.toggle_pause(),
            "restart" => self.restart_recording(),
            "cancel_countdown" => self.cancel_countdown(),
            "toggle_camera" => self.toggle_camera(),
            #[cfg(target_os = "macos")]
            "open_iphone_mirroring" => {
                match sc_capture_macos::workspace::open_app("com.apple.ScreenContinuity") {
                    Ok(()) => Ok(Value::Null),
                    Err(sc_platform::Error::NotFound(_)) => {
                        Err("iPhone Mirroring needs macOS 15 or later".into())
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            "open_privacy_policy" => crate::core::open_url(PRIVACY_POLICY).map(|()| Value::Null),
            "open_source_release" => crate::core::open_url(concat!(
                "https://github.com/thtmnisamnstr/simple-capture/releases/tag/v",
                env!("CARGO_PKG_VERSION")
            ))
            .map(|()| Value::Null),
            "tray_item" => crate::tray::on_item(
                self,
                args.get("id")
                    .and_then(Value::as_str)
                    .ok_or("missing argument \"id\"")?,
            ),
            "rec_status" => Ok(json!({
                "paused": self.is_paused(),
                "elapsed_ns": self.elapsed_ns(),
            })),
            "library" => {
                let root = self.projects_dir();
                let items = crate::library::list(&root);
                Ok(json!({
                    "root": root,
                    "free_bytes": self.platform.free_disk_bytes(&root).ok(),
                    "items": items,
                }))
            }
            "library_trash" => {
                let dir = self.library_dir(args)?;
                if self.editor_dir().as_deref() == Some(dir.as_path()) {
                    self.editor_close_then(false)?;
                    self.hide_window("editor");
                }
                self.platform
                    .desktop()
                    .trash(&dir)
                    .map_err(|e| e.to_string())?;
                self.emit("library-changed", json!({}));
                Ok(Value::Null)
            }
            "shortcuts" => Ok(self.shortcuts_state()),
            "shortcut_set" => {
                let id = args
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("missing argument \"id\"")?;
                let keys = args.get("keys").and_then(Value::as_str).unwrap_or("");
                self.set_shortcut(id, keys)
            }
            "shortcuts_pause" => {
                let paused = args.get("paused").and_then(Value::as_bool).unwrap_or(false);
                self.pause_shortcuts(paused)
            }
            "library_item" => {
                let dir = self.library_dir(args)?;
                serde_json::to_value(crate::library::summarize(&dir)).map_err(|e| e.to_string())
            }
            "library_rename" => {
                let dir = self.library_dir(args)?;
                let title = args
                    .get("title")
                    .and_then(Value::as_str)
                    .ok_or("missing argument \"title\"")?;
                let to = self.rename_recording(&dir, title)?;
                Ok(json!({ "dir": to }))
            }
            "library_export" => {
                let dir = self.library_dir(args)?;
                self.editor_open(&dir)?;
                self.emit("editor-export-sheet", json!({}));
                Ok(Value::Null)
            }
            other => Err(format!("unknown app action {other:?}")),
        }
    }

    fn library_dir(&self, args: &Value) -> Result<PathBuf, String> {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or("missing argument \"name\"")?;
        let dir = crate::library::project_in(&self.projects_dir(), name)
            .ok_or("no such recording in the library")?;
        let project = sc_core::project::ProjectV1::load(&dir).map_err(|e| e.to_string())?;
        if project.state == sc_core::project::ProjectState::Recording && self.phase().is_recording()
        {
            return Err("This recording is still in progress.".into());
        }
        Ok(dir)
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
