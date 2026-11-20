use std::path::Path;
use std::sync::Arc;

use sc_core::project::ProjectV1;
use sc_platform::{Decoder, Error, Result};

/// Continuous audio in recording time: silence before the first sample and after the end.
pub struct TrackAudio {
    _decoder: Arc<dyn Decoder>,
}

impl TrackAudio {
    pub fn open(
        _project: &ProjectV1,
        _dir: &Path,
        _track_id: &str,
        _decoder: Arc<dyn Decoder>,
        _rate: u32,
        _channels: u16,
    ) -> Result<TrackAudio> {
        Err(Error::Unsupported("sc_decode: not implemented yet"))
    }

    /// Interleaved f32; returns frames written.
    pub fn read(&mut self, _out: &mut [f32]) -> Result<usize> {
        Err(Error::Unsupported("sc_decode: not implemented yet"))
    }
}
