//! `sc-devtools soak`: the long-run test behind `just soak N` (M1 plan §8.3 wp-soak-test).
//!
//! ```text
//! soak run --minutes N --cli PATH --runsh PATH --work DIR [--switch-every S] [--keep]
//!          [--camera NAME|default|none] [--mic NAME|default|none] [--system-audio off|all]
//! soak report WORK      re-analyse a run kept with --keep
//! soak flash [--size WxH]
//! ```
//! `run` records display:main (native, 30 fps) with camera, mic and system audio through the dev
//! host, switches display ↔ the flash window every S s (300), flashes + beeps at 5 s and at
//! N − 1 min, samples CPU and RSS every 5 s, then writes run.json, report.json and summary.md
//! into WORK, prints the summary and deletes the recording unless --keep.
//! Exit codes: 0 every threshold passed, 1 a threshold failed or an error, 2 usage, 3 not macOS.

mod driver;
#[cfg(target_os = "macos")]
mod flash;
#[cfg(target_os = "macos")]
mod media;
mod procstat;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod stats;

use std::path::PathBuf;

const USAGE: &str = "usage: sc-devtools soak run --minutes N --cli PATH --runsh PATH --work DIR \
[--switch-every S] [--keep] [--camera C] [--mic M] [--system-audio off|all] | soak report WORK \
| soak flash [--size WxH]";

fn get<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn parse_run(args: &[String]) -> Result<driver::RunArgs, String> {
    let need = |n: &str| get(args, n).ok_or_else(|| format!("{n} is required"));
    let num = |n: &str, v: &str| {
        v.parse::<f64>()
            .ok()
            .filter(|x| x.is_finite() && *x > 0.0)
            .ok_or_else(|| format!("{n} {v:?}: expected a positive number"))
    };
    let minutes = num("--minutes", need("--minutes")?)?;
    let switch_every_s = match get(args, "--switch-every") {
        Some(v) => num("--switch-every", v)?,
        None => 300.0,
    };
    Ok(driver::RunArgs {
        minutes,
        switch_every_s,
        cli: PathBuf::from(need("--cli")?),
        runsh: PathBuf::from(need("--runsh")?),
        work: PathBuf::from(need("--work")?),
        keep: args.iter().any(|a| a == "--keep"),
        camera: get(args, "--camera").unwrap_or("default").to_string(),
        mic: get(args, "--mic").unwrap_or("default").to_string(),
        system_audio: get(args, "--system-audio").unwrap_or("all").to_string(),
    })
}

fn verdict(r: anyhow::Result<bool>) -> u8 {
    match r {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("sc-devtools soak: {e:#}");
            1
        }
    }
}

pub fn run(args: &[String]) -> u8 {
    let Some((cmd, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    match cmd.as_str() {
        "run" | "report" | "flash" if !cfg!(target_os = "macos") => {
            eprintln!("sc-devtools soak: macOS only in M1");
            3
        }
        "run" => match parse_run(rest) {
            Ok(a) => verdict(driver::run(&a)),
            Err(e) => {
                eprintln!("sc-devtools soak: {e}\n{USAGE}");
                2
            }
        },
        "report" => match rest.first() {
            Some(w) => verdict(driver::report(&PathBuf::from(w))),
            None => {
                eprintln!("{USAGE}");
                2
            }
        },
        "flash" => flash_cmd(rest),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

#[cfg(target_os = "macos")]
fn flash_cmd(args: &[String]) -> u8 {
    match flash::run(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("sc-devtools soak flash: {e:#}");
            1
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn flash_cmd(_args: &[String]) -> u8 {
    3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_run_args() {
        let a = parse_run(&s(&[
            "--minutes",
            "30",
            "--cli",
            "/c",
            "--runsh",
            "/r",
            "--work",
            "/w",
            "--keep",
            "--mic",
            "none",
        ]))
        .expect("parse");
        assert_eq!(a.minutes, 30.0);
        assert_eq!(a.switch_every_s, 300.0);
        assert!(a.keep);
        assert_eq!(a.mic, "none");
        assert_eq!(a.camera, "default");
        assert_eq!(a.system_audio, "all");
        assert!(
            parse_run(&s(&[
                "--minutes",
                "0",
                "--cli",
                "c",
                "--runsh",
                "r",
                "--work",
                "w"
            ]))
            .is_err()
        );
        assert!(parse_run(&s(&["--cli", "c"])).is_err());
    }

    #[test]
    fn usage_errors() {
        assert_eq!(run(&[]), 2);
        assert_eq!(run(&s(&["nope"])), 2);
        if cfg!(target_os = "macos") {
            assert_eq!(run(&s(&["run", "--minutes", "x"])), 2);
            assert_eq!(run(&s(&["report"])), 2);
        }
    }
}
