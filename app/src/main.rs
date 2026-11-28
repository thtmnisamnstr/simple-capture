#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! simple-capture desktop app: Tauri 2 shell (M1 plan §5.2–§5.4).

mod about;
#[cfg(target_os = "macos")]
mod appmenu;
mod commands;
mod core;
#[cfg(debug_assertions)]
mod driver;
mod dto;
mod editor;
mod editor_media;
mod editor_render;
mod exports;
mod library;
mod live;
mod meter;
#[cfg(target_os = "macos")]
mod objc;
mod preview;
mod protocol;
#[cfg(target_os = "macos")]
mod sandbox;
mod settings;
mod shortcuts;
mod signals;
mod sink;
mod stage;
mod state;
mod still;
mod tray;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sc_core::journal::StopReason;
use sc_platform::Platform;
use tauri::{Manager, RunEvent, WindowEvent};

use crate::core::Core;

const APP_ID: &str = "com.simplecapture.desktop";
const LOG_ROTATE_BYTES: u64 = 10 * 1024 * 1024;

fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SC_DATA_DIR") {
        return PathBuf::from(d);
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local/share")))
    };
    base.unwrap_or_else(std::env::temp_dir).join(APP_ID)
}

fn init_tracing(data_dir: &std::path::Path) -> Option<PathBuf> {
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let filter = || {
        tracing_subscriber::EnvFilter::try_from_env("SC_LOG")
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };
    let logs = data_dir.join("logs");
    let log_path = logs.join("simple-capture-app.log");
    if std::fs::metadata(&log_path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES) {
        let _ = std::fs::rename(&log_path, logs.join("simple-capture-app.1.log"));
    }
    let file = std::fs::create_dir_all(&logs)
        .and_then(|_| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
        })
        .ok();
    let stderr = tracing_subscriber::fmt::layer()
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr)
        .with_filter(filter());
    let logged = file.is_some();
    let file_layer = file.map(|f| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(f))
            .with_filter(filter())
    });
    let _ = tracing_subscriber::registry()
        .with(stderr)
        .with(file_layer)
        .try_init();
    logged.then_some(log_path)
}

fn platform() -> Arc<dyn Platform> {
    #[cfg(debug_assertions)]
    if std::env::var("SC_APP_PLATFORM").as_deref() == Ok("synthetic") {
        tracing::info!("synthetic platform (SC_APP_PLATFORM)");
        let p: Arc<dyn Platform> =
            sc_platform::synthetic::SyntheticPlatform::new(sc_platform::synthetic::Scenario::new());
        return p;
    }
    #[cfg(target_os = "macos")]
    return sc_capture_macos::platform();
    #[cfg(target_os = "windows")]
    return sc_capture_windows::platform();
    #[cfg(target_os = "linux")]
    return sc_capture_linux::platform();
}

fn projects_dir(p: &dyn Platform) -> PathBuf {
    if let Some(d) = std::env::var_os("SC_PROJECTS_DIR") {
        return PathBuf::from(d);
    }
    p.default_projects_dir().unwrap_or_else(|e| {
        tracing::warn!("default projects folder: {e}");
        std::env::temp_dir().join("simple-capture")
    })
}

fn driver_active() -> bool {
    #[cfg(debug_assertions)]
    return driver::active();
    #[cfg(not(debug_assertions))]
    false
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(r) = app.path().resource_dir() {
        sc_text::fonts::set_resource_fonts_dir(r.join("fonts"));
    }
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    #[cfg(target_os = "linux")]
    sc_capture_linux::set_state_dir(data_dir());
    let platform = platform();
    let data = data_dir();
    let projects = projects_dir(&*platform);
    tracing::info!(
        platform = platform.name(),
        data_dir = %data.display(),
        projects_dir = %projects.display(),
        permission_host = ?platform.permission_host(),
        "starting"
    );
    FIRST_LAUNCH.store(!data.join("settings.json").exists(), Ordering::SeqCst);
    let core = Arc::new(Core::new(
        app.handle().clone(),
        platform,
        data,
        projects,
        driver_active(),
    ));
    app.manage(core.clone());
    {
        use tauri::Listener;
        let c = core.clone();
        app.listen_any("ui-log", move |e| {
            let line = serde_json::from_str::<String>(e.payload())
                .unwrap_or_else(|_| e.payload().to_string());
            c.on_ui_log(line);
        });
    }
    core.configure_windows()?;
    stage::spawn(&core);
    editor_render::spawn(&core);
    let tray = tray::Tray::build(app.handle(), core.clone())?;
    let _ = core.tray.set(tray);
    tracing::info!("tray ready");
    tray::start_timer(core.clone());
    let errors = core.apply_shortcuts();
    if !errors.is_empty() {
        let text = errors
            .iter()
            .map(|(id, e)| format!("{id}: {e}"))
            .collect::<Vec<_>>()
            .join("; ");
        core.emit("notice", dto::hotkey_unavailable(&text));
    }
    signals::attach(core.clone());
    let c = core.clone();
    std::thread::Builder::new()
        .name("launch-tasks".into())
        .spawn(move || c.launch_tasks())?;
    Ok(())
}

