//! Canvas compositor: background + contain-fit screen layer + camera overlay, BT.709 limited in, RGBA or I420
//! out (plan §4.6, ARCHITECTURE §8).

mod compositor;
mod diff;
mod gpu;
#[cfg(target_os = "macos")]
mod iosurface;
mod maths;
mod surface;

pub mod cpu;

pub use compositor::{
    AdapterSummary, Backend, CameraLayer, Compositor, FrameJob, OutputFormat, RenderedFrame,
    ScreenLayer, camera_layer,
};
pub use diff::{DiffStats, diff};
pub use sc_platform::{Error, Result};
pub use surface::NativeSurface;
