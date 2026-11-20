use std::path::{Path, PathBuf};

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

/// Stub until wp-helpers.
pub fn helper_info(_path: &Path) -> Result<HelperInfo> {
    Err(Error::Unsupported("sc_helpers: not implemented yet"))
}
