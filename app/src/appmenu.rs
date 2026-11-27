//! The macOS menu bar (shown while the editor or another full window is in front): our About
//! window, the recorder, the Library, Export and the standard Edit and Window menus.

use std::sync::Arc;

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Runtime};

use crate::core::Core;

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let sep = || PredefinedMenuItem::separator(app);
    let item =
        |id: &str, text: &str, keys: Option<&str>| MenuItem::with_id(app, id, text, true, keys);
    let app_menu = Submenu::with_items(
        app,
        "simple-capture",
        true,
        &[
            &item("app:about", "About simple-capture", None)?,
            &sep()?,
            &item("app:recorder", "Recorder…", Some("CmdOrCtrl+,"))?,
            &item("app:library", "Library", Some("CmdOrCtrl+L"))?,
            &sep()?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &sep()?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &item("app:export", "Export…", Some("CmdOrCtrl+E"))?,
            &sep()?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &sep()?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &sep()?,
            &PredefinedMenuItem::bring_all_to_front(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &window])
}

pub fn handle<R: Runtime>(app: &AppHandle<R>, ev: MenuEvent) {
    let Some(core) = app.try_state::<Arc<Core>>().map(|c| c.inner().clone()) else {
        return;
    };
    let shown = match ev.id().as_ref() {
        "app:about" => core.show_window("about", true),
        "app:recorder" => core.show_window("panel", true),
        "app:library" => core.show_window("library", true),
        "app:export" if core.editor_dir().is_some() => {
            core.emit("editor-export-sheet", serde_json::json!({}));
            Ok(())
        }
        _ => Ok(()),
    };
    if let Err(e) = shown {
        tracing::warn!("menu {}: {e}", ev.id().as_ref());
    }
}
