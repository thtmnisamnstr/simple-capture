//! App state and every operation behind the commands, tray, hotkey and debug driver.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak, mpsc};
use std::time::{Duration, Instant};

use sc_core::journal::StopReason;
use sc_core::project::{Background, Fit, ProjectV1, Via};
use sc_export::probe::ProbeV1;
use sc_export::{ExportJob, ExportObserver, ExportProgress, ExportState, QuickExportRequest};
use sc_geometry::{PointF, RectPt, SizePx};
use sc_platform::{
    AudioBuffer, DeviceStream, FrameSink, OverlayRole, PermissionKind, PermissionStatus,
    PickedSource, PickerRequest, Platform, Resolution, SharedCamera, SourceEvent, VideoFrame,
    WindowId, WithSharedCamera,
};
use sc_record::recovery::RecoveryReport;
use sc_record::{HealthSnapshot, Notice, Session, SessionObserver, SessionState};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager};

use crate::dto::{self, RecordRequestDto, RequestEnv, SourceSpecDto};
use crate::preview::StagePage;
use crate::settings::{CanvasDto, RecorderSettings, SettingsStore};
use crate::state::{Input, Phase};
use crate::still::{self, StillCache};

pub const WINDOWS: [(&str, OverlayRole); 12] = [
    ("panel", OverlayRole::Panel),
    ("onboarding", OverlayRole::Onboarding),
    ("bar", OverlayRole::FloatingBar),
    ("switcher", OverlayRole::Popover),
    ("region", OverlayRole::RegionOverlay),
    ("pick", OverlayRole::PickOverlay),
    ("saved", OverlayRole::Saved),
    ("stage", OverlayRole::Stage),
    ("editor", OverlayRole::Editor),
    ("library", OverlayRole::Editor),
    ("diagnostics", OverlayRole::Editor),
    ("about", OverlayRole::Editor),
];

pub type Reply = Result<Value, String>;

/// Pointer and window positions (global points) when a bar drag began.
type BarDrag = ((f64, f64), (f64, f64));

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct Core {
    pub app: AppHandle,
    pub platform: Arc<dyn Platform>,
    pub camera: Arc<SharedCamera>,
    mic_meter: Mutex<Option<(String, Box<dyn sc_platform::DeviceStream>)>>,
    pub live: crate::live::LiveHub,
    pub preview: Mutex<crate::preview::PreviewState>,
    pub editor: crate::editor::EditorHost,
    bar_drag: Mutex<Option<BarDrag>>,
    pub data_dir: PathBuf,
    pub diagnostics_busy: std::sync::atomic::AtomicBool,
    projects_dir: PathBuf,
    settings: SettingsStore,
    settings_write: Mutex<()>,
    phase: Mutex<Phase>,
    session: Mutex<Option<Session>>,
    rec_started: Mutex<Option<Instant>>,
    paused_total: Mutex<std::time::Duration>,
    paused_since: Mutex<Option<Instant>>,
    countdown_cancel: std::sync::atomic::AtomicBool,
    last_request: Mutex<Option<RecordRequestDto>>,
    raw_after_export: Mutex<HashMap<String, PathBuf>>,
    window_ids: Mutex<Vec<(String, WindowId)>>,
    picks: Mutex<HashMap<String, PickedSource>>,
    stills: Mutex<StillCache>,
    exports: Mutex<HashMap<String, ExportJob>>,
    recovery: Mutex<Vec<RecoveryReport>>,
    probe: Mutex<Option<ProbeV1>>,
    overlay_purpose: Mutex<String>,
    watcher: Mutex<Option<Box<dyn DeviceStream>>>,
    last_health: Mutex<Option<(Instant, i64)>>,
    last_project: Mutex<Option<PathBuf>>,
    last_export: Mutex<Option<String>>,
    ui_log: Mutex<Vec<String>>,
    exiting: AtomicBool,
    pub launch_done: AtomicBool,
    pub exit_done: AtomicBool,
    pub no_key_windows: bool,
    pub tray: OnceLock<crate::tray::Tray>,
}

impl Core {
    pub fn new(
        app: AppHandle,
        platform: Arc<dyn Platform>,
        data_dir: PathBuf,
        projects_dir: PathBuf,
        no_key_windows: bool,
    ) -> Core {
        let (platform, camera) = WithSharedCamera::new(platform);
        Core {
            settings: SettingsStore::new(&data_dir),
            settings_write: Mutex::new(()),
            app,
            platform,
            camera,
            mic_meter: Mutex::new(None),
            live: crate::live::LiveHub::default(),
            preview: Mutex::new(crate::preview::PreviewState::default()),
            editor: crate::editor::EditorHost::default(),
            bar_drag: Mutex::new(None),
            data_dir,
            diagnostics_busy: std::sync::atomic::AtomicBool::new(false),
            projects_dir,
            phase: Mutex::new(Phase::Idle),
            session: Mutex::new(None),
            rec_started: Mutex::new(None),
            paused_total: Mutex::new(std::time::Duration::ZERO),
            paused_since: Mutex::new(None),
            countdown_cancel: std::sync::atomic::AtomicBool::new(false),
            last_request: Mutex::new(None),
            raw_after_export: Mutex::new(HashMap::new()),
            window_ids: Mutex::new(Vec::new()),
            picks: Mutex::new(HashMap::new()),
            stills: Mutex::new(StillCache::default()),
            exports: Mutex::new(HashMap::new()),
            recovery: Mutex::new(Vec::new()),
            probe: Mutex::new(None),
            overlay_purpose: Mutex::new("start".into()),
            watcher: Mutex::new(None),
            last_health: Mutex::new(None),
            last_project: Mutex::new(None),
            last_export: Mutex::new(None),
            ui_log: Mutex::new(Vec::new()),
            exiting: AtomicBool::new(false),
            launch_done: AtomicBool::new(false),
            exit_done: AtomicBool::new(false),
            no_key_windows,
            tray: OnceLock::new(),
        }
    }

    pub fn phase(&self) -> Phase {
        lock(&self.phase).clone()
    }

    fn advance(&self, input: Input) -> Result<Phase, String> {
        let (before, next) = {
            let mut p = lock(&self.phase);
            let next = p.next(input)?;
            (std::mem::replace(&mut *p, next.clone()), next)
        };
        if is_recording(&before) != is_recording(&next)
            && let Err(e) = self.refresh_camera()
        {
            tracing::warn!("camera after {next:?}: {e}");
        }
        Ok(next)
    }

    pub fn emit(&self, event: &str, payload: Value) {
        if self.no_key_windows {
            eprintln!(
                "{}",
                json!({"event": event, "rec_ns": self.rec_ns_now(), "payload": payload})
            );
        }
        if let Err(e) = self.app.emit(event, payload) {
            tracing::warn!(event, "emit failed: {e}");
        }
    }

    /// Recording time now, extrapolated from the last 1 Hz health snapshot.
    pub fn rec_ns_now(&self) -> Option<i64> {
        if !self.phase().is_recording() {
            return None;
        }
        lock(&self.last_health).map(|(at, rec)| {
            rec.saturating_add(i64::try_from(at.elapsed().as_nanos()).unwrap_or(i64::MAX))
        })
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.projects_dir.clone()
    }

    pub fn last_project(&self) -> Option<PathBuf> {
        lock(&self.last_project).clone()
    }

    pub fn last_export(&self) -> Option<String> {
        lock(&self.last_export).clone()
    }

    /// A console line the UI forwarded (`ui-log` event): CSP violations, errors, driver probes.
    pub fn on_ui_log(&self, line: String) {
        tracing::warn!(target: "ui", "{line}");
        let mut log = lock(&self.ui_log);
        if log.len() < 1000 {
            log.push(line);
        }
    }

    pub fn take_ui_log(&self) -> Vec<String> {
        std::mem::take(&mut *lock(&self.ui_log))
    }

    /// Recorded time so far, paused time left out (what the bar and tray show).
    pub fn elapsed_ns(&self) -> i64 {
        let Some(t) = *lock(&self.rec_started) else {
            return 0;
        };
        let paused = *lock(&self.paused_total)
            + lock(&self.paused_since).map_or(std::time::Duration::ZERO, |p| p.elapsed());
        i64::try_from(t.elapsed().saturating_sub(paused).as_nanos()).unwrap_or(i64::MAX)
    }

