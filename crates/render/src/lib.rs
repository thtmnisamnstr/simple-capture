//! Canvas compositor: background + contain-fit screen layer, BT.709 limited in, RGBA or I420
//! out (plan §4.6, ARCHITECTURE §8).

mod compositor;
mod diff;

pub mod cpu;

pub use compositor::{
    AdapterSummary, Backend, Compositor, FrameJob, OutputFormat, RenderedFrame, ScreenLayer,
};
pub use diff::{DiffStats, diff};
pub use sc_platform::{Error, Result};
