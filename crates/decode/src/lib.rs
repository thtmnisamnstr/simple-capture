//! Segment-aware frames and continuous track audio in recording time (plan §4.6,
//! ARCHITECTURE §7).

mod audio;
mod frames;

pub use audio::TrackAudio;
pub use frames::{SegmentFrame, SegmentFrameSource, cfr_frame_times};
pub use sc_platform::{Error, Result};
