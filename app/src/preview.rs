//! The preview window's state (ADR-043): the camera placement shown and edited there, and what
//! the stage renderer draws.

use std::time::Instant;

use sc_core::project::CameraKey;
use sc_platform::DisplayInfo;

use crate::dto::SourceSpecDto;
use crate::settings::{CanvasDto, RecorderSettings};

#[derive(Debug, Default)]
pub struct PreviewState {
    pub key: Option<CameraKey>,
    pub panel_open: bool,
    pub stage: Option<(SourceSpecDto, CanvasDto)>,
    pub last_key_sent: Option<Instant>,
    pub placed: bool,
    /// Where `place_preview` put the window: the move event that follows is not the user's.
    pub placed_at: Option<sc_geometry::PointF>,
    /// A position waiting to be saved (moves are saved after the window comes to rest).
    pub moved_to: Option<sc_geometry::PointF>,
    pub frames: u64,
    pub last: Option<StageFrame>,
    pub page: Option<StagePage>,
    pub view: Option<usize>,
}

/// The last frame the stage renderer showed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageFrame {
    pub native: bool,
    pub drawable: sc_geometry::SizePx,
    pub canvas: sc_geometry::SizePx,
    /// The camera's shape rectangle in canvas pixels, when it was drawn.
    pub camera: Option<sc_geometry::RectF>,
}

/// The preview page's size in points (the window's content area; on macOS below the title bar)
/// and the window's backing scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StagePage {
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

pub fn display_for<'a>(
    s: &RecorderSettings,
    displays: &'a [DisplayInfo],
) -> Option<&'a DisplayInfo> {
    let id = match &s.source {
        Some(SourceSpecDto::Display { display_id })
        | Some(SourceSpecDto::Application { display_id, .. })
        | Some(SourceSpecDto::Region { display_id, .. }) => Some(display_id.as_str()),
        _ => None,
    };
    displays
        .iter()
        .find(|d| Some(d.id.0.as_str()) == id)
        .or_else(|| displays.iter().find(|d| d.is_main))
        .or(displays.first())
}
