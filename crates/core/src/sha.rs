use std::path::Path;

use crate::{Error, Result};

/// Lower-case hex SHA-256 of a file. Stub until wp-core.
pub fn sha256_file(_p: &Path) -> Result<String> {
    Err(Error::Unsupported("sc_core::sha: not implemented yet"))
}

/// Lower-case hex SHA-256. Stub until wp-core.
pub fn sha256_bytes(_b: &[u8]) -> String {
    String::new()
}
