//! Where a renderer's frames go: a `CAMetalLayer` under the window's page on macOS (ADR-007), or
//! I420 frames sent to the page ("webview frames") through the live hub.

use sc_geometry::SizePx;
use sc_render::{Compositor, FrameJob, OutputFormat};

use crate::core::Core;
use crate::preview::StagePage;

pub enum Sink {
    Webview {
        channel: &'static str,
    },
    #[cfg(target_os = "macos")]
    Native {
        channel: &'static str,
        surface: sc_render::NativeSurface,
        told: Option<SizePx>,
    },
}

impl Sink {
    /// `layer` is the window's native layer, when it has one.
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    pub fn open(
        compositor: &Compositor,
        channel: &'static str,
        layer: impl FnOnce() -> Result<std::ptr::NonNull<std::ffi::c_void>, String>,
    ) -> Sink {
        #[cfg(target_os = "macos")]
        if std::env::var("SC_PREVIEW").as_deref() != Ok("webview") {
            let surface = layer().and_then(|layer| {
                // SAFETY: the layer lives as long as its window, which is never destroyed.
                unsafe { compositor.metal_layer_surface(layer) }.map_err(|e| e.to_string())
            });
            match surface {
                Ok(surface) => {
                    return Sink::Native {
                        channel,
                        surface,
                        told: None,
                    };
                }
                Err(e) => tracing::warn!(channel, "no native layer, using webview frames: {e}"),
            }
        }
        Sink::Webview { channel }
    }

    pub fn native(&self) -> bool {
        !matches!(self, Sink::Webview { .. })
    }

    /// Native layers draw at the display's pixel size, webview frames at the page's point size.
    pub fn scale(&self, page: StagePage) -> f64 {
        if self.native() { page.scale } else { 1.0 }
    }

    pub fn drawable(&self, page: StagePage) -> SizePx {
        let scale = self.scale(page);
        let px = |v: f64| ((v * scale).round() as u32).max(2);
        SizePx {
            w: px(page.w),
            h: px(page.h),
        }
    }

    pub fn forget_page(&mut self) {
        #[cfg(target_os = "macos")]
        if let Sink::Native { told, .. } = self {
            *told = None;
        }
    }

    /// Draws `job` with its canvas at `at` in the drawable (centred when `None`) over `clear`.
    /// `Ok(false)`: not on screen, nothing drawn.
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    pub fn show(
        &mut self,
        core: &Core,
        compositor: &mut Compositor,
        job: &FrameJob,
        drawable: SizePx,
        at: Option<(u32, u32)>,
        clear: [u8; 3],
    ) -> sc_render::Result<bool> {
        let canvas = job.canvas;
        match self {
            Sink::Webview { channel } => {
                let out = compositor.render(job, OutputFormat::I420Bt709Limited)?;
                core.live
                    .publish(channel, canvas.w, canvas.h, crate::live::I420, &out.data);
                Ok(true)
            }
            #[cfg(target_os = "macos")]
            Sink::Native {
                channel,
                surface,
                told,
            } => {
                let at = at.unwrap_or((
                    drawable.w.saturating_sub(canvas.w) / 2,
                    drawable.h.saturating_sub(canvas.h) / 2,
                ));
                let shown = compositor.present(job, surface, drawable, at, clear)?;
                if shown && *told != Some(canvas) {
                    core.live
                        .publish(channel, canvas.w, canvas.h, crate::live::NATIVE, &[]);
                    *told = Some(canvas);
                }
                Ok(shown)
            }
        }
    }
}

/// Puts a `CAMetalLayer` under window `label`'s page (main thread work, waited for here).
/// Returns the layer, its view and the page.
#[cfg(target_os = "macos")]
pub fn attach_layer(
    app: &tauri::AppHandle,
    label: &str,
) -> Result<(usize, usize, StagePage), String> {
    use tauri::Manager;
    let w = app
        .get_webview_window(label)
        .ok_or_else(|| format!("no {label} window"))?;
    let (tx, rx) = std::sync::mpsc::channel();
    w.with_webview(move |pw| {
        let made = std::ptr::NonNull::new(pw.inner())
            .ok_or_else(|| "no webview".to_string())
            .and_then(|wk| {
                // SAFETY: Tauri runs this on the main thread with the live WKWebView.
                unsafe { sc_capture_macos::preview::metal_layer_under(wk) }
                    .map_err(|e| e.to_string())
            });
        let _ =
            tx.send(made.map(|m| (m.layer.as_ptr() as usize, m.view.as_ptr() as usize, m.page)));
    })
    .map_err(|e| e.to_string())?;
    let (layer, view, page) = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("main thread: {e}"))??;
    Ok((
        layer,
        view,
        StagePage {
            w: page.w,
            h: page.h,
            scale: page.scale,
        },
    ))
}

/// Main thread: refits a view from [`attach_layer`] to its window's page.
#[cfg(target_os = "macos")]
pub fn refit(view: usize) -> Option<StagePage> {
    let view = std::ptr::NonNull::new(view as *mut std::ffi::c_void)?;
    // SAFETY: views from attach_layer live as long as their window; main thread only.
    let p = unsafe { sc_capture_macos::preview::fit_to_page(view) }?;
    Some(StagePage {
        w: p.w,
        h: p.h,
        scale: p.scale,
    })
}
