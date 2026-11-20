//! Text layout: M4 placeholder; every function returns `Unsupported`.

use sc_core::{Error, Result};

pub fn layout_placeholder() -> Result<()> {
    Err(Error::Unsupported("text layout arrives in M4"))
}