    pub fn is_paused(&self) -> bool {
        lock(&self.paused_since).is_some()
    }

    pub fn pause_recording(&self) -> Reply {
        let guard = lock(&self.session);
        let s = guard.as_ref().ok_or("not recording")?;
        s.pause().map_err(|e| e.to_string())?;
        lock(&self.paused_since).get_or_insert_with(Instant::now);
        drop(guard);
        self.emit(
            "recording-paused",
            json!({ "paused": true, "elapsed_ns": self.elapsed_ns() }),
        );
        self.refresh_tray();
        Ok(Value::Null)
    }

    pub fn resume_recording(&self) -> Reply {
        let guard = lock(&self.session);
        let s = guard.as_ref().ok_or("not recording")?;
        s.resume().map_err(|e| e.to_string())?;
        if let Some(p) = lock(&self.paused_since).take() {
            *lock(&self.paused_total) += p.elapsed();
        }
        drop(guard);
        self.emit(
            "recording-paused",
            json!({ "paused": false, "elapsed_ns": self.elapsed_ns() }),
        );
        self.refresh_tray();
        Ok(Value::Null)
    }

    pub fn toggle_pause(&self) -> Reply {
        if self.is_paused() {
            self.resume_recording()
        } else {
            self.pause_recording()
        }
    }

    /// Throws the current recording away and starts a new one with the same request.
    pub fn restart_recording(self: &Arc<Self>) -> Reply {
        let req = lock(&self.last_request)
            .clone()
            .ok_or("nothing to restart")?;
        self.discard_recording()?;
        self.start_recording_now(req)
    }

    pub fn cancel_countdown(&self) -> Reply {
        self.countdown_cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(Value::Null)
    }

    /// Hotkey: hide or show the camera in the recording from now on.
    pub fn toggle_camera(&self) -> Reply {
        let mut settings = self.settings.load();
        settings.camera_key.visible = !settings.camera_key.visible;
        let key = settings.camera_key.clone();
        self.settings.save(&settings)?;
        self.emit(
            "settings-changed",
            serde_json::to_value(&settings).unwrap_or(Value::Null),
        );
        if let Some(s) = lock(&self.session).as_ref() {
            s.camera_key(key).map_err(|e| e.to_string())?;
        }
        Ok(Value::Null)
    }

    /// Main thread: swaps every window to our non-activating panel and records its ID.
    pub fn configure_windows(&self) -> Result<(), String> {
        let mut ids = Vec::new();
        for (label, role) in WINDOWS {
            let w = self
                .app
                .get_webview_window(label)
                .ok_or_else(|| format!("window {label} missing from tauri.conf.json"))?;
            let id = configure_one(&*self.platform, &w, role)
                .map_err(|e| format!("configure window {label}: {e}"))?;
            #[cfg(target_os = "macos")]
            match label {
                "bar" => apply_material(&w, sc_capture_macos::material::Material::Hud, 16.0),
                "switcher" => {
                    apply_material(&w, sc_capture_macos::material::Material::Popover, 12.0)
                }
                _ => {}
            }
            tracing::info!(label, id = id.0, ?role, "configured window");
            ids.push((label.to_string(), id));
        }
        *lock(&self.window_ids) = ids;
        Ok(())
    }

    pub fn window_ids(&self) -> Vec<WindowId> {
        let mut v: Vec<WindowId> = lock(&self.window_ids).iter().map(|(_, id)| *id).collect();
        for id in self.platform.overlays().registered() {
            if !v.contains(&id) {
                v.push(id);
            }
        }
        v
    }

    pub fn show_window(&self, label: &str, key: bool) -> Result<(), String> {
        let w = self
            .app
            .get_webview_window(label)
            .ok_or_else(|| format!("no window {label:?}"))?;
        if key && !self.no_key_windows {
            w.show().map_err(|e| format!("show {label}: {e}"))?;
        } else {
            show_without_key(&w)?;
        }
        if label == "panel"
            && let Err(e) = self.camera_preview(true)
        {
            tracing::warn!("camera preview: {e}");
        }
        Ok(())
    }

    pub fn hide_window(&self, label: &str) {
        if label == "panel"
            && let Err(e) = self.camera_preview(false)
        {
            tracing::warn!("camera preview: {e}");
        }
        if let Some(w) = self.app.get_webview_window(label)
            && let Err(e) = w.hide()
        {
            tracing::warn!(label, "hide failed: {e}");
        }
    }

    pub fn window_rects(&self) -> Value {
        let ids = lock(&self.window_ids).clone();
        let list: Vec<Value> = ids
            .iter()
            .map(|(label, id)| {
                let Some(w) = self.app.get_webview_window(label) else {
                    return json!({"label": label, "id": id.0, "error": "missing"});
                };
                let scale = w.scale_factor().unwrap_or(1.0);
                let pos = w.outer_position().ok().map(|p| p.to_logical::<f64>(scale));
                let size = w.outer_size().ok().map(|s| s.to_logical::<f64>(scale));
                json!({
                    "label": label, "id": id.0,
                    "visible": w.is_visible().unwrap_or(false),
                    "rect_pt": match (pos, size) {
                        (Some(p), Some(s)) => json!({"x": p.x, "y": p.y, "w": s.width, "h": s.height}),
                        _ => Value::Null,
                    },
                })
            })
            .collect();
        Value::Array(list)
    }

    fn move_to_display(&self, label: &str, display_id: &str) -> Result<(), String> {
        let displays = self
            .platform
            .screen()
            .displays()
            .map_err(|e| e.to_string())?;
        let d = displays
            .iter()
            .find(|d| d.id.0 == display_id)
            .ok_or_else(|| format!("no display {display_id:?}"))?;
        let w = self
            .app
            .get_webview_window(label)
            .ok_or_else(|| format!("no window {label:?}"))?;
        let f = d.frame_pt;
        // Size first: AppKit resizes around the bottom-left corner, so a resize after the move
        // would push the window's top above the display.
        w.set_size(LogicalSize::new(f.w, f.h))
            .and_then(|_| w.set_position(LogicalPosition::new(f.x, f.y)))
            .map_err(|e| format!("move {label}: {e}"))
    }

    /// The display under the cursor, or the main display.
    pub fn display_under_cursor(&self) -> Result<String, String> {
        let displays = self
            .platform
            .screen()
            .displays()
            .map_err(|e| e.to_string())?;
        let cursor = self.app.cursor_position().ok();
        if let Some(c) = cursor {
            for d in &displays {
                let (x, y) = (c.x / d.scale.max(1.0), c.y / d.scale.max(1.0));
                let f = d.frame_pt;
                if x >= f.x && x < f.x + f.w && y >= f.y && y < f.y + f.h {
                    return Ok(d.id.0.clone());
                }
            }
        }
        displays
            .iter()
            .find(|d| d.is_main)
            .or(displays.first())
            .map(|d| d.id.0.clone())
            .ok_or_else(|| "no displays".to_string())
    }

    pub fn get_permissions(&self) -> Reply {
        let p = self.platform.permissions();
        Ok(json!({
            "screen": dto::permission_text(p.status(PermissionKind::Screen)),
            "camera": dto::permission_text(p.status(PermissionKind::Camera)),
            "mic": dto::permission_text(p.status(PermissionKind::Microphone)),
        }))
    }

    pub fn request_permission(&self, kind: &str) -> Reply {
        let k = dto::permission_kind(kind)?;
        let (tx, rx) = mpsc::channel();
        self.platform.permissions().request(
            k,
            Box::new(move |s| {
                let _ = tx.send(s);
            }),
        );
        let s = rx
            .recv()
            .map_err(|_| "permission request dropped".to_string())?;
        Ok(json!(dto::permission_text(s)))
    }

    pub fn open_permission_settings(&self, kind: &str) -> Reply {
        let k = dto::permission_kind(kind)?;
        let url = self
            .platform
            .permissions()
            .settings_url(k)
            .ok_or("no settings page for this permission on this OS")?;
        open_url(&url)?;
        Ok(Value::Null)
    }

