//! Clock model (ARCHITECTURE §5). All values are i64 nanoseconds.

use serde::{Deserialize, Serialize};

/// The OS host clock a recording is stamped with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockKind {
    MachHost,
    Qpc,
    ClockMonotonic,
}

/// Host clock time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct HostTime(pub i64);

/// Recording time: host time minus start minus pauses.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct RecTime(pub i64);

/// Edited-timeline time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct TlTime(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PauseSpan {
    pub start: HostTime,
    pub end: Option<HostTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecClock {
    pub start: HostTime,
    pub pauses: Vec<PauseSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToRec {
    BeforeStart,
    Paused,
    At(RecTime),
}

impl RecClock {
    /// Stub until wp-core.
    pub fn to_rec(&self, _h: HostTime) -> ToRec {
        ToRec::BeforeStart
    }

    /// Stub until wp-core.
    pub fn pause_total_before(&self, _h: HostTime) -> i64 {
        0
    }
}
