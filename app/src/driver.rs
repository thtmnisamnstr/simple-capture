//! Debug app driver (plan §5.4), debug builds only. `SC_APP_SCRIPT=<file>` runs a line-based
//! script after `SC_READY`, calling the same functions the commands, tray and hotkey use. Results
//! go to stderr as JSON lines and the app exits with the script's status. No synthetic OS input
//! events are ever posted.

use std::sync::Arc;
use std::time::Duration;

use sc_core::journal::StopReason;
use sc_geometry::RectPt;
use sc_platform::SourceEvent;
use serde_json::{Value, json};

use crate::core::{Core, Reply};
use crate::dto::SourceSpecDto;

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Wait(u64),
    Cmd { name: String, json: String },
    TrayOpenMenu,
    TrayItem(String),
    HotkeySwitch,
    RegionSet { display: String, rect: String },
    Pick { kind: String, id: String },
    Show(String),
    LogWindowRects,
    Sim(String),
    Quit,
}

pub fn parse_line(line: &str) -> Result<Option<Step>, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let (head, rest) = line.split_once(' ').unwrap_or((line, ""));
    let rest = rest.trim();
    let step = match head {
        "wait" => Step::Wait(rest.parse().map_err(|_| format!("wait: bad ms {rest:?}"))?),
        "cmd" => {
            let (name, json) = rest.split_once(' ').unwrap_or((rest, "{}"));
            if name.is_empty() {
                return Err(format!("cmd: missing NAME in {line:?}"));
            }
            Step::Cmd {
                name: name.to_string(),
                json: json.to_string(),
            }
        }
        "tray" if rest == "open-menu" => Step::TrayOpenMenu,
        "tray" if rest.starts_with("item ") => {
            Step::TrayItem(rest["item ".len()..].trim().trim_matches('"').to_string())
        }
        "hotkey" if rest == "switch" => Step::HotkeySwitch,
        "region" if rest.starts_with("set ") => {
            let mut it = rest["set ".len()..].split_whitespace();
            match (it.next(), it.next()) {
                (Some(d), Some(r)) => Step::RegionSet {
                    display: d.to_string(),
                    rect: r.to_string(),
                },
                _ => return Err(format!("region set: expected DISPLAY X,Y,W,H in {line:?}")),
            }
        }
        "pick" => {
            let mut it = rest.split_whitespace();
            match (it.next(), it.next()) {
                (Some(k @ ("window" | "display")), Some(id)) => Step::Pick {
                    kind: k.to_string(),
                    id: id.to_string(),
                },
                _ => return Err(format!("pick: expected window ID|display ID in {line:?}")),
            }
        }
        "show" if !rest.is_empty() => Step::Show(rest.to_string()),
        "log-window-rects" => Step::LogWindowRects,
        "sim" if rest == "sleep" || rest == "wake" => Step::Sim(rest.to_string()),
        "quit" => Step::Quit,
        _ => return Err(format!("unknown step {line:?}")),
    };
    Ok(Some(step))
}

pub fn parse_rect(s: &str) -> Result<RectPt, String> {
    let v: Vec<f64> = s
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("bad rect {s:?}: expected X,Y,W,H"))?;
    match v[..] {
        [x, y, w, h] if w > 0.0 && h > 0.0 => Ok(RectPt { x, y, w, h }),
        _ => Err(format!("bad rect {s:?}: expected X,Y,W,H with W,H > 0")),
    }
}

fn report(n: usize, line: &str, r: &Reply) {
    let v = match r {
        Ok(detail) => json!({"step": n, "line": line, "ok": true, "result": detail}),
        Err(e) => json!({"step": n, "line": line, "ok": false, "error": e}),
    };
    eprintln!("{v}");
}