    pub fn list_displays(&self) -> Reply {
        let d = self
            .platform
            .screen()
            .displays()
            .map_err(|e| e.to_string())?;
        serde_json::to_value(d).map_err(|e| e.to_string())
    }

    pub fn list_sources(&self) -> Reply {
        let st = self.platform.permissions().status(PermissionKind::Screen);
        if st != PermissionStatus::Granted {
            return Err(format!(
                "screen recording permission is {}; sources need it granted",
                dto::permission_text(st)
            ));
        }
        let list = self
            .platform
            .screen()
            .list(&self.window_ids())
            .map_err(|e| e.to_string())?;
        serde_json::to_value(list).map_err(|e| e.to_string())
    }

    pub fn list_devices(&self) -> Reply {
        let cams = self.platform.camera().devices();
        let mics = self.platform.mic().devices();
        if let (Err(c), Err(m)) = (&cams, &mics) {
            return Err(format!("devices: camera: {c}; mic: {m}"));
        }
        Ok(json!({
            "cameras": cams.unwrap_or_default(),
            "mics": mics.unwrap_or_default(),
        }))
    }

    pub fn show_mic_modes(&self) -> Reply {
        if !self.platform.capabilities().mic_modes {
            return Err("mic modes are not available on this system".into());
        }
        self.platform
            .mic()
            .show_mic_modes()
            .map_err(|e| e.to_string())?;
        Ok(Value::Null)
    }

    pub fn show_video_effects(&self) -> Reply {
        if !self.platform.capabilities().mic_modes {
            return Err("video effects are not available on this system".into());
        }
        self.platform
            .camera()
            .show_video_effects()
            .map_err(|e| e.to_string())?;
        Ok(Value::Null)
    }

    pub fn get_settings(&self) -> Reply {
        serde_json::to_value(self.settings.load()).map_err(|e| e.to_string())
    }

    /// The window positions in `s` are ignored: the app keeps those itself. The change is
    /// announced in save order, before the camera catches up.
    pub fn set_settings(&self, mut s: RecorderSettings) -> Reply {
        let (before, v) = {
            let _write = lock(&self.settings_write);
            let current = self.settings.load();
            s.bar_at = current.bar_at;
            s.preview_at = current.preview_at;
            self.settings.save(&s)?;
            let v = serde_json::to_value(&s).map_err(|e| e.to_string())?;
            self.emit("settings-changed", v.clone());
            (current.camera_key, v)
        };
        if s.camera_key != before {
            self.send_camera_key(&s.camera_key, true);
        }
        if let Err(e) = self.refresh_camera() {
            tracing::warn!("camera after settings: {e}");
        }
        Ok(v)
    }

    /// Loads, changes and saves the settings under the write lock.
    fn edit_settings(
        &self,
        f: impl FnOnce(&mut RecorderSettings),
    ) -> Result<RecorderSettings, String> {
        let _write = lock(&self.settings_write);
        let mut s = self.settings.load();
        f(&mut s);
        self.settings.save(&s)?;
        Ok(s)
    }

    pub fn get_capabilities(&self) -> Reply {
        let caps = self.platform.capabilities();
        let displays = self.platform.screen().displays().unwrap_or_default();
        let probe = lock(&self.probe).clone();
        let per: Vec<dto::DisplayProbeDto> = displays
            .iter()
            .map(|d| {
                let allows = |r| {
                    probe
                        .as_ref()
                        .and_then(|p| p.allows_60fps(&d.id, r))
                        .unwrap_or(false)
                };
                dto::DisplayProbeDto {
                    display_id: d.id.0.clone(),
                    can_60fps_native: allows(Resolution::Native),
                    can_60fps_1x: allows(Resolution::OneX),
                }
            })
            .collect();
        Ok(dto::capabilities(&caps, &per))
    }

