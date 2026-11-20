//! Hand-written argument parser (ported in spirit from spike 3's `Args`; no clap).
// Fields are consumed by the command bodies wp-cli writes.
#![allow(dead_code)]

use std::path::PathBuf;

pub const USAGE: &str = "\
usage: simple-capture <command> [options]   (all commands accept --json and --platform native|synthetic)

  sources                    displays, windows, apps (needs the Screen Recording grant)
  displays                   displays only, never needs a permission
  devices                    cameras and mics
  perms                      permission status (never prompts)
  record --source SPEC [--duration S] [--switch T=SPEC]...
         [--fps 30|60] [--resolution native|1x] [--no-cursor] [--clicks]
         [--camera NAME|default|none] [--mic NAME|default|none] [--system-audio off|all|app]
         [--aspect original|16:9|4:3|1:1|9:16] [--background BG] [--title T]
         [--projects-dir DIR] [--events] [--own-test-window T=SECONDS]
  recover [--projects-dir DIR]
  export PROJECT [--out FILE] [--size reference|2160|1440|1080|720|480] [--fps N]
  validate FILE --size WxH --fps N --duration S [--audio] [--vcodec h264]
  probe [--refresh]
  --self-test

SPEC := display[:ID|main|N] | window:ID | window-title:SUBSTRING
        | app:BUNDLE_ID[@DISPLAY] | app-pid:PID[@DISPLAY]
        | region:DISPLAY:X,Y,W,H | synthetic:NAME[:WxH]
BG   := solid:#RRGGBB | linear:DEG:#RRGGBB@0,#RRGGBB@1[,...] | radial:CX,CY,R:#RRGGBB@0,... | image:PATH";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformChoice {
    Native,
    Synthetic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub json: bool,
    pub platform: PlatformChoice,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Sources,
    Displays,
    Devices,
    Perms,
    Record(Box<RecordArgs>),
    Recover { projects_dir: Option<PathBuf> },
    Export(ExportArgs),
    Validate(ValidateArgs),
    Probe { refresh: bool },
    SelfTest,
    Help,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordArgs {
    /// Raw SPEC text; resolved against the platform by the record command.
    pub source: String,
    pub duration_s: Option<f64>,
    /// (seconds after start, SPEC)
    pub switches: Vec<(f64, String)>,
    pub fps: u32,
    pub resolution: String,
    pub cursor: bool,
    pub clicks: bool,
    pub camera: String,
    pub mic: String,
    pub system_audio: String,
    pub aspect: String,
    pub background: Option<String>,
    pub title: Option<String>,
    pub projects_dir: Option<PathBuf>,
    pub events: bool,
    /// (title, seconds); debug builds only.
    pub own_test_windows: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportArgs {
    pub project: PathBuf,
    pub out: Option<PathBuf>,
    pub size: String,
    pub fps: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidateArgs {
    pub file: PathBuf,
    pub size: (u32, u32),
    pub fps: u32,
    pub duration_s: f64,
    pub audio: bool,
    pub vcodec: String,
}

struct Cursor<'a> {
    args: &'a [String],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn next(&mut self) -> Option<&'a str> {
        let a = self.args.get(self.i)?;
        self.i += 1;
        Some(a.as_str())
    }

    fn value(&mut self, flag: &str) -> Result<&'a str, String> {
        self.next().ok_or_else(|| format!("{flag} needs a value"))
    }
}

fn num<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, String> {
    v.parse().map_err(|_| format!("{flag}: cannot parse {v:?}"))
}

fn one_of<'a>(flag: &str, v: &'a str, allowed: &[&str]) -> Result<&'a str, String> {
    if allowed.contains(&v) {
        Ok(v)
    } else {
        Err(format!("{flag}: expected one of {}", allowed.join("|")))
    }
}

fn split_eq<'a>(flag: &str, v: &'a str) -> Result<(&'a str, &'a str), String> {
    v.split_once('=')
        .ok_or_else(|| format!("{flag}: expected KEY=VALUE, got {v:?}"))
}

