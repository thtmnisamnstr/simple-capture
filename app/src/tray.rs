//! Status item (plan §5.3): Open Recorder · Start/Stop Recording · Switch Source… · Quit, with
//! the elapsed time as its title while recording.

use std::sync::Arc;
use std::time::Duration;

use sc_core::journal::StopReason;
use serde_json::Value;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, LogicalPosition, Manager};

use crate::core::{Core, Reply};
use crate::state::Phase;

pub const ITEMS: [(&str, &str); 4] = [
    ("open", "Open Recorder"),
    ("start-stop", "Start Recording"),
    ("switch", "Switch Source…"),
    ("quit", "Quit"),
];

pub struct Tray {
    icon: TrayIcon,
    start_stop: MenuItem<tauri::Wry>,
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

pub fn item_id(label_or_id: &str) -> Option<&'static str> {
    let l = label_or_id.trim();
    ITEMS.iter().find_map(|(id, label)| {
        let plain = label.trim_end_matches('…');
        (l == *id
            || l == *label
            || l == plain
            || l.eq_ignore_ascii_case("Stop Recording") && *id == "start-stop")
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
        let png = sc_text::decode_image(include_bytes!("../icons/tray-template@2x.png"))
            .map_err(|e| format!("tray icon: {e}"))?;
        let image = Image::new_owned(png.data, png.size.w, png.size.h);
        let icon = TrayIconBuilder::with_id("main")
            .icon(image)
            .icon_as_template(true)
            .tooltip("Simple Capture")
            .menu(&menu)
            .show_menu_on_left_click(true)
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
        Ok(Tray { icon, start_stop })
    }

    pub fn refresh(&self, phase: &Phase, elapsed_ns: i64) {
        let recording = phase.is_recording();
        let title = recording.then(|| format_elapsed(elapsed_ns));
        if let Err(e) = self.icon.set_title(title) {
            tracing::debug!("tray title: {e}");
        }
        let text = if recording {
            "Stop Recording"
        } else {
            "Start Recording"
        };
        if let Err(e) = self.start_stop.set_text(text) {
            tracing::debug!("tray item: {e}");
        }
    }

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

    /// Opens the menu in-process (no synthetic input) and closes it after `hold`.
    pub fn open_menu(&self, hold: Duration) -> Result<(), String> {
        let item = self.status_item();
        let icon = self.icon.clone();
        let opener = std::thread::spawn(move || icon.with_inner_tray_icon(|t| t.show_menu()));
        std::thread::sleep(hold);
        #[cfg(target_os = "macos")]
        crate::objc::cancel_status_item_menu(item);
        #[cfg(not(target_os = "macos"))]
        let _ = item;
        opener
            .join()
            .map_err(|_| "tray menu thread panicked".to_string())?
            .map_err(|e| format!("tray menu: {e}"))
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

pub fn on_item(core: &Arc<Core>, id: &str) -> Reply {
    match id {
        "open" => core.show_window("panel", true).map(|_| Value::Null),
        "start-stop" => {
            if core.phase().is_recording() {
                core.stop_recording()
            } else {
                core.tray_start()
            }
        }
        "switch" => {
            if let Some(t) = core.tray.get() {
                t.below_icon(&core.app, "switcher");
            }
            core.show_window("switcher", true).map(|_| Value::Null)
        }
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
        assert_eq!(item_id("quit"), Some("quit"));
        assert_eq!(item_id("Frob"), None);
    }
}