    /// `camera_key`: the placement to show, when the caller has one newer than the saved settings.
    pub fn preview_still(
        &self,
        source: SourceSpecDto,
        canvas: CanvasDto,
        camera_key: Option<sc_core::project::CameraKey>,
    ) -> Reply {
        let spec = source.to_spec(|id| self.picked(id))?;
        let snapshot = if self.platform.permissions().status(PermissionKind::Screen)
            == PermissionStatus::Granted
        {
            match self
                .platform
                .screen()
                .snapshot(&spec, SizePx { w: 1920, h: 1920 })
            {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!("still snapshot: {e}");
                    None
                }
            }
        } else {
            None
        };
        let size = still::still_canvas(canvas.aspect, snapshot.as_ref().map(|s| s.size));
        let bg = match &canvas.background_image {
            Some(p) => Background::Image {
                asset: p.clone(),
                sha256: String::new(),
                fit: Fit::Cover,
                focus_norm: PointF { x: 0.5, y: 0.5 },
            },
            None => canvas.background.clone(),
        };
        let settings = self.settings.load();
        let key = camera_key.unwrap_or(settings.camera_key);
        let cam_frame = settings
            .camera
            .as_ref()
            .filter(|_| key.visible)
            .and_then(|_| self.camera.latest());
        let placement = sc_core::camera::camera_at(std::slice::from_ref(&key), 0);
        let cam = cam_frame
            .as_ref()
            .zip(placement.as_ref())
            .map(|(f, p)| (p, &f.buffer));
        let png =
            still::compose_png(&bg, size, Path::new("/"), snapshot.as_ref(), cam).or_else(|e| {
                tracing::warn!("still: {e}; retrying with the canvas background");
                still::compose_png(
                    &canvas.background,
                    size,
                    Path::new("/"),
                    snapshot.as_ref(),
                    cam,
                )
            })?;
        let id = sc_core::ids::new_id();
        lock(&self.stills).insert(id.clone(), png);
        Ok(json!(still::still_url(&id)))
    }

    pub fn still_png(&self, id: &str) -> Option<Arc<Vec<u8>>> {
        lock(&self.stills).get(id)
    }

    pub fn choose_background_image(&self) -> Reply {
        let (tx, rx) = mpsc::channel();
        self.platform.desktop().choose_image(Box::new(move |p| {
            let _ = tx.send(p);
        }));
        let p = rx.recv().map_err(|_| "image chooser dropped".to_string())?;
        Ok(p.map(|p| json!(p.to_string_lossy())).unwrap_or(Value::Null))
    }

    pub fn reveal(&self, path: &str) -> Reply {
        self.platform
            .desktop()
            .reveal(Path::new(path))
            .map_err(|e| e.to_string())?;
        Ok(Value::Null)
    }

    fn picked(&self, id: &str) -> Option<PickedSource> {
        lock(&self.picks).get(id).cloned()
    }

    fn set_purpose(&self, purpose: &str) -> Result<(), String> {
        if purpose != "start" && purpose != "switch" {
            return Err(format!("purpose must be start or switch, got {purpose:?}"));
        }
        *lock(&self.overlay_purpose) = purpose.to_string();
        Ok(())
    }

    pub fn purpose(&self) -> String {
        lock(&self.overlay_purpose).clone()
    }

    pub fn begin_region_select(&self, purpose: &str, display_id: Option<String>) -> Reply {
        self.set_purpose(purpose)?;
        let display = match display_id {
            Some(d) => d,
            None => self.display_under_cursor()?,
        };
        self.move_to_display("region", &display)?;
        self.emit(
            "overlay-context",
            json!({"purpose": purpose, "display_id": display}),
        );
        self.show_window("region", true)?;
        Ok(Value::Null)
    }

    pub fn begin_pick_mode(self: &Arc<Self>, purpose: &str) -> Reply {
        self.set_purpose(purpose)?;
        let display = self.display_under_cursor()?;
        self.move_to_display("pick", &display)?;
        self.emit(
            "overlay-context",
            json!({"purpose": purpose, "display_id": display}),
        );
        self.show_window("pick", true)?;
        let core = Arc::downgrade(self);
        std::thread::Builder::new()
            .name("pick-follow".into())
            .spawn(move || follow_cursor(core, display))
            .map_err(|e| format!("pick follow thread: {e}"))?;
        Ok(Value::Null)
    }

    pub fn open_system_picker(self: &Arc<Self>, purpose: &str) -> Reply {
        self.set_purpose(purpose)?;
        let purpose = purpose.to_string();
        let core = Arc::downgrade(self);
        self.platform.picker().present(
            PickerRequest {
                exclude: self.window_ids(),
            },
            Box::new(move |r| {
                if let Some(core) = core.upgrade() {
                    core.on_system_pick(&purpose, r);
                }
            }),
        );
        Ok(Value::Null)
    }

    fn on_system_pick(&self, purpose: &str, r: sc_platform::Result<Option<PickedSource>>) {
        match r {
            Ok(Some(p)) => {
                let id = self.store_pick(p);
                let source = SourceSpecDto::Picked {
                    pick_id: id,
                    region_pt: None,
                };
                self.emit(
                    "source-picked",
                    json!({"purpose": purpose, "source": source, "via": "system_picker"}),
                );
                if let Err(e) = self.switch_if_switching(purpose, source) {
                    tracing::warn!("system picker switch: {e}");
                }
            }
            Ok(None) => self.emit("pick-cancelled", json!({"purpose": purpose})),
            Err(e) => {
                tracing::warn!("system picker: {e}");
                self.emit("pick-cancelled", json!({"purpose": purpose}));
            }
        }
    }

    fn store_pick(&self, p: PickedSource) -> String {
        let id = sc_core::ids::new_id();
        lock(&self.picks).insert(id.clone(), p);
        id
    }

    /// A re-pick from the menu-bar picker: switches while recording.
    pub fn on_repick(&self, p: PickedSource) {
        let id = self.store_pick(p);
        let source = SourceSpecDto::Picked {
            pick_id: id,
            region_pt: None,
        };
        if self.phase().is_recording() {
            if let Err(e) = self.switch_source(source) {
                tracing::warn!("re-pick switch: {e}");
            }
        } else {
            self.emit(
                "source-picked",
                json!({"purpose": "start", "source": source, "via": "system_picker"}),
            );
        }
    }

    /// A pick made for `purpose = "switch"` switches the running recording (the overlays do the
    /// same from the UI through `switch_source`).
    fn switch_if_switching(&self, purpose: &str, source: SourceSpecDto) -> Reply {
        if purpose != "switch" {
            return Ok(Value::Null);
        }
        if !self.phase().is_recording() {
            return Err("switch pick while not recording".into());
        }
        self.switch_source(source)
    }

    /// What the region overlay does when the user confirms a region.
    pub fn region_selected(&self, display_id: &str, region: RectPt) -> Reply {
        let purpose = self.purpose();
        self.hide_window("region");
        self.emit(
            "region-selected",
            json!({"purpose": purpose, "display_id": display_id, "region_pt": region}),
        );
        self.switch_if_switching(
            &purpose,
            SourceSpecDto::Region {
                display_id: display_id.to_string(),
                region_pt: region,
            },
        )
    }

    /// What the pick overlay does when the user clicks a window or display.
    pub fn source_picked(&self, source: SourceSpecDto) -> Reply {
        let purpose = self.purpose();
        self.hide_window("pick");
        self.emit(
            "source-picked",
            json!({"purpose": purpose, "source": source, "via": "own_chooser"}),
        );
        self.switch_if_switching(&purpose, source)
    }

    pub fn hotkey_switch(self: &Arc<Self>) -> Reply {
        let purpose = if self.phase().is_recording() {
            "switch"
        } else {
            "start"
        };
        self.begin_pick_mode(purpose)
    }

    pub fn start_recording(self: &Arc<Self>, req: RecordRequestDto) -> Reply {
        if self.phase().is_recording() {
            return Err("already recording".into());
        }
        let n = if self.no_key_windows {
            0
        } else {
            req.settings.countdown_s
        };
        if n > 0 {
            self.countdown_cancel
                .store(false, std::sync::atomic::Ordering::SeqCst);
            if self.platform.capabilities().exclusion != sc_platform::ExclusionKind::AutoHide {
                self.hide_window("panel");
            }
            self.place_bar();
            if let Err(e) = self.show_window("bar", false) {
                tracing::warn!("{e}");
            }
            for left in (1..=n).rev() {
                self.emit("countdown", json!({ "left": left }));
                std::thread::sleep(std::time::Duration::from_secs(1));
                if self
                    .countdown_cancel
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    self.emit("countdown", json!({ "left": 0, "cancelled": true }));
                    self.hide_window("bar");
                    return Ok(json!({ "cancelled": true }));
                }
            }
            self.emit("countdown", json!({ "left": 0 }));
        }
        self.start_recording_now(req)
    }

    fn start_recording_now(self: &Arc<Self>, req: RecordRequestDto) -> Reply {
        *lock(&self.last_request) = Some(req.clone());
        *lock(&self.paused_total) = std::time::Duration::ZERO;
        *lock(&self.paused_since) = None;
        let spec = req.source.to_spec(|id| self.picked(id))?;
        std::fs::create_dir_all(&self.projects_dir)
            .map_err(|e| format!("create {}: {e}", self.projects_dir.display()))?;
        let default_canvas = ProjectV1::new_recording(
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            self.platform.clock_kind(),
        )
        .output
        .canvas;
        let request = dto::record_request(
            &req,
            spec,
            RequestEnv {
                projects_dir: self.projects_dir.clone(),
                exclude: self.window_ids(),
                default_canvas,
            },
        )?;
        self.advance(Input::Start)?;
        self.emit(
            "recording-state",
            dto::recording_state(&SessionState::Preparing, 0),
        );
        if self.platform.capabilities().exclusion == sc_platform::ExclusionKind::AutoHide {
            for (label, _) in WINDOWS {
                self.hide_window(label);
            }
        } else {
            self.place_bar();
            if let Err(e) = self.show_window("bar", false) {
                tracing::warn!("{e}");
            }
        }
        let obs = Arc::new(Observer {
            core: Arc::downgrade(self),
        });
        match Session::start(self.platform.clone(), request, obs) {
            Ok(s) => {
                *lock(&self.rec_started) = Some(Instant::now());
                tracing::info!(project = %s.project_dir().display(), "recording started");
                *lock(&self.session) = Some(s);
                self.refresh_tray();
                Ok(Value::Null)
            }
            Err(e) => {
                let error = format!("start recording: {e}");
                self.on_failed(&error);
                Err(error)
            }
        }
    }

    pub fn stop_recording(&self) -> Reply {
        self.stop_with(StopReason::User)
    }

    fn stop_with(&self, reason: StopReason) -> Reply {
        let Some(session) = lock(&self.session).take() else {
            return Err("not recording".into());
        };
        let _ = self.advance(Input::StopBegin);
        self.emit(
            "recording-state",
            dto::recording_state(&SessionState::Stopping, 0),
        );
        match session.stop(reason) {
            Ok(summary) => {
                tracing::info!(
                    project = %summary.project_dir.display(),
                    duration_ns = summary.duration_ns,
                    segments = summary.segments,
                    ?reason,
                    "recording stopped"
                );
                self.on_finalized(summary.project_dir);
                Ok(Value::Null)
            }
            Err(e) => {
                let error = format!("stop recording: {e}");
                self.on_failed(&error);
                Err(error)
            }
        }
    }

    pub fn discard_recording(&self) -> Reply {
        let Some(session) = lock(&self.session).take() else {
            return Err("not recording".into());
        };
        let r = session.discard().map_err(|e| format!("discard: {e}"));
        let _ = self.advance(Input::Failed);
        *lock(&self.rec_started) = None;
        self.hide_window("bar");
        self.emit("recording-state", dto::idle_state());
        self.refresh_tray();
        r.map(|_| Value::Null)
    }

    pub fn switch_source(&self, source: SourceSpecDto) -> Reply {
        let via = if matches!(source, SourceSpecDto::Picked { .. }) {
            Via::SystemPicker
        } else {
            Via::OwnChooser
        };
        let spec = source.to_spec(|id| self.picked(id))?;
        let guard = lock(&self.session);
        let s = guard.as_ref().ok_or("not recording")?;
        s.switch_source(spec, via).map_err(|e| e.to_string())?;
        Ok(Value::Null)
    }

    fn on_session_state(&self, s: &SessionState) {
        match s {
            SessionState::Switching => {
                let _ = self.advance(Input::SwitchBegin);
            }
            SessionState::Recording => {
                let _ = self.advance(Input::SwitchEnd);
            }
            SessionState::Stopping => {
                let _ = self.advance(Input::StopBegin);
            }
            SessionState::Finalized { project_dir } => {
                if self.phase().is_recording() {
                    self.on_finalized(project_dir.clone());
                }
                return;
            }
            SessionState::Failed { error } => {
                if let Ok(mut s) = self.session.try_lock() {
                    s.take();
                }
                self.on_failed(error);
                return;
            }
            SessionState::Preparing => {}
        }
        self.emit(
            "recording-state",
            dto::recording_state(s, self.elapsed_ns()),
        );
        self.refresh_tray();
    }

    fn on_finalized(&self, project_dir: PathBuf) {
        if self
            .advance(Input::Finalized {
                project_dir: project_dir.clone(),
            })
            .is_err()
        {
            return;
        }
        *lock(&self.rec_started) = None;
        *lock(&self.last_health) = None;
        *lock(&self.last_project) = Some(project_dir.clone());
        let project_dir_for_editor = project_dir.clone();
        let project = sc_core::project::ProjectV1::load(&project_dir).ok();
        let duration = project.as_ref().and_then(|p| p.duration_ns());
        let mut kinds: Vec<&str> = Vec::new();
        for t in project.iter().flat_map(|p| &p.recording.tracks) {
            let k = match t.kind {
                sc_core::project::TrackKind::Screen => "screen",
                sc_core::project::TrackKind::Camera => "camera",
                sc_core::project::TrackKind::Mic => "mic",
                sc_core::project::TrackKind::SystemAudio => "system_audio",
            };
            if !kinds.contains(&k) {
                kinds.push(k);
            }
        }
        let mut state = dto::recording_state(&SessionState::Finalized { project_dir }, 0);
        state["duration_ns"] = json!(duration);
        state["tracks"] = json!(kinds);
        self.emit("recording-state", state);
        self.hide_window("bar");
        self.after_recording(project_dir_for_editor);
        self.refresh_tray();
    }

    /// What happens when a recording is saved: the editor opens with it, unless the
    /// settings ask for the Saved window.
    fn after_recording(&self, project_dir: PathBuf) {
        let Some(core) = self.app.try_state::<Arc<Core>>().map(|c| c.inner().clone()) else {
            return;
        };
        match self.settings.load().after_recording {
            crate::settings::AfterRecording::Saved => {
                if let Err(e) = self.show_window("saved", false) {
                    tracing::warn!("{e}");
                }
                return;
            }
            crate::settings::AfterRecording::QuickExport => {
                if let Err(e) = self.show_window("saved", false) {
                    tracing::warn!("{e}");
                }
                std::thread::spawn(move || {
                    if let Err(e) = core.export_with_last_settings(&project_dir) {
                        tracing::warn!("export after recording: {e}");
                        core.emit("notice", json!({ "kind": "export_failed", "error": e }));
                    }
                });
                return;
            }
            crate::settings::AfterRecording::Editor => {}
        }
        std::thread::spawn(move || {
            if let Err(e) = core.editor_open(&project_dir) {
                tracing::warn!("editor after recording: {e}");
                if let Err(e) = core.show_window("saved", false) {
                    tracing::warn!("{e}");
                }
            }
        });
    }

    fn on_failed(&self, error: &str) {
        tracing::error!("recording failed: {error}");
        let _ = self.advance(Input::Failed);
        *lock(&self.rec_started) = None;
        self.emit(
            "recording-state",
            dto::recording_state(
                &SessionState::Failed {
                    error: error.to_string(),
                },
                0,
            ),
        );
        self.hide_window("bar");
        self.refresh_tray();
    }

    pub fn refresh_tray(&self) {
        if let Some(t) = self.tray.get() {
            t.refresh(&self.phase(), self.elapsed_ns(), self.is_paused());
        }
    }

    /// Tray "Start Recording": the saved settings' source, or the panel when there is none.
    pub fn tray_start(self: &Arc<Self>) -> Reply {
        let settings = self.settings.load();
        match settings.source.clone() {
            Some(source) if !matches!(source, SourceSpecDto::Picked { .. }) => self
                .start_recording(RecordRequestDto {
                    source,
                    via: Via::OwnChooser,
                    title: None,
                    settings,
                }),
            _ => {
                self.show_window("panel", true)?;
                Ok(Value::Null)
            }
        }
    }

    pub fn probe_snapshot(&self) -> Option<ProbeV1> {
        lock(&self.probe).clone()
    }

    /// Starts an export of `project_dir` through `start`; returns the job id.
    pub fn run_export(
        self: &Arc<Self>,
        project_dir: &str,
        start: impl FnOnce(Arc<dyn ExportObserver>) -> sc_export::Result<ExportJob>,
    ) -> Reply {
        let job_id = sc_core::ids::new_id();
        let before = self.phase();
        self.advance(Input::ExportStart {
            project_dir: PathBuf::from(project_dir),
            job: job_id.clone(),
        })?;
        let obs: Arc<dyn ExportObserver> = Arc::new(ExportObs {
            core: Arc::downgrade(self),
            id: job_id.clone(),
        });
        match start(obs) {
            Ok(job) => {
                lock(&self.exports).insert(job_id.clone(), job);
                *lock(&self.last_export) = Some(job_id.clone());
                Ok(json!(job_id))
            }
            Err(e) => {
                *lock(&self.phase) = before;
                Err(format!("export: {e}"))
            }
        }
    }

    /// The export sheet's last format and its remembered settings, without the sheet.
    pub fn export_with_last_settings(self: &Arc<Self>, project_dir: &Path) -> Reply {
        let store = crate::exports::load(&self.data_dir);
        let format = store
            .last_format
            .unwrap_or(sc_export::settings::Format::Mp4);
        let key = serde_json::to_value(format)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let settings = store
            .per_format
            .get(&key)
            .cloned()
            .unwrap_or_else(|| sc_export::settings::ExportSettings::for_format(format))
            .normalized();
        let platform = self.platform.clone();
        let d = project_dir.to_path_buf();
        let r = self.run_export(&project_dir.to_string_lossy(), move |obs| {
            ExportJob::start_with(
                platform,
                sc_export::plan::ExportRequest {
                    project_dir: d,
                    settings,
                    out_path: None,
                    canvas: None,
                },
                obs,
            )
        })?;
        if let Some(id) = r.as_str() {
            lock(&self.raw_after_export).insert(id.to_string(), project_dir.to_path_buf());
        }
        Ok(r)
    }

    pub fn quick_export(self: &Arc<Self>, project_dir: &str, size: &str) -> Reply {
        let size = dto::export_size(size)?;
        let job_id = sc_core::ids::new_id();
        let before = self.phase();
        self.advance(Input::ExportStart {
            project_dir: PathBuf::from(project_dir),
            job: job_id.clone(),
        })?;
        let obs = Arc::new(ExportObs {
            core: Arc::downgrade(self),
            id: job_id.clone(),
        });
        let req = QuickExportRequest {
            project_dir: PathBuf::from(project_dir),
            out_path: None,
            size,
            fps: None,
        };
        match ExportJob::start(self.platform.clone(), req, obs) {
            Ok(job) => {
                lock(&self.exports).insert(job_id.clone(), job);
                *lock(&self.last_export) = Some(job_id.clone());
                lock(&self.raw_after_export).insert(job_id.clone(), PathBuf::from(project_dir));
                Ok(json!(job_id))
            }
            Err(e) => {
                *lock(&self.phase) = before;
                Err(format!("export: {e}"))
            }
        }
    }

    pub fn cancel_export(&self, id: &str) -> Reply {
        let jobs = lock(&self.exports);
        let job = jobs.get(id).ok_or_else(|| format!("no export {id:?}"))?;
        job.cancel();
        Ok(Value::Null)
    }

    fn on_export_state(&self, id: &str, s: &ExportState) {
        self.emit("export-state", dto::export_state(id, s));
        if matches!(
            s,
            ExportState::Done { .. } | ExportState::Canceled | ExportState::Failed { .. }
        ) {
            if let Some(job) = lock(&self.exports).remove(id) {
                std::thread::spawn(move || {
                    if let Err(e) = job.wait() {
                        tracing::warn!("export ended: {e}");
                    }
                });
            }
            let _ = self.advance(Input::ExportEnd);
            let dir = lock(&self.raw_after_export).remove(id);
            if let (ExportState::Done { .. }, Some(dir)) = (s, dir)
                && self.settings.load().delete_raw_after_export
                && self.editor_dir().as_deref() != Some(dir.as_path())
            {
                match self.platform.desktop().trash(&dir) {
                    Ok(()) => {
                        tracing::info!(dir = %dir.display(), "raw files moved to the Trash after export");
                        self.emit("library-changed", json!({}));
                    }
                    Err(e) => tracing::warn!("delete raw files after export: {e}"),
                }
            }
        }
    }

    pub fn recovery_report(&self) -> Reply {
        Ok(Value::Array(
            lock(&self.recovery).iter().map(dto::recovery).collect(),
        ))
    }

    /// Background thread at launch: recovery scan, stale `.part` sweep, probe, system watcher.
    pub fn launch_tasks(self: &Arc<Self>) {
        let t0 = Instant::now();
        if self.projects_dir.is_dir() {
            match sc_record::recovery::scan(&self.projects_dir, &*self.platform.decoder()) {
                Ok(r) => {
                    for rep in &r {
                        tracing::info!(dir = %rep.dir.display(), outcome = ?rep.outcome, "recovery");
                    }
                    *lock(&self.recovery) = r;
                }
                Err(e) => tracing::warn!("recovery scan: {e}"),
            }
            let mut swept = 0;
            let mut dirs = vec![self.projects_dir.clone()];
            if let Ok(rd) = std::fs::read_dir(&self.projects_dir) {
                dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
            }
            for d in dirs {
                match sc_helpers::part::sweep_stale(&d) {
                    Ok(n) => swept += n,
                    Err(e) => tracing::warn!(dir = %d.display(), "part sweep: {e}"),
                }
            }
            tracing::info!(swept, "stale .part sweep");
        }
        match self.platform.screen().displays() {
            Ok(displays) => {
                match sc_export::probe::load_or_run(&*self.platform, &self.data_dir, &displays) {
                    Ok(p) => *lock(&self.probe) = Some(p),
                    Err(e) => tracing::warn!("probe: {e}"),
                }
            }
            Err(e) => tracing::warn!("probe: displays: {e}"),
        }
        let sink = Arc::new(SystemSink {
            core: Arc::downgrade(self),
        });
        match self.platform.watch_system(sink) {
            Ok(w) => *lock(&self.watcher) = Some(w),
            Err(e) => tracing::warn!("watch_system: {e}"),
        }
        self.launch_done.store(true, Ordering::SeqCst);
        self.emit(
            "launch-tasks-done",
            json!({"recovery": self.recovery_report().unwrap_or(Value::Null),
                   "probe": lock(&self.probe).is_some()}),
        );
        let core = Arc::downgrade(self);
        self.platform.picker().set_observer(Some(Box::new(move |p| {
            if let Some(c) = core.upgrade() {
                c.on_repick(p);
            }
        })));
        tracing::info!(ms = t0.elapsed().as_millis() as u64, "launch tasks done");
    }

    /// Off the main thread, since opening a camera blocks: closes `lost` if it is the open camera,
    /// then opens the preview's camera again if it is missing (unplugged, then plugged back in).
    fn camera_in_background(self: &Arc<Self>, lost: Option<String>) {
        let core = Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("camera-devices".into())
            .spawn(move || {
                let Some(core) = core.upgrade() else { return };
                if let Some(id) = lost {
                    tracing::warn!(device = id, "camera unplugged");
                    core.camera.device_lost(&id);
                }
                if let Err(e) = core.refresh_camera() {
                    tracing::warn!("camera after a device change: {e}");
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("camera device thread: {e}");
        }
    }

    pub fn on_system_event(self: &Arc<Self>, e: SourceEvent) {
        match e {
            SourceEvent::DevicesChanged { kind } => {
                self.emit(
                    "devices-changed",
                    json!({"kind": format!("{kind:?}").to_lowercase()}),
                );
                if kind == sc_platform::DeviceKind::Camera {
                    self.camera_in_background(None);
                }
            }
            SourceEvent::DeviceLost {
                kind: sc_platform::DeviceKind::Camera,
                device_id,
            } => self.camera_in_background(Some(device_id)),
            SourceEvent::OwnWindowsChanged => {
                if lock(&self.session).is_none() {
                    return;
                }
                let core = Arc::downgrade(self);
                let spawned = std::thread::Builder::new()
                    .name("own-windows".into())
                    .spawn(move || refresh_exclusions(core));
                if let Err(e) = spawned {
                    tracing::warn!("refresh exclusions thread: {e}");
                }
            }
            SourceEvent::SystemWillSleep => {
                if self.phase().is_recording() {
                    tracing::info!("system will sleep: stopping the recording");
                    let core = self.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = core.stop_with(StopReason::Sleep) {
                            tracing::warn!("stop for sleep: {e}");
                        }
                    });
                }
            }
            SourceEvent::SystemDidWake => tracing::info!("system did wake"),
            other => tracing::debug!(?other, "system event"),
        }
    }

    /// The one exit path (tray Quit, Cmd-Q/logout, SIGTERM/SIGINT, driver `quit`).
    pub fn shutdown(&self, reason: StopReason, code: i32) {
        if self.exiting.swap(true, Ordering::SeqCst) {
            return;
        }
        tracing::info!(?reason, code, "exiting");
        if lock(&self.session).is_some()
            && let Err(e) = self.stop_with(reason)
        {
            tracing::error!("stop on exit: {e}");
        }
        let jobs: Vec<(String, ExportJob)> = lock(&self.exports).drain().collect();
        for (id, job) in jobs {
            job.cancel();
            if let Err(e) = job.wait() {
                tracing::info!(id, "export cancelled on exit: {e}");
            }
        }
        if let Some(w) = lock(&self.watcher).take()
            && let Err(e) = w.stop()
        {
            tracing::warn!("stop system watcher: {e}");
        }
        self.exit_done.store(true, Ordering::SeqCst);
        self.app.exit(code);
    }
}

