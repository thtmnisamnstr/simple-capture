//! Global shortcuts: the actions they run, their default keys, and registering the keys the
//! settings name. Keys are written like `Alt+Shift+KeyR` (modifiers, then a `KeyboardEvent.code`).

use std::collections::BTreeMap;
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

use serde_json::{Value, json};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::core::{Core, Reply};

/// Action id, what it does, default keys.
pub const ACTIONS: [(&str, &str, &str); 5] = [
    ("record", "Start or stop recording", "Alt+Shift+KeyR"),
    ("pause", "Pause or resume", "Alt+Shift+KeyP"),
    ("switch", "Switch source", "Alt+Shift+KeyS"),
    ("camera", "Show or hide the camera", "Alt+Shift+KeyC"),
    ("open", "Open the recorder", "Alt+Shift+KeyO"),
];

pub fn defaults() -> BTreeMap<String, String> {
    ACTIONS
        .iter()
        .map(|(id, _, keys)| (id.to_string(), keys.to_string()))
        .collect()
}

fn run(core: &Arc<Core>, action: &str) -> Reply {
    match action {
        "record" => core.start_stop(),
        "pause" => core.toggle_pause(),
        "switch" => core.hotkey_switch(),
        "camera" => core.toggle_camera(),
        "open" => core.show_window("panel", true).map(|_| Value::Null),
        other => Err(format!("unknown shortcut action {other:?}")),
    }
}

/// The keys each action has after `want` is applied to `current`: unknown actions are dropped,
/// missing ones get their default, and an empty string means none.
pub fn merged(current: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    ACTIONS
        .iter()
        .map(|(id, _, keys)| {
            let k = current.get(*id).map_or(*keys, String::as_str);
            (id.to_string(), k.trim().to_string())
        })
        .collect()
}

/// Why `keys` cannot be a global shortcut, if it cannot.
pub fn check(keys: &str) -> Result<Shortcut, String> {
    let s: Shortcut = keys
        .parse()
        .map_err(|e| format!("“{keys}” is not a shortcut: {e}"))?;
    let lower = keys.to_ascii_lowercase();
    let held = [
        "alt", "option", "control", "ctrl", "super", "cmd", "command", "meta",
    ];
    if !held.iter().any(|m| lower.split('+').any(|p| p == *m)) {
        return Err("Add ⌘, ⌃ or ⌥ so typing doesn’t trigger it".into());
    }
    if cfg!(target_os = "macos") && lower.split('+').any(|p| p.starts_with("media")) {
        return Err("Media keys can’t be shortcuts on a Mac".into());
    }
    Ok(s)
}

