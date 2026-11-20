//! FFmpeg argument recipes.

use std::path::Path;

use sc_geometry::SizePx;

/// x264 medium CRF 22 + `aac_at` 160k MP4 from y4m on stdin (+ a WAV). Stub until wp-export.
pub fn x264_quick(_size: SizePx, _fps: u32, _audio: bool, _out: &Path) -> Vec<String> {
    Vec::new()
}