#[cfg(target_os = "macos")]
fn apply_material(
    w: &tauri::WebviewWindow,
    material: sc_capture_macos::material::Material,
    radius: f64,
) {
    let label = w.label().to_string();
    let applied = w.with_webview(move |pw| {
        let Some(wk) = std::ptr::NonNull::new(pw.inner()) else {
            return;
        };
        // SAFETY: Tauri runs this on the main thread with the live WKWebView.
        if let Err(e) = unsafe { sc_capture_macos::material::material_under(wk, material, radius) }
        {
            tracing::warn!(label, "window material: {e}");
        }
    });
    if let Err(e) = applied {
        tracing::warn!("window material: {e}");
    }
}

#[cfg(target_os = "macos")]
fn configure_one(
    platform: &dyn Platform,
    w: &tauri::WebviewWindow,
    role: OverlayRole,
) -> Result<WindowId, String> {
    let raw = w.ns_window().map_err(|e| e.to_string())?;
    unsafe { platform.overlays().configure(raw, role) }.map_err(|e| e.to_string())
}

#[cfg(windows)]
fn configure_one(
    platform: &dyn Platform,
    w: &tauri::WebviewWindow,
    role: OverlayRole,
) -> Result<WindowId, String> {
    let raw = w.hwnd().map_err(|e| e.to_string())?.0;
    unsafe { platform.overlays().configure(raw, role) }.map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
fn configure_one(
    platform: &dyn Platform,
    _w: &tauri::WebviewWindow,
    role: OverlayRole,
) -> Result<WindowId, String> {
    unsafe { platform.overlays().configure(std::ptr::null_mut(), role) }.map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn show_without_key(w: &tauri::WebviewWindow) -> Result<(), String> {
    let raw = w.ns_window().map_err(|e| e.to_string())? as usize;
    w.run_on_main_thread(move || unsafe {
        crate::objc::order_front_regardless(raw as *mut std::ffi::c_void)
    })
    .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
fn show_without_key(w: &tauri::WebviewWindow) -> Result<(), String> {
    w.show().map_err(|e| e.to_string())
}

pub fn open_path(path: &str) -> Result<(), String> {
    open_url(path)
}

fn open_url(url: &str) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{program} {url}: {e}"))
}

/// A menu of ours began tracking: its window may appear a moment after the notification, so the
/// exclusions are refreshed now and twice more while it opens.
fn refresh_exclusions(core: Weak<Core>) {
    for delay_ms in [0, 120, 400] {
        std::thread::sleep(Duration::from_millis(delay_ms));
        let Some(c) = core.upgrade() else { return };
        let t0 = Instant::now();
        let r = match lock(&c.session).as_ref() {
            Some(s) => s.set_exclusions(c.window_ids()),
            None => return,
        };
        let ms = t0.elapsed().as_millis() as u64;
        match r {
            Ok(()) => tracing::info!(delay_ms, ms, "own windows changed: exclusions refreshed"),
            Err(e) => tracing::warn!(delay_ms, ms, "refresh exclusions: {e}"),
        }
    }
}

fn follow_cursor(core: Weak<Core>, mut display: String) {
    let until = Instant::now() + Duration::from_secs(600);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(150));
        let Some(c) = core.upgrade() else { return };
        let visible = c
            .app
            .get_webview_window("pick")
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        if !visible {
            return;
        }
        if let Ok(d) = c.display_under_cursor()
            && d != display
        {
            if let Err(e) = c.move_to_display("pick", &d) {
                tracing::warn!("pick follow: {e}");
            }
            c.emit(
                "overlay-context",
                json!({"purpose": c.purpose(), "display_id": d}),
            );
            display = d;
        }
    }
}

