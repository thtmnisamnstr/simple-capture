//! Skia FreeType build (ADR-018): background rasterization and PNG/JPEG decode in M1; text
//! layout arrives in M4 (plan §4.6).

mod background;
mod build;
mod codec;
mod raster;

pub mod layout;

pub use background::{BackgroundCache, rasterize_background};
pub use build::{BuildInfo, build_info};
pub use codec::{decode_image, encode_png};
pub use sc_core::{Error, Result};
