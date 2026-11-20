//! `<project>/session.log`: plain lines, no tracing (plan §10 item 17).

use std::path::Path;

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

/// Append-only, line-buffered and flushed per line: `"<rfc3339> <host_ns> LEVEL msg"`.
#[derive(Debug)]
pub struct SessionLog {
    _priv: (),
}

impl SessionLog {
    pub fn open(_dir: &Path) -> Result<SessionLog> {
        Err(Error::Unsupported(
            "sc_core::sessionlog: not implemented yet",
        ))
    }

    pub fn line(&mut self, _level: Level, _msg: &str) -> Result<()> {
        Err(Error::Unsupported(
            "sc_core::sessionlog: not implemented yet",
        ))
    }
}
