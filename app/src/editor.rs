//! The editor window's session (SPEC Editor, ARCHITECTURE §7): one open project, its undo
//! history, the clipboard, the playhead and playback, and autosave. The UI sends `editor`
//! commands; the session answers with the document and emits `editor-doc` and
//! `editor-playhead` events.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use sc_core::edit::{Clipboard, EditOp, History, Limits, copy_clips, copy_range};
use sc_core::project::{ProjectState, ProjectV1, TrackKind};
use serde::Deserialize;
use serde_json::{Value, json};
use tauri::Manager;

use crate::core::{Core, Reply};
use crate::preview::StagePage;

pub const AUTOSAVE_AFTER: Duration = Duration::from_millis(400);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Where the preview sits in the page, in points.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct PreviewRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub struct Session {
    pub id: u64,
    pub dir: PathBuf,
    pub project: Arc<ProjectV1>,
    pub history: History,
    pub limits: Limits,
    pub clipboard: Clipboard,
    pub playhead: i64,
    pub playing: bool,
    pub saved_rev: u64,
    pub changed_at: Option<Instant>,
    pub rect: Option<PreviewRect>,
    pub fps: u32,
    pub opened_at: Instant,
    pub first_frame_ms: Option<u64>,
    pub retitled: bool,
    pub loudness: Option<Arc<Loudness>>,
}

/// Per-10 ms loudness of the tracks that are heard, kept while the editor has the project open.
pub struct Loudness {
    tracks: Vec<String>,
    db: Vec<f32>,
}

impl Session {
    pub fn duration(&self) -> i64 {
        self.history.doc().duration()
    }
}

#[derive(Default)]
pub struct EditorHost {
    pub session: Mutex<Option<Session>>,
    pub wake: Condvar,
    pub playback: Mutex<Option<sc_player::output::Playback>>,
    pub page: Mutex<Option<StagePage>>,
    #[cfg(target_os = "macos")]
    pub view: Mutex<Option<usize>>,
    pub frames: AtomicU64,
    next_id: AtomicU64,
    scan: Mutex<()>,
}

impl EditorHost {
    pub fn poke(&self) {
        self.wake.notify_all();
    }
}

#[derive(Deserialize)]
struct Range {
    a_tl_ns: Option<i64>,
    b_tl_ns: Option<i64>,
    ids: Option<Vec<String>>,
}

fn arg<T: serde::de::DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    let v = args
        .get(name)
        .filter(|v| !v.is_null())
        .ok_or_else(|| format!("missing argument {name:?}"))?;
    serde_json::from_value(v.clone()).map_err(|e| format!("argument {name:?}: {e}"))
}

fn opt<T: serde::de::DeserializeOwned>(args: &Value, name: &str) -> Result<Option<T>, String> {
    match args.get(name).filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone())
            .map(Some)
            .map_err(|e| format!("argument {name:?}: {e}")),
        None => Ok(None),
    }
}

fn track_json(p: &ProjectV1) -> Value {
    let kind = |k: TrackKind| match k {
        TrackKind::Screen => "screen",
        TrackKind::Camera => "camera",
        TrackKind::Mic => "mic",
        TrackKind::SystemAudio => "system_audio",
    };
    Value::Array(
        p.recording
            .tracks
            .iter()
            .map(|t| json!({ "id": t.id, "kind": kind(t.kind) }))
            .collect(),
    )
}

fn editor_fonts() -> Value {
    let bundled: Vec<Value> = sc_text::fonts::BUNDLED
        .iter()
        .map(|f| {
            let lo = f.faces.iter().map(|x| x.weights.0).min().unwrap_or(400);
            let hi = f.faces.iter().map(|x| x.weights.1).max().unwrap_or(400);
            let mut weights: Vec<u32> = f
                .faces
                .iter()
                .flat_map(|x| [x.weights.0, x.weights.1])
                .collect();
            weights.sort_unstable();
            weights.dedup();
            json!({
                "name": f.name,
                "kind": f.kind,
                "default_weight": f.default_weight,
                "weights": [lo, hi],
                "static_weights": weights,
                "italic": f.faces.iter().any(|x| x.italic),
                "faces": f.faces.iter().map(|x| json!({
                    "file": x.file, "italic": x.italic, "weights": [x.weights.0, x.weights.1],
                })).collect::<Vec<_>>(),
                "variable": f.faces.iter().any(|x| x.weights.0 < x.weights.1),
            })
        })
        .collect();
    json!({ "bundled": bundled, "system": sc_text::fonts::system_families() })
}