/// The keys the compositor bound through the portal, as it describes them (Wayland).
#[cfg(target_os = "linux")]
fn portal_triggers() -> std::sync::MutexGuard<'static, BTreeMap<String, String>> {
    static SYSTEM: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
    SYSTEM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Core {
    /// Registers the settings' shortcuts in place of the current ones (none while paused) and
    /// remembers why any failed.
    pub fn apply_shortcuts(self: &Arc<Self>) -> BTreeMap<String, String> {
        #[cfg(target_os = "linux")]
        if sc_capture_linux::shortcuts::wayland() {
            return self.apply_portal_shortcuts();
        }
        let mut current = self.shortcut_errors();
        let gs = self.app.global_shortcut();
        if let Err(e) = gs.unregister_all() {
            tracing::warn!("shortcuts: unregister: {e}");
        }
        let mut errors = BTreeMap::new();
        if self.shortcuts_paused() {
            current.clear();
            return errors;
        }
        let keys = merged(&self.current_settings().shortcuts);
        let mut taken: BTreeMap<u32, &str> = BTreeMap::new();
        for (id, label, _) in ACTIONS {
            let k = &keys[id];
            if k.is_empty() {
                continue;
            }
            let shortcut = match check(k) {
                Ok(s) => s,
                Err(e) => {
                    errors.insert(id.to_string(), e);
                    continue;
                }
            };
            if let Some(other) = taken.insert(shortcut.id(), label) {
                errors.insert(id.to_string(), format!("Also used for “{other}”"));
                continue;
            }
            let c = self.clone();
            let r = gs.on_shortcut(shortcut, move |_app, _sc, ev| {
                if ev.state == ShortcutState::Pressed {
                    let c = c.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = run(&c, id) {
                            tracing::warn!("shortcut {id}: {e}");
                        }
                    });
                }
            });
            match r {
                Ok(()) => tracing::info!("shortcut {id} = {k}"),
                Err(e) => {
                    errors.insert(id.to_string(), format!("Another app is using it ({e})"));
                }
            }
        }
        if !errors.is_empty() {
            tracing::warn!(?errors, "shortcuts");
        }
        *current = errors.clone();
        errors
    }

    /// Wayland: the GlobalShortcuts portal binds the keys (X11 grabs see no keys while a Wayland
    /// window has focus). Binding waits for the compositor, which may ask the user first, so it
    /// runs on its own thread and reports problems as a notice. While a new shortcut is being
    /// typed, presses are ignored instead of rebinding, which would ask again.
    #[cfg(target_os = "linux")]
    fn apply_portal_shortcuts(self: &Arc<Self>) -> BTreeMap<String, String> {
        use sc_capture_linux::shortcuts::{Wanted, bind};
        static BOUND_FOR: Mutex<Option<Vec<Wanted>>> = Mutex::new(None);
        let keys = merged(&self.current_settings().shortcuts);
        let mut errors = BTreeMap::new();
        let mut wanted = Vec::new();
        for (id, label, _) in ACTIONS {
            let k = &keys[id];
            if k.is_empty() {
                continue;
            }
            match check(k) {
                Ok(_) => wanted.push(Wanted {
                    id: id.to_string(),
                    description: label.to_string(),
                    keys: k.clone(),
                }),
                Err(e) => {
                    errors.insert(id.to_string(), e);
                }
            }
        }
        {
            let mut last = BOUND_FOR
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if last.as_ref() == Some(&wanted) {
                let current = self.shortcut_errors().clone();
                return current;
            }
            *last = Some(wanted.clone());
        }
        *self.shortcut_errors() = errors.clone();
        let core = Arc::downgrade(self);
        let on_press: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |pressed| {
            let Some(c) = core.upgrade() else { return };
            if c.shortcuts_paused() {
                return;
            }
            let Some((id, _, _)) = ACTIONS.iter().find(|(a, _, _)| *a == pressed) else {
                return;
            };
            std::thread::spawn(move || {
                if let Err(e) = run(&c, id) {
                    tracing::warn!("shortcut {id}: {e}");
                }
            });
        });
        let core = Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("portal-shortcuts".into())
            .spawn(move || {
                let result = bind(wanted.clone(), on_press);
                let Some(c) = core.upgrade() else { return };
                match result {
                    Ok(bound) => {
                        let mut system = portal_triggers();
                        system.clear();
                        for b in bound {
                            tracing::info!("shortcut {} = {} (portal)", b.id, b.trigger);
                            system.insert(b.id, b.trigger);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("shortcuts: {e}");
                        let text = format!("the system did not bind them ({e})");
                        {
                            let mut errs = c.shortcut_errors();
                            for w in &wanted {
                                errs.insert(w.id.clone(), text.clone());
                            }
                        }
                        *BOUND_FOR
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                        c.emit("notice", crate::dto::hotkey_unavailable(&text));
                    }
                }
                c.emit("shortcuts-changed", c.shortcuts_state());
            });
        if let Err(e) = spawned {
            tracing::warn!("shortcuts thread: {e}");
        }
        errors
    }

    /// The actions with their keys, labels and any registration problem.
    pub fn shortcuts_state(&self) -> Value {
        let keys = merged(&self.current_settings().shortcuts);
        let errors = self.shortcut_errors().clone();
        #[cfg(target_os = "linux")]
        let system = portal_triggers().clone();
        #[cfg(not(target_os = "linux"))]
        let system: BTreeMap<String, String> = BTreeMap::new();
        json!(
            ACTIONS
                .iter()
                .map(|(id, label, default)| json!({
                    "id": id,
                    "label": label,
                    "keys": keys[*id],
                    "default": default,
                    "error": errors.get(*id),
                    "system": system.get(*id),
                }))
                .collect::<Vec<_>>()
        )
    }

    /// Sets one action's keys (`""` for none) and registers them all again.
    pub fn set_shortcut(self: &Arc<Self>, id: &str, keys: &str) -> Reply {
        if !ACTIONS.iter().any(|(a, _, _)| *a == id) {
            return Err(format!("unknown shortcut action {id:?}"));
        }
        let keys = keys.trim();
        if !keys.is_empty() {
            check(keys)?;
        }
        self.edit_settings_pub(|s| {
            let mut m = merged(&s.shortcuts);
            m.insert(id.to_string(), keys.to_string());
            s.shortcuts = m;
        })?;
        self.apply_shortcuts();
        Ok(self.shortcuts_state())
    }

    /// While the user types a new shortcut, the old ones must not fire.
    pub fn pause_shortcuts(self: &Arc<Self>, paused: bool) -> Reply {
        self.set_shortcuts_paused(paused);
        self.apply_shortcuts();
        Ok(self.shortcuts_state())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_need_a_held_modifier_and_must_parse() {
        assert!(check("Alt+Shift+KeyR").is_ok());
        assert!(check("Control+Super+Digit5").is_ok());
        assert!(check("Shift+KeyR").is_err());
        assert!(check("KeyR").is_err());
        assert!(check("Alt+Shift+Nope").is_err());
        assert_ne!(
            check("Alt+Shift+KeyR").expect("r").id(),
            check("Alt+Shift+KeyP").expect("p").id()
        );
        assert_eq!(
            check("Shift+Alt+KeyR").expect("a").id(),
            check("Alt+Shift+KeyR").expect("b").id()
        );
    }

    #[test]
    fn merged_fills_defaults_and_drops_unknown_actions() {
        let mut m = BTreeMap::new();
        m.insert("pause".to_string(), String::new());
        m.insert("record".to_string(), " Control+Alt+KeyR ".to_string());
        m.insert("old".to_string(), "Alt+KeyX".to_string());
        let out = merged(&m);
        assert_eq!(out.len(), ACTIONS.len());
        assert_eq!(out["record"], "Control+Alt+KeyR");
        assert_eq!(out["pause"], "");
        assert_eq!(out["switch"], "Alt+Shift+KeyS");
        assert!(!out.contains_key("old"));
    }
}
