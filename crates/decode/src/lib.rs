//! Segment-aware frames and continuous track audio in recording time (plan §4.6,
//! ARCHITECTURE §7).

mod audio;
mod frames;

pub use audio::TrackAudio;
pub use frames::{
    SEEK_AHEAD_NS, SegmentFrame, SegmentFrameSource, TrackFrameSource, cfr_frame_times,
};
pub use sc_platform::{Error, Result};
