use sc_geometry::RectPt;
use sc_platform::{AppInfo, WindowId, WindowInfo};

use super::content::app_content_layer;

/// The pointer (28×40), status items, the menu bar, tooltips and status bubbles are all smaller
/// on one side; a titled window's title bar alone is 28 pt tall.
pub(crate) const MIN_SIDE_PT: f64 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivationPolicy {
    Regular,
    Accessory,
    Prohibited,
}

#[derive(Debug, Clone)]
pub(crate) struct WindowFacts<'a> {
    pub id: u32,
    pub pid: i32,
    pub bundled: bool,
    /// `None` when the owner is not an application (WindowServer).
    pub policy: Option<ActivationPolicy>,
    pub layer: isize,
    pub on_screen: bool,
    pub frame_pt: RectPt,
    pub title: &'a str,
}

/// Regular apps, plus unbundled programs that show windows under the accessory policy (the
/// devtools test fixtures). Every system agent and menu-bar app is bundled.
pub(crate) fn listable_owner(policy: Option<ActivationPolicy>, bundled: bool) -> bool {
    match policy {
        Some(ActivationPolicy::Regular) => true,
        Some(ActivationPolicy::Accessory) => !bundled,
        Some(ActivationPolicy::Prohibited) | None => false,
    }
}

fn overlaps(a: RectPt, b: RectPt) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

pub(crate) fn listable_window(
    w: &WindowFacts,
    our_pid: i32,
    hide: &[WindowId],
    displays_pt: &[RectPt],
) -> bool {
    w.pid != our_pid
        && !hide.iter().any(|h| h.0 == w.id)
        && listable_owner(w.policy, w.bundled)
        && w.on_screen
        && app_content_layer(w.layer)
        && w.frame_pt.w >= MIN_SIDE_PT
        && w.frame_pt.h >= MIN_SIDE_PT
        && displays_pt.iter().any(|d| overlaps(w.frame_pt, *d))
        && !w.title.trim().is_empty()
}

