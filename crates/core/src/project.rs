//! project.json v1 (ARCHITECTURE §6), field for field. Every struct keeps unknown fields in
//! `extra`, so a rewrite never drops data written by a newer version.

use std::collections::BTreeMap;
use std::path::Path;

use sc_geometry::{Aspect, PointF, Ratio, RectPt, RectPx, SizePx};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::time::{HostTime, PauseSpan, RecClock};
use crate::{ClockKind, Error, Result};

pub type Extra = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectV1 {
    pub schema_version: u32,
    pub id: String,
    pub created_at: String,
    pub app_version: String,
    pub state: ProjectState,
    pub title: String,
    pub recording: Recording,
    pub camera_layout: CameraLayout,
    pub edl: Edl,
    pub audio_mix: BTreeMap<String, AudioMixEntry>,
    pub text: Vec<TextItem>,
    pub output: Output,
    pub export_presets: ExportPresets,
    pub fonts: Vec<FontRef>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectState {
    Recording,
    Finalized,
    Recovered,
    Damaged,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    pub clock: ClockKind,
    pub start_host_ns: i64,
    pub stop_host_ns: Option<i64>,
    pub pauses: Vec<PauseRecord>,
    pub screen: ScreenSettings,
    pub tracks: Vec<Track>,
    pub system_audio: SystemAudioMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_host: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PauseRecord {
    pub start_host_ns: i64,
    pub end_host_ns: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Capture resolution: native pixels or 1× points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Resolution {
    #[serde(rename = "native")]
    Native,
    #[serde(rename = "1x")]
    OneX,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemAudioMode {
    Off,
    All,
    App,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenSettings {
    pub fps_requested: u32,
    pub resolution: Resolution,
    pub cursor: bool,
    pub click_highlight: bool,
    pub segments: Vec<ScreenSegment>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenSegment {
    pub id: String,
    pub track_id: String,
    pub start_rec_ns: i64,
    pub end_rec_ns: Option<i64>,
    pub start_reason: StartReason,
    pub end_reason: Option<EndReason>,
    pub via: Via,
    pub source: SourceDescriptor,
    pub frame_px: SizePx,
    pub content_px: RectPx,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartReason {
    RecordStart,
    UserSwitch,
    SourceLost,
    SourceResized,
    Recovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    UserSwitch,
    SourceLost,
    SourceResized,
    RecordStop,
    Recovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    OwnChooser,
    SystemPicker,
    Portal,
}

/// What a segment captured. `Region.region_pt` is display-local points, top-left origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceDescriptor {
    Display {
        display_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        scale: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame_pt: Option<RectPt>,
    },
    Window {
        window_id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_bundle_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window_title: Option<String>,
        scale: f64,
    },
    Application {
        display_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_bundle_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_pid: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_name: Option<String>,
        scale: f64,
    },
    Region {
        display_id: String,
        scale: f64,
        region_pt: RectPt,
        method: RegionMethod,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionMethod {
    SourceRect,
    Crop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub aspect: Aspect,
    pub ratio: Ratio,
    pub fit: Fit,
    pub background: Background,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Contain,
    Cover,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Background {
    Solid {
        color: Rgba,
    },
    Linear {
        stops: Vec<GradientStop>,
        angle_deg: f64,
    },
    Radial {
        stops: Vec<GradientStop>,
        center_norm: PointF,
        radius_norm: f64,
    },
    Image {
        asset: String,
        sha256: String,
        fit: Fit,
        focus_norm: PointF,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub at: f64,
    pub color: Rgba,
    #[serde(flatten)]
    pub extra: Extra,
}

/// sRGB, straight alpha, 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rgba(pub [f64; 4]);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub kind: TrackKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_id: Option<String>,
    pub file: String,
    /// `"mov_fragmented"` on macOS; `"sctrk"` for synthetic tracks.
    pub container: String,
    /// `"hevc" | "h264" | "aac"` (synthetic: `"sctrk"`).
    pub codec: String,
    pub first_sample_offset_ns: i64,
    pub duration_ns: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceInfo>,
    pub health: TrackHealth,
    pub fragment_interval_s: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Screen,
    Camera,
    Mic,
    SystemAudio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoInfo {
    pub size_px: SizePx,
    pub nominal_fps: u32,
    pub vfr: bool,
    pub keyframe_interval_s: f64,
    pub color: ColorInfo,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorInfo {
    pub primaries: String,
    pub transfer: String,
    pub matrix: String,
    pub range: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: u16,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub unique_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackHealth {
    pub frames: u64,
    pub dropped: u64,
    pub writer_not_ready: u64,
    pub gaps: Vec<GapRecord>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapRecord {
    pub at_rec_ns: i64,
    pub dur_ns: i64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraLayout {
    pub overlays: Vec<CameraOverlay>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraOverlay {
    pub id: String,
    pub track_id: String,
    pub keyframes: Vec<CameraKey>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraKey {
    pub t_rec_ns: i64,
    pub visible: bool,
    pub shape: CameraShape,
    pub corner_radius: f64,
    pub center_norm: PointF,
    pub size_norm: f64,
    pub zoom: f64,
    pub pan_norm: PointF,
    pub mirror: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<CameraBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<CameraShadow>,
    pub easing: Easing,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraShape {
    Circle,
    Square,
    #[serde(rename = "rect_4_3")]
    Rect4x3,
    #[serde(rename = "rect_16_9")]
    Rect16x9,
    #[serde(rename = "rect_3_4")]
    Rect3x4,
    #[serde(rename = "rect_9_16")]
    Rect9x16,
    Rounded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraBorder {
    pub width_px_1080: f64,
    pub color: Rgba,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraShadow {
    pub strength: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Step,
    Linear,
    EaseInOut,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edl {
    pub clips: Vec<Clip>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub src_in_rec_ns: i64,
    pub src_out_rec_ns: i64,
    pub speed: Speed,
    pub muted: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Speed {
    pub num: u32,
    pub den: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioMixEntry {
    pub gain_db: f64,
    pub muted: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub canvas: Canvas,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_framing: Option<BTreeMap<String, SegmentFraming>>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Reserved for cover + pan (M3+).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentFraming {
    pub fit: Fit,
    pub focus_norm: PointF,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportPresets {
    /// `"mp4" | "webm" | "gif"`.
    pub last_format: String,
    /// ExportSettings per format; the settings type arrives with M5.
    pub per_format: Map<String, Value>,
    pub named: Vec<NamedPreset>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedPreset {
    pub name: String,
    pub settings: Value,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontRef {
    pub family: String,
    pub source: FontSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_index: Option<u32>,
    pub sha256: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontSource {
    Bundled,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextItem {
    pub id: String,
    pub start_tl_ns: i64,
    pub end_tl_ns: i64,
    pub layer: i32,
    pub content: String,
    pub style: TextStyle,
    pub transform: TextTransform,
    pub anim: TextAnim,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub font_family: String,
    pub size_1080: f64,
    pub weight: u32,
    pub italic: bool,
    pub uppercase: bool,
    pub align: TextAlign,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub opacity: f64,
    pub fill: TextFill,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<TextOutline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<TextShadow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow: Option<TextGlow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<TextBackground>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// `{ "solid": Rgba }` or `{ "linear": { stops, angle_deg } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFill {
    Solid(Rgba),
    Linear(TextLinearFill),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextLinearFill {
    pub stops: [Rgba; 2],
    pub angle_deg: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextOutline {
    pub width_1080: f64,
    pub color: Rgba,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextShadow {
    pub blur_1080: f64,
    pub offset_1080: PointF,
    pub color: Rgba,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextGlow {
    pub radius_1080: f64,
    pub color: Rgba,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBackground {
    pub kind: TextBackgroundKind,
    pub color: Rgba,
    pub radius_1080: f64,
    pub padding_1080: PointF,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextBackgroundKind {
    PerLine,
    Merged,
    Inverted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextTransform {
    pub center_norm: PointF,
    pub scale: f64,
    pub rotation_deg: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextAnim {
    #[serde(rename = "in", default, skip_serializing_if = "Option::is_none")]
    pub in_: Option<Anim>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out: Option<Anim>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emphasis: Option<Emphasis>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anim {
    pub kind: String,
    pub duration_s: f64,
    pub easing: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Emphasis {
    pub kind: EmphasisKind,
    pub period_s: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmphasisKind {
    Pulse,
    Wiggle,
    Float,
}

impl ProjectV1 {
    pub const SCHEMA_VERSION: u32 = 1;
    pub const FILE_NAME: &'static str = "project.json";

    /// A fresh project in state `recording`: 30 fps, native resolution, cursor on, no system
    /// audio, `Original` aspect (resolved at record start) with a dark solid background.
    pub fn new_recording(
        id: String,
        title: String,
        created_at: String,
        app_version: String,
        clock: ClockKind,
    ) -> ProjectV1 {
        ProjectV1 {
            schema_version: Self::SCHEMA_VERSION,
            id,
            created_at,
            app_version,
            state: ProjectState::Recording,
            title,
            recording: Recording {
                clock,
                start_host_ns: 0,
                stop_host_ns: None,
                pauses: Vec::new(),
                screen: ScreenSettings {
                    fps_requested: 30,
                    resolution: Resolution::Native,
                    cursor: true,
                    click_highlight: false,
                    segments: Vec::new(),
                    extra: Extra::new(),
                },
                tracks: Vec::new(),
                system_audio: SystemAudioMode::Off,
                permission_host: None,
                extra: Extra::new(),
            },
            camera_layout: CameraLayout {
                overlays: Vec::new(),
                extra: Extra::new(),
            },
            edl: Edl {
                clips: Vec::new(),
                extra: Extra::new(),
            },
            audio_mix: BTreeMap::new(),
            text: Vec::new(),
            output: Output {
                canvas: Canvas {
                    aspect: Aspect::Original,
                    ratio: Ratio::new(16, 9),
                    fit: Fit::Contain,
                    background: Background::Solid {
                        color: Rgba([0.07, 0.07, 0.08, 1.0]),
                    },
                    extra: Extra::new(),
                },
                segment_framing: None,
                extra: Extra::new(),
            },
            export_presets: ExportPresets {
                last_format: "mp4".to_string(),
                per_format: Map::new(),
                named: Vec::new(),
                extra: Extra::new(),
            },
            fonts: Vec::new(),
            extra: Extra::new(),
        }
    }

    /// Reads `<dir>/project.json`. Unknown fields are kept in each struct's `extra`.
    pub fn load(dir: &Path) -> Result<ProjectV1> {
        let path = dir.join(Self::FILE_NAME);
        let bytes = std::fs::read(&path).map_err(|e| Error::io_at(e, "read", &path))?;
        let version = serde_json::from_slice::<Value>(&bytes)
            .map_err(|e| Error::Json(format!("{}: {e}", path.display())))?
            .get("schema_version")
            .and_then(Value::as_u64);
        if version != Some(u64::from(Self::SCHEMA_VERSION)) {
            return Err(Error::Invalid(format!(
                "{}: unsupported schema_version {version:?} (this build reads {})",
                path.display(),
                Self::SCHEMA_VERSION
            )));
        }
        serde_json::from_slice(&bytes).map_err(|e| Error::Json(format!("{}: {e}", path.display())))
    }

    /// Writes `<dir>/project.json` atomically (temp, fsync, rename, fsync folder).
    pub fn save(&self, dir: &Path) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        crate::fsio::atomic_write(&dir.join(Self::FILE_NAME), &bytes)
    }

    /// The recording's clock (start and pauses) as a [`RecClock`].
    pub fn rec_clock(&self) -> RecClock {
        RecClock {
            start: HostTime(self.recording.start_host_ns),
            pauses: self
                .recording
                .pauses
                .iter()
                .map(|p| PauseSpan {
                    start: HostTime(p.start_host_ns),
                    end: Some(HostTime(p.end_host_ns)),
                })
                .collect(),
        }
    }

    /// Recording length in RecTime: `stop − start − pauses` once stopped, otherwise the latest
    /// closed segment end. `None` while nothing is known.
    pub fn duration_ns(&self) -> Option<i64> {
        if let Some(stop) = self.recording.stop_host_ns {
            let clock = self.rec_clock();
            let d = stop
                .saturating_sub(self.recording.start_host_ns)
                .saturating_sub(clock.pause_total_before(HostTime(stop)));
            return Some(d.max(0));
        }
        self.recording
            .screen
            .segments
            .iter()
            .filter_map(|s| s.end_rec_ns)
            .max()
    }

    /// The segment whose `[start_rec_ns, end_rec_ns)` contains `t_rec` (an open segment, with no
    /// end yet, runs on forever). `None` before the first segment and inside gaps; the caller
    /// decides whether to hold the previous segment's last frame there.
    pub fn segment_at(&self, t_rec: i64) -> Option<&ScreenSegment> {
        self.recording
            .screen
            .segments
            .iter()
            .filter(|s| s.start_rec_ns <= t_rec && s.end_rec_ns.is_none_or(|e| t_rec < e))
            .max_by_key(|s| s.start_rec_ns)
    }
}

/// Copies a PNG/JPEG (recognised by its magic bytes) into
/// `<project_dir>/assets/background-<sha256[0..12]>.<png|jpg>` and returns the cover-fit, centred
/// image background that points at it. Copying the same image twice reuses the file.
pub fn copy_background_asset(src: &Path, project_dir: &Path) -> Result<Background> {
    let bytes = std::fs::read(src).map_err(|e| Error::io_at(e, "read background image", src))?;
    let ext = image_ext(&bytes).ok_or_else(|| {
        Error::Invalid(format!(
            "background image must be PNG or JPEG: {}",
            src.display()
        ))
    })?;
    let sha = crate::sha::sha256_bytes(&bytes);
    let asset = format!("assets/background-{}.{ext}", &sha[..12]);
    let dst = project_dir.join(&asset);
    let reuse = std::fs::metadata(&dst).is_ok_and(|m| m.len() == bytes.len() as u64)
        && crate::sha::sha256_file(&dst).is_ok_and(|h| h == sha);
    if !reuse {
        let assets = project_dir.join("assets");
        std::fs::create_dir_all(&assets).map_err(|e| Error::io_at(e, "create folder", &assets))?;
        crate::fsio::atomic_write(&dst, &bytes)?;
    }
    Ok(Background::Image {
        asset,
        sha256: sha,
        fit: Fit::Cover,
        focus_norm: PointF { x: 0.5, y: 0.5 },
    })
}

fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else {
        None
    }
}
