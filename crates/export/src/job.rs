use std::path::PathBuf;
use std::sync::Arc;

use sc_geometry::SizeClass;
use sc_platform::{Error, Platform, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportSize {
    Reference,
    Class(SizeClass),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickExportRequest {
    pub project_dir: PathBuf,
    pub out_path: Option<PathBuf>,
    pub size: ExportSize,
    pub fps: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportState {
    Preparing,
    Rendering,
    Finalizing,
    Done { path: PathBuf },
    Canceled,
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportProgress {
    pub frames_done: u64,
    pub frames_total: u64,
    pub fps: f64,
    pub eta_s: Option<f64>,
}

pub trait ExportObserver: Send + Sync {
    fn on_state(&self, s: &ExportState);
    fn on_progress(&self, p: &ExportProgress);
}

const STUB: &str = "sc_export: not implemented yet";

#[derive(Debug)]
pub struct ExportJob {
    _priv: (),
}

impl ExportJob {
    pub fn start(
        _platform: Arc<dyn Platform>,
        _req: QuickExportRequest,
        _obs: Arc<dyn ExportObserver>,
    ) -> Result<ExportJob> {
        Err(Error::Unsupported(STUB))
    }

    pub fn cancel(&self) {}

    pub fn wait(self) -> Result<PathBuf> {
        Err(Error::Unsupported(STUB))
    }
}
