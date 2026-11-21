// Typed wrappers for the app commands (plan §5.2). Falls back to the mock backend outside Tauri.

import { invoke } from '@tauri-apps/api/core';
import { mockInvoke } from './mock.ts';
import type {
  CapabilitiesDto,
  DevicesDto,
  DisplayInfo,
  ExportSizeDto,
  PermissionKind,
  PermissionStatus,
  PermissionsDto,
  Purpose,
  RecordRequestDto,
  RecorderSettings,
  RecoveryReportDto,
  SourceList,
  SourceSpecDto,
  CanvasDto,
  CameraKey,
  PreviewStateDto,
} from './types.ts';

export const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  return inTauri() ? invoke<T>(cmd, args) : mockInvoke<T>(cmd, args);
}

export const api = {
  getPermissions: () => call<PermissionsDto>('get_permissions'),
  requestPermission: (kind: PermissionKind) => call<PermissionStatus>('request_permission', { kind }),
  openPermissionSettings: (kind: PermissionKind) => call<void>('open_permission_settings', { kind }),
  listDisplays: () => call<DisplayInfo[]>('list_displays'),
  listSources: () => call<SourceList>('list_sources'),
  listDevices: () => call<DevicesDto>('list_devices'),
  showMicModes: () => call<void>('show_mic_modes'),
  getSettings: () => call<RecorderSettings>('get_settings'),
  setSettings: (settings: RecorderSettings) => call<RecorderSettings>('set_settings', { settings }),
  getCapabilities: () => call<CapabilitiesDto>('get_capabilities'),
  previewStill: (source: SourceSpecDto, canvas: CanvasDto) => call<string>('preview_still', { source, canvas }),
  chooseBackgroundImage: () => call<string | null>('choose_background_image'),
  beginRegionSelect: (purpose: Purpose, displayId?: string) =>
    call<void>('begin_region_select', { purpose, displayId: displayId ?? null }),
  beginPickMode: (purpose: Purpose) => call<void>('begin_pick_mode', { purpose }),
  openSystemPicker: (purpose: Purpose) => call<void>('open_system_picker', { purpose }),
  startRecording: (request: RecordRequestDto) => call<void>('start_recording', { request }),
  stopRecording: () => call<void>('stop_recording'),
  discardRecording: () => call<void>('discard_recording'),
  switchSource: (spec: SourceSpecDto) => call<void>('switch_source', { spec }),
  quickExport: (projectDir: string, size: ExportSizeDto) => call<string>('quick_export', { projectDir, size }),
  cancelExport: (id: string) => call<void>('cancel_export', { id }),
  reveal: (path: string) => call<void>('reveal', { path }),
  recoveryReport: () => call<RecoveryReportDto[]>('recovery_report'),
  cameraPreview: (active: boolean) => call<{ shown: boolean }>('camera_preview', { active }),
  previewState: () => call<PreviewStateDto>('preview_state'),
  cameraKeyLive: (key: CameraKey) => call<void>('camera_key_live', { key }),
  showVideoEffects: () => call<void>('show_video_effects'),
  barDrag: (phase: 'begin' | 'move' | 'end', x: number, y: number) => call<void>('bar_drag', { phase, x, y }),
};

export const COMMANDS = [
  'get_permissions',
  'request_permission',
  'open_permission_settings',
  'list_displays',
  'list_sources',
  'list_devices',
  'show_mic_modes',
  'get_settings',
  'set_settings',
  'get_capabilities',
  'preview_still',
  'choose_background_image',
  'begin_region_select',
  'begin_pick_mode',
  'open_system_picker',
  'start_recording',
  'stop_recording',
  'discard_recording',
  'switch_source',
  'quick_export',
  'cancel_export',
  'reveal',
  'recovery_report',
  'camera_preview',
  'preview_state',
  'camera_key_live',
  'show_video_effects',
  'bar_drag',
] as const;

export function errorText(err: unknown): string {
  if (err instanceof Error) return err.message;
  return typeof err === 'string' ? err : JSON.stringify(err);
}
