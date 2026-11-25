//! Status item (plan §5.3): Open Recorder · Start/Stop Recording · Switch Source… · Quit, with
//! the elapsed time as its title while recording.

use std::sync::Arc;
use std::time::Duration;

use sc_core::journal::StopReason;
use serde_json::Value;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, LogicalPosition, Manager};

use crate::core::{Core, Reply};
use crate::state::Phase;

pub const ITEMS: [(&str, &str); 9] = [
    ("open", "Open Recorder"),
    ("start-stop", "Start Recording"),
    ("pause", "Pause Recording"),
    ("switch", "Switch Source…"),
    ("edit", "Edit Last Recording"),
    ("library", "Library…"),
    ("diagnostics", "Diagnostics…"),
    ("about", "About Simple Capture"),
    ("quit", "Quit"),
];

pub struct Tray {
    icon: TrayIcon,
    menu: Menu<tauri::Wry>,
    start_stop: MenuItem<tauri::Wry>,
    pause: MenuItem<tauri::Wry>,
    recording: std::sync::atomic::AtomicBool,
}

fn tray_image(png: &[u8]) -> Result<Image<'static>, String> {
    let img = sc_text::decode_image(png).map_err(|e| format!("tray icon: {e}"))?;
    Ok(Image::new_owned(img.data, img.size.w, img.size.h))
}

pub fn format_elapsed(ns: i64) -> String {
    let s = ns.max(0) / 1_000_000_000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(debug_assertions)]
pub fn item_id(label_or_id: &str) -> Option<&'static str> {
    let l = label_or_id.trim();
    ITEMS.iter().find_map(|(id, label)| {
        let plain = label.trim_end_matches('…');
        (l == *id
            || l == *label
            || l == plain
            || (l.eq_ignore_ascii_case("Stop Recording")
                || l.eq_ignore_ascii_case("Cancel Countdown"))
                && *id == "start-stop")
            .then_some(*id)
    })
}

