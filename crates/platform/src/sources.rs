use std::any::Any;
use std::fmt;
use std::sync::Arc;

use sc_core::project::SourceDescriptor;
use sc_geometry::{RectPt, SizePx};
use serde::{Deserialize, Serialize};

/// Identifies one capture stream; frames and events carry it so routing never guesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StreamTag(pub u64);

/// CGWindowID on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WindowId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DisplayId(pub String);

/// Test apps have no bundle id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AppRef {
    pub pid: i32,
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SourceSpec {
    Display {
        display: DisplayId,
    },
    Window {
        window: WindowId,
    },
    Application {
        display: DisplayId,
        app: AppRef,
    },
    /// Display-local points, top-left origin.
    Region {
        display: DisplayId,
        region_pt: RectPt,
    },
    Picked(PickedSource),
}

/// A source chosen in the system picker. `handle` holds the OS filter object.
#[derive(Clone)]
pub struct PickedSource {
    pub descriptor: SourceDescriptor,
    pub handle: Arc<dyn Any + Send + Sync>,
    /// "Pick a display, then set a region".
    pub region_pt: Option<RectPt>,
}

impl fmt::Debug for PickedSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PickedSource")
            .field("descriptor", &self.descriptor)
            .field("region_pt", &self.region_pt)
            .finish_non_exhaustive()
    }
}

/// Picked sources are equal when they hold the same OS filter object.
impl PartialEq for SourceSpec {
    fn eq(&self, other: &SourceSpec) -> bool {
        use SourceSpec::*;
        match (self, other) {
            (Display { display: a }, Display { display: b }) => a == b,
            (Window { window: a }, Window { window: b }) => a == b,
            (
                Application {
                    display: d1,
                    app: a1,
                },
                Application {
                    display: d2,
                    app: a2,
                },
            ) => d1 == d2 && a1 == a2,
            (
                Region {
                    display: d1,
                    region_pt: r1,
                },
                Region {
                    display: d2,
                    region_pt: r2,
                },
            ) => d1 == d2 && r1 == r2,
            (Picked(a), Picked(b)) => {
                Arc::ptr_eq(&a.handle, &b.handle) && a.region_pt == b.region_pt
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub id: DisplayId,
    pub name: String,
    /// Global points, top-left of the main display.
    pub frame_pt: RectPt,
    pub scale: f64,
    pub size_px: SizePx,
    pub is_main: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: WindowId,
    pub title: String,
    pub app_name: String,
    pub app: AppRef,
    /// Global points.
    pub frame_pt: RectPt,
    pub display: Option<DisplayId>,
    pub on_screen: bool,
    pub layer: i32,
    /// 0 = frontmost, from CGWindowListCopyWindowInfo order.
    pub z_index: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    pub app: AppRef,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SourceList {
    pub displays: Vec<DisplayInfo>,
    pub windows: Vec<WindowInfo>,
    pub apps: Vec<AppInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenStreamConfig {
    pub fps: u32,
    pub resolution: sc_core::project::Resolution,
    pub show_cursor: bool,
    pub show_clicks: bool,
    pub queue_depth: u32,
    pub exclude: Vec<WindowId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Camera,
    Mic,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceEvent {
    StreamStopped {
        stream: StreamTag,
        error: Option<String>,
    },
    SourceLost {
        stream: StreamTag,
        reason: String,
    },
    SourceResized {
        stream: StreamTag,
        content_rect_pt: Option<RectPt>,
        content_scale: Option<f64>,
    },
    DeviceLost {
        kind: DeviceKind,
        device_id: String,
    },
    DevicesChanged {
        kind: DeviceKind,
    },
    /// A menu or panel of ours opened: refresh exclusions.
    OwnWindowsChanged,
    SystemWillSleep,
    SystemDidWake,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureDevice {
    pub id: String,
    pub name: String,
    pub model_id: Option<String>,
    pub is_default: bool,
    pub kind_hint: Option<String>,
}

/// Default 1920×1080 @ 30.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraLimit {
    pub max: SizePx,
    pub max_fps: u32,
}

impl Default for CameraLimit {
    fn default() -> Self {
        CameraLimit {
            max: SizePx { w: 1920, h: 1080 },
            max_fps: 30,
        }
    }
}

/// Picker modes are fixed per launch (plan §10 item 10).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PickerRequest {
    pub exclude: Vec<WindowId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    Screen,
    Camera,
    Microphone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
    Restricted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayRole {
    Panel,
    FloatingBar,
    Popover,
    RegionOverlay,
    PickOverlay,
    Saved,
    Onboarding,
    Stage,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionKind {
    PerWindow,
    AutoHide,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub system_picker: bool,
    pub application_source: bool,
    pub app_audio: bool,
    pub show_clicks: bool,
    pub mic_modes: bool,
    pub switch_source: bool,
    pub exclusion: ExclusionKind,
}

impl Capabilities {
    /// Everything off (placeholder platforms).
    pub fn none() -> Capabilities {
        Capabilities {
            system_picker: false,
            application_source: false,
            app_audio: false,
            show_clicks: false,
            mic_modes: false,
            switch_source: false,
            exclusion: ExclusionKind::None,
        }
    }
}