pub fn parse(argv: &[String]) -> Result<Invocation, String> {
    let mut json = false;
    let mut platform = PlatformChoice::Native;
    let mut rest = Vec::new();
    let mut c = Cursor { args: argv, i: 0 };
    while let Some(a) = c.next() {
        match a {
            "--json" => json = true,
            "--platform" => {
                platform = match c.value(a)? {
                    "native" => PlatformChoice::Native,
                    "synthetic" => PlatformChoice::Synthetic,
                    other => return Err(format!("--platform: unknown {other:?}")),
                }
            }
            _ => rest.push(a.to_string()),
        }
    }
    let command = parse_command(&rest)?;
    Ok(Invocation {
        json,
        platform,
        command,
    })
}

fn parse_command(argv: &[String]) -> Result<Command, String> {
    let mut c = Cursor { args: argv, i: 0 };
    let Some(cmd) = c.next() else {
        return Err("missing command".to_string());
    };
    let no_more = |c: &mut Cursor| match c.next() {
        Some(a) => Err(format!("unexpected argument {a:?}")),
        None => Ok(()),
    };
    match cmd {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "--self-test" => no_more(&mut c).map(|_| Command::SelfTest),
        "sources" => no_more(&mut c).map(|_| Command::Sources),
        "displays" => no_more(&mut c).map(|_| Command::Displays),
        "devices" => no_more(&mut c).map(|_| Command::Devices),
        "perms" => no_more(&mut c).map(|_| Command::Perms),
        "record" => parse_record(&mut c).map(|r| Command::Record(Box::new(r))),
        "recover" => {
            let mut projects_dir = None;
            while let Some(a) = c.next() {
                match a {
                    "--projects-dir" => projects_dir = Some(PathBuf::from(c.value(a)?)),
                    other => return Err(format!("recover: unknown argument {other:?}")),
                }
            }
            Ok(Command::Recover { projects_dir })
        }
        "export" => {
            let mut project = None;
            let mut out = None;
            let mut size = "reference".to_string();
            let mut fps = None;
            while let Some(a) = c.next() {
                match a {
                    "--out" => out = Some(PathBuf::from(c.value(a)?)),
                    "--size" => {
                        size = one_of(
                            a,
                            c.value(a)?,
                            &["reference", "2160", "1440", "1080", "720", "480"],
                        )?
                        .to_string()
                    }
                    "--fps" => fps = Some(num(a, c.value(a)?)?),
                    other if other.starts_with("--") => {
                        return Err(format!("export: unknown flag {other:?}"));
                    }
                    other if project.is_none() => project = Some(PathBuf::from(other)),
                    other => return Err(format!("export: unexpected argument {other:?}")),
                }
            }
            let project = project.ok_or("export: missing PROJECT")?;
            Ok(Command::Export(ExportArgs {
                project,
                out,
                size,
                fps,
            }))
        }
        "validate" => {
            let mut file = None;
            let mut size = None;
            let mut fps = None;
            let mut duration_s = None;
            let mut audio = false;
            let mut vcodec = "h264".to_string();
            while let Some(a) = c.next() {
                match a {
                    "--size" => {
                        let v = c.value(a)?;
                        let (w, h) = v
                            .split_once('x')
                            .ok_or_else(|| format!("--size: expected WxH, got {v:?}"))?;
                        size = Some((num(a, w)?, num(a, h)?));
                    }
                    "--fps" => fps = Some(num(a, c.value(a)?)?),
                    "--duration" => duration_s = Some(num(a, c.value(a)?)?),
                    "--audio" => audio = true,
                    "--vcodec" => vcodec = c.value(a)?.to_string(),
                    other if other.starts_with("--") => {
                        return Err(format!("validate: unknown flag {other:?}"));
                    }
                    other if file.is_none() => file = Some(PathBuf::from(other)),
                    other => return Err(format!("validate: unexpected argument {other:?}")),
                }
            }
            Ok(Command::Validate(ValidateArgs {
                file: file.ok_or("validate: missing FILE")?,
                size: size.ok_or("validate: missing --size")?,
                fps: fps.ok_or("validate: missing --fps")?,
                duration_s: duration_s.ok_or("validate: missing --duration")?,
                audio,
                vcodec,
            }))
        }
        "probe" => {
            let mut refresh = false;
            while let Some(a) = c.next() {
                match a {
                    "--refresh" => refresh = true,
                    other => return Err(format!("probe: unknown argument {other:?}")),
                }
            }
            Ok(Command::Probe { refresh })
        }
        other => Err(format!("unknown command {other:?}")),
    }
}