/// A menu-bar app shows nothing when it starts, so the first launch and opening it again from
/// Finder or Launchpad bring up the recorder; otherwise it looks as if nothing happened.
static FIRST_LAUNCH: AtomicBool = AtomicBool::new(false);

fn show_recorder(app: &tauri::AppHandle) {
    if let Some(core) = app.try_state::<Arc<Core>>()
        && let Err(e) = core.show_window("panel", true)
    {
        tracing::warn!("show the recorder: {e}");
    }
}

fn on_run_event(app: &tauri::AppHandle, ev: RunEvent) {
    match ev {
        RunEvent::Ready => {
            eprintln!("SC_READY");
            if FIRST_LAUNCH.load(Ordering::SeqCst) && !driver_active() {
                show_recorder(app);
            }
            #[cfg(debug_assertions)]
            if let Some(core) = app.try_state::<Arc<Core>>() {
                driver::maybe_start(core.inner().clone());
            }
        }
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } => {
            api.prevent_close();
            if label == "panel"
                && let Some(core) = app.try_state::<Arc<Core>>()
            {
                let core = core.inner().clone();
                std::thread::spawn(move || {
                    if let Err(e) = core.camera_preview(false) {
                        tracing::warn!("camera preview: {e}");
                    }
                });
            }
            if label == "stage"
                && let Some(core) = app.try_state::<Arc<Core>>()
            {
                core.preview_closed();
            }
            if label == "editor"
                && let Some(core) = app.try_state::<Arc<Core>>()
            {
                let core = core.inner().clone();
                std::thread::spawn(move || {
                    if let Err(e) = core.editor_close() {
                        tracing::warn!("editor close: {e}");
                    }
                });
            }
            if let Some(w) = app.get_webview_window(&label)
                && let Err(e) = w.hide()
            {
                tracing::warn!(label, "hide on close: {e}");
            }
            if crate::core::FULL_WINDOWS.contains(&label.as_str())
                && let Some(core) = app.try_state::<Arc<Core>>()
            {
                core.settle_activation();
            }
        }
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::Moved(pos),
            ..
        } if label == "stage" => {
            if let (Some(core), Some(w)) =
                (app.try_state::<Arc<Core>>(), app.get_webview_window(&label))
            {
                let p = pos.to_logical::<f64>(w.scale_factor().unwrap_or(1.0));
                core.preview_moved(p.x, p.y);
            }
        }
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. },
            ..
        } if label == "stage" || label == "editor" => {
            if let Some(core) = app.try_state::<Arc<Core>>() {
                if label == "stage" {
                    core.stage_window_changed();
                } else {
                    core.editor_window_changed();
                }
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } if !driver_active() => show_recorder(app),
        RunEvent::ExitRequested { api, code, .. } => {
            let Some(core) = app.try_state::<Arc<Core>>().map(|c| c.inner().clone()) else {
                return;
            };
            if core.exit_done.load(Ordering::SeqCst) {
                return;
            }
            api.prevent_exit();
            std::thread::spawn(move || core.shutdown(StopReason::Quit, code.unwrap_or(0)));
        }
        _ => {}
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    sc_capture_linux::set_program_name();
    let log = init_tracing(&data_dir());
    if let Some(p) = &log {
        tracing::info!(log = %p.display(), "app log");
    }
    signals::install();
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.activate_ignoring_other_apps(false);
    #[cfg(target_os = "macos")]
    let builder = builder.menu(appmenu::build).on_menu_event(appmenu::handle);
    let app = builder
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol("sc", protocol::handle)
        .invoke_handler(tauri::generate_handler![
            commands::get_permissions,
            commands::request_permission,
            commands::open_permission_settings,
            commands::list_displays,
            commands::list_sources,
            commands::list_devices,
            commands::show_mic_modes,
            commands::get_settings,
            commands::set_settings,
            commands::get_capabilities,
            commands::preview_still,
            commands::choose_background_image,
            commands::begin_region_select,
            commands::begin_pick_mode,
            commands::open_system_picker,
            commands::start_recording,
            commands::stop_recording,
            commands::discard_recording,
            commands::switch_source,
            commands::quick_export,
            commands::cancel_export,
            commands::reveal,
            commands::recovery_report,
            commands::camera_preview,
            commands::preview_state,
            commands::camera_key_live,
            commands::show_video_effects,
            commands::bar_drag,
            commands::editor,
            commands::app,
        ])
        .setup(setup)
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(on_run_event),
        Err(e) => {
            tracing::error!("simple-capture-app: {e}");
            std::process::exit(1);
        }
    }
}
