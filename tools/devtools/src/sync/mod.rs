//! `sc-devtools sync`: the flash + beep (+ camera) + clap sync test (M1 plan §8.3).
//!
//! ```text
//! sync flashbeep --schedule T[:LABEL],... [--fullscreen-at T[:LABEL],...] --out LOG.json
//!                [--start-on-signal] [--pos X,Y] [--size WxH] [--linger S]
//! sync analyze PROJECT --log LOG.json [--json OUT.json] [--dump DIR (screen.csv, camera.csv)]
//! sync summarize REPORT.json...
//! sync clap PROJECT [--from S] [--to S] [--json OUT.json]
//! ```
//! Offsets read "A→B" = onset of B − onset of A, in ms: positive means B comes later.

mod detect;
mod report;

#[cfg(target_os = "macos")]
mod analyze;
#[cfg(target_os = "macos")]
mod clap;
#[cfg(target_os = "macos")]
mod flashbeep;

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

pub const TONE_MS: u64 = 100;
pub const TARGET_MS: f64 = 33.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Flash,
    Fullscreen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedEvent {
    pub kind: EventKind,
    pub label: String,
    pub t_s: f64,
    pub host_ns: i64,
    pub player_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashLog {
    pub title: String,
    pub pid: u32,
    pub window_id: isize,
    pub rect_pt: [f64; 4],
    pub screen_pt: [f64; 2],
    pub tone_hz: f64,
    pub tone_ms: u64,
    pub flash_ms: u64,
    pub fullscreen_ms: u64,
    pub t0_host_ns: Option<i64>,
    pub events: Vec<IssuedEvent>,
}

pub struct Opts(Vec<String>);

impl Opts {
    pub fn get(&self, name: &str) -> Option<&str> {
        let key = format!("--{name}");
        self.0
            .iter()
            .position(|a| *a == key)
            .and_then(|i| self.0.get(i + 1))
            .map(String::as_str)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.0.iter().any(|a| *a == format!("--{name}"))
    }

    pub fn num(&self, name: &str) -> Result<Option<f64>> {
        self.get(name)
            .map(|v| {
                v.parse()
                    .map_err(|_| anyhow!("--{name} {v:?}: not a number"))
            })
            .transpose()
    }

    pub fn pair(&self, name: &str, sep: char) -> Result<Option<(f64, f64)>> {
        let Some(v) = self.get(name) else {
            return Ok(None);
        };
        let (a, b) = v
            .split_once(sep)
            .ok_or_else(|| anyhow!("--{name} {v:?}: expected A{sep}B"))?;
        Ok(Some((a.trim().parse()?, b.trim().parse()?)))
    }

    pub fn positional(&self) -> Vec<&str> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.0.len() {
            let a = &self.0[i];
            if a.starts_with("--") {
                let takes_value = !matches!(a.as_str(), "--start-on-signal");
                i += if takes_value { 2 } else { 1 };
            } else {
                out.push(a.as_str());
                i += 1;
            }
        }
        out
    }
}

pub fn parse_schedule(s: &str) -> Result<Vec<(f64, String)>> {
    let mut out = Vec::new();
    for item in s.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let (t, label) = match item.split_once(':') {
            Some((t, l)) => (t, l.to_string()),
            None => (item, String::new()),
        };
        let t: f64 = t
            .parse()
            .map_err(|_| anyhow!("schedule item {item:?}: expected SECONDS[:LABEL]"))?;
        if !t.is_finite() || t < 0.0 {
            bail!("schedule item {item:?}: time must be >= 0");
        }
        let label = if label.is_empty() {
            format!("t{t}")
        } else {
            label
        };
        out.push((t, label));
    }
    Ok(out)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
pub fn host_now_ns() -> i64 {
    let mut tb = libc::mach_timebase_info { numer: 0, denom: 0 };
    let rc = unsafe { libc::mach_timebase_info(&mut tb) };
    let (n, d) = if rc != 0 || tb.denom == 0 {
        (1, 1)
    } else {
        (tb.numer, tb.denom)
    };
    let t = unsafe { libc::mach_absolute_time() };
    i64::try_from(u128::from(t) * u128::from(n) / u128::from(d)).unwrap_or(i64::MAX)
}

const USAGE: &str = "usage: sc-devtools sync <flashbeep|analyze|summarize|clap> [args...]";

pub fn run(args: &[String]) -> u8 {
    let Some((cmd, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    let o = Opts(rest.to_vec());
    let r = match cmd.as_str() {
        "summarize" => report::summarize_files(&o.positional()),
        "flashbeep" | "analyze" | "clap" => mac(cmd, &o),
        "-h" | "--help" => {
            println!("{USAGE}");
            return 0;
        }
        other => {
            eprintln!("sc-devtools sync: unknown command {other:?}\n{USAGE}");
            return 2;
        }
    };
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("sc-devtools sync {cmd}: {e:#}");
            1
        }
    }
}

#[cfg(target_os = "macos")]
fn mac(cmd: &str, o: &Opts) -> Result<()> {
    match cmd {
        "flashbeep" => flashbeep::run(o),
        "analyze" => analyze::run(o),
        _ => clap::run(o),
    }
}

#[cfg(not(target_os = "macos"))]
fn mac(cmd: &str, _o: &Opts) -> Result<()> {
    bail!("{cmd} is macOS only in M1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_parses_labels_and_defaults() {
        let s = parse_schedule("2:start, 38.5:after-switch-1,66").expect("parse");
        assert_eq!(
            s,
            vec![
                (2.0, "start".to_string()),
                (38.5, "after-switch-1".to_string()),
                (66.0, "t66".to_string())
            ]
        );
        assert!(parse_schedule("x:start").is_err());
        assert!(parse_schedule("-1").is_err());
        assert!(parse_schedule("").expect("empty").is_empty());
    }

    #[test]
    fn positional_skips_option_values() {
        let o = Opts(
            ["P", "--log", "L", "--start-on-signal", "Q"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(o.positional(), vec!["P", "Q"]);
        assert_eq!(o.get("log"), Some("L"));
        assert!(o.flag("start-on-signal"));
    }
}
