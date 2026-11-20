//! Capability probe v1 (ARCHITECTURE §11, plan §10 item 11). Cache file
//! `<cache_dir>/probe-v1.json`, keyed by helper sha256 + OS version + display sizes.

use std::path::Path;

use sc_geometry::SizePx;
use sc_helpers::HelperInfo;
use sc_platform::{DisplayId, DisplayInfo, Error, Platform, Resolution, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayProbe {
    pub display_id: String,
    pub native_px: SizePx,
    pub one_x_px: SizePx,
    pub hevc_fps_native: Option<f64>,
    pub hevc_fps_1x: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeV1 {
    /// Always 1.
    pub schema: u32,
    pub created_at: String,
    pub os_version: String,
    pub helper: Option<HelperInfo>,
    pub vt_h264_opens: Vec<(SizePx, bool)>,
    pub displays: Vec<DisplayProbe>,
    pub quarantined: Vec<String>,
}

impl ProbeV1 {
    /// Measured ≥ 1.3 × 60 fps; `None` when the display was not probed. Stub until wp-export.
    pub fn allows_60fps(&self, _display: &DisplayId, _res: Resolution) -> Option<bool> {
        None
    }
}

const STUB: &str = "sc_export::probe: not implemented yet";

pub fn run(_platform: &dyn Platform, _displays: &[DisplayInfo]) -> Result<ProbeV1> {
    Err(Error::Unsupported(STUB))
}

pub fn load_or_run(
    _platform: &dyn Platform,
    _cache_dir: &Path,
    _displays: &[DisplayInfo],
) -> Result<ProbeV1> {
    Err(Error::Unsupported(STUB))
}
