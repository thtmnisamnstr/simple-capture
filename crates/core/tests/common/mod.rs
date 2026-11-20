#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A fresh folder under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!("sc-core-{tag}-{}", sc_core::ids::new_id()));
        std::fs::create_dir_all(&p).expect("create temp dir");
        TempDir(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Re-runs this test binary as a helper process that runs only `test_name`, with `env` set.
pub fn spawn_helper(test_name: &str, env: &[(&str, &str)]) -> Child {
    let exe = std::env::current_exe().expect("current exe");
    let mut cmd = Command::new(exe);
    cmd.args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.spawn().expect("spawn helper")
}

/// Reads the helper's stdout until a line equal to `marker` appears.
pub fn wait_for_line(child: &mut Child, marker: &str) {
    let out = child.stdout.take().expect("helper stdout");
    let mut reader = BufReader::new(out);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).expect("read helper stdout");
        assert!(n > 0, "helper exited before printing {marker}");
        if line.trim_end().ends_with(marker) {
            break;
        }
    }
    // Keep draining so the helper never blocks on a full pipe.
    std::thread::spawn(move || {
        let mut sink = String::new();
        while reader.read_line(&mut sink).is_ok_and(|n| n > 0) {
            sink.clear();
        }
    });
}

/// `800` and `800.0` are the same JSON number to every reader of the journal.
pub fn numbers_as_f64(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Number(n) => n
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Null, Value::Number),
        Value::Array(a) => Value::Array(a.into_iter().map(numbers_as_f64).collect()),
        Value::Object(o) => {
            Value::Object(o.into_iter().map(|(k, v)| (k, numbers_as_f64(v))).collect())
        }
        other => other,
    }
}
