//! Quick export, ffprobe validation and the capability probe (plan §4.6, ARCHITECTURE §10–§11).

mod job;

pub mod audio;
pub mod encode;
pub mod plan;
pub mod probe;
pub mod recipe;
pub mod settings;
pub mod validate;

pub use job::{
    ExportJob, ExportObserver, ExportProgress, ExportSize, ExportState, QuickExportRequest,
};
pub use sc_platform::{Error, Result};
