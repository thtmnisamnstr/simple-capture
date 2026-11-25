use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sc_core::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperInfo {
    pub path: PathBuf,
    pub sha256: String,
    pub version_line: String,
    pub buildconf: Vec<String>,
    pub network_disabled: bool,
}

fn stdout_of(path: &Path, args: &[&str]) -> Result<Option<String>> {
    let out = crate::hide_console(&mut Command::new(path))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Other(format!("run {} {}: {e}", path.display(), args.join(" "))))?;
    Ok(out
        .status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned()))
}

fn first_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|l| !l.is_empty())
}

/// Hash, version line and `-buildconf` flags. FFmpeg tools answer `-version`; gifski answers
/// only `--version` and has no build configuration, so its `buildconf` is empty.
pub fn helper_info(path: &Path) -> Result<HelperInfo> {
    let sha256 = sc_core::sha::sha256_file(path)?;
    let ffmpeg_style = stdout_of(path, &["-hide_banner", "-version"])?
        .and_then(|t| first_line(&t).map(str::to_owned))
        .filter(|l| l.contains(" version "));
    let (version_line, buildconf) = match ffmpeg_style {
        Some(line) => {
            let conf = stdout_of(path, &["-hide_banner", "-buildconf"])?.unwrap_or_default();
            let flags = conf
                .split_whitespace()
                .filter(|t| t.starts_with("--"))
                .map(str::to_owned)
                .collect();
            (line, flags)
        }
        None => {
            let line = stdout_of(path, &["--version"])?
                .and_then(|t| first_line(&t).map(str::to_owned))
                .ok_or_else(|| {
                    Error::Other(format!(
                        "helper {} printed no version (-version and --version)",
                        path.display()
                    ))
                })?;
            (line, Vec::new())
        }
    };
    let network_disabled = buildconf.iter().any(|f| f == "--disable-network");
    Ok(HelperInfo {
        path: path.to_path_buf(),
        sha256,
        version_line,
        buildconf,
        network_disabled,
    })
}