impl Core {
    pub fn pick_color(&self) -> Reply {
        let (tx, rx) = std::sync::mpsc::channel();
        self.platform.desktop().pick_color(Box::new(move |c| {
            let _ = tx.send(c);
        }));
        let c = rx
            .recv()
            .map_err(|_| "the eyedropper went away".to_string())?;
        Ok(c.map(|c| json!(c)).unwrap_or(Value::Null))
    }

    pub fn editor(self: &Arc<Self>, action: &str, args: &Value) -> Reply {
        match action {
            "open" => self.editor_open(Path::new(&arg::<String>(args, "project_dir")?)),
            "state" => self.editor_state(),
            "edit" => self.editor_edit(
                &arg::<EditOp>(args, "op")?,
                opt::<String>(args, "coalesce")?.as_deref(),
            ),
            "undo" => self.editor_history(|h| h.undo()),
            "redo" => self.editor_history(|h| h.redo()),
            "seal" => self.editor_history(|h| {
                h.seal();
                false
            }),
            "copy" => self.editor_copy(&arg::<Range>(args, "range")?, false),
            "cut" => self.editor_copy(&arg::<Range>(args, "range")?, true),
            "paste" => self.editor_paste(arg(args, "at_tl_ns")?),
            "play" => self.editor_play(opt(args, "from_tl_ns")?),
            "pause" => self.editor_pause(),
            "seek" => self.editor_seek(arg(args, "tl_ns")?),
            "rect" => self.editor_rect(opt(args, "rect")?),
            "export" => {
                self.editor_export(&opt::<String>(args, "size")?.unwrap_or("reference".into()))
            }
            "close" => self.editor_close(),
            "silences" => self.editor_silences(
                opt(args, "threshold_db")?,
                opt(args, "min_ms")?.unwrap_or(750),
                opt(args, "pad_ms")?.unwrap_or(150),
            ),
            "rename" => {
                let dir = self
                    .editor_dir()
                    .ok_or("no project is open in the editor")?;
                let title = arg::<String>(args, "title")?;
                self.rename_recording(&dir, &title).map(|d| json!(d))
            }
            "background_image" => self.editor_background_image(&arg::<String>(args, "path")?),
            "fonts" => Ok(editor_fonts()),
            "pick_color" => self.pick_color(),
            a if a.starts_with("export_")
                || matches!(a, "choose_folder" | "copy_file" | "open_file") =>
            {
                self.export_action(a, args)
            }
            other => Err(format!("unknown editor action {other:?}")),
        }
    }

    fn with_session<T>(
        &self,
        f: impl FnOnce(&mut Session) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut s = lock(&self.editor.session);
        let s = s.as_mut().ok_or("no project is open in the editor")?;
        f(s)
    }

    pub fn editor_open(self: &Arc<Self>, dir: &Path) -> Reply {
        let project = ProjectV1::load(dir).map_err(|e| format!("open {}: {e}", dir.display()))?;
        if project.state == ProjectState::Recording {
            return Err("this recording is still in progress".into());
        }
        self.editor_save_now();
        let doc = sc_player::timeline_doc(&project);
        let rec_len = project.duration_ns().unwrap_or_else(|| doc.duration()).max(
            doc.edl
                .clips
                .iter()
                .chain(doc.camera_clips().clips.iter())
                .filter(|c| !c.hidden)
                .map(|c| c.src_out_rec_ns)
                .max()
                .unwrap_or(0),
        );
        let fps = project.recording.screen.fps_requested.clamp(1, 120);
        let project = Arc::new(project);
        if let Some(p) = lock(&self.editor.playback).take() {
            p.pause();
        }
        *lock(&self.editor.playback) = Some(sc_player::output::Playback::start(
            project.clone(),
            dir,
            self.platform.decoder(),
            doc.clone(),
        ));
        let id = self.editor.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let before = lock(&self.editor.session).take();
        let retitled = match before {
            Some(old) if old.dir == dir => old.retitled,
            Some(old) => {
                self.settle_folder(old);
                false
            }
            None => false,
        };
        *lock(&self.editor.session) = Some(Session {
            id,
            dir: dir.to_path_buf(),
            project: project.clone(),
            history: History::new(doc),
            limits: Limits {
                rec_len_ns: rec_len,
            },
            clipboard: Clipboard::default(),
            playhead: 0,
            playing: false,
            saved_rev: 0,
            changed_at: None,
            rect: None,
            fps,
            opened_at: Instant::now(),
            first_frame_ms: None,
            retitled,
            loudness: None,
        });
        self.editor.poke();
        self.emit("editor-open", json!({ "project_dir": dir }));
        crate::editor_media::spawn(self, id, dir.to_path_buf(), project);
        self.show_editor_window();
        self.editor_state()
    }

