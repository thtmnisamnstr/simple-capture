use std::ffi::OsString;
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

    /// The .deb installs sidecars into /usr/bin, where a bare `ffmpeg` would clash with the
    /// distro's package and sit on everyone's `$PATH`.
    fn bundled_file_name(self) -> String {
        format!("sc-{}{}", self.name(), std::env::consts::EXE_SUFFIX)
    }

    fn build_file_name(self) -> String {
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

/// Where the bundle keeps `kind`: `sc-<name>` next to `current_exe()`. For callers that need a
/// path even when [`locate`] fails; spawning it then fails with that path, never a `$PATH` match.
pub fn bundled_path(kind: HelperKind) -> PathBuf {
    let name = kind.bundled_file_name();
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(&name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Order: `$SC_FFMPEG` / `$SC_FFPROBE` / `$SC_GIFSKI` → `sc-<name>` next to `current_exe()`
/// (the bundle layout) → debug builds only: `<name>` in `concat!(env!("CARGO_MANIFEST_DIR"),
/// "/../../third_party/out/<os>-<arch>/")`. Never `$PATH`. The error names every path tried.
pub fn locate(kind: HelperKind) -> Result<PathBuf> {
    let build_out = cfg!(debug_assertions).then(|| {
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../third_party/out"
        ))
        .join(platform_dir_name())
    });
    search(
        kind,
        std::env::var_os(kind.env_var()),
        std::env::current_exe(),
        build_out.as_deref(),
    )
}

fn search(
    kind: HelperKind,
    env: Option<OsString>,
    exe: std::io::Result<PathBuf>,
    build_out: Option<&Path>,
) -> Result<PathBuf> {
    let mut tried: Vec<String> = Vec::new();

    if let Some(v) = env.filter(|v| !v.is_empty()) {
        let p = PathBuf::from(v);
        if p.is_file() {
            return Ok(p);
        }
        tried.push(format!("${}={}", kind.env_var(), p.display()));
    }

    match exe {
        Ok(exe) => {
            if let Some(dir) = exe.parent() {
                let p = dir.join(kind.bundled_file_name());
                if p.is_file() {
                    return Ok(p);
                }
                tried.push(p.display().to_string());
            }
        }
        Err(e) => tried.push(format!("<current_exe unavailable: {e}>")),
    }

    if let Some(dir) = build_out {
        let p = dir.join(kind.build_file_name());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("sc-helpers-locate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("scratch dir");
        d
    }

    fn touch(p: &Path) {
        std::fs::write(p, b"").expect("touch");
    }

    #[test]
    fn bundled_names_carry_the_sc_prefix() {
        let exe = std::env::consts::EXE_SUFFIX;
        assert_eq!(
            HelperKind::Ffmpeg.bundled_file_name(),
            format!("sc-ffmpeg{exe}")
        );
        assert_eq!(
            HelperKind::Ffprobe.bundled_file_name(),
            format!("sc-ffprobe{exe}")
        );
        assert_eq!(
            HelperKind::Gifski.bundled_file_name(),
            format!("sc-gifski{exe}")
        );
    }

    #[test]
    fn a_distro_ffmpeg_beside_the_app_is_never_used() {
        let bin = scratch("distro");
        let app = bin.join("simple-capture-app");
        touch(&bin.join(HelperKind::Ffmpeg.build_file_name()));
        let err = search(HelperKind::Ffmpeg, None, Ok(app.clone()), None)
            .expect_err("a bare ffmpeg is not ours")
            .to_string();
        let ours = bin.join(HelperKind::Ffmpeg.bundled_file_name());
        assert!(err.contains(&ours.display().to_string()), "{err}");

        touch(&ours);
        assert_eq!(
            search(HelperKind::Ffmpeg, None, Ok(app), None).expect("bundled"),
            ours
        );
        let _ = std::fs::remove_dir_all(&bin);
    }

    #[test]
    fn override_then_bundle_then_build_output() {
        let bundle = scratch("bundle");
        let out = scratch("out");
        let app = bundle.join("simple-capture-app");
        let kind = HelperKind::Gifski;
        let find = |env: Option<&Path>| {
            search(
                kind,
                env.map(|p| p.as_os_str().to_owned()),
                Ok(app.clone()),
                Some(&out),
            )
        };

        let built = out.join(kind.build_file_name());
        touch(&built);
        assert_eq!(find(None).expect("build output"), built);

        let bundled = bundle.join(kind.bundled_file_name());
        touch(&bundled);
        assert_eq!(find(None).expect("bundled"), bundled);

        let custom = out.join("my-gifski");
        assert_eq!(find(Some(&custom)).expect("missing override"), bundled);
        touch(&custom);
        assert_eq!(find(Some(&custom)).expect("override"), custom);
        assert_eq!(find(Some(Path::new(""))).expect("empty override"), bundled);

        let _ = std::fs::remove_dir_all(&bundle);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn the_error_names_every_path_tried() {
        let dir = scratch("missing");
        let err = search(
            HelperKind::Ffprobe,
            Some(dir.join("nope").into_os_string()),
            Ok(dir.join("simple-capture-app")),
            Some(&dir),
        )
        .expect_err("nothing there")
        .to_string();
        assert!(err.contains("$SC_FFPROBE="), "{err}");
        assert!(
            err.contains(
                &dir.join(HelperKind::Ffprobe.bundled_file_name())
                    .display()
                    .to_string()
            ),
            "{err}"
        );
        assert!(
            err.contains(
                &dir.join(HelperKind::Ffprobe.build_file_name())
                    .display()
                    .to_string()
            ),
            "{err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
