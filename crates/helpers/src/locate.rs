use std::path::{Path, PathBuf};

use sc_core::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelperKind {
    Ffmpeg,
    Ffprobe,
    Gifski,
}

impl HelperKind {
    pub fn name(self) -> &'static str {
        match self {
            HelperKind::Ffmpeg => "ffmpeg",
            HelperKind::Ffprobe => "ffprobe",
            HelperKind::Gifski => "gifski",
        }
    }

    pub fn env_var(self) -> &'static str {
        match self {
            HelperKind::Ffmpeg => "SC_FFMPEG",
            HelperKind::Ffprobe => "SC_FFPROBE",
            HelperKind::Gifski => "SC_GIFSKI",
        }
    }

    fn file_name(self) -> String {
        format!("{}{}", self.name(), std::env::consts::EXE_SUFFIX)
    }
}

pub fn platform_dir_name() -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    format!("{}-{arch}", std::env::consts::OS)
}

/// Order: `$SC_FFMPEG` / `$SC_FFPROBE` / `$SC_GIFSKI` → next to `current_exe()` (sidecar
/// layout) → debug builds only: `concat!(env!("CARGO_MANIFEST_DIR"),
/// "/../../third_party/out/<os>-<arch>/")`. Never `$PATH`. The error names every path tried.
pub fn locate(kind: HelperKind) -> Result<PathBuf> {
    let mut tried: Vec<String> = Vec::new();

    if let Some(v) = std::env::var_os(kind.env_var()).filter(|v| !v.is_empty()) {
        let p = PathBuf::from(v);
        if p.is_file() {
            return Ok(p);
        }
        tried.push(format!("${}={}", kind.env_var(), p.display()));
    }

    match std::env::current_exe() {
        Ok(exe) => {
            if let Some(dir) = exe.parent() {
                let p = dir.join(kind.file_name());
                if p.is_file() {
                    return Ok(p);
                }
                tried.push(p.display().to_string());
            }
        }
        Err(e) => tried.push(format!("<current_exe unavailable: {e}>")),
    }

    if cfg!(debug_assertions) {
        let dir = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../third_party/out"
        ))
        .join(platform_dir_name());
        let p = dir.join(kind.file_name());
        if p.is_file() {
            return Ok(p);
        }
        tried.push(p.display().to_string());
    }

    Err(Error::Other(format!(
        "helper {} not found (never looked up on $PATH); tried: {}",
        kind.name(),
        tried.join(", ")
    )))
}
