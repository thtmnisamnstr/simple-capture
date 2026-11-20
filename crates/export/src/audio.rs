//! Mixes every audio track to one WAV for the export.

use std::path::Path;
use std::sync::Arc;

use sc_core::project::ProjectV1;
use sc_platform::{Decoder, Error, Result};

/// `false` = the project has no audio.
pub fn mix_to_wav(
    _project: &ProjectV1,
    _dir: &Path,
    _decoder: Arc<dyn Decoder>,
    _fps: u32,
    _frames: u64,
    _wav: &Path,
) -> Result<bool> {
    Err(Error::Unsupported("sc_export::audio: not implemented yet"))
}
