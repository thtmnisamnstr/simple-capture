//! journal.jsonl: append-only, fsync'd recording events (ARCHITECTURE §6, §12).

use std::path::Path;

use sc_geometry::{RectPt, RectPx, SizePx};
use serde::{Deserialize, Serialize};

use crate::project::{EndReason, SourceDescriptor, StartReason, SystemAudioMode, Via};
use crate::{ClockKind, Error, Result};

pub const FILE_NAME: &str = "journal.jsonl";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum JournalEvent {
    Start {
        start_host_ns: i64,
        clock: ClockKind,
        fps_requested: u32,
        resolution: String,
        system_audio: SystemAudioMode,
    },
    /// Written before a pre-created writer's file exists.
    SegmentPrepare {
        seg: String,
        file: String,
        host_ns: i64,
    },
    SegmentOpen {
        seg: String,
        track_id: String,
        start_rec_ns: i64,
        file: String,
        via: Via,
        reason: StartReason,
        source: SourceDescriptor,
        frame_px: SizePx,
        content_px: RectPx,
    },
    SwitchBegin {
        host_ns: i64,
        to: SourceDescriptor,
        via: Via,
    },
    SegmentClose {
        seg: String,
        end_rec_ns: i64,
        reason: EndReason,
        trailing_dropped: u32,
    },
    SwitchFailed {
        host_ns: i64,
        error: String,
        next: SwitchNext,
    },
    SourceLost {
        host_ns: i64,
        seg: String,
        reason: String,
    },
    SourceResized {
        host_ns: i64,
        seg: String,
        content_rect_pt: Option<RectPt>,
        content_scale: Option<f64>,
    },
    AudioGap {
        track: String,
        kind: GapKind,
        at_rec_ns: i64,
        dur_ns: i64,
    },
    /// Written from M7; folded by recovery now.
    Pause {
        host_ns: i64,
    },
    Resume {
        host_ns: i64,
    },
    Device {
        host_ns: i64,
        track_id: Option<String>,
        kind: DeviceKindJ,
        change: DeviceChange,
        name: String,
        unique_id: String,
    },
    TrackFinished {
        track_id: String,
        duration_ns: Option<i64>,
        samples: u64,
    },
    Health {
        host_ns: i64,
        rec_ns: i64,
        tracks: Vec<HealthEntry>,
    },
    Stop {
        stop_host_ns: i64,
        reason: StopReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchNext {
    RetryStopThenStart,
    RestoreOld,
    SourceLost,
    KeepOld,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    Gap,
    Overlap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKindJ {
    Camera,
    Mic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceChange {
    Lost,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthEntry {
    pub track_id: String,
    pub frames: u64,
    pub dropped: u64,
    pub writer_not_ready: u64,
    pub gaps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    User,
    Duration,
    Signal,
    Quit,
    Disk,
    Sleep,
    Error,
}

/// Appends one JSON line per event and fsyncs it.
#[derive(Debug)]
pub struct JournalWriter {
    _priv: (),
}

impl JournalWriter {
    /// Stub until wp-core.
    pub fn create(_dir: &Path) -> Result<JournalWriter> {
        Err(Error::Unsupported("sc_core::journal: not implemented yet"))
    }

    /// Write + fsync. Stub until wp-core.
    pub fn append(&mut self, _ev: &JournalEvent) -> Result<()> {
        Err(Error::Unsupported("sc_core::journal: not implemented yet"))
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct JournalRead {
    pub events: Vec<JournalEvent>,
    pub torn_tail: bool,
    pub unknown_lines: usize,
}

/// Stub until wp-core.
pub fn read_journal(_dir: &Path) -> Result<JournalRead> {
    Err(Error::Unsupported("sc_core::journal: not implemented yet"))
}
