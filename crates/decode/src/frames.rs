use std::path::Path;
use std::sync::Arc;

use sc_core::project::ProjectV1;
use sc_geometry::{RectPx, SizePx};
use sc_platform::{Decoder, Error, FrameBuffer, Result};

const STUB: &str = "sc_decode: not implemented yet";

/// Sequential frames across screen segments for monotonic `t`; holds the last frame across
/// gaps (ARCHITECTURE §7).
pub struct SegmentFrameSource {
    _decoder: Arc<dyn Decoder>,
}

impl SegmentFrameSource {
    pub fn open(
        _project: &ProjectV1,
        _dir: &Path,
        _decoder: Arc<dyn Decoder>,
    ) -> Result<SegmentFrameSource> {
        Err(Error::Unsupported(STUB))
    }

    pub fn frame_at(&mut self, _t_rec_ns: i64) -> Result<Option<SegmentFrame>> {
        Err(Error::Unsupported(STUB))
    }
}

#[derive(Debug, Clone)]
pub struct SegmentFrame {
    pub segment_id: String,
    pub frame_px: SizePx,
    pub content_px: RectPx,
    pub frame: FrameBuffer,
    pub held: bool,
}

/// Constant-frame-rate output times in recording ns. Stub until wp-decode.
pub fn cfr_frame_times(_duration_ns: i64, _fps: u32) -> impl Iterator<Item = i64> {
    std::iter::empty()
}
