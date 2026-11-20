use std::path::Path;
use std::sync::Arc;

use sc_core::image::RgbaImage;
use sc_core::project::Background;
use sc_core::{Error, Result};
use sc_geometry::SizePx;

/// LRU of rasterized backgrounds keyed by (stable hash of Background, w, h).
#[derive(Debug)]
pub struct BackgroundCache {
    capacity: usize,
}

impl BackgroundCache {
    pub fn new(capacity: usize) -> BackgroundCache {
        BackgroundCache { capacity }
    }

    pub fn get(
        &mut self,
        bg: &Background,
        size: SizePx,
        project_dir: &Path,
    ) -> Result<Arc<RgbaImage>> {
        let _ = self.capacity;
        rasterize_background(bg, size, project_dir).map(Arc::new)
    }
}

pub fn rasterize_background(
    _bg: &Background,
    _size: SizePx,
    _project_dir: &Path,
) -> Result<RgbaImage> {
    Err(Error::Unsupported("sc_text: not implemented yet"))
}
