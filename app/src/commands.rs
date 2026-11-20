//! App commands (M1 plan §5.2). Stubs until wp-app-shell; DTOs are JSON values for now and the
//! UI mirrors them in ui/src/lib/types.ts (snake_case).

use serde_json::Value;

type Reply = Result<Value, String>;

fn not_yet(name: &str) -> Reply {
    Err(format!("{name}: not implemented yet"))
}

/// `{screen, camera, mic}`: granted|denied|not_determined|restricted|unknown. Never prompts.
#[tauri::command]
pub fn get_permissions() -> Reply {
    not_yet("get_permissions")
}

/// Prompts; only from a user click.
#[tauri::command]
pub fn request_permission(kind: String) -> Reply {
    not_yet(&format!("request_permission({kind})"))
}

/// User click only; agents never invoke it.
#[tauri::command]
pub fn open_permission_settings(kind: String) -> Reply {
    not_yet(&format!("open_permission_settings({kind})"))
}

/// Never needs a permission.
#[tauri::command]
pub fn list_displays() -> Reply {
    not_yet("list_displays")
}

/// Only after `get_permissions` reports screen = granted; hides our windows.
#[tauri::command]
pub fn list_sources() -> Reply {
    not_yet("list_sources")
}

#[tauri::command]
pub fn list_devices() -> Reply {
    not_yet("list_devices")
}

#[tauri::command]
pub fn show_mic_modes() -> Reply {
    not_yet("show_mic_modes")
}

#[tauri::command]
pub fn get_settings() -> Reply {
    not_yet("get_settings")
}

#[tauri::command]
pub fn set_settings(settings: Value) -> Reply {
    let _ = settings;
    not_yet("set_settings")
}

#[tauri::command]
pub fn get_capabilities() -> Reply {
    not_yet("get_capabilities")
}

/// Returns an `sc://still/<id>.png` URL.
#[tauri::command]
pub fn preview_still(source: Value, canvas: Value) -> Reply {
    let _ = (source, canvas);
    not_yet("preview_still")
}

#[tauri::command]
pub fn choose_background_image() -> Reply {
    not_yet("choose_background_image")
}

/// `purpose`: "start" | "switch". Events `region-selected` / `region-cancelled`.
#[tauri::command]
pub fn begin_region_select(purpose: String, display_id: Option<String>) -> Reply {
    let _ = display_id;
    not_yet(&format!("begin_region_select({purpose})"))
}

/// Events `source-picked` / `pick-cancelled`.
#[tauri::command]
pub fn begin_pick_mode(purpose: String) -> Reply {
    not_yet(&format!("begin_pick_mode({purpose})"))
}

#[tauri::command]
pub fn open_system_picker(purpose: String) -> Reply {
    not_yet(&format!("open_system_picker({purpose})"))
}

/// Events `recording-state`, `health` (1 Hz), `notice`.
#[tauri::command]
pub fn start_recording(request: Value) -> Reply {
    let _ = request;
    not_yet("start_recording")
}

#[tauri::command]
pub fn stop_recording() -> Reply {
    not_yet("stop_recording")
}

/// Only after a UI confirmation.
#[tauri::command]
pub fn discard_recording() -> Reply {
    not_yet("discard_recording")
}

#[tauri::command]
pub fn switch_source(spec: Value) -> Reply {
    let _ = spec;
    not_yet("switch_source")
}

/// Returns a job id; events `export-progress`, `export-state`.
#[tauri::command]
pub fn quick_export(project_dir: String, size: String) -> Reply {
    let _ = (project_dir, size);
    not_yet("quick_export")
}

#[tauri::command]
pub fn cancel_export(id: String) -> Reply {
    not_yet(&format!("cancel_export({id})"))
}

#[tauri::command]
pub fn reveal(path: String) -> Reply {
    let _ = path;
    not_yet("reveal")
}

#[tauri::command]
pub fn recovery_report() -> Reply {
    not_yet("recovery_report")
}
