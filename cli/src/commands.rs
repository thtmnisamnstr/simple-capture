//! Command bodies (plan §5.1). Exit codes: 0 ok, 1 error, 2 usage, 3 unsupported on this OS.

use std::path::Path;
use std::sync::Arc;

use sc_platform::{PermissionKind, Platform};
use serde_json::{Value, json};

use crate::args::{Command, Invocation, PlatformChoice, USAGE};
use crate::host;
use crate::spec::ResolveError;

pub const EXIT_OK: u8 = 0;
pub const EXIT_ERROR: u8 = 1;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_UNSUPPORTED: u8 = 3;

#[derive(Debug)]
pub struct CliError {
    pub code: u8,
    pub msg: String,
}

impl CliError {
    pub fn error(msg: impl Into<String>) -> Self {
        CliError {
            code: EXIT_ERROR,
            msg: msg.into(),
        }
    }

    pub fn usage(msg: impl Into<String>) -> Self {
        CliError {
            code: EXIT_USAGE,
            msg: msg.into(),
        }
    }

    pub fn unsupported(msg: impl Into<String>) -> Self {
        CliError {
            code: EXIT_UNSUPPORTED,
            msg: msg.into(),
        }
    }

    pub fn context(self, what: &str) -> Self {
        CliError {
            code: self.code,
            msg: format!("{what}: {}", self.msg),
        }
    }
}

impl From<sc_platform::Error> for CliError {
    fn from(e: sc_platform::Error) -> Self {
        match e {
            sc_platform::Error::Unsupported(_) => CliError::unsupported(e.to_string()),
            e => CliError::error(e.to_string()),
        }
    }
}

impl From<sc_record::Error> for CliError {
    fn from(e: sc_record::Error) -> Self {
        match e {
            sc_record::Error::Platform(p) => p.into(),
            sc_record::Error::Unsupported(_)
            | sc_record::Error::Core(sc_core::Error::Unsupported(_)) => {
                CliError::unsupported(e.to_string())
            }
            e => CliError::error(e.to_string()),
        }
    }
}

impl From<sc_core::Error> for CliError {
    fn from(e: sc_core::Error) -> Self {
        sc_record::Error::Core(e).into()
    }
}

impl From<ResolveError> for CliError {
    fn from(e: ResolveError) -> Self {
        match e {
            ResolveError::Usage(m) => CliError::usage(m),
            ResolveError::NotFound(m) => CliError::error(m),
            ResolveError::Platform(p) => p.into(),
        }
    }
}

pub type CliResult = Result<u8, CliError>;

pub fn run(inv: Invocation) -> u8 {
    match dispatch(inv) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("simple-capture: {}", e.msg);
            if e.code == EXIT_USAGE {
                eprintln!();
                eprintln!("{USAGE}");
            }
            e.code
        }
    }
}

fn dispatch(inv: Invocation) -> CliResult {
    let json = inv.json;
    let choice = inv.platform;
    match inv.command {
        Command::Help => {
            println!("{USAGE}");
            Ok(EXIT_OK)
        }
        #[cfg(feature = "export")]
        Command::SelfTest => on_platform(choice, move |p| crate::selftest::run(p, json)),
        #[cfg(not(feature = "export"))]
        Command::SelfTest => {
            eprintln!("simple-capture --self-test needs the export feature");
            Ok(EXIT_UNSUPPORTED)
        }
        Command::Record(r) => crate::record::run(json, choice, *r),
        Command::Sources => on_platform(choice, move |p| sources(&*p, json)),
        Command::Displays => on_platform(choice, move |p| displays(&*p, json)),
        Command::Devices => on_platform(choice, move |p| devices(&*p, json)),
        Command::Perms => on_platform(choice, move |p| perms(&*p, json)),
        Command::Recover { projects_dir } => on_platform(choice, move |p| {
            let dir = match projects_dir {
                Some(d) => d,
                None => p.default_projects_dir()?,
            };
            recover(&*p, &dir, json)
        }),
        cmd @ (Command::Export(_) | Command::Validate(_) | Command::Probe { .. }) => {
            export_family(cmd, choice, json)
        }
    }
}

/// Runs a command body against the chosen platform; on macOS native the body runs off the main
/// thread while the main thread pumps the run loop.
fn on_platform(
    choice: PlatformChoice,
    body: impl FnOnce(Arc<dyn Platform>) -> CliResult + Send + 'static,
) -> CliResult {
    let p = host::platform(choice)?;
    host::run_main(host::needs_main_loop(choice), move || body(p), || {})
}

pub fn print_json(v: &Value) {
    println!("{v}");
}

fn sources(p: &dyn Platform, json: bool) -> CliResult {
    let list = p.screen().list(&[])?;
    if json {
        print_json(&serde_json::to_value(&list).map_err(|e| CliError::error(e.to_string()))?);
        return Ok(EXIT_OK);
    }
    println!("displays:");
    for d in &list.displays {
        println!("  {}", display_line(d));
    }
    println!("windows:");
    for w in &list.windows {
        println!(
            "  window:{:<8} {:<24} {:?}{}{}",
            w.id.0,
            w.app_name,
            w.title,
            if w.on_screen { "" } else { "  (off screen)" },
            if w.layer != 0 {
                format!("  layer {}", w.layer)
            } else {
                String::new()
            }
        );
    }
    println!("apps:");
    for a in &list.apps {
        println!(
            "  app-pid:{:<8} {:<24} {}",
            a.app.pid,
            a.name,
            a.app.bundle_id.as_deref().unwrap_or("-")
        );
    }
    Ok(EXIT_OK)
}

