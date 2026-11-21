//! The edited timeline: its audio mix, its pictures and (with the `output` feature) playback.

pub mod audio;
#[cfg(feature = "output")]
pub mod output;
pub mod video;

use sc_core::edit::Doc;
use sc_core::project::{Clip, Extra, ProjectV1, Speed};

/// The project's edits; a project without clips plays its whole recording as one clip.
pub fn timeline_doc(project: &ProjectV1) -> Doc {
    let mut doc = Doc::of(project);
    if doc.edl.clips.is_empty()
        && let Some(d) = project.duration_ns().filter(|d| *d > 0)
    {
        doc.edl.clips.push(Clip {
            id: "c0".into(),
            src_in_rec_ns: 0,
            src_out_rec_ns: d,
            speed: Speed::ONE,
            muted: false,
            extra: Extra::new(),
        });
    }
    doc
}
