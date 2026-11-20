//! App commands (plan §5.2). Every command goes through [`dispatch`], which the debug driver's
//! `cmd NAME JSON` step calls too; work runs on a blocking pool thread, never the main thread.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tauri::State;

use crate::core::{Core, Reply};

#[cfg(test)]
const COMMANDS: [&str; 28] = [
    "get_permissions",
    "request_permission",
    "open_permission_settings",
    "list_displays",
    "list_sources",
    "list_devices",
    "show_mic_modes",
    "get_settings",
    "set_settings",
    "get_capabilities",
    "preview_still",
    "choose_background_image",
    "begin_region_select",
    "begin_pick_mode",
    "open_system_picker",
    "start_recording",
    "stop_recording",
    "discard_recording",
    "switch_source",
    "quick_export",
    "cancel_export",
    "reveal",
    "recovery_report",
    "camera_preview",
    "bubble_state",
    "bubble_drag",
    "camera_key_live",
    "show_video_effects",
];

fn snake_to_camel(s: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in s.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Argument `name` (snake_case, or the camelCase the Tauri JS API sends).
fn arg_value<'a>(args: &'a Value, name: &str) -> Option<&'a Value> {
    args.get(name)
        .or_else(|| args.get(snake_to_camel(name)))
        .filter(|v| !v.is_null())
}

fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    let v = arg_value(args, name).ok_or_else(|| format!("missing argument {name:?}"))?;
    serde_json::from_value(v.clone()).map_err(|e| format!("argument {name:?}: {e}"))
}

fn opt_arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<Option<T>, String> {
    arg_value(args, name)
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("argument {name:?}: {e}")))
        .transpose()
}

pub fn dispatch(core: &Arc<Core>, name: &str, args: &Value) -> Reply {
    match name {
        "get_permissions" => core.get_permissions(),
        "request_permission" => core.request_permission(&arg::<String>(args, "kind")?),
        "open_permission_settings" => core.open_permission_settings(&arg::<String>(args, "kind")?),
        "list_displays" => core.list_displays(),
        "list_sources" => core.list_sources(),
        "list_devices" => core.list_devices(),
        "show_mic_modes" => core.show_mic_modes(),
        "get_settings" => core.get_settings(),
        "set_settings" => core.set_settings(arg(args, "settings")?),
        "get_capabilities" => core.get_capabilities(),
        "preview_still" => core.preview_still(arg(args, "source")?, arg(args, "canvas")?),
        "choose_background_image" => core.choose_background_image(),
        "begin_region_select" => core.begin_region_select(
            &arg::<String>(args, "purpose")?,
            opt_arg(args, "display_id")?,
        ),
        "begin_pick_mode" => core.begin_pick_mode(&arg::<String>(args, "purpose")?),
        "open_system_picker" => core.open_system_picker(&arg::<String>(args, "purpose")?),
        "start_recording" => core.start_recording(arg(args, "request")?),
        "stop_recording" => core.stop_recording(),
        "discard_recording" => core.discard_recording(),
        "switch_source" => core.switch_source(arg(args, "spec")?),
        "quick_export" => core.quick_export(
            &arg::<String>(args, "project_dir")?,
            &arg::<String>(args, "size")?,
        ),
        "cancel_export" => core.cancel_export(&arg::<String>(args, "id")?),
        "reveal" => core.reveal(&arg::<String>(args, "path")?),
        "recovery_report" => core.recovery_report(),
        "camera_preview" => core.camera_preview(arg(args, "active")?),
        "bubble_state" => core.bubble_state(),
        "camera_key_live" => core.camera_key_live(arg(args, "key")?),
        "show_video_effects" => core.show_video_effects(),
        "bubble_drag" => core.bubble_drag(
            &arg::<String>(args, "phase")?,
            arg(args, "x")?,
            arg(args, "y")?,
            opt_arg::<(f64, f64)>(args, "corner")?,
        ),
        other => Err(format!("unknown command {other:?}")),
    }
}

async fn run(core: State<'_, Arc<Core>>, name: &'static str, args: Value) -> Reply {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let r = dispatch(&core, name, &args);
        if let Err(e) = &r {
            tracing::warn!(command = name, "{e}");
        }
        r
    })
    .await
    .map_err(|e| format!("{name}: {e}"))?
}

#[tauri::command]
pub async fn get_permissions(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "get_permissions", Value::Null).await
}

#[tauri::command]
pub async fn request_permission(core: State<'_, Arc<Core>>, kind: String) -> Reply {
    run(core, "request_permission", json!({ "kind": kind })).await
}

#[tauri::command]
pub async fn open_permission_settings(core: State<'_, Arc<Core>>, kind: String) -> Reply {
    run(core, "open_permission_settings", json!({ "kind": kind })).await
}

#[tauri::command]
pub async fn list_displays(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "list_displays", Value::Null).await
}

#[tauri::command]
pub async fn list_sources(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "list_sources", Value::Null).await
}

#[tauri::command]
pub async fn list_devices(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "list_devices", Value::Null).await
}

#[tauri::command]
pub async fn show_mic_modes(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "show_mic_modes", Value::Null).await
}

#[tauri::command]
pub async fn get_settings(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "get_settings", Value::Null).await
}

