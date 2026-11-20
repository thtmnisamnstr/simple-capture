//! Project folder naming and locking (ADR-032; Q4: plain folder with the `.sc` suffix).

use std::path::{Path, PathBuf};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocalDateTime {
    pub y: i32,
    pub mo: u8,
    pub d: u8,
    pub h: u8,
    pub mi: u8,
    pub s: u8,
    pub offset_min: i32,
}

pub const PROJECT_SUFFIX: &str = ".sc";

/// `"<YYYY-MM-DD HH.MM.SS> <title>.sc"`. Stub until wp-core.
pub fn project_folder_name(_t: &LocalDateTime, _title: &str) -> String {
    String::new()
}

pub fn create_project_dir(_root: &Path, _t: &LocalDateTime, _title: &str) -> Result<PathBuf> {
    Err(Error::Unsupported("sc_core::folder: not implemented yet"))
}

pub fn is_project_dir(_p: &Path) -> bool {
    false
}

/// Per-project `.lock`; `Err(Locked)` when another live process holds it.
#[derive(Debug)]
pub struct ProjectLock {
    _priv: (),
}

impl ProjectLock {
    pub fn acquire(_dir: &Path) -> Result<ProjectLock> {
        Err(Error::Unsupported("sc_core::folder: not implemented yet"))
    }
}

pub fn rfc3339(_t: &LocalDateTime) -> String {
    String::new()
}
