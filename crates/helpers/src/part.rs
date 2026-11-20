//! `<final>.part` files renamed into place on success.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sc_core::{Error, Result};

#[derive(Debug)]
pub struct PartFile {
    path: PathBuf,
    final_path: PathBuf,
    live: bool,
}

/// A `.part` file touched more recently than this may belong to an export still running in
/// another process, so [`sweep_stale`] leaves it alone.
pub const STALE_AFTER: Duration = Duration::from_secs(60);

/// Only `<name>.<ext>.part` with one of these extensions is ours; a browser's `.part` download
/// in the same folder is never swept.
pub const SWEPT_EXTENSIONS: &[&str] = &["mp4", "mov", "m4v", "gif", "webm", "mkv", "wav", "m4a"];

fn io_err(e: std::io::Error, what: &str, path: &Path) -> Error {
    Error::Io(std::io::Error::new(
        e.kind(),
        format!("{what} {}: {e}", path.display()),
    ))
}

pub fn part_path_for(final_path: &Path) -> PathBuf {
    let mut s: OsString = final_path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

impl PartFile {
    /// Creates `<final>.part` exclusively. Fails when the final file or the `.part` exists.
    pub fn create(final_path: &Path) -> Result<PartFile> {
        if final_path.file_name().is_none() {
            return Err(Error::Invalid(format!(
                "part file: {} has no file name",
                final_path.display()
            )));
        }
        if final_path.symlink_metadata().is_ok() {
            return Err(io_err(
                std::io::Error::from(std::io::ErrorKind::AlreadyExists),
                "refusing to overwrite",
                final_path,
            ));
        }
        let path = part_path_for(final_path);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| io_err(e, "create part file", &path))?;
        Ok(PartFile {
            path,
            final_path: final_path.to_path_buf(),
            live: true,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn final_path(&self) -> &Path {
        &self.final_path
    }

    /// Renames the `.part` onto the final path without ever replacing an existing file.
    pub fn commit(mut self) -> Result<PathBuf> {
        rename_no_replace(&self.path, &self.final_path)?;
        self.live = false;
        Ok(self.final_path.clone())
    }

    pub fn discard(self) {}
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if self.live {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn rename_no_replace(from: &Path, to: &Path) -> Result<()> {
    match std::fs::hard_link(from, to) {
        Ok(()) => std::fs::remove_file(from).map_err(|e| io_err(e, "remove part file", from)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(io_err(e, "refusing to overwrite", to))
        }
        Err(link_err) => {
            // File systems without hard links (exFAT, some network shares): check, then rename.
            if to.symlink_metadata().is_ok() {
                return Err(io_err(
                    std::io::Error::from(std::io::ErrorKind::AlreadyExists),
                    "refusing to overwrite",
                    to,
                ));
            }
            std::fs::rename(from, to).map_err(|e| {
                io_err(
                    e,
                    &format!(
                        "rename (hard link failed: {link_err}) {} ->",
                        from.display()
                    ),
                    to,
                )
            })
        }
    }
}

fn is_ours(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".part") else {
        return false;
    };
    let Some((base, ext)) = stem.rsplit_once('.') else {
        return false;
    };
    !base.is_empty() && SWEPT_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

/// Deletes stale `.part` files; returns how many. Stale means named `<name>.<ext>.part` with an
/// extension in [`SWEPT_EXTENSIONS`] and not modified for [`STALE_AFTER`].
pub fn sweep_stale(dir: &Path) -> Result<usize> {
    sweep_older_than(dir, STALE_AFTER)
}

pub fn sweep_older_than(dir: &Path, age: Duration) -> Result<usize> {
    let entries = std::fs::read_dir(dir).map_err(|e| io_err(e, "read dir", dir))?;
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|e| io_err(e, "read dir entry in", dir))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !is_ours(name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let idle = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .unwrap_or(Duration::ZERO);
        if idle < age {
            continue;
        }
        let p = entry.path();
        match std::fs::remove_file(&p) {
            Ok(()) => {
                tracing::info!(path = %p.display(), "swept stale part file");
                removed += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err(e, "remove stale part file", &p)),
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::is_ours;

    #[test]
    fn only_export_parts_are_ours() {
        assert!(is_ours("Recording 1.mp4.part"));
        assert!(is_ours("a.GIF.part"));
        assert!(!is_ours("setup.dmg.part"));
        assert!(!is_ours("movie.part"));
        assert!(!is_ours(".mp4.part"));
        assert!(!is_ours("a.mp4"));
    }
}