struct Observer {
    core: Weak<Core>,
}

impl SessionObserver for Observer {
    fn on_state(&self, s: &SessionState) {
        if let Some(c) = self.core.upgrade() {
            c.on_session_state(s);
        }
    }
    fn on_health(&self, h: &HealthSnapshot) {
        if let Some(c) = self.core.upgrade() {
            *lock(&c.last_health) = Some((Instant::now(), h.rec_ns));
            let mut v = dto::health(h);
            if let Some(o) = v.as_object_mut() {
                o.insert("elapsed_ns".into(), json!(c.elapsed_ns()));
                o.insert("paused".into(), json!(c.is_paused()));
            }
            c.emit("health", v);
            c.refresh_tray();
        }
    }
    fn on_level(&self, track: sc_platform::AudioTrackKind, peak: f32) {
        if track != sc_platform::AudioTrackKind::Mic {
            return;
        }
        if let Some(c) = self.core.upgrade() {
            let db = if peak <= 1e-6 {
                -120.0
            } else {
                20.0 * peak.log10()
            };
            c.emit("mic-level", json!({ "peak_db": db }));
        }
    }
    fn on_notice(&self, n: &Notice) {
        if let Some(c) = self.core.upgrade() {
            c.emit("notice", dto::notice(n));
        }
    }
}

