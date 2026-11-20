use sc_core::image::RgbaImage;
use sc_geometry::{RectPx, SizePx};
use sc_platform::{Error, FrameBuffer, Result};

use crate::cpu;
use crate::gpu::{self, Gpu};

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
    gpu: Option<Gpu>,
}

impl Backend {
    /// Reads `SC_WGPU_BACKEND` (`auto`, `metal`, `dx12`, `vulkan`, `gl`, `cpu`); `None` when unset.
    pub fn from_env() -> Result<Option<Backend>> {
        match std::env::var("SC_WGPU_BACKEND") {
            Ok(v) if v.trim().is_empty() => Ok(None),
            Ok(v) => Backend::parse(&v).map(Some).ok_or_else(|| {
                Error::Other(format!(
                    "SC_WGPU_BACKEND={v}: expected auto, metal, dx12, vulkan, gl or cpu"
                ))
            }),
            Err(_) => Ok(None),
        }
    }

    pub fn parse(s: &str) -> Option<Backend> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Backend::Auto,
            "metal" => Backend::Metal,
            "dx12" => Backend::Dx12,
            "vulkan" => Backend::Vulkan,
            "gl" | "gles" => Backend::Gl,
            "cpu" | "cpu-only" | "cpuonly" | "none" => Backend::CpuOnly,
            _ => return None,
        })
    }
}

impl Compositor {
    /// `Auto` tries a hardware adapter, then a software adapter, then falls back to the CPU
    /// renderer; an explicit backend fails when it has no adapter.
    pub fn new(backend: Backend) -> Result<Compositor> {
        let backend = match backend {
            Backend::Auto => Backend::from_env()?.unwrap_or(Backend::Auto),
            b => b,
        };
        let gpu = match backend {
            Backend::CpuOnly => None,
            Backend::Auto => gpu::open(wgpu::Backends::all(), true)?,
            Backend::Metal => gpu::open(wgpu::Backends::METAL, false)?,
            Backend::Dx12 => gpu::open(wgpu::Backends::DX12, false)?,
            Backend::Vulkan => gpu::open(wgpu::Backends::VULKAN, false)?,
            Backend::Gl => gpu::open(wgpu::Backends::GL, false)?,
        };
        Ok(Compositor { gpu })
    }

    /// `None` = CPU only.
    pub fn adapter(&self) -> Option<AdapterSummary> {
        self.gpu.as_ref().map(|g| g.summary().clone())
    }

    pub fn render(&mut self, job: &FrameJob, out: OutputFormat) -> Result<RenderedFrame> {
        match &mut self.gpu {
            Some(gpu) => gpu.render(job, out),
            None => cpu::render_cpu(job, out),
        }
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
