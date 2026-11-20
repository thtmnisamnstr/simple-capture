use std::path::PathBuf;

use sc_core::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub frame_bytes: usize,
    pub queue_frames: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub frame: u64,
    pub out_time_ns: i64,
    pub speed: Option<f64>,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitReport {
    pub code: Option<i32>,
    pub stderr_tail: Vec<String>,
    pub frames_written: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelReport {
    pub escalated_to_kill: bool,
    pub waited_ms: u64,
}

/// A helper fed raw frames on stdin through a bounded queue.
#[derive(Debug)]
pub struct HelperProcess {
    _priv: (),
}

const STUB: &str = "sc_helpers: not implemented yet";

impl HelperProcess {
    pub fn spawn(_spec: SpawnSpec) -> Result<HelperProcess> {
        Err(Error::Unsupported(STUB))
    }

    /// Blocks when the queue is full.
    pub fn write_frame(&self, _bytes: &[u8]) -> Result<()> {
        Err(Error::Unsupported(STUB))
    }

    pub fn progress(&self) -> Option<Progress> {
        None
    }

    pub fn finish(self) -> Result<ExitReport> {
        Err(Error::Unsupported(STUB))
    }

    /// SIGTERM → 1.5 s → SIGKILL.
    pub fn cancel(self) -> CancelReport {
        CancelReport {
            escalated_to_kill: false,
            waited_ms: 0,
        }
    }
}
