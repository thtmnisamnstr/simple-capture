//! Atomic file writes (ARCHITECTURE §6 Rules). Stubs until wp-core.

use std::path::Path;

use crate::{Error, Result};

/// Temp file, fsync, rename over `path`, fsync the folder.
pub fn atomic_write(_path: &Path, _bytes: &[u8]) -> Result<()> {
    Err(Error::Unsupported("sc_core::fsio: not implemented yet"))
}

pub fn fsync_dir(_dir: &Path) -> Result<()> {
    Err(Error::Unsupported("sc_core::fsio: not implemented yet"))
}
