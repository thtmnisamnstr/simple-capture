//! `<project>/session.log`: plain lines, no tracing (plan §10 item 17).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::folder::{LocalDateTime, rfc3339};
use crate::{Error, Result};

pub const FILE_NAME: &str = "session.log";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

/// Append-only, one `write` per line (so each line is flushed), no fsync:
/// `"<rfc3339> <host_ns> LEVEL msg"`. Newlines in `msg` are escaped as `\n`.
#[derive(Debug)]
pub struct SessionLog {
    file: File,
    path: PathBuf,
}

impl SessionLog {
    pub fn open(dir: &Path) -> Result<SessionLog> {
        let path = dir.join(FILE_NAME);
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|e| Error::io_at(e, "open session log", &path))?;
        Ok(SessionLog { file, path })
    }

    pub fn line(&mut self, level: Level, msg: &str) -> Result<()> {
        let line = format_line(
            &LocalDateTime::now(),
            crate::time::host_now_ns(),
            level,
            msg,
        );
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.flush())
            .map_err(|e| Error::io_at(e, "write session log", &self.path))
    }
}

/// One session.log line, newline included.
pub fn format_line(t: &LocalDateTime, host_ns: i64, level: Level, msg: &str) -> String {
    let msg = msg.replace('\r', "\\r").replace('\n', "\\n");
    format!("{} {host_ns} {} {msg}\n", rfc3339(t), level.as_str())
}