/// Apps with at least one listed window, by name.
pub(crate) fn listed_apps(apps: Vec<AppInfo>, windows: &[WindowInfo]) -> Vec<AppInfo> {
    let mut out: Vec<AppInfo> = apps
        .into_iter()
        .filter(|a| windows.iter().any(|w| w.app.pid == a.app.pid))
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::ActivationPolicy::{Accessory, Prohibited, Regular};
    use super::*;
    use sc_platform::AppRef;

    const ME: i32 = 100;
    const DISPLAY: RectPt = RectPt {
        x: 0.0,
        y: 0.0,
        w: 3840.0,
        h: 1080.0,
    };

    fn rect(x: f64, y: f64, w: f64, h: f64) -> RectPt {
        RectPt { x, y, w, h }
    }

    fn chrome() -> WindowFacts<'static> {
        WindowFacts {
            id: 14761,
            pid: 17321,
            bundled: true,
            policy: Some(Regular),
            layer: 0,
            on_screen: true,
            frame_pt: rect(0.0, 30.0, 1920.0, 949.0),
            title: "Inbox - Gmail",
        }
    }

    fn listed(w: &WindowFacts) -> bool {
        listable_window(w, ME, &[], &[DISPLAY])
    }

    #[test]
    fn normal_window_of_a_regular_app_is_listed() {
        assert!(listed(&chrome()));
    }

    #[test]
    fn floating_panel_of_a_regular_app_is_listed() {
        assert!(listed(&WindowFacts {
            layer: 3,
            frame_pt: rect(200.0, 200.0, 320.0, 240.0),
            title: "Colors",
            ..chrome()
        }));
    }

    #[test]
    fn cursor_window_is_not_listed() {
        assert!(!listed(&WindowFacts {
            id: 4,
            pid: 630,
            bundled: false,
            policy: None,
            layer: 2147483630,
            frame_pt: rect(1000.0, 500.0, 28.0, 40.0),
            title: "Cursor",
            ..chrome()
        }));
    }

    #[test]
    fn status_item_at_layer_25_is_not_listed_even_for_a_regular_app() {
        assert!(!listed(&WindowFacts {
            layer: 25,
            frame_pt: rect(3500.0, 0.0, 48.0, 48.0),
            title: "Item-0",
            ..chrome()
        }));
    }

    #[test]
    fn dock_and_control_center_are_not_listed() {
        let dock = WindowFacts {
            id: 26,
            pid: 1446,
            policy: Some(Accessory),
            layer: 20,
            frame_pt: DISPLAY,
            title: "Dock",
            ..chrome()
        };
        assert!(!listed(&dock));
        let control_center = WindowFacts {
            id: 14,
            pid: 1220,
            policy: Some(Accessory),
            layer: 0,
            frame_pt: rect(3000.0, 40.0, 320.0, 400.0),
            title: "Control Center",
            ..chrome()
        };
        assert!(!listed(&control_center));
        assert!(!listed(&WindowFacts {
            policy: Some(Prohibited),
            ..chrome()
        }));
    }

    #[test]
    fn unbundled_accessory_fixture_is_listed_but_not_a_bundled_or_prohibited_one() {
        let fixture = WindowFacts {
            id: 500,
            pid: 7000,
            bundled: false,
            policy: Some(Accessory),
            layer: 3,
            frame_pt: rect(40.0, 100.0, 640.0, 480.0),
            title: "SC-TEST-RED",
            ..chrome()
        };
        assert!(listed(&fixture));
        assert!(!listed(&WindowFacts {
            bundled: true,
            ..fixture.clone()
        }));
        assert!(!listed(&WindowFacts {
            policy: Some(Prohibited),
            ..fixture
        }));
    }

    #[test]
    fn tiny_windows_are_not_listed() {
        for (w, h) in [(230.0, 22.0), (93.0, 19.0), (39.9, 400.0), (2.0, 2.0)] {
            assert!(
                !listed(&WindowFacts {
                    frame_pt: rect(100.0, 100.0, w, h),
                    ..chrome()
                }),
                "{w}x{h}"
            );
        }
        assert!(listed(&WindowFacts {
            frame_pt: rect(100.0, 100.0, MIN_SIDE_PT, MIN_SIDE_PT),
            ..chrome()
        }));
    }

    #[test]
    fn off_screen_windows_are_not_listed() {
        assert!(!listed(&WindowFacts {
            on_screen: false,
            ..chrome()
        }));
        assert!(!listed(&WindowFacts {
            frame_pt: rect(-20000.0, -20000.0, 800.0, 600.0),
            ..chrome()
        }));
        assert!(listed(&WindowFacts {
            frame_pt: rect(-700.0, 100.0, 800.0, 600.0),
            ..chrome()
        }));
    }

    #[test]
    fn untitled_windows_are_not_listed() {
        for title in ["", "  "] {
            assert!(!listed(&WindowFacts { title, ..chrome() }));
        }
    }

    #[test]
    fn our_own_and_hidden_windows_are_not_listed() {
        assert!(!listed(&WindowFacts {
            pid: ME,
            ..chrome()
        }));
        let w = chrome();
        assert!(!listable_window(&w, ME, &[WindowId(w.id)], &[DISPLAY]));
    }

    fn app(pid: i32, name: &str) -> AppInfo {
        AppInfo {
            app: AppRef {
                pid,
                bundle_id: None,
            },
            name: name.to_string(),
        }
    }

    fn window_of(pid: i32) -> WindowInfo {
        WindowInfo {
            id: WindowId(pid as u32),
            title: "t".into(),
            app_name: String::new(),
            app: AppRef {
                pid,
                bundle_id: None,
            },
            frame_pt: rect(0.0, 0.0, 100.0, 100.0),
            display: None,
            on_screen: true,
            layer: 0,
            z_index: 0,
        }
    }

    #[test]
    fn apps_without_a_listed_window_are_dropped_and_the_rest_sorted_by_name() {
        let apps = vec![
            app(1, "slack"),
            app(2, "Finder"),
            app(3, "Asana"),
            app(4, "Dock"),
        ];
        let got = listed_apps(apps, &[window_of(1), window_of(3), window_of(1)]);
        let names: Vec<&str> = got.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Asana", "slack"]);
    }
}
