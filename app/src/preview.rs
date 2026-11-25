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
    /// The source a recording switched to; it differs from the saved settings until the take ends.
    pub switched_to: Option<SourceSpecDto>,
    pub last_key_sent: Option<Instant>,
    pub placed: bool,
    /// Where `place_preview` put the window: the move event that follows is not the user's.
    pub placed_at: Option<sc_geometry::PointF>,
    /// A position waiting to be saved (moves are saved after the window comes to rest).
    pub moved_to: Option<sc_geometry::PointF>,
    pub frames: u64,
    pub last: Option<StageFrame>,
    pub page: Option<StagePage>,
    #[cfg(target_os = "macos")]
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

/// A `size` with about the same area at `ratio` (width / height), grown to `min` and then shrunk
/// to `max` without changing the ratio.
pub fn reshaped(size: (f64, f64), ratio: f64, min: (f64, f64), max: (f64, f64)) -> (f64, f64) {
    let area = (size.0 * size.1).max(1.0);
    let mut w = (area * ratio).sqrt();
    let mut h = w / ratio;
    let up = (min.0 / w).max(min.1 / h).max(1.0);
    (w, h) = (w * up, h * up);
    let down = (max.0 / w).min(max.1 / h).min(1.0);
    (w * down, h * down)
}

#[cfg(test)]
mod reshape_tests {
    use super::reshaped;

    #[test]
    fn keeps_the_area_and_takes_the_ratio() {
        let (w, h) = reshaped((560.0, 315.0), 9.0 / 16.0, (240.0, 160.0), (2000.0, 1000.0));
        assert!((w / h - 9.0 / 16.0).abs() < 1e-9);
        assert!((w * h - 560.0 * 315.0).abs() < 1.0);
        assert!(h > w);
    }

    #[test]
    fn stays_within_the_limits_at_the_ratio() {
        let (w, h) = reshaped((300.0, 170.0), 9.0 / 16.0, (240.0, 160.0), (2000.0, 600.0));
        assert!(w >= 240.0 - 1e-9 && h <= 600.0 + 1e-9, "{w}×{h}");
        assert!((w / h - 9.0 / 16.0).abs() < 1e-9);
        let (w, h) = reshaped((100.0, 100.0), 16.0 / 9.0, (240.0, 160.0), (2000.0, 1000.0));
        assert!(w >= 240.0 - 1e-9 && h >= 160.0 - 1e-9, "{w}×{h}");
    }
}
