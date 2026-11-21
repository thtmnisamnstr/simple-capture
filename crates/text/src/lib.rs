//! Skia FreeType build (ADR-018): background rasterization, PNG/JPEG decode and text overlays
//! (ADR-020, ADR-021).

mod background;
mod build;
mod codec;
mod raster;

pub mod anim;
pub mod engine;
pub mod fonts;
pub mod item;

pub use background::{BackgroundCache, rasterize_background};
pub use build::{BuildInfo, build_info};
pub use codec::{decode_image, encode_png};
pub use engine::{TextEngine, TextFrame};
pub use sc_core::{Error, Result};
