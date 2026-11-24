// DTOs mirroring the app commands and events (plan §5.2). Hand-written, snake_case.

export type PermissionKind = 'screen' | 'camera' | 'microphone';
export type PermissionStatus = 'granted' | 'denied' | 'not_determined' | 'restricted' | 'unknown';
export interface PermissionsDto {
  screen: PermissionStatus;
  camera: PermissionStatus;
  mic: PermissionStatus;
}

export interface SizePx { w: number; h: number }
export interface RectPt { x: number; y: number; w: number; h: number }
export interface PointF { x: number; y: number }

export interface AppRef { pid: number; bundle_id: string | null }
export interface DisplayInfo {
  id: string;
  name: string;
  frame_pt: RectPt;
  scale: number;
  size_px: SizePx;
  is_main: boolean;
}
export interface WindowInfo {
  id: number;
  title: string;
  app_name: string;
  app: AppRef;
  frame_pt: RectPt;
  display: string | null;
  on_screen: boolean;
  layer: number;
  z_index: number;
}
export interface AppInfo { app: AppRef; name: string }
export interface SourceList { displays: DisplayInfo[]; windows: WindowInfo[]; apps: AppInfo[] }

export interface CaptureDevice {
  id: string;
  name: string;
  model_id: string | null;
  is_default: boolean;
  kind_hint: string | null;
}
export interface DevicesDto { cameras: CaptureDevice[]; mics: CaptureDevice[]; screens?: CaptureDevice[] }

export type ExclusionKind = 'per_window' | 'auto_hide' | 'none';
export interface DisplayProbeDto { display_id: string; can_60fps_native: boolean; can_60fps_1x: boolean }
export interface CapabilitiesDto {
  system_picker: boolean;
  application_source: boolean;
  app_audio: boolean;
  show_clicks: boolean;
  /** Simple Capture draws the click highlights itself (the system cannot). */
  own_clicks?: boolean;
  mic_modes: boolean;
  switch_source: boolean;
  exclusion: ExclusionKind;
  displays: DisplayProbeDto[];
}

export type Purpose = 'start' | 'switch';
export type Via = 'own_chooser' | 'system_picker' | 'portal';

export type SourceSpecDto =
  | { kind: 'display'; display_id: string }
  | { kind: 'window'; window_id: number }
  | { kind: 'application'; display_id: string; pid: number; bundle_id: string | null }
  | { kind: 'region'; display_id: string; region_pt: RectPt }
  | { kind: 'picked'; pick_id: string; region_pt: RectPt | null }
  | { kind: 'camera'; device_id: string }
  | { kind: 'device'; device_id: string };

export type Rgba = [number, number, number, number];
export interface GradientStop { at: number; color: Rgba }
export type Background =
  | { kind: 'solid'; color: Rgba }
  | { kind: 'linear'; stops: GradientStop[]; angle_deg: number }
  | { kind: 'radial'; stops: GradientStop[]; center_norm: PointF; radius_norm: number }
  | { kind: 'image'; asset: string; sha256: string; fit: 'cover'; focus_norm: PointF };

export type Aspect = 'original' | '16:9' | '4:3' | '1:1' | '4:5' | '9:16';
export interface CanvasDto { aspect: Aspect; background: Background; background_image: string | null; fit?: 'contain' | 'cover' }

export interface RecorderSettings {
  source: SourceSpecDto | null;
  canvas: CanvasDto;
  camera: string | null;
  camera_quality?: 'auto' | 'p720' | 'p1080x60' | 'p2160';
  mic: string | null;
  system_audio: 'off' | 'all' | 'app';
  fps: 30 | 60;
  resolution: 'native' | '1x';
  cursor: boolean;
  click_highlight: boolean;
  camera_key: CameraKey;
  preview_shown: boolean;
  preview_at?: PointF | null;
  bar_at?: PointF | null;
  after_recording?: 'editor' | 'saved' | 'quick_export';
  countdown_s?: number;
  delete_raw_after_export?: boolean;
  debug_overlay?: boolean;
}

export type CameraShape = 'circle' | 'square' | 'rect_4_3' | 'rect_16_9' | 'rect_3_4' | 'rect_9_16' | 'rounded';

export interface CameraKey {
  t_rec_ns: number;
  visible: boolean;
  shape: CameraShape;
  corner_radius: number;
  center_norm: PointF;
  size_norm: number;
  zoom: number;
  pan_norm: PointF;
  mirror: boolean;
  border?: { width_px_1080: number; color: [number, number, number, number] } | null;
  shadow?: { strength: number } | null;
  easing: 'step' | 'linear' | 'ease_in_out';
}

export interface RecordRequestDto {
  source: SourceSpecDto;
  via: Via;
  title: string | null;
  settings: RecorderSettings;
}

export type RecordingState =
  | { state: 'idle' }
  | { state: 'preparing' }
  | { state: 'recording'; elapsed_ns: number }
  | { state: 'switching'; elapsed_ns: number }
  | { state: 'stopping' }
  | { state: 'finalized'; project_dir: string; duration_ns?: number | null; tracks?: string[] }
  | { state: 'failed'; error: string };

export interface TrackHealthDto {
  track_id: string;
  frames: number;
  dropped: number;
  writer_not_ready: number;
  gaps: number;
  overlaps: number;
  fps: number;
  drift_ns: number;
}
export interface HealthDto { rec_ns: number; tracks: TrackHealthDto[]; free_disk_bytes: number; elapsed_ns?: number; paused?: boolean }

export type NoticeDto =
  | { kind: 'switch_failed'; error: string; restored: boolean }
  | { kind: 'source_lost'; segment: string }
  | { kind: 'device_lost'; device: 'camera' | 'mic'; name: string }
  | { kind: 'disk_low'; bytes_left: number }
  | { kind: 'audio_gap'; track: 'mic' | 'system_audio'; dur_ns: number }
  | { kind: 'frames_dropping'; track: string; pct: number }
  | { kind: 'stopped_for_sleep' }
  | { kind: 'stopped_for_disk' }
  | { kind: 'hotkey_unavailable'; error: string }
  | { kind: 'paused'; paused: boolean }
  | { kind: 'export_failed'; error: string };

export type ExportSizeDto = 'reference' | '2160' | '1440' | '1080' | '720' | '480';
export interface ExportProgressDto { id: string; frames_done: number; frames_total: number; fps: number; eta_s: number | null }
export type ExportStateDto =
  | { id: string; state: 'preparing' | 'rendering' | 'finalizing' | 'canceled' }
  | { id: string; state: 'done'; path: string }
  | { id: string; state: 'failed'; error: string };

export type RecoveryOutcome = 'recovered' | 'damaged' | 'already_final' | 'locked' | 'not_a_project';
export interface RecoveryReportDto {
  dir: string;
  outcome: RecoveryOutcome;
  segments: number;
  dropped_segments: string[];
  notes: string[];
}

export interface PickedDto { purpose: Purpose; source: SourceSpecDto; via: Via }
export interface RegionSelectedDto { purpose: Purpose; display_id: string; region_pt: RectPt }

export const EVENTS = {
  devicesChanged: 'devices-changed',
  regionSelected: 'region-selected',
  regionCancelled: 'region-cancelled',
  sourcePicked: 'source-picked',
  pickCancelled: 'pick-cancelled',
  recordingState: 'recording-state',
  health: 'health',
  notice: 'notice',
  exportProgress: 'export-progress',
  exportState: 'export-state',
} as const;

export interface PreviewStateDto { shown: boolean; key: CameraKey | null; frame: number; camera_open: string | null }