fn run_step(core: &Arc<Core>, step: Step) -> Reply {
    match step {
        Step::Wait(ms) => {
            std::thread::sleep(Duration::from_millis(ms));
            Ok(Value::Null)
        }
        Step::Cmd { name, json } => {
            let args: Value = serde_json::from_str(&json)
                .map_err(|e| format!("cmd {name}: bad JSON {json:?}: {e}"))?;
            crate::commands::dispatch(core, &name, &args)
        }
        Step::TrayOpenMenu => {
            let t = core.tray.get().ok_or("no tray")?;
            t.open_menu(Duration::from_millis(400))?;
            Ok(Value::Null)
        }
        Step::TrayItem(label) => {
            let id =
                crate::tray::item_id(&label).ok_or_else(|| format!("no tray item {label:?}"))?;
            crate::tray::on_item(core, id)
        }
        Step::HotkeySwitch => core.hotkey_switch(),
        Step::RegionSet { display, rect } => core.region_selected(&display, parse_rect(&rect)?),
        Step::Pick { kind, id } => {
            let source = if kind == "window" {
                SourceSpecDto::Window {
                    window_id: id.parse().map_err(|_| format!("bad window id {id:?}"))?,
                }
            } else {
                SourceSpecDto::Display { display_id: id }
            };
            core.source_picked(source)
        }
        Step::Show(label) => core.show_window(&label, false).map(|_| Value::Null),
        Step::LogWindowRects => Ok(core.window_rects()),
        Step::Sim(what) => {
            core.on_system_event(if what == "sleep" {
                SourceEvent::SystemWillSleep
            } else {
                SourceEvent::SystemDidWake
            });
            Ok(Value::Null)
        }
        Step::Quit => Ok(Value::Null),
    }
}

/// Starts the script thread when `SC_APP_SCRIPT` is set; returns whether it did.
pub fn maybe_start(core: Arc<Core>) -> bool {
    let Ok(path) = std::env::var("SC_APP_SCRIPT") else {
        return false;
    };
    let spawned = std::thread::Builder::new()
        .name("driver".into())
        .spawn(move || {
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    report(0, "", &Err(format!("read {path}: {e}")));
                    core.shutdown(StopReason::Quit, 2);
                    return;
                }
            };
            let mut status = 0;
            for (i, line) in text.lines().enumerate() {
                let n = i + 1;
                match parse_line(line) {
                    Ok(None) => {}
                    Ok(Some(Step::Quit)) => {
                        report(n, line, &Ok(Value::Null));
                        break;
                    }
                    Ok(Some(step)) => {
                        let r = run_step(&core, step);
                        if r.is_err() {
                            status = 1;
                        }
                        report(n, line, &r);
                    }
                    Err(e) => {
                        report(n, line, &Err(e));
                        status = 2;
                        break;
                    }
                }
            }
            core.shutdown(StopReason::Quit, status);
        });
    if let Err(e) = spawned {
        eprintln!("driver: thread: {e}");
        return false;
    }
    true
}

pub fn active() -> bool {
    std::env::var_os("SC_APP_SCRIPT").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_step() {
        let script = "wait 100\ncmd get_permissions {}\ntray open-menu\ntray item \"Switch Source…\"\n\
                      hotkey switch\nregion set 1 10,10,640,360\npick window 42\nshow bar\n\
                      log-window-rects\nsim sleep\n# comment\n\nquit\n";
        let steps: Vec<Step> = script
            .lines()
            .filter_map(|l| parse_line(l).expect("parses"))
            .collect();
        assert_eq!(steps.len(), 11);
        assert_eq!(
            steps[1],
            Step::Cmd {
                name: "get_permissions".into(),
                json: "{}".into()
            }
        );
        assert_eq!(steps[3], Step::TrayItem("Switch Source…".to_string()));
        assert_eq!(
            steps[5],
            Step::RegionSet {
                display: "1".into(),
                rect: "10,10,640,360".into()
            }
        );
        assert_eq!(steps[10], Step::Quit);
        assert_eq!(
            parse_line("cmd list_displays"),
            Ok(Some(Step::Cmd {
                name: "list_displays".into(),
                json: "{}".into()
            }))
        );
        assert_eq!(
            parse_line(r#"cmd begin_region_select {"purpose":"start","display_id":"1"}"#),
            Ok(Some(Step::Cmd {
                name: "begin_region_select".into(),
                json: r#"{"purpose":"start","display_id":"1"}"#.into()
            }))
        );
    }

    #[test]
    fn rejects_bad_lines() {
        for bad in [
            "frob",
            "wait x",
            "cmd",
            "region set 1",
            "pick app 3",
            "sim nap",
            "show",
            "tray shake",
        ] {
            assert!(parse_line(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rects() {
        assert_eq!(
            parse_rect("10, 20,640,360"),
            Ok(RectPt {
                x: 10.0,
                y: 20.0,
                w: 640.0,
                h: 360.0
            })
        );
        assert!(parse_rect("1,2,3").is_err());
        assert!(parse_rect("1,2,0,5").is_err());
        assert!(parse_rect("a,b,c,d").is_err());
    }
}
