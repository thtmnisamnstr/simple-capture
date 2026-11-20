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

#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn native_sources_outside_the_dev_host_hits_the_tripwire() {
    let out = cli(&["sources"]);
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(
        err.contains("TCC tripwire") && err.contains("tools/macos-dev-host/run.sh"),
        "{err}"
    );
    assert!(out.stdout.is_empty(), "{}", text(&out.stdout));
}

fn project_state(dir: &Path) -> serde_json::Value {
    let bytes = std::fs::read(dir.join("project.json")).expect("read project.json");
    serde_json::from_slice(&bytes).expect("parse project.json")
}

fn helper_present(name: &str) -> bool {
    let env = format!("SC_{}", name.to_uppercase());
    if std::env::var_os(&env).is_some_and(|v| std::path::Path::new(&v).is_file()) {
        return true;
    }
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        a => a,
    };
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../third_party/out")
        .join(format!("{}-{arch}", std::env::consts::OS))
        .join(name)
        .is_file()
}

#[test]
fn synthetic_record_with_switch() {
    let projects = temp_dir("record");
    let projects_arg = projects.to_str().expect("utf-8 temp path");
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
        projects_arg,
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

    let rec = cli(&[
        "--platform",
        "synthetic",
        "--json",
        "recover",
        "--projects-dir",
        projects_arg,
    ]);
    assert_eq!(rec.status.code(), Some(0), "{}", text(&rec.stderr));
    let v: serde_json::Value = serde_json::from_slice(&rec.stdout).expect("recover json");
    let reports = v["reports"].as_array().expect("reports");
    assert!(
        reports
            .iter()
            .all(|r| r["outcome"] == "already_final" || r["outcome"] == "not_a_project"),
        "recover found work after a clean stop: {v}"
    );
    assert_eq!(project_state(&dir)["state"], "finalized");

    if cfg!(feature = "export") && helper_present("ffmpeg") && helper_present("ffprobe") {
        let mp4 = projects.join("out.mp4");
        let mp4_arg = mp4.to_str().expect("utf-8 temp path");
        let ex = cli(&[
            "--platform",
            "synthetic",
            "export",
            dir.to_str().expect("utf-8 project path"),
            "--out",
            mp4_arg,
            "--size",
            "480",
        ]);
        assert_eq!(ex.status.code(), Some(0), "{}", text(&ex.stderr));
        assert!(mp4.is_file(), "no export at {}", mp4.display());
        let val = cli(&[
            "--json",
            "validate",
            mp4_arg,
            "--size",
            "854x480",
            "--fps",
            "30",
            "--duration",
            "3",
        ]);
        assert_eq!(val.status.code(), Some(0), "{}", text(&val.stderr));
        let v: serde_json::Value = serde_json::from_slice(&val.stdout).expect("validate json");
        assert_eq!(v["ok"], true, "{v}");
    } else {
        println!("SKIPPED: export (no ffmpeg/ffprobe helper)");
    }
    let _ = std::fs::remove_dir_all(&projects);
}
