//! Runs the built `simple-capture` binary. Native calls here never touch TCC: the child gets
//! neither SC_DEV_HOST nor SC_LIVE, so the capture-macos tripwire refuses anything that could
//! prompt.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_simple-capture"))
        .args(args)
        .env_remove("SC_DEV_HOST")
        .env_remove("SC_LIVE")
        .output()
        .expect("run simple-capture")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sc-cli-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

#[test]
fn usage_and_self_test_exit_codes() {
    assert_eq!(cli(&[]).status.code(), Some(2));
    assert_eq!(cli(&["frobnicate"]).status.code(), Some(2));
    assert_eq!(
        cli(&["record", "--source", "nonsense"]).status.code(),
        Some(2)
    );
    assert_eq!(
        cli(&["record", "--source", "display", "--background", "solid:red"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        cli(&["record", "--source", "synthetic:a", "--switch", "x=display"])
            .status
            .code(),
        Some(2)
    );
    let help = cli(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(text(&help.stdout).contains("SPEC :="));
    let st = cli(&["--self-test"]);
    assert_eq!(st.status.code(), Some(3));
    assert!(text(&st.stderr).contains("not implemented until M6"));
}

#[test]
fn synthetic_record_rejects_native_only_flags() {
    let out = cli(&[
        "--platform",
        "synthetic",
        "record",
        "--source",
        "synthetic:a",
        "--own-test-window",
        "1=2",
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
}

#[test]
fn synthetic_perms_and_displays() {
    let out = cli(&["--platform", "synthetic", "perms", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("perms json");
    for k in ["screen", "camera", "microphone"] {
        assert_eq!(v[k], "granted", "{v}");
    }
    let out = cli(&["--platform", "synthetic", "displays", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("displays json");
    assert!(v.is_array(), "{v}");
}

#[test]
fn native_perms_prints_three_statuses_without_prompting() {
    let out = cli(&["perms"]);
    if cfg!(target_os = "macos") {
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        let stdout = text(&out.stdout);
        for k in ["screen: ", "camera: ", "microphone: "] {
            assert!(stdout.contains(k), "{stdout}");
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn native_sources_outside_the_dev_host_is_refused() {
    let out = cli(&["sources"]);
    assert_ne!(out.status.code(), Some(0));
    let err = text(&out.stderr);
    // Until wp-mac-sck lands, the macOS screen source is a stub that says so.
    assert!(
        err.contains("TCC tripwire") || err.contains("not implemented yet"),
        "{err}"
    );
}

fn project_state(dir: &Path) -> serde_json::Value {
    let bytes = std::fs::read(dir.join("project.json")).expect("read project.json");
    serde_json::from_slice(&bytes).expect("parse project.json")
}

#[test]
#[ignore = "integration: un-ignored by wp-e2e-cli"]
fn synthetic_record_with_switch() {
    let projects = temp_dir("record");
    let out = cli(&[
        "--platform",
        "synthetic",
        "record",
        "--source",
        "synthetic:a",
        "--switch",
        "1=synthetic:b",
        "--duration",
        "3",
        "--projects-dir",
        projects.to_str().expect("utf-8 temp path"),
    ]);
    let stdout = text(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let dir = PathBuf::from(stdout.lines().last().expect("prints the project folder"));
    assert!(dir.starts_with(&projects), "{}", dir.display());
    let p = project_state(&dir);
    assert_eq!(p["state"], "finalized");
    let segs = p["recording"]["screen"]["segments"]
        .as_array()
        .expect("segments");
    assert_eq!(segs.len(), 2, "{p}");
    let _ = std::fs::remove_dir_all(&projects);
}
