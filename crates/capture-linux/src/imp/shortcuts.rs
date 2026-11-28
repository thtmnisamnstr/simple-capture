//! Global shortcuts on Wayland, through the GlobalShortcuts portal. Key grabs through X11 (what
//! the shortcut plugin does) see no keys while a Wayland window has focus, which is nearly always.
//! The compositor owns the final keys: ours are only the preferred triggers, and the user can
//! change them in the system settings.

use std::sync::{Arc, Mutex};

use ashpd::desktop::Session;
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;
use sc_platform::{Error, Result};

use super::portal::{ready, runtime};

/// A shortcut to bind: our action id, what it does, and the keys in the app's own notation
/// (`Alt+Shift+KeyR`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub id: String,
    pub description: String,
    pub keys: String,
}

/// What the compositor bound for an action, as it describes the keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    pub id: String,
    pub trigger: String,
}

struct Active {
    session: Arc<Session<GlobalShortcuts>>,
    listener: tokio::task::JoinHandle<()>,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);

/// Whether this session is Wayland, where only the portal can give global shortcuts.
pub fn wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty())
}

/// The XDG shortcut notation (`ALT+SHIFT+r`) for keys in the app's notation, or None when a key
/// has no keysym name here (the compositor then asks the user for one).
pub fn xdg_trigger(keys: &str) -> Option<String> {
    let mut mods = Vec::new();
    let mut key = None;
    for part in keys.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "control" | "ctrl" => mods.push("CTRL"),
            "alt" | "option" => mods.push("ALT"),
            "shift" => mods.push("SHIFT"),
            "super" | "meta" | "cmd" | "command" => mods.push("LOGO"),
            _ => key = Some(keysym(part)?),
        }
    }
    let key = key?;
    mods.push(&key);
    Some(mods.join("+"))
}

fn keysym(code: &str) -> Option<String> {
    if let Some(c) = code.strip_prefix("Key")
        && c.len() == 1
    {
        return Some(c.to_ascii_lowercase());
    }
    if let Some(d) = code.strip_prefix("Digit")
        && d.len() == 1
    {
        return Some(d.to_string());
    }
    if let Some(n) = code.strip_prefix('F')
        && n.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n))
    {
        return Some(code.to_string());
    }
    let named = match code {
        "Space" => "space",
        "Enter" => "Return",
        "Escape" => "Escape",
        "Tab" => "Tab",
        "Backspace" => "BackSpace",
        "Delete" => "Delete",
        "Insert" => "Insert",
        "Home" => "Home",
        "End" => "End",
        "PageUp" => "Page_Up",
        "PageDown" => "Page_Down",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "Minus" => "minus",
        "Equal" => "equal",
        "Comma" => "comma",
        "Period" => "period",
        "Slash" => "slash",
        "Backslash" => "backslash",
        "Semicolon" => "semicolon",
        "Quote" => "apostrophe",
        "Backquote" => "grave",
        "BracketLeft" => "bracketleft",
        "BracketRight" => "bracketright",
        _ => return None,
    };
    Some(named.to_string())
}

fn stop_active() {
    let old = ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(a) = old {
        a.listener.abort();
        let session = a.session;
        runtime().spawn(async move {
            let _ = session.close().await;
        });
    }
}

/// Binds `wanted` in a new portal session in place of any earlier one, and calls `activated`
/// with an action id each time its keys are pressed. Blocks until the compositor answers, which
/// can include a dialog where the user confirms or changes the keys; call it off the UI thread.
pub fn bind(wanted: Vec<Wanted>, activated: Arc<dyn Fn(&str) + Send + Sync>) -> Result<Vec<Bound>> {
    stop_active();
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let started = runtime().block_on(async {
        ready().await;
        let portal = GlobalShortcuts::new().await?;
        tracing::info!(version = portal.version(), "shortcuts portal: session");
        let session = Arc::new(portal.create_session(Default::default()).await?);
        let list: Vec<NewShortcut> = wanted
            .iter()
            .map(|w| {
                let trigger = xdg_trigger(&w.keys);
                NewShortcut::new(w.id.clone(), w.description.clone())
                    .preferred_trigger(trigger.as_deref())
            })
            .collect();
        let parent = tokio::task::spawn_blocking(super::parent::focused)
            .await
            .ok()
            .flatten();
        let request = portal
            .bind_shortcuts(&session, &list, parent.as_ref(), Default::default())
            .await?;
        tracing::info!(
            "shortcuts portal: waiting for the system to bind {} shortcuts",
            list.len()
        );
        let response = request.response()?;
        let bound: Vec<Bound> = response
            .shortcuts()
            .iter()
            .map(|s| Bound {
                id: s.id().to_string(),
                trigger: s.trigger_description().to_string(),
            })
            .collect();
        let stream = portal.receive_activated().await?;
        Ok::<_, ashpd::Error>((portal, session, bound, stream))
    });
    let (portal, session, bound, mut stream) = started.map_err(|e| {
        let text = e.to_string();
        if text.contains("UnknownInterface") || text.contains("No such interface") {
            Error::Unsupported("this desktop offers no global shortcuts to apps")
        } else {
            Error::Other(format!("global shortcuts portal: {e}"))
        }
    })?;
    // The portal sends Activated only to the connection that owns the session, and there is one
    // session at a time (the old listener is gone before a rebind).
    let listener = runtime().spawn(async move {
        let _portal = portal;
        while let Some(a) = stream.next().await {
            activated(a.shortcut_id());
        }
    });
    *ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Active { session, listener });
    Ok(bound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_keys_become_xdg_triggers() {
        assert_eq!(
            xdg_trigger("Alt+Shift+KeyR").as_deref(),
            Some("ALT+SHIFT+r")
        );
        assert_eq!(
            xdg_trigger("Control+Super+Digit5").as_deref(),
            Some("CTRL+LOGO+5")
        );
        assert_eq!(xdg_trigger("Ctrl+Alt+F9").as_deref(), Some("CTRL+ALT+F9"));
        assert_eq!(
            xdg_trigger("Shift+Alt+ArrowUp").as_deref(),
            Some("SHIFT+ALT+Up")
        );
        assert_eq!(xdg_trigger("Alt+MediaPlayPause"), None);
        assert_eq!(xdg_trigger("Alt+Shift"), None);
    }
}
