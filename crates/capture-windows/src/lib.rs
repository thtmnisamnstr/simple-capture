//! Windows capture backend (ADR-048): Windows.Graphics.Capture for screens and windows, Media
//! Foundation for cameras, WASAPI through cpal for the microphone and system audio, raw tracks
//! through the FFmpeg helper. Elsewhere it builds as an unsupported stub so the workspace
//! compiles on every OS.

use std::sync::Arc;

use sc_platform::Platform;

#[cfg(windows)]
mod imp;

pub fn platform() -> Arc<dyn Platform> {
    #[cfg(windows)]
    return imp::platform();
    #[cfg(not(windows))]
    Arc::new(sc_platform::unsupported::UnsupportedPlatform::new(
        "windows",
        sc_core::ClockKind::Qpc,
        "the Windows backend runs on Windows",
    ))
}
