//! CPU reference renderer, the golden for GPU tests.

use sc_platform::{Error, Result};

use crate::{FrameJob, OutputFormat, RenderedFrame};

pub fn render_cpu(_job: &FrameJob, _out: OutputFormat) -> Result<RenderedFrame> {
    Err(Error::Unsupported("sc_render::cpu: not implemented yet"))
}
