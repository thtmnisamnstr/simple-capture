//! A window-system surface the compositor draws into directly (ADR-007): no readback and no
//! copy through the webview.

use sc_geometry::SizePx;
use sc_platform::{Error, Result};

/// Belongs to the [`crate::Compositor`] that made it; another compositor's device can't draw it.
pub struct NativeSurface {
    surface: wgpu::Surface<'static>,
    pub(crate) format: wgpu::TextureFormat,
    size: Option<SizePx>,
}

impl NativeSurface {
    #[cfg(target_os = "macos")]
    pub(crate) fn new(surface: wgpu::Surface<'static>, adapter: &wgpu::Adapter) -> Result<Self> {
        let caps = surface.get_capabilities(adapter);
        let format = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ]
        .into_iter()
        .find(|f| caps.formats.contains(f))
        .ok_or_else(|| {
            Error::Other(format!(
                "sc_render: no 8-bit surface format in {:?}",
                caps.formats
            ))
        })?;
        Ok(NativeSurface {
            surface,
            format,
            size: None,
        })
    }

    /// The next drawable at `size`; `None` while the surface is off screen or being rebuilt.
    pub(crate) fn acquire(
        &mut self,
        device: &wgpu::Device,
        size: SizePx,
    ) -> Result<Option<wgpu::SurfaceTexture>> {
        if self.size != Some(size) {
            self.surface.configure(
                device,
                &wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format: self.format,
                    width: size.w,
                    height: size.h,
                    present_mode: wgpu::PresentMode::Fifo,
                    desired_maximum_frame_latency: 2,
                    alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                    view_formats: vec![],
                    color_space: wgpu::SurfaceColorSpace::Srgb,
                },
            );
            self.size = Some(size);
        }
        Ok(match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                self.size = None;
                Some(t)
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => None,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.size = None;
                None
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(Error::Other(
                    "sc_render: surface texture failed validation".into(),
                ));
            }
        })
    }
}
