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

#[cfg(target_os = "linux")]
pub use imp::portal::{LastPick, last_pick, restore_last, set_state_dir};

#[cfg(target_os = "linux")]
pub mod shortcuts {
    pub use crate::imp::shortcuts::{Bound, Wanted, bind, wayland, xdg_trigger};
}

/// Names the process after the app ID before GTK starts: GTK makes the Wayland app_id and the X11
/// WM_CLASS from it, and desktops match those to the installed desktop file for the window's icon,
/// name and dock entry (Flatpak expects the same ID).
#[cfg(target_os = "linux")]
pub fn set_program_name() {
    gstreamer::glib::set_prgname(Some(imp::portal::APP_ID));
}

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