struct ExportObs {
    core: Weak<Core>,
    id: String,
}

impl ExportObserver for ExportObs {
    fn on_state(&self, s: &ExportState) {
        if let Some(c) = self.core.upgrade() {
            c.on_export_state(&self.id, s);
        }
    }
    fn on_progress(&self, p: &ExportProgress) {
        if let Some(c) = self.core.upgrade() {
            c.emit("export-progress", dto::export_progress(&self.id, p));
        }
    }
}

struct SystemSink {
    core: Weak<Core>,
}

impl FrameSink for SystemSink {
    fn on_screen(&self, _f: VideoFrame) {}
    fn on_camera(&self, _f: VideoFrame) {}
    fn on_audio(&self, _t: sc_platform::AudioTrackKind, _a: AudioBuffer) {}
    fn on_event(&self, e: SourceEvent) {
        if let Some(c) = self.core.upgrade() {
            c.on_system_event(e);
        }
    }
}

fn is_recording(p: &Phase) -> bool {
    matches!(p, Phase::Recording | Phase::Switching | Phase::Stopping)
}

impl Core {
    pub fn exiting(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }

    /// The recorder panel opened (`true`) or closed. The camera and the preview window run while
    /// the panel is open or a recording is on (ADR-041, ADR-043).
    pub fn camera_preview(&self, active: bool) -> Reply {
        lock(&self.preview).panel_open = active;
        self.refresh_camera()
    }

    /// Runs the panel's microphone level while the panel is open, a mic is chosen and allowed,
    /// and nothing records (the recorder then owns the mic).
    fn refresh_mic_meter(&self, s: &crate::settings::RecorderSettings, recording: bool) {
        let want = s.mic.clone().filter(|_| {
            !recording
                && !self.no_key_windows
                && lock(&self.preview).panel_open
                && self
                    .platform
                    .permissions()
                    .status(sc_platform::PermissionKind::Microphone)
                    == sc_platform::PermissionStatus::Granted
        });
        let mut cur = lock(&self.mic_meter);
        if cur.as_ref().map(|(d, _)| d.as_str()) == want.as_deref() {
            return;
        }
        if let Some((_, stream)) = cur.take() {
            let _ = stream.stop();
        }
        if let Some(device) = want {
            let sink = Arc::new(crate::meter::MeterSink {
                app: self.app.clone(),
                last: Mutex::new((Instant::now(), 0.0)),
            });
            match self.platform.mic().start(&device, sink) {
                Ok(stream) => *cur = Some((device, stream)),
                Err(e) => tracing::debug!("mic meter: {e}"),
            }
        }
    }

    /// Opens or releases the camera and shows or hides the preview window to match the settings.
    pub fn refresh_camera(&self) -> Reply {
        let s = self.settings.load();
        let recording = is_recording(&self.phase());
        self.refresh_mic_meter(&s, recording);
        let active = lock(&self.preview).panel_open || recording;
        let device = if active { s.camera.clone() } else { None };
        let camera_error = self.camera.set_preview(device.clone()).err();
        if let Some(e) = &camera_error {
            tracing::warn!("camera preview: {e}");
        }
        let show = active && s.preview_shown && s.source.is_some();
        let place = {
            let mut p = lock(&self.preview);
            p.key = Some(s.camera_key.clone());
            p.stage = s
                .source
                .clone()
                .filter(|_| show)
                .map(|src| (src, s.canvas.clone()));
            show && !std::mem::replace(&mut p.placed, true)
        };
        if show {
            if place {
                self.place_preview(&s);
            }
            self.show_window("stage", false)?;
        } else {
            self.hide_window("stage");
            self.live.clear("stage");
        }
        match camera_error {
            Some(e) if !recording => Err(format!("camera: {e}")),
            _ => Ok(json!({ "shown": show })),
        }
    }

    /// First showing: where it was last moved to, if still on a display; otherwise the bottom-right
    /// corner of the recorded display.
    fn place_preview(&self, s: &RecorderSettings) {
        let Some(w) = self.app.get_webview_window("stage") else {
            return;
        };
        let displays = self.platform.screen().displays().unwrap_or_default();
        let size = w
            .outer_size()
            .ok()
            .map(|p| p.to_logical::<f64>(w.scale_factor().unwrap_or(1.0)))
            .map_or((560.0, 360.0), |l| (l.width, l.height));
        let fits = |p: &sc_geometry::PointF| {
            displays.iter().any(|d| {
                let f = d.frame_pt;
                p.x >= f.x && p.y >= f.y && p.x + size.0 <= f.x + f.w && p.y + size.1 <= f.y + f.h
            })
        };
        let at = s.preview_at.filter(fits).or_else(|| {
            crate::preview::display_for(s, &displays).map(|d| {
                let f = d.frame_pt;
                sc_geometry::PointF {
                    x: f.x + f.w - size.0 - 24.0,
                    y: f.y + f.h - size.1 - 96.0,
                }
            })
        });
        if let Some(at) = at {
            lock(&self.preview).placed_at = Some(at);
            if let Err(e) = w.set_position(LogicalPosition::new(at.x, at.y)) {
                tracing::warn!("place preview: {e}");
            }
        }
    }

    /// The preview window's close button turns the preview off (the panel's "Show preview").
    pub fn preview_closed(&self) {
        if self.settings.load().preview_shown {
            let _write = lock(&self.settings_write);
            let mut s = self.settings.load();
            s.preview_shown = false;
            match self.settings.save(&s) {
                Ok(()) => self.emit("settings-changed", json!(s)),
                Err(e) => tracing::warn!("preview off: {e}"),
            }
        }
        if let Err(e) = self.refresh_camera() {
            tracing::warn!("preview off: {e}");
        }
    }