fn display_line(d: &sc_platform::DisplayInfo) -> String {
    format!(
        "display:{:<12} {}×{} px  scale {}  frame {},{} {}×{} pt{}  {:?}",
        d.id.0,
        d.size_px.w,
        d.size_px.h,
        d.scale,
        d.frame_pt.x,
        d.frame_pt.y,
        d.frame_pt.w,
        d.frame_pt.h,
        if d.is_main { "  main" } else { "" },
        d.name
    )
}

fn displays(p: &dyn Platform, json: bool) -> CliResult {
    let ds = p.screen().displays()?;
    if json {
        print_json(&serde_json::to_value(&ds).map_err(|e| CliError::error(e.to_string()))?);
    } else {
        for d in &ds {
            println!("{}", display_line(d));
        }
    }
    Ok(EXIT_OK)
}

fn devices(p: &dyn Platform, json: bool) -> CliResult {
    let cams = p.camera().devices();
    let mics = p.mic().devices();
    let mut first_err: Option<CliError> = None;
    let mut part = |name: &str, r: sc_platform::Result<Vec<sc_platform::CaptureDevice>>| match r {
        Ok(v) => {
            if !json {
                println!("{name}:");
                for d in &v {
                    println!(
                        "  {:<40} {:?}{}",
                        d.id,
                        d.name,
                        if d.is_default { "  default" } else { "" }
                    );
                }
            }
            serde_json::to_value(v).unwrap_or(Value::Null)
        }
        Err(e) => {
            eprintln!("simple-capture devices: {name}: {e}");
            let msg = e.to_string();
            first_err.get_or_insert_with(|| CliError::from(e).context(name));
            json!({ "error": msg })
        }
    };
    let cams = part("cameras", cams);
    let mics = part("mics", mics);
    if json {
        print_json(&json!({ "cameras": cams, "mics": mics }));
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(EXIT_OK),
    }
}

fn status_text(s: sc_platform::PermissionStatus) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{s:?}"))
}

fn perms(p: &dyn Platform, json: bool) -> CliResult {
    let perms = p.permissions();
    let rows = [
        ("screen", PermissionKind::Screen),
        ("camera", PermissionKind::Camera),
        ("microphone", PermissionKind::Microphone),
    ]
    .map(|(name, k)| (name, status_text(perms.status(k))));
    let host = p.permission_host();
    if json {
        let mut v = serde_json::Map::new();
        for (name, s) in &rows {
            v.insert((*name).to_string(), json!(s));
        }
        v.insert("permission_host".to_string(), json!(host));
        v.insert("platform".to_string(), json!(p.name()));
        print_json(&Value::Object(v));
    } else {
        for (name, s) in &rows {
            println!("{name}: {s}");
        }
        println!("permission host: {}", host.as_deref().unwrap_or("-"));
    }
    Ok(EXIT_OK)
}

fn outcome_text(o: sc_record::recovery::Outcome) -> &'static str {
    use sc_record::recovery::Outcome;
    match o {
        Outcome::Recovered => "recovered",
        Outcome::Damaged => "damaged",
        Outcome::AlreadyFinal => "already_final",
        Outcome::Locked => "locked",
        Outcome::NotAProject => "not_a_project",
    }
}

fn recover(p: &dyn Platform, dir: &Path, json: bool) -> CliResult {
    let decoder = p.decoder();
    let reports = sc_record::recovery::scan(dir, &*decoder)
        .map_err(|e| CliError::from(e).context(&format!("recover {}", dir.display())))?;
    if json {
        let v: Vec<Value> = reports
            .iter()
            .map(|r| {
                json!({
                    "dir": r.dir,
                    "outcome": outcome_text(r.outcome),
                    "segments": r.segments,
                    "dropped_segments": r.dropped_segments,
                    "notes": r.notes,
                })
            })
            .collect();
        print_json(&json!({ "projects_dir": dir, "reports": v }));
    } else if reports.is_empty() {
        println!("nothing to recover in {}", dir.display());
    } else {
        for r in &reports {
            println!(
                "{}: {} ({} segments{})",
                r.dir.display(),
                outcome_text(r.outcome),
                r.segments,
                if r.dropped_segments.is_empty() {
                    String::new()
                } else {
                    format!(", dropped {}", r.dropped_segments.join(" "))
                }
            );
            for n in &r.notes {
                println!("  {n}");
            }
        }
    }
    Ok(EXIT_OK)
}

#[cfg(feature = "export")]
fn export_family(cmd: Command, choice: PlatformChoice, json: bool) -> CliResult {
    match cmd {
        Command::Export(a) => crate::export::export(choice, json, a),
        Command::Validate(a) => crate::export::validate(json, a),
        Command::Probe { refresh } => {
            on_platform(choice, move |p| crate::export::probe(&*p, json, refresh))
        }
        _ => Err(CliError::error("not an export command")),
    }
}

#[cfg(not(feature = "export"))]
fn export_family(_cmd: Command, _choice: PlatformChoice, _json: bool) -> CliResult {
    Err(CliError::unsupported(
        "built without the `export` feature (export, validate and probe)",
    ))
}