#[tauri::command]
pub async fn set_settings(core: State<'_, Arc<Core>>, settings: Value) -> Reply {
    run(core, "set_settings", json!({ "settings": settings })).await
}

#[tauri::command]
pub async fn get_capabilities(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "get_capabilities", Value::Null).await
}

#[tauri::command]
pub async fn preview_still(core: State<'_, Arc<Core>>, source: Value, canvas: Value) -> Reply {
    run(
        core,
        "preview_still",
        json!({ "source": source, "canvas": canvas }),
    )
    .await
}

#[tauri::command]
pub async fn choose_background_image(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "choose_background_image", Value::Null).await
}

#[tauri::command]
pub async fn begin_region_select(
    core: State<'_, Arc<Core>>,
    purpose: String,
    display_id: Option<String>,
) -> Reply {
    let args = json!({ "purpose": purpose, "display_id": display_id });
    run(core, "begin_region_select", args).await
}

#[tauri::command]
pub async fn begin_pick_mode(core: State<'_, Arc<Core>>, purpose: String) -> Reply {
    run(core, "begin_pick_mode", json!({ "purpose": purpose })).await
}

#[tauri::command]
pub async fn open_system_picker(core: State<'_, Arc<Core>>, purpose: String) -> Reply {
    run(core, "open_system_picker", json!({ "purpose": purpose })).await
}

#[tauri::command]
pub async fn start_recording(core: State<'_, Arc<Core>>, request: Value) -> Reply {
    run(core, "start_recording", json!({ "request": request })).await
}

#[tauri::command]
pub async fn stop_recording(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "stop_recording", Value::Null).await
}

#[tauri::command]
pub async fn discard_recording(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "discard_recording", Value::Null).await
}

#[tauri::command]
pub async fn switch_source(core: State<'_, Arc<Core>>, spec: Value) -> Reply {
    run(core, "switch_source", json!({ "spec": spec })).await
}

#[tauri::command]
pub async fn quick_export(core: State<'_, Arc<Core>>, project_dir: String, size: String) -> Reply {
    let args = json!({ "project_dir": project_dir, "size": size });
    run(core, "quick_export", args).await
}

#[tauri::command]
pub async fn cancel_export(core: State<'_, Arc<Core>>, id: String) -> Reply {
    run(core, "cancel_export", json!({ "id": id })).await
}

#[tauri::command]
pub async fn reveal(core: State<'_, Arc<Core>>, path: String) -> Reply {
    run(core, "reveal", json!({ "path": path })).await
}

#[tauri::command]
pub async fn recovery_report(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "recovery_report", Value::Null).await
}

#[tauri::command]
pub async fn camera_preview(core: State<'_, Arc<Core>>, active: bool) -> Reply {
    run(core, "camera_preview", json!({ "active": active })).await
}

#[tauri::command]
pub async fn bubble_state(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "bubble_state", Value::Null).await
}

#[tauri::command]
pub async fn bubble_drag(
    core: State<'_, Arc<Core>>,
    phase: String,
    x: f64,
    y: f64,
    corner: Option<(f64, f64)>,
) -> Reply {
    run(
        core,
        "bubble_drag",
        json!({ "phase": phase, "x": x, "y": y, "corner": corner }),
    )
    .await
}

#[tauri::command]
pub async fn show_video_effects(core: State<'_, Arc<Core>>) -> Reply {
    run(core, "show_video_effects", Value::Null).await
}

#[tauri::command]
pub async fn camera_key_live(core: State<'_, Arc<Core>>, key: Value) -> Reply {
    run(core, "camera_key_live", json!({ "key": key })).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_accept_both_spellings() {
        let a = json!({"displayId": "7", "purpose": "start", "project_dir": "/p", "nil": null});
        assert_eq!(opt_arg::<String>(&a, "display_id"), Ok(Some("7".into())));
        assert_eq!(arg::<String>(&a, "project_dir"), Ok("/p".into()));
        assert_eq!(opt_arg::<String>(&a, "nil"), Ok(None));
        assert!(arg::<String>(&a, "kind").is_err());
        assert!(arg::<u32>(&a, "purpose").is_err());
        assert_eq!(snake_to_camel("a_bc_d"), "aBcD");
    }

    #[test]
    fn command_list_matches_ui() {
        let ui = include_str!("../../ui/src/lib/api.ts");
        let start = ui
            .find("export const COMMANDS")
            .expect("COMMANDS in api.ts");
        let block = &ui[start..];
        let block = &block[..block.find("] as const").expect("end of COMMANDS")];
        let names: Vec<&str> = block
            .split('\'')
            .enumerate()
            .filter_map(|(i, s)| (i % 2 == 1).then_some(s))
            .collect();
        assert_eq!(names, COMMANDS.to_vec());
    }

    #[test]
    fn every_command_is_registered_with_tauri() {
        let main = include_str!("main.rs");
        let handler = &main[main.find("generate_handler![").expect("invoke handler")..];
        let handler = &handler[..handler.find("])").expect("end of handler")];
        for name in COMMANDS {
            assert!(
                handler.contains(&format!("commands::{name},")),
                "{name} is dispatched but not in generate_handler!"
            );
        }
    }
}
