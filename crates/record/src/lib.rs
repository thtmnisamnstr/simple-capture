//! Recording orchestration (M1 plan §4.5): one `Session` per recording, segment-per-source
//! screen tracks, make-before-break switching, crash recovery.

mod error;

pub mod recovery;
pub mod session;
pub mod testhooks;

pub use error::{Error, Result};
pub use sc_core::journal::StopReason;
pub use session::{
    HealthSnapshot, Notice, RecordRequest, Session, SessionObserver, SessionState, StopSummary,
    TrackHealthSnap, Tuning,
};
