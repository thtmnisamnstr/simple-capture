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
