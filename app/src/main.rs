//! Simple Capture desktop app: Tauri 2 shell (M1 plan §5.2–§5.4).
//! Every window is created hidden at launch; overlays are configured through
//! `OverlayWindows::configure` by wp-app-shell.

mod commands;
#[cfg(debug_assertions)]
mod driver;
mod protocol;

use tauri::Manager;

/// Window labels declared in tauri.conf.json, all hidden at launch.
pub const WINDOW_LABELS: [&str; 7] = [
    "panel",
    "onboarding",
    "bar",
    "switcher",
    "region",
    "pick",
    "saved",
];

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("SC_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

fn main() {
    init_tracing();
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .register_uri_scheme_protocol("sc", protocol::handle)
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
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            for label in WINDOW_LABELS {
                if app.get_webview_window(label).is_none() {
                    tracing::warn!(label, "window missing from tauri.conf.json");
                }
            }
            #[cfg(debug_assertions)]
            driver::maybe_start(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("simple-capture-app: {e}");
        std::process::exit(1);
    }
}