    fn show_editor_window(&self) {
        if let Err(e) = self.show_window("editor", true) {
            tracing::warn!("editor window: {e}");
        }
    }

    pub fn editor_state(&self) -> Reply {
        self.with_session(|s| {
            Ok(json!({
                "project_dir": s.dir,
                "title": s.project.title,
                "rec_len_ns": s.limits.rec_len_ns,
                "fps": s.fps,
                "tracks": track_json(&s.project),
                "segments": s.project.recording.screen.segments.iter().map(|g| json!({
                    "id": g.id, "start_rec_ns": g.start_rec_ns, "end_rec_ns": g.end_rec_ns,
                })).collect::<Vec<_>>(),
                "doc": s.history.doc(),
                "revision": s.history.revision(),
                "can_undo": s.history.can_undo(),
                "can_redo": s.history.can_redo(),
                "playhead_tl_ns": s.playhead,
                "playing": s.playing,
                "clipboard_ns": s.clipboard.len(),
                "frames_presented": self.editor.frames.load(Ordering::Relaxed),
                "first_frame_ms": s.first_frame_ms,
            }))
        })
    }

    fn doc_changed(&self, s: &mut Session) -> Value {
        s.changed_at = Some(Instant::now());
        let dur = s.duration();
        if s.playhead > dur {
            s.playhead = dur;
        }
        if let Some(p) = lock(&self.editor.playback).as_ref() {
            p.set_doc(s.history.doc().clone());
        }
        let v = json!({
            "doc": s.history.doc(),
            "revision": s.history.revision(),
            "can_undo": s.history.can_undo(),
            "can_redo": s.history.can_redo(),
            "playhead_tl_ns": s.playhead,
        });
        self.emit("editor-doc", v.clone());
        self.editor.poke();
        v
    }

    fn editor_edit(&self, op: &EditOp, coalesce: Option<&str>) -> Reply {
        self.with_session(|s| {
            let changed = s
                .history
                .apply(op, s.limits, coalesce)
                .map_err(|e| e.to_string())?;
            Ok(if changed {
                self.doc_changed(s)
            } else {
                json!({ "revision": s.history.revision() })
            })
        })
    }

    fn editor_history(&self, f: impl FnOnce(&mut History) -> bool) -> Reply {
        self.with_session(|s| {
            Ok(if f(&mut s.history) {
                self.doc_changed(s)
            } else {
                json!({ "revision": s.history.revision() })
            })
        })
    }

    fn editor_copy(&self, r: &Range, cut: bool) -> Reply {
        self.with_session(|s| {
            let doc = s.history.doc().clone();
            let (cb, del) = match (&r.ids, r.a_tl_ns, r.b_tl_ns) {
                (Some(ids), _, _) if !ids.is_empty() => (
                    copy_clips(&doc, ids).map_err(|e| e.to_string())?,
                    EditOp::DeleteClips { ids: ids.clone() },
                ),
                (_, Some(a), Some(b)) if b != a => (
                    copy_range(&doc, a.min(b), a.max(b)),
                    EditOp::DeleteRange {
                        a_tl_ns: a.min(b),
                        b_tl_ns: a.max(b),
                    },
                ),
                _ => return Err("nothing is selected".into()),
            };
            let len = cb.len();
            s.clipboard = cb;
            if cut {
                s.history.seal();
                s.history
                    .apply(&del, s.limits, None)
                    .map_err(|e| e.to_string())?;
                let mut v = self.doc_changed(s);
                v["clipboard_ns"] = json!(len);
                return Ok(v);
            }
            Ok(json!({ "clipboard_ns": len }))
        })
    }

    fn editor_paste(&self, at: i64) -> Reply {
        self.with_session(|s| {
            if s.clipboard.is_empty() {
                return Err("the clipboard is empty".into());
            }
            let op = EditOp::Paste {
                at_tl_ns: at,
                clipboard: s.clipboard.clone(),
            };
            s.history.seal();
            s.history
                .apply(&op, s.limits, None)
                .map_err(|e| e.to_string())?;
            Ok(self.doc_changed(s))
        })
    }

    fn editor_play(&self, from: Option<i64>) -> Reply {
        let start = self.with_session(|s| {
            let dur = s.duration();
            let mut at = from.unwrap_or(s.playhead).clamp(0, dur);
            if at >= dur - 1_000_000 {
                at = 0;
            }
            s.playhead = at;
            s.playing = true;
            Ok(at)
        })?;
        if let Some(p) = lock(&self.editor.playback).as_ref() {
            p.play(start);
        }
        self.editor.poke();
        self.emit_playhead(start, true);
        Ok(json!({ "playhead_tl_ns": start, "playing": true }))
    }

