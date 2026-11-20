//! Recovery scan (ARCHITECTURE §12). Stub until wp-record-recovery.

use std::path::{Path, PathBuf};

use sc_platform::Decoder;

use crate::{Error, Result};

const STUB: &str = "sc_record::recovery: not implemented yet";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Recovered,
    Damaged,
    AlreadyFinal,
    Locked,
    NotAProject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub dir: PathBuf,
    pub outcome: Outcome,
    pub segments: usize,
    pub dropped_segments: Vec<String>,
    pub notes: Vec<String>,
}

pub fn recover_project(_dir: &Path, _decoder: &dyn Decoder) -> Result<RecoveryReport> {
    Err(Error::Unsupported(STUB))
}

pub fn scan(_projects_dir: &Path, _decoder: &dyn Decoder) -> Result<Vec<RecoveryReport>> {
    Err(Error::Unsupported(STUB))
}
