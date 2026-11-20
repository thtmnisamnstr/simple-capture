use std::path::Path;

use sc_core::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavFormat {
    F32,
    S16,
}

#[derive(Debug)]
pub struct WavWriter {
    frames: u64,
}

const STUB: &str = "sc_helpers::wav: not implemented yet";

impl WavWriter {
    pub fn create(_path: &Path, _rate: u32, _channels: u16, _fmt: WavFormat) -> Result<WavWriter> {
        Err(Error::Unsupported(STUB))
    }

    pub fn write_f32(&mut self, _interleaved: &[f32]) -> Result<()> {
        Err(Error::Unsupported(STUB))
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Patches the header; returns frames written.
    pub fn finish(self) -> Result<u64> {
        Err(Error::Unsupported(STUB))
    }
}
