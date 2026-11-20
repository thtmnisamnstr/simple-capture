//! Simple Capture desktop app: Tauri 2 shell (M1 plan §5.2–§5.4).

mod commands;
mod core;
#[cfg(debug_assertions)]
mod driver;
mod dto;
#[cfg(target_os = "macos")]
mod objc;
mod protocol;
mod settings;
mod signals;
mod state;
mod still;
mod tray;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use sc_core::journal::StopReason;
use sc_platform::Platform;
use tauri::{Manager, RunEvent, WindowEvent};

use crate::core::Core;

const APP_ID: &str = "com.simplecapture.desktop";

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
    let log_path = data_dir.join("logs").join("simple-capture-app.log");
    let file = std::fs::create_dir_all(data_dir.join("logs"))
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
        std::env::temp_dir().join("Simple Capture")
    })
}

fn driver_active() -> bool {
    #[cfg(debug_assertions)]
    return driver::active();
    #[cfg(not(debug_assertions))]
    false
}

fn register_hotkey(core: &Arc<Core>) {
    use tauri_plugin_global_shortcut::{
        Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
    };
    let shortcut = Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::KeyS);
    let c = core.clone();
    let r = core
        .app
        .global_shortcut()
        .on_shortcut(shortcut, move |_app, _sc, ev| {
            if ev.state == ShortcutState::Pressed {
                let c = c.clone();
                std::thread::spawn(move || {
                    if let Err(e) = c.hotkey_switch() {
                        tracing::warn!("hotkey switch: {e}");
                    }
                });
            }
        });
    match r {
        Ok(()) => tracing::info!("hotkey ⌥⇧S registered"),
        Err(e) => {
            let error = e.to_string();
            tracing::warn!("hotkey ⌥⇧S: {error}");
            core.emit("notice", dto::hotkey_unavailable(&error));
        }
    }
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
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
    let tray = tray::Tray::build(app.handle(), core.clone())?;
    let _ = core.tray.set(tray);
    tracing::info!("tray ready");
    tray::start_timer(core.clone());
    register_hotkey(&core);
    signals::attach(core.clone());
    let c = core.clone();
    std::thread::Builder::new()
        .name("launch-tasks".into())
        .spawn(move || c.launch_tasks())?;
    Ok(())
}

fn on_run_event(app: &tauri::AppHandle, ev: RunEvent) {
    match ev {
        RunEvent::Ready => {
            eprintln!("SC_READY");
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
            if let Some(w) = app.get_webview_window(&label)
                && let Err(e) = w.hide()
            {
                tracing::warn!(label, "hide on close: {e}");
            }
        }
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
    let log = init_tracing(&data_dir());
    if let Some(p) = &log {
        tracing::info!(log = %p.display(), "app log");
    }
    signals::install();
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.activate_ignoring_other_apps(false);
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
