use sc_core::image::RgbaImage;
use sc_geometry::{RectPx, SizePx};
use sc_platform::{Error, FrameBuffer, Result};

/// `SC_WGPU_BACKEND` overrides `Auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Auto,
    Metal,
    Dx12,
    Vulkan,
    Gl,
    CpuOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterSummary {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub software: bool,
}

pub struct Compositor {
    _backend: Backend,
}

impl Compositor {
    pub fn new(_backend: Backend) -> Result<Compositor> {
        Err(Error::Unsupported("sc_render: not implemented yet"))
    }

    /// `None` = CPU only.
    pub fn adapter(&self) -> Option<AdapterSummary> {
        None
    }

    pub fn render(&mut self, _job: &FrameJob, _out: OutputFormat) -> Result<RenderedFrame> {
        Err(Error::Unsupported("sc_render: not implemented yet"))
    }
}

#[derive(Debug, Clone)]
pub struct FrameJob<'a> {
    pub canvas: SizePx,
    pub background: &'a RgbaImage,
    pub screen: Option<ScreenLayer<'a>>,
}

/// BT.709 limited input.
#[derive(Debug, Clone)]
pub struct ScreenLayer<'a> {
    pub frame: &'a FrameBuffer,
    pub src: RectPx,
    pub dst: RectPx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Rgba8,
    I420Bt709Limited,
}

/// Tightly packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFrame {
    pub size: SizePx,
    pub format: OutputFormat,
    pub data: Vec<u8>,
}
