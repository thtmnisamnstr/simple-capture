use std::path::PathBuf;

use sc_core::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelperKind {
    Ffmpeg,
    Ffprobe,
    Gifski,
}

/// Order: `$SC_FFMPEG` / `$SC_FFPROBE` / `$SC_GIFSKI` → next to `current_exe()` (sidecar
/// layout) → debug builds only: `concat!(env!("CARGO_MANIFEST_DIR"),
/// "/../../third_party/out/<os>-<arch>/")`. Never `$PATH`. The error names every path tried.
/// Stub until wp-helpers.
pub fn locate(_kind: HelperKind) -> Result<PathBuf> {
    Err(Error::Unsupported("sc_helpers: not implemented yet"))
}
