//! macOS backend (M1 plan §4.4). Every TCC-touching call goes through `tcc::guard` first.
#![cfg(target_os = "macos")]

mod platform_impl;
mod tcc;
mod util;

pub mod av;
pub mod mainloop;
pub mod preview;
pub mod sck;

use std::sync::Arc;

use sc_platform::{Decoder, Platform, WriterFactory};

/// The macOS platform. Creating it touches no TCC-protected API.
pub fn platform() -> Arc<dyn Platform> {
    Arc::new(platform_impl::MacPlatform::new())
}

/// AVAssetWriter-backed track writers; no TCC.
pub fn writers() -> Arc<dyn WriterFactory> {
    Arc::new(av::MacWriterFactory::new())
}

/// AVAssetReader-backed decoder; no TCC.
pub fn decoder() -> Arc<dyn Decoder> {
    Arc::new(av::MacDecoder::new())
}