    pub fn editor_pause(&self) -> Reply {
        let now = lock(&self.editor.playback)
            .as_ref()
            .filter(|p| p.playing())
            .map(|p| p.now_tl());
        if let Some(p) = lock(&self.editor.playback).as_ref() {
            p.pause();
        }
        let at = self.with_session(|s| {
            if s.playing
                && let Some(t) = now
            {
                s.playhead = t.clamp(0, s.duration());
            }
            s.playing = false;
            Ok(s.playhead)
        })?;
        self.editor.poke();
        self.emit_playhead(at, false);
        Ok(json!({ "playhead_tl_ns": at, "playing": false }))
    }

    fn editor_seek(&self, tl: i64) -> Reply {
        let playing = self.with_session(|s| {
            s.playhead = tl.clamp(0, s.duration());
            Ok(s.playing)
        })?;
        if playing && let Some(p) = lock(&self.editor.playback).as_ref() {
            p.play(tl);
        }
        self.editor.poke();
        Ok(Value::Null)
    }

    fn editor_rect(&self, rect: Option<PreviewRect>) -> Reply {
        self.with_session(|s| {
            s.rect = rect;
            Ok(())
        })?;
        self.editor.poke();
        Ok(Value::Null)
    }

    pub fn emit_playhead(&self, tl: i64, playing: bool) {
        self.emit(
            "editor-playhead",
            json!({ "tl_ns": tl, "playing": playing }),
        );
    }

    pub fn editor_save_now(&self) {
        self.editor_save(false);
    }

    fn editor_save(&self, force: bool) {
        let mut guard = lock(&self.editor.session);
        let Some(s) = guard.as_mut() else { return };
        if !force && s.history.revision() == s.saved_rev {
            return;
        }
        let mut p = (*s.project).clone();
        s.history.doc().apply_to(&mut p);
        match p.save(&s.dir) {
            Ok(()) => {
                s.saved_rev = s.history.revision();
                s.project = Arc::new(p);
                s.changed_at = None;
            }
            Err(e) => tracing::warn!(dir = %s.dir.display(), "autosave: {e}"),
        }
    }

    pub fn editor_autosave_tick(&self) {
        let due = lock(&self.editor.session)
            .as_ref()
            .and_then(|s| s.changed_at)
            .is_some_and(|t| t.elapsed() >= AUTOSAVE_AFTER);
        if due {
            self.editor_save_now();
        }
    }

    fn editor_background_image(&self, path: &str) -> Reply {
        let dir = self.with_session(|s| Ok(s.dir.clone()))?;
        let bg = sc_core::project::copy_background_asset(Path::new(path), &dir)
            .map_err(|e| e.to_string())?;
        serde_json::to_value(bg).map_err(|e| e.to_string())
    }

    fn editor_export(self: &Arc<Self>, size: &str) -> Reply {
        let _ = self.editor_pause();
        self.editor_save_now();
        let dir = self.with_session(|s| Ok(s.dir.clone()))?;
        self.quick_export(&dir.to_string_lossy(), size)
    }

    pub fn editor_close(&self) -> Reply {
        self.editor_close_then(true)
    }

    /// `settle`: move a renamed project's folder to its title; skipped when the folder is about
    /// to go to the Trash.
    pub fn editor_close_then(&self, settle: bool) -> Reply {
        let _ = self.editor_pause();
        self.editor_save_now();
        if let Some(p) = lock(&self.editor.playback).take() {
            p.pause();
        }
        let closed = lock(&self.editor.session).take();
        self.editor.poke();
        if let Some(s) = closed.filter(|_| settle) {
            self.settle_folder(s);
        }
        self.hide_window("editor");
        Ok(Value::Null)
    }

