use sc_core::image::RgbaImage;
use sc_geometry::{RectPx, SizePx};
use sc_platform::{Error, FrameBuffer, Result};

use crate::cpu;
use crate::gpu::{self, Gpu};
use crate::surface::NativeSurface;

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

    /// Camera frames the GPU read in place (macOS IOSurfaces) instead of uploading.
    pub fn camera_frames_read_in_place(&self) -> u64 {
        self.gpu.as_ref().map_or(0, |g| g.camera_in_place)
    }

    pub fn render(&mut self, job: &FrameJob, out: OutputFormat) -> Result<RenderedFrame> {
        match &mut self.gpu {
            Some(gpu) => gpu.render(job, out),
            None => cpu::render_cpu(job, out),
        }
    }

    /// A surface on a `CAMetalLayer` for [`Compositor::present`]. Fails without a GPU.
    ///
    /// # Safety
    /// `layer` points to a live `CAMetalLayer`.
    #[cfg(target_os = "macos")]
    pub unsafe fn metal_layer_surface(
        &self,
        layer: std::ptr::NonNull<std::ffi::c_void>,
    ) -> Result<NativeSurface> {
        let gpu = self.gpu.as_ref().ok_or_else(no_gpu)?;
        // SAFETY: forwarded from the caller.
        unsafe { gpu.metal_layer_surface(layer) }
    }

    /// Composites `job` into a `drawable`-sized frame of `surface` filled with `clear`, the canvas
    /// at `at` (pixels from the top left), shown from the display's next refresh. `Ok(false)`:
    /// the surface is not on screen (hidden, occluded or resizing) and nothing was drawn.
    pub fn present(
        &mut self,
        job: &FrameJob,
        surface: &mut NativeSurface,
        drawable: SizePx,
        at: (u32, u32),
        clear: [u8; 3],
    ) -> Result<bool> {
        let gpu = self.gpu.as_mut().ok_or_else(no_gpu)?;
        gpu.present(job, surface, drawable, at, clear)
    }
}

fn no_gpu() -> Error {
    Error::Other("sc_render: a native surface needs a GPU".into())
}

#[derive(Debug, Clone)]
pub struct FrameJob<'a> {
    pub canvas: SizePx,
    pub background: &'a RgbaImage,
    /// Same key as the previous job's ⇒ same background pixels: the GPU reuses its copy instead
    /// of uploading again. `None` always uploads.
    pub background_key: Option<u64>,
    pub screen: Option<ScreenLayer<'a>>,
    pub camera: Option<CameraLayer<'a>>,
    /// Drawn over the camera, in order.
    pub text: &'a [TextLayer<'a>],
}

/// A premultiplied RGBA image (a text item) placed by an affine map, image pixel to canvas
/// pixel: `x' = m0·x + m2·y + m4`, `y' = m1·x + m3·y + m5`, at `opacity` (0..=1).
#[derive(Debug, Clone)]
pub struct TextLayer<'a> {
    pub image: &'a RgbaImage,
    /// Same key as an earlier layer's ⇒ same pixels: the GPU reuses its upload.
    pub key: u64,
    pub to_canvas: [f32; 6],
    pub opacity: f32,
}

/// BT.709 limited input.
#[derive(Debug, Clone)]
pub struct ScreenLayer<'a> {
    pub frame: &'a FrameBuffer,
    pub src: RectPx,
    pub dst: RectPx,
    /// Drawn as a mosaic or black, in order; at most [`MAX_REDACTIONS`].
    pub redact: &'a [Redact],
}

/// Redactions drawn at once; later ones are left out.
pub const MAX_REDACTIONS: usize = 8;

/// A box over the screen picture: `rect` is x, y, w, h as fractions of `src`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Redact {
    pub rect: [f64; 4],
    pub black: bool,
}

/// The camera drawn over the screen: `src` of the camera frame (BT.709, either range) scaled
/// into the rounded rectangle `dst`, optionally mirrored, with an optional opaque border inside
/// the edge and an optional soft shadow (`shadow` 0..=255, 0 = none).
#[derive(Debug, Clone)]
pub struct CameraLayer<'a> {
    pub frame: &'a FrameBuffer,
    pub src: RectPx,
    pub dst: RectPx,
    pub radius_px: u32,
    pub mirror: bool,
    pub border: Option<(u32, [u8; 3])>,
    pub shadow: u8,
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

/// The camera layer for `placement` drawn into `dst` (canvas or bubble pixels). The border width
/// scales with the shape: `width_px_1080` is in 1080 units of the output canvas, whose short side
/// is the shape's short side over `size_norm`, so the bubble and the output agree.
pub fn camera_layer<'a>(
    placement: &sc_core::camera::CameraPlacement,
    frame: &'a FrameBuffer,
    dst: sc_geometry::RectF,
) -> CameraLayer<'a> {
    let aspect = placement.shape.aspect();
    let src = sc_geometry::camera_rects(
        SizePx { w: 1, h: 1 },
        frame.0.size(),
        aspect,
        &placement.pose,
    )
    .src;
    let round = |r: sc_geometry::RectF| RectPx {
        x: r.x.round() as i32,
        y: r.y.round() as i32,
        w: r.w.round().max(1.0) as u32,
        h: r.h.round().max(1.0) as u32,
    };
    let dst = round(dst);
    let short = f64::from(dst.w.min(dst.h));
    let unit = short / (1080.0 * placement.pose.size.max(1e-6));
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    CameraLayer {
        frame,
        src: round(src),
        dst,
        radius_px: (placement.pose.radius.clamp(0.0, 1.0) * short / 2.0).round() as u32,
        mirror: placement.mirror,
        border: placement.border.as_ref().map(|b| {
            let c = b.color.0;
            (
                (b.width_px_1080 * unit).round().max(1.0) as u32,
                [byte(c[0]), byte(c[1]), byte(c[2])],
            )
        }),
        shadow: placement.shadow.as_ref().map_or(0, |s| byte(s.strength)),
    }
}
