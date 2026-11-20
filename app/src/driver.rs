//! Debug app driver (M1 plan §5.4), debug builds only. `SC_APP_SCRIPT=<file>` runs a line-based
//! script after `SC_READY`, calling the same functions the commands, tray and hotkey use. Results
//! go to stderr as JSON lines and the app exits with the script's status. Skeleton until
//! wp-app-shell: `wait` and `quit` work, every other step reports "not implemented yet". No
//! synthetic OS input events are ever posted.

use std::time::Duration;

use tauri::AppHandle;

#[allow(dead_code)] // fields are consumed by wp-app-shell's step runner
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

fn report(n: usize, ok: bool, detail: &str) {
    let detail = serde_json::to_string(detail).unwrap_or_else(|_| "\"\"".to_string());
    eprintln!("{{\"step\":{n},\"ok\":{ok},\"detail\":{detail}}}");
}

pub fn maybe_start(app: AppHandle) {
    let Ok(path) = std::env::var("SC_APP_SCRIPT") else {
        return;
    };
    std::thread::spawn(move || {
        eprintln!("SC_READY");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                report(0, false, &format!("read {path}: {e}"));
                app.exit(2);
                return;
            }
        };
        let mut status = 0;
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            match parse_line(line) {
                Ok(None) => {}
                Ok(Some(Step::Wait(ms))) => {
                    std::thread::sleep(Duration::from_millis(ms));
                    report(n, true, "wait");
                }
                Ok(Some(Step::Quit)) => {
                    report(n, true, "quit");
                    break;
                }
                Ok(Some(step)) => {
                    report(n, false, &format!("{step:?}: not implemented yet"));
                    status = 1;
                }
                Err(e) => {
                    report(n, false, &e);
                    status = 2;
                    break;
                }
            }
        }
        app.exit(status);
    });
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
        assert_eq!(steps[3], Step::TrayItem("Switch Source…".to_string()));
        assert_eq!(steps[10], Step::Quit);
        assert!(parse_line("frob").is_err());
    }
}