fn parse_record(c: &mut Cursor) -> Result<RecordArgs, String> {
    let mut r = RecordArgs {
        source: String::new(),
        duration_s: None,
        switches: Vec::new(),
        fps: 30,
        resolution: "native".to_string(),
        cursor: true,
        clicks: false,
        camera: "none".to_string(),
        mic: "none".to_string(),
        system_audio: "off".to_string(),
        aspect: "original".to_string(),
        background: None,
        title: None,
        projects_dir: None,
        events: false,
        own_test_windows: Vec::new(),
    };
    let mut source = None;
    while let Some(a) = c.next() {
        match a {
            "--source" => source = Some(c.value(a)?.to_string()),
            "--duration" => r.duration_s = Some(num(a, c.value(a)?)?),
            "--switch" => {
                let (t, spec) = split_eq(a, c.value(a)?)?;
                r.switches.push((num(a, t)?, spec.to_string()));
            }
            "--fps" => r.fps = num(a, one_of(a, c.value(a)?, &["30", "60"])?)?,
            "--resolution" => r.resolution = one_of(a, c.value(a)?, &["native", "1x"])?.to_string(),
            "--no-cursor" => r.cursor = false,
            "--clicks" => r.clicks = true,
            "--camera" => r.camera = c.value(a)?.to_string(),
            "--mic" => r.mic = c.value(a)?.to_string(),
            "--system-audio" => {
                r.system_audio = one_of(a, c.value(a)?, &["off", "all", "app"])?.to_string()
            }
            "--aspect" => {
                r.aspect =
                    one_of(a, c.value(a)?, &["original", "16:9", "4:3", "1:1", "9:16"])?.to_string()
            }
            "--background" => r.background = Some(c.value(a)?.to_string()),
            "--title" => r.title = Some(c.value(a)?.to_string()),
            "--projects-dir" => r.projects_dir = Some(PathBuf::from(c.value(a)?)),
            "--events" => r.events = true,
            "--own-test-window" => {
                let (t, secs) = split_eq(a, c.value(a)?)?;
                r.own_test_windows.push((t.to_string(), num(a, secs)?));
            }
            other => return Err(format!("record: unknown argument {other:?}")),
        }
    }
    r.source = source.ok_or("record: missing --source")?;
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Invocation, String> {
        let v: Vec<String> = s.split_whitespace().map(String::from).collect();
        parse(&v)
    }

    #[test]
    fn record_with_switches() {
        let inv = p("--platform synthetic record --source synthetic:a --duration 3 --switch 1.5=synthetic:b --json")
            .expect("parses");
        assert!(inv.json);
        assert_eq!(inv.platform, PlatformChoice::Synthetic);
        let Command::Record(r) = inv.command else {
            panic!("not record")
        };
        assert_eq!(r.source, "synthetic:a");
        assert_eq!(r.duration_s, Some(3.0));
        assert_eq!(r.switches, vec![(1.5, "synthetic:b".to_string())]);
    }

    #[test]
    fn usage_errors() {
        assert!(p("").is_err());
        assert!(p("record").is_err());
        assert!(p("record --source x --fps 25").is_err());
        assert!(p("validate f.mp4 --fps 30").is_err());
        assert!(p("frobnicate").is_err());
    }

    #[test]
    fn validate_and_export() {
        let inv =
            p("validate out.mp4 --size 1920x1080 --fps 30 --duration 2.5 --audio").expect("parses");
        assert!(matches!(
            inv.command,
            Command::Validate(ValidateArgs {
                size: (1920, 1080),
                ..
            })
        ));
        let inv = p("export proj.sc --size 1080").expect("parses");
        assert!(matches!(inv.command, Command::Export(_)));
        assert_eq!(p("--self-test").expect("parses").command, Command::SelfTest);
    }
}