impl Tray {
    pub fn build(app: &AppHandle, core: Arc<Core>) -> Result<Tray, String> {
        let e = |e: tauri::Error| format!("tray: {e}");
        let items: Vec<MenuItem<tauri::Wry>> = ITEMS
            .iter()
            .map(|(id, label)| MenuItem::with_id(app, *id, *label, true, None::<&str>))
            .collect::<Result<_, _>>()
            .map_err(e)?;
        let refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = items
            .iter()
            .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
            .collect();
        let menu = Menu::with_items(app, &refs).map_err(e)?;
        let image = tray_image(include_bytes!("../icons/tray-template@2x.png"))?;
        let icon = TrayIconBuilder::with_id("main")
            .icon(image)
            .icon_as_template(true)
            .tooltip("Simple Capture")
            .menu(&menu)
            .show_menu_on_left_click(true)
            .on_tray_icon_event({
                let core = core.clone();
                move |_tray, ev| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = ev
                        && core.phase().is_recording()
                    {
                        show_recording_menu(&core);
                    }
                }
            })
            .on_menu_event(move |_app, ev| {
                let id = ev.id().as_ref().to_string();
                let core = core.clone();
                std::thread::spawn(move || {
                    if let Err(e) = on_item(&core, &id) {
                        tracing::warn!(item = id, "tray: {e}");
                    }
                });
            })
            .build(app)
            .map_err(e)?;
        let start_stop = items[1].clone();
        let pause = items[2].clone();
        let _ = pause.set_enabled(false);
        Ok(Tray {
            icon,
            menu,
            start_stop,
            pause,
            recording: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// `counting`: a recording start, its countdown included, is under way.
    pub fn refresh(&self, phase: &Phase, elapsed_ns: i64, paused: bool, counting: bool) {
        let recording = phase.is_recording();
        let title = recording.then(|| {
            if paused {
                format!("❚❚ {}", format_elapsed(elapsed_ns))
            } else {
                format_elapsed(elapsed_ns)
            }
        });
        let _ = self.pause.set_enabled(recording);
        let _ = self.pause.set_text(if paused {
            "Resume Recording"
        } else {
            "Pause Recording"
        });
        if let Err(e) = self.icon.set_title(title) {
            tracing::debug!("tray title: {e}");
        }
        if self
            .recording
            .swap(recording, std::sync::atomic::Ordering::Relaxed)
            != recording
        {
            let (bytes, template): (&[u8], bool) = if recording {
                (include_bytes!("../icons/tray-recording@2x.png"), false)
            } else {
                (include_bytes!("../icons/tray-template@2x.png"), true)
            };
            match tray_image(bytes) {
                Ok(image) => {
                    let _ = self.icon.set_icon(Some(image));
                    let _ = self.icon.set_icon_as_template(template);
                }
                Err(e) => tracing::debug!("{e}"),
            }
            // A system menu only becomes excludable from the recording once it is already on
            // screen, so its first frame can be captured; while recording, a click opens the
            // `traymenu` window instead, which was excluded before the recording began.
            let menu = (!recording).then(|| self.menu.clone());
            if let Err(e) = self.icon.set_menu(menu) {
                tracing::warn!("tray menu: {e}");
            }
        }
        let text = if recording {
            "Stop Recording"
        } else if counting {
            "Cancel Countdown"
        } else {
            "Start Recording"
        };
        if let Err(e) = self.start_stop.set_text(text) {
            tracing::debug!("tray item: {e}");
        }
    }

    #[cfg(debug_assertions)]
    pub fn status_item(&self) -> usize {
        #[cfg(target_os = "macos")]
        {
            self.icon
                .with_inner_tray_icon(|t| {
                    t.ns_status_item()
                        .map(|i| &*i as *const _ as *const std::ffi::c_void as usize)
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        }
        #[cfg(not(target_os = "macos"))]
        0
    }

    /// Opens the menu in-process (no synthetic input), closes it after `hold`, and returns this
    /// process's on-screen windows listed halfway through (the menu is the one not in `known`).
    #[cfg(debug_assertions)]
    pub fn open_menu(&self, hold: Duration, known: &[u32]) -> Result<Value, String> {
        let item = self.status_item();
        let icon = self.icon.clone();
        let opener = std::thread::spawn(move || icon.with_inner_tray_icon(|t| t.show_menu()));
        std::thread::sleep(hold / 2);
        #[cfg(target_os = "macos")]
        let windows: Vec<Value> = crate::objc::own_windows_on_screen()
            .into_iter()
            .filter(|w| !known.contains(&w.id))
            .map(|w| serde_json::to_value(w).unwrap_or(Value::Null))
            .collect();
        #[cfg(not(target_os = "macos"))]
        let windows: Vec<Value> = {
            let _ = known;
            Vec::new()
        };
        std::thread::sleep(hold - hold / 2);
        #[cfg(target_os = "macos")]
        crate::objc::cancel_status_item_menu(item);
        #[cfg(not(target_os = "macos"))]
        let _ = item;
        opener
            .join()
            .map_err(|_| "tray menu thread panicked".to_string())?
            .map_err(|e| format!("tray menu: {e}"))?;
        Ok(serde_json::json!({ "other_windows": windows }))
    }

    fn below_icon(&self, app: &AppHandle, label: &str) {
        let (Ok(Some(rect)), Some(w)) = (self.icon.rect(), app.get_webview_window(label)) else {
            return;
        };
        let scale = w.scale_factor().unwrap_or(1.0);
        let p = rect.position.to_logical::<f64>(scale);
        let s = rect.size.to_logical::<f64>(scale);
        if let Err(e) = w.set_position(LogicalPosition::new(p.x, p.y + s.height)) {
            tracing::debug!("position {label}: {e}");
        }
    }
}

/// The recording menu below the tray icon; a second click closes it.
pub fn show_recording_menu(core: &Arc<Core>) {
    if let Some(w) = core.app.get_webview_window("traymenu")
        && w.is_visible().unwrap_or(false)
    {
        let _ = w.hide();
        return;
    }
    if let Some(t) = core.tray.get() {
        t.below_icon(&core.app, "traymenu");
    }
    if let Err(e) = core.show_window("traymenu", true) {
        tracing::warn!("tray menu: {e}");
    }
}

pub fn on_item(core: &Arc<Core>, id: &str) -> Reply {
    match id {
        "open" => core.show_window("panel", true).map(|_| Value::Null),
        "start-stop" => core.start_stop(),
        "switch" => {
            if let Some(t) = core.tray.get() {
                t.below_icon(&core.app, "switcher");
            }
            core.show_window("switcher", true).map(|_| Value::Null)
        }
        "pause" => core.toggle_pause(),
        "edit" => core.edit_last(),
        "library" | "diagnostics" | "about" => core.show_window(id, true).map(|_| Value::Null),
        "quit" => {
            core.shutdown(StopReason::Quit, 0);
            Ok(Value::Null)
        }
        other => Err(format!("unknown tray item {other:?}")),
    }
}

pub fn start_timer(core: Arc<Core>) {
    let spawn = std::thread::Builder::new()
        .name("tray-timer".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if core.phase().is_recording() {
                    core.refresh_tray();
                }
            }
        });
    if let Err(e) = spawn {
        tracing::warn!("tray timer: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_text() {
        assert_eq!(format_elapsed(0), "0:00");
        assert_eq!(format_elapsed(61_500_000_000), "1:01");
        assert_eq!(format_elapsed(3_723_000_000_000), "1:02:03");
        assert_eq!(format_elapsed(-5), "0:00");
    }

    #[test]
    fn items_by_label() {
        assert_eq!(item_id("Switch Source…"), Some("switch"));
        assert_eq!(item_id("Switch Source"), Some("switch"));
        assert_eq!(item_id("Stop Recording"), Some("start-stop"));
        assert_eq!(item_id("Cancel Countdown"), Some("start-stop"));
        assert_eq!(item_id("quit"), Some("quit"));
        assert_eq!(item_id("Frob"), None);
    }
}
