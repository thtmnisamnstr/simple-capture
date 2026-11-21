//! Linux capture backend (ADR-048): screens and windows through the xdg-desktop-portal
//! ScreenCast picker and PipeWire, cameras through V4L2, the microphone and system audio
//! through cpal's PipeWire host, raw tracks through the FFmpeg helper. Elsewhere it builds as an
//! unsupported stub so the workspace compiles on every OS.

use std::sync::Arc;

use sc_platform::Platform;

#[cfg(target_os = "linux")]
mod imp;

#[cfg(target_os = "linux")]
#[doc(hidden)]
pub use imp::node_stream;

pub fn platform() -> Arc<dyn Platform> {
    #[cfg(target_os = "linux")]
    return imp::platform();
    #[cfg(not(target_os = "linux"))]
    Arc::new(sc_platform::unsupported::UnsupportedPlatform::new(
        "linux",
        sc_core::ClockKind::ClockMonotonic,
        "the Linux backend runs on Linux",
    ))
}
