//! Atomic file writes (ARCHITECTURE §6 Rules).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Error, Result};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Temp file in the same folder, fsync, rename over `path`, fsync the folder. A reader (or a
/// crash) sees either the old file or the new one, never a mix.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let name = path.file_name().ok_or_else(|| {
        Error::Invalid(format!("atomic_write: no file name in {}", path.display()))
    })?;
    let dir = parent_dir(path);
    let tmp = dir.join(format!(
        ".{}.tmp-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = write_synced(&tmp, bytes).and_then(|()| {
        fs::rename(&tmp, path).map_err(|e| Error::io_at(e, "rename temp file over", path))
    });
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    fsync_dir(&dir)
}

/// Makes a rename or create inside `dir` durable. On Windows this is a no-op beyond checking that
/// `dir` exists (directory handles cannot be flushed there; NTFS journals the rename).
pub fn fsync_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| Error::io_at(e, "fsync folder", dir))
    }
    #[cfg(not(unix))]
    {
        fs::metadata(dir)
            .map(|_| ())
            .map_err(|e| Error::io_at(e, "fsync folder", dir))
    }
}

fn write_synced(tmp: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)
        .map_err(|e| Error::io_at(e, "create temp file", tmp))?;
    f.write_all(bytes)
        .and_then(|()| f.sync_all())
        .map_err(|e| Error::io_at(e, "write temp file", tmp))
}

pub(crate) fn parent_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}
