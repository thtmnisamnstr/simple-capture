//! Recorder settings (`<data dir>/settings.json`), mirrored by `RecorderSettings` in
//! ui/src/lib/types.ts.

use std::path::{Path, PathBuf};

use sc_core::project::{Background, CameraKey, Fit, Rgba};
use sc_geometry::Aspect;
use serde::{Deserialize, Serialize};

use crate::dto::SourceSpecDto;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasDto {
    pub aspect: Aspect,
    pub background: Background,
    #[serde(default)]
    pub background_image: Option<String>,
    /// Fill crops the main picture to the frame's shape instead of fitting it inside.
    #[serde(default = "contain")]
    pub fit: Fit,
}

fn contain() -> Fit {
    Fit::Contain
}

impl Default for CanvasDto {
    fn default() -> Self {
        CanvasDto {
            aspect: Aspect::Original,
            background: Background::Solid {
                color: Rgba([0.07, 0.07, 0.08, 1.0]),
            },
            background_image: None,
            fit: Fit::Contain,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SystemAudioDto {
    #[default]
    Off,
    All,
    App,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ResolutionDto {
    #[default]
    #[serde(rename = "native")]
    Native,
    #[serde(rename = "1x")]
    OneX,
}

/// What opens when a recording is saved (SPEC Recording flow).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AfterRecording {
    #[default]
    Editor,
    Saved,
    /// Export with the settings the export sheet used last, then show the Saved window.
    QuickExport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecorderSettings {
    pub source: Option<SourceSpecDto>,
    pub canvas: CanvasDto,
    pub camera: Option<String>,
    pub mic: Option<String>,
    pub system_audio: SystemAudioDto,
    pub fps: u32,
    pub resolution: ResolutionDto,
    pub cursor: bool,
    pub click_highlight: bool,
    pub camera_key: CameraKey,
    #[serde(alias = "bubble_shown")]
    pub preview_shown: bool,
    /// Where the preview window was last moved to (global points, top-left).
    pub preview_at: Option<sc_geometry::PointF>,
    /// Where the recording control bar was last moved to (global points, top-left).
    pub bar_at: Option<sc_geometry::PointF>,
    pub after_recording: AfterRecording,
    /// Seconds of 3-2-1 before recording starts (0 = none).
    pub countdown_s: u32,
    /// After an export made from the Saved window (or right after recording), move the
    /// recording's raw files to the Trash.
    pub delete_raw_after_export: bool,
    /// Frame rate, drops and drift in the recording bar.
    pub debug_overlay: bool,
    /// Keys per global shortcut action (`""` = none); see `shortcuts::ACTIONS`.
    pub shortcuts: std::collections::BTreeMap<String, String>,
}

pub fn default_camera_key() -> CameraKey {
    CameraKey::default_placement()
}

impl Default for RecorderSettings {
    fn default() -> Self {
        RecorderSettings {
            source: None,
            canvas: CanvasDto::default(),
            camera: None,
            mic: None,
            system_audio: SystemAudioDto::Off,
            fps: 30,
            resolution: ResolutionDto::Native,
            cursor: true,
            click_highlight: false,
            camera_key: default_camera_key(),
            preview_shown: true,
            preview_at: None,
            bar_at: None,
            after_recording: AfterRecording::Editor,
            countdown_s: 3,
            delete_raw_after_export: false,
            debug_overlay: false,
            shortcuts: crate::shortcuts::defaults(),
        }
    }
}

impl RecorderSettings {
    /// Rejects values the recorder cannot honour (fps other than 30/60).
    pub fn validate(&self) -> Result<(), String> {
        if self.fps != 30 && self.fps != 60 {
            return Err(format!("fps must be 30 or 60, got {}", self.fps));
        }
        if self.countdown_s > 10 {
            return Err(format!(
                "countdown_s must be 0 to 10, got {}",
                self.countdown_s
            ));
        }
        let k = &self.camera_key;
        let finite = [
            k.center_norm.x,
            k.center_norm.y,
            k.size_norm,
            k.zoom,
            k.pan_norm.x,
            k.pan_norm.y,
            k.corner_radius,
        ];
        if finite.iter().any(|v| !v.is_finite()) {
            return Err("camera_key has a non-finite value".into());
        }
        Ok(())
    }
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(data_dir: &Path) -> SettingsStore {
        SettingsStore {
            path: data_dir.join("settings.json"),
        }
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A missing file gives the defaults; an unreadable one is logged and gives the defaults.
    pub fn load(&self) -> RecorderSettings {
        let bytes = match std::fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return RecorderSettings::default();
            }
            Err(e) => {
                tracing::warn!(path = %self.path.display(), "read settings: {e}");
                return RecorderSettings::default();
            }
        };
        match serde_json::from_slice::<RecorderSettings>(&bytes) {
            Ok(s) if s.validate().is_ok() => s,
            Ok(_) | Err(_) => {
                tracing::warn!(path = %self.path.display(), "settings.json invalid; using defaults");
                RecorderSettings::default()
            }
        }
    }

    pub fn save(&self, s: &RecorderSettings) -> Result<(), String> {
        s.validate()?;
        let bytes = serde_json::to_vec_pretty(s).map_err(|e| format!("encode settings: {e}"))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        }
        sc_core::fsio::atomic_write(&self.path, &bytes)
            .map_err(|e| format!("write {}: {e}", self.path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::project::GradientStop;
    use sc_geometry::{PointF, RectPt};

    fn temp_dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("sc-app-settings-{name}-{}", sc_core::ids::new_id()));
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    #[test]
    fn round_trip_through_file() {
        let dir = temp_dir("rt");
        let store = SettingsStore::new(&dir);
        assert_eq!(store.load(), RecorderSettings::default());
        let s = RecorderSettings {
            source: Some(SourceSpecDto::Region {
                display_id: "1".into(),
                region_pt: RectPt {
                    x: 10.0,
                    y: 20.0,
                    w: 640.0,
                    h: 360.0,
                },
            }),
            canvas: CanvasDto {
                aspect: Aspect::R16x9,
                background: Background::Radial {
                    stops: vec![GradientStop {
                        at: 0.0,
                        color: Rgba([1.0, 0.0, 0.0, 1.0]),
                        extra: Default::default(),
                    }],
                    center_norm: PointF { x: 0.5, y: 0.5 },
                    radius_norm: 0.7,
                },
                background_image: Some("/tmp/bg.png".into()),
                fit: Fit::Cover,
            },
            camera: Some("cam-1".into()),
            mic: None,
            system_audio: SystemAudioDto::App,
            fps: 60,
            resolution: ResolutionDto::OneX,
            cursor: false,
            click_highlight: true,
            camera_key: CameraKey {
                mirror: true,
                zoom: 1.5,
                ..default_camera_key()
            },
            preview_shown: false,
            preview_at: None,
            bar_at: Some(sc_geometry::PointF { x: 10.0, y: 20.0 }),
            after_recording: AfterRecording::QuickExport,
            countdown_s: 5,
            delete_raw_after_export: true,
            debug_overlay: true,
            shortcuts: [("record".to_string(), "Control+Alt+KeyR".to_string())].into(),
        };
        store.save(&s).expect("save");
        assert_eq!(store.load(), s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn matches_ui_json_shape() {
        let json = r##"{"source":{"kind":"display","display_id":"main"},
            "canvas":{"aspect":"9:16","background":{"kind":"solid","color":[0,0,0,1]},"background_image":null},
            "camera":null,"mic":"m","system_audio":"all","fps":30,"resolution":"1x",
            "cursor":true,"click_highlight":false}"##;
        let s: RecorderSettings = serde_json::from_str(json).expect("parse");
        assert_eq!(s.canvas.aspect, Aspect::R9x16);
        assert_eq!(s.resolution, ResolutionDto::OneX);
        assert_eq!(s.system_audio, SystemAudioDto::All);
        let back = serde_json::to_value(&s).expect("encode");
        assert_eq!(back["resolution"], "1x");
        assert_eq!(back["canvas"]["aspect"], "9:16");
        assert_eq!(back["source"]["kind"], "display");
    }

    #[test]
    fn bad_file_and_bad_fps_fall_back() {
        let dir = temp_dir("bad");
        let store = SettingsStore::new(&dir);
        std::fs::write(store.path(), b"{not json").expect("write");
        assert_eq!(store.load(), RecorderSettings::default());
        let s = RecorderSettings {
            fps: 25,
            ..RecorderSettings::default()
        };
        assert!(store.save(&s).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