    /// The pauses the current threshold finds, as timeline ranges, without changing anything.
    fn editor_silences(&self, threshold_db: Option<f32>, min_ms: i64, pad_ms: i64) -> Reply {
        let (id, dir, project, doc, cached) = self.with_session(|s| {
            Ok((
                s.id,
                s.dir.clone(),
                s.project.clone(),
                s.history.doc().clone(),
                s.loudness.clone(),
            ))
        })?;
        let tracks: Vec<String> = sc_player::audio::audio_track_ids(&project)
            .into_iter()
            .filter(|t| !doc.audio_mix.get(t).is_some_and(|m| m.muted))
            .collect();
        if tracks.is_empty() {
            return Err("This recording has no sound to find pauses in.".into());
        }
        let _one_scan = lock(&self.editor.scan);
        let cached = lock(&self.editor.session)
            .as_ref()
            .filter(|s| s.id == id)
            .and_then(|s| s.loudness.clone())
            .or(cached);
        let loud = match cached.filter(|l| l.tracks == tracks) {
            Some(l) => l,
            None => {
                let db = crate::editor_media::loudness_db(self, &dir, &project, &tracks)?;
                let l = Arc::new(Loudness { tracks, db });
                if let Some(s) = lock(&self.editor.session).as_mut().filter(|s| s.id == id) {
                    s.loudness = Some(l.clone());
                }
                l
            }
        };
        let suggested = sc_core::silence::suggested_threshold(&loud.db);
        let rule = sc_core::silence::QuietRule {
            threshold_db: threshold_db.unwrap_or(suggested).clamp(-80.0, -5.0),
            min_ns: min_ms.clamp(100, 10_000) * 1_000_000,
            pad_ns: pad_ms.clamp(0, 1_000) * 1_000_000,
        };
        let window = 1_000_000_000 / i64::from(crate::editor_media::PEAKS_PER_S);
        let spans = sc_core::silence::quiet_spans(&loud.db, window, rule);
        let ranges = sc_core::silence::on_doc(&doc, &spans);
        let removed: i64 = ranges.iter().map(|(a, b)| b - a).sum();
        Ok(json!({
            "ranges": ranges,
            "removed_ns": removed,
            "duration_ns": doc.duration(),
            "threshold_db": rule.threshold_db,
            "suggested_db": suggested,
        }))
    }

    pub fn editor_retitle(&self, title: &str) -> Result<(), String> {
        self.with_session(|s| {
            let mut p = (*s.project).clone();
            p.title = title.to_string();
            s.project = Arc::new(p);
            s.retitled = true;
            Ok(())
        })?;
        self.editor_save(true);
        Ok(())
    }

    /// A project leaving the editor after a rename: its folder now follows the title.
    fn settle_folder(&self, s: Session) {
        if s.retitled {
            self.move_folder_to_title(&s.dir, &s.project.title);
        }
    }

    pub fn edit_last(self: &Arc<Self>) -> Reply {
        let dir = self
            .last_project()
            .or_else(|| crate::library::latest_editable(&self.projects_dir()))
            .ok_or("there are no recordings yet")?;
        self.editor_open(&dir)
    }

    #[cfg(debug_assertions)]
    pub fn editor_segment_ids(&self) -> Vec<String> {
        lock(&self.editor.session)
            .as_ref()
            .map(|s| {
                s.project
                    .recording
                    .screen
                    .segments
                    .iter()
                    .map(|g| g.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[cfg(debug_assertions)]
    pub fn editor_clip_ids(&self, camera: bool) -> Vec<String> {
        lock(&self.editor.session)
            .as_ref()
            .map(|s| {
                let d = s.history.doc();
                let edl = if camera { d.camera_clips() } else { &d.edl };
                edl.clips.iter().map(|c| c.id.clone()).collect()
            })
            .unwrap_or_default()
    }

    pub fn editor_dir(&self) -> Option<PathBuf> {
        lock(&self.editor.session).as_ref().map(|s| s.dir.clone())
    }

    pub fn editor_page(&self) -> Option<StagePage> {
        if let Some(p) = *lock(&self.editor.page) {
            return Some(p);
        }
        let w = self.app.get_webview_window("editor")?;
        let scale = w.scale_factor().unwrap_or(1.0);
        let l = w.inner_size().ok()?.to_logical::<f64>(scale);
        let p = StagePage {
            w: l.width,
            h: l.height,
            scale,
        };
        *lock(&self.editor.page) = Some(p);
        Some(p)
    }

    pub fn editor_window_changed(&self) {
        *lock(&self.editor.page) = None;
        #[cfg(target_os = "macos")]
        if let Some(view) = *lock(&self.editor.view) {
            *lock(&self.editor.page) = crate::sink::refit(view);
        }
        self.editor.poke();
    }

    #[cfg(target_os = "macos")]
    pub fn editor_layer(&self) -> Result<std::ptr::NonNull<std::ffi::c_void>, String> {
        let (layer, view, page) = crate::sink::attach_layer(&self.app, "editor")?;
        *lock(&self.editor.view) = Some(view);
        *lock(&self.editor.page) = Some(page);
        std::ptr::NonNull::new(layer as *mut std::ffi::c_void).ok_or_else(|| "null layer".into())
    }
}
