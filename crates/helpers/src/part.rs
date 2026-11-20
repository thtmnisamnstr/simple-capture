//! `<final>.part` files renamed into place on success.

use std::path::{Path, PathBuf};

use sc_core::{Error, Result};

#[derive(Debug)]
pub struct PartFile {
    path: PathBuf,
}

const STUB: &str = "sc_helpers::part: not implemented yet";

impl PartFile {
    pub fn create(_final_path: &Path) -> Result<PartFile> {
        Err(Error::Unsupported(STUB))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn commit(self) -> Result<PathBuf> {
        Err(Error::Unsupported(STUB))
    }

    pub fn discard(self) {}
}

/// Deletes stale `.part` files; returns how many.
pub fn sweep_stale(_dir: &Path) -> Result<usize> {
    Err(Error::Unsupported(STUB))
}
