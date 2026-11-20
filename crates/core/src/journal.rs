//! journal.jsonl: append-only, fsync'd recording events (ARCHITECTURE §6, §12).

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

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
    file: File,
    path: PathBuf,
}

impl JournalWriter {
    /// Opens `<dir>/journal.jsonl` for appending (created if missing, never truncated). If an
    /// earlier writer died mid-line, a newline is added first so the torn line stays on its own.
    pub fn create(dir: &Path) -> Result<JournalWriter> {
        let path = dir.join(FILE_NAME);
        let existed = path.exists();
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .read(true)
            .open(&path)
            .map_err(|e| Error::io_at(e, "open journal", &path))?;
        if existed
            && !ends_with_newline(&mut file).map_err(|e| Error::io_at(e, "read journal", &path))?
        {
            file.write_all(b"\n")
                .and_then(|()| file.sync_data())
                .map_err(|e| Error::io_at(e, "repair journal", &path))?;
        }
        if !existed {
            file.sync_all()
                .map_err(|e| Error::io_at(e, "fsync journal", &path))?;
            crate::fsio::fsync_dir(dir)?;
        }
        Ok(JournalWriter { file, path })
    }

    /// One `write` of the whole line, then fsync.
    pub fn append(&mut self, ev: &JournalEvent) -> Result<()> {
        let mut line = serde_json::to_vec(ev)?;
        line.push(b'\n');
        self.file
            .write_all(&line)
            .and_then(|()| self.file.sync_data())
            .map_err(|e| Error::io_at(e, "append to journal", &self.path))
    }
}

fn ends_with_newline(file: &mut File) -> std::io::Result<bool> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::Start(len - 1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct JournalRead {
    pub events: Vec<JournalEvent>,
    /// The last line was cut off mid-write (no newline and not a complete event, or NUL fill).
    pub torn_tail: bool,
    /// Complete lines that are not a known event: newer event kinds, or corrupt lines.
    pub unknown_lines: usize,
}

/// Reads `<dir>/journal.jsonl`. A missing journal reads as empty. A torn last line and unknown
/// lines are skipped and reported, never an error.
pub fn read_journal(dir: &Path) -> Result<JournalRead> {
    let path = dir.join(FILE_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(JournalRead::default()),
        Err(e) => return Err(Error::io_at(e, "read journal", &path)),
    };
    Ok(parse_journal(&bytes))
}

/// Parses journal bytes (see [`read_journal`]).
pub fn parse_journal(bytes: &[u8]) -> JournalRead {
    let mut out = JournalRead::default();
    let trimmed_len = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    if trimmed_len < bytes.len() {
        out.torn_tail = true;
    }
    let bytes = &bytes[..trimmed_len];
    let complete = bytes.last().is_none_or(|&b| b == b'\n');
    let mut lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
    if complete {
        lines.pop();
    }
    let last = lines.len().saturating_sub(1);
    for (i, raw) in lines.into_iter().enumerate() {
        let line = raw.strip_suffix(b"\r").unwrap_or(raw);
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<JournalEvent>(line) {
            Ok(ev) => out.events.push(ev),
            Err(_) if i == last && !complete => out.torn_tail = true,
            Err(_) => out.unknown_lines += 1,
        }
    }
    out
}