    /// The preview window was moved: remembered for next time.
    pub fn preview_moved(self: &Arc<Self>, x: f64, y: f64) {
        let at = sc_geometry::PointF { x, y };
        {
            let mut p = lock(&self.preview);
            if p.placed_at
                .is_some_and(|q| (q.x - x).abs() < 1.0 && (q.y - y).abs() < 1.0)
            {
                return;
            }
            if p.moved_to.replace(at).is_some() {
                return;
            }
        }
        let core = Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("preview-moved".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(400));
                let Some(core) = core.upgrade() else { return };
                let Some(at) = lock(&core.preview).moved_to.take() else {
                    return;
                };
                if let Err(e) = core.edit_settings(|s| s.preview_at = Some(at)) {
                    tracing::warn!("remember preview position: {e}");
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("preview position thread: {e}");
        }
    }

    /// What the stage renderer draws, or `None` while the stage is hidden.
    pub fn stage_inputs(&self) -> Option<crate::stage::StageInputs> {
        let (src, canvas, key) = {
            let b = lock(&self.preview);
            let (src, canvas) = b.stage.clone()?;
            (src, canvas, b.key.clone())
        };
        let background = match &canvas.background_image {
            Some(p) => sc_core::project::Background::Image {
                asset: p.clone(),
                sha256: String::new(),
                fit: sc_core::project::Fit::Cover,
                focus_norm: sc_geometry::PointF { x: 0.5, y: 0.5 },
            },
            None => canvas.background.clone(),
        };
        Some(crate::stage::StageInputs {
            source: src.to_spec(|id| self.picked(id)).ok(),
            aspect: canvas.aspect,
            background,
            key: key.filter(|k| k.visible),
        })
    }

    /// Kept current by [`Core::stage_window_changed`]; measured once when unknown.
    pub fn stage_page(&self) -> Option<StagePage> {
        if let Some(page) = lock(&self.preview).page {
            return Some(page);
        }
        let page = self.measure_stage_page()?;
        lock(&self.preview).page = Some(page);
        Some(page)
    }

    fn measure_stage_page(&self) -> Option<StagePage> {
        let w = self.app.get_webview_window("stage")?;
        let scale = w.scale_factor().unwrap_or(1.0);
        let l = w.inner_size().ok()?.to_logical::<f64>(scale);
        Some(StagePage {
            w: l.width,
            h: l.height,
            scale,
        })
    }

    /// Main thread: the preview window was resized or moved to a display with another scale.
    pub fn stage_window_changed(&self) {
        let page = self.measure_stage_page();
        #[cfg(target_os = "macos")]
        let page = self.fit_stage_view().or(page);
        lock(&self.preview).page = page;
    }

    /// Main thread: refits the native preview view, if there is one, and returns the page.
    #[cfg(target_os = "macos")]
    fn fit_stage_view(&self) -> Option<StagePage> {
        crate::sink::refit(lock(&self.preview).view?)
    }

    pub fn stage_presented(&self, frame: crate::preview::StageFrame) {
        let mut p = lock(&self.preview);
        p.frames += 1;
        p.last = Some(frame);
    }

    /// A `CAMetalLayer` under the preview window's page (ADR-007), valid while that window lives.
    #[cfg(target_os = "macos")]
    pub fn stage_layer(&self) -> Result<std::ptr::NonNull<std::ffi::c_void>, String> {
        let (layer, view, page) = crate::sink::attach_layer(&self.app, "stage")?;
        {
            let mut p = lock(&self.preview);
            p.view = Some(view);
            p.page = Some(page);
        }
        std::ptr::NonNull::new(layer as *mut std::ffi::c_void).ok_or_else(|| "null layer".into())
    }

    /// A camera placement while it is being dragged in the preview: shown at once (and a keyframe
    /// while recording) but not saved; the preview saves the settings when the drag ends.
    /// `last`: the end of a drag, always sent.
    pub fn camera_key_live(&self, key: sc_core::project::CameraKey, last: bool) -> Reply {
        lock(&self.preview).key = Some(key.clone());
        self.send_camera_key(&key, last);
        Ok(Value::Null)
    }

    /// During a recording, `key` becomes a camera keyframe now. Drags send at most 30 a second
    /// (`force` sends regardless, for the last position and discrete edits).
    fn send_camera_key(&self, key: &sc_core::project::CameraKey, force: bool) {
        if !is_recording(&self.phase()) {
            return;
        }
        {
            let mut b = lock(&self.preview);
            let now = Instant::now();
            if !force
                && b.last_key_sent
                    .is_some_and(|t| now.duration_since(t) < Duration::from_millis(33))
            {
                return;
            }
            b.last_key_sent = Some(now);
        }
        if let Some(s) = lock(&self.session).as_ref()
            && let Err(e) = s.camera_key(key.clone())
        {
            tracing::warn!("camera key: {e}");
        }
    }

    /// The control bar where it was last moved to, if that is still on a display; otherwise at
    /// the bottom centre of the recorded display, clear of the Dock.
    fn place_bar(&self) {
        let Some(w) = self.app.get_webview_window("bar") else {
            return;
        };
        let s = self.settings.load();
        let displays = self.platform.screen().displays().unwrap_or_default();
        let size = w
            .outer_size()
            .ok()
            .map(|p| p.to_logical::<f64>(w.scale_factor().unwrap_or(1.0)))
            .map_or((520.0, 56.0), |l| (l.width, l.height));
        let on_screen = |p: &sc_geometry::PointF| {
            displays.iter().any(|d| {
                let f = d.frame_pt;
                p.x >= f.x && p.y >= f.y && p.x + size.0 <= f.x + f.w && p.y + size.1 <= f.y + f.h
            })
        };
        let at = s.bar_at.filter(on_screen).or_else(|| {
            crate::preview::display_for(&s, &displays).map(|d| {
                let f = d.frame_pt;
                sc_geometry::PointF {
                    x: f.x + (f.w - size.0) / 2.0,
                    y: f.y + f.h - size.1 - 96.0,
                }
            })
        });
        if let Some(at) = at
            && let Err(e) = w.set_position(LogicalPosition::new(at.x, at.y))
        {
            tracing::warn!("place bar: {e}");
        }
    }

    /// Dragging the control bar by its grip: `phase` is `begin`, `move` or `end`; `x`, `y` are
    /// global points. The end position is remembered.
    pub fn bar_drag(&self, phase: &str, x: f64, y: f64) -> Reply {
        let w = self.app.get_webview_window("bar").ok_or("no bar window")?;
        let scale = w.scale_factor().unwrap_or(1.0);
        let pos = w
            .outer_position()
            .map_err(|e| e.to_string())?
            .to_logical::<f64>(scale);
        match phase {
            "begin" => {
                *lock(&self.bar_drag) = Some(((x, y), (pos.x, pos.y)));
            }
            "move" => {
                if let Some(((px, py), (wx, wy))) = *lock(&self.bar_drag) {
                    w.set_position(LogicalPosition::new(wx + x - px, wy + y - py))
                        .map_err(|e| e.to_string())?;
                }
            }
            "end" => {
                if lock(&self.bar_drag).take().is_some() {
                    let at = sc_geometry::PointF { x: pos.x, y: pos.y };
                    self.edit_settings(|s| s.bar_at = Some(at))?;
                }
            }
            other => return Err(format!("bar_drag: unknown phase {other:?}")),
        }
        Ok(Value::Null)
    }

    pub fn preview_state(&self) -> Reply {
        let p = lock(&self.preview);
        Ok(json!({
            "shown": p.stage.is_some(),
            "key": p.key,
            "frame": p.frames,
            "native": p.last.is_some_and(|f| f.native),
            "page": p.page.map(|g| json!({"w": g.w, "h": g.h, "scale": g.scale})),
            "drawable": p.last.map(|f| f.drawable),
            "canvas": p.last.map(|f| f.canvas),
            "camera_px": p.last.and_then(|f| f.camera).map(|r| json!([r.x, r.y, r.w, r.h])),
            "camera_open": self.camera.open_device(),
        }))
    }
}
