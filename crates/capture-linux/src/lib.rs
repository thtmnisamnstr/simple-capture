//! linux backend placeholder: every call returns `Unsupported` until M6, and every
//! capability is off.

use std::sync::Arc;

use sc_core::ClockKind;
use sc_platform::Platform;
use sc_platform::unsupported::UnsupportedPlatform;

pub const UNSUPPORTED: &str = "linux capture arrives in M6";

pub fn platform() -> Arc<dyn Platform> {
    Arc::new(UnsupportedPlatform::new(
        "linux",
        ClockKind::ClockMonotonic,
        UNSUPPORTED,
    ))
}
