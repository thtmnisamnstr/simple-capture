//! ffprobe validation of an exported file.

use std::path::Path;

use sc_geometry::SizePx;
use sc_platform::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expect {
    pub size: SizePx,
    pub fps: u32,
    pub duration_ns: i64,
    pub audio: bool,
    pub vcodec: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeReport {
    pub vcodec: String,
    pub size: SizePx,
    pub fps: f64,
    pub duration_ns: i64,
    pub frames: Option<u64>,
    pub acodec: Option<String>,
    pub audio_duration_ns: Option<i64>,
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_space: Option<String>,
    pub color_range: Option<String>,
}

pub fn ffprobe_check(_file: &Path, _e: &Expect) -> Result<ProbeReport> {
    Err(Error::Unsupported(
        "sc_export::validate: not implemented yet",
    ))
}
