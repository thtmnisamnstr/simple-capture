// Mock backend for the UI outside Tauri (Vite dev server, Browser pane, node tests).

import type {
  CapabilitiesDto,
  DevicesDto,
  DisplayInfo,
  PermissionsDto,
  RecorderSettings,
  RecoveryReportDto,
  SourceList,
} from './types.ts';

const display: DisplayInfo = {
  id: '1',
  name: 'Built-in Display',
  frame_pt: { x: 0, y: 0, w: 1512, h: 982 },
  scale: 2,
  size_px: { w: 3024, h: 1964 },
  is_main: true,
};

let settings: RecorderSettings = {
  source: { kind: 'display', display_id: '1' },
  canvas: { aspect: 'original', background: { kind: 'solid', color: [0.07, 0.07, 0.08, 1] }, background_image: null },
  camera: null,
  mic: null,
  system_audio: 'off',
  fps: 30,
  resolution: 'native',
  cursor: true,
  click_highlight: false,
};

const permissions: PermissionsDto = { screen: 'granted', camera: 'not_determined', mic: 'not_determined' };

const capabilities: CapabilitiesDto = {
  system_picker: true,
  application_source: true,
  app_audio: true,
  show_clicks: true,
  mic_modes: true,
  switch_source: true,
  exclusion: 'per_window',
  displays: [{ display_id: '1', can_60fps_native: true, can_60fps_1x: true }],
};

const sources: SourceList = { displays: [display], windows: [], apps: [] };
const devices: DevicesDto = { cameras: [], mics: [] };
const recovery: RecoveryReportDto[] = [];

type Handler = (args: Record<string, unknown>) => unknown;

const handlers: Record<string, Handler> = {
  get_permissions: () => ({ ...permissions }),
  request_permission: (a) => permissions[(a.kind === 'microphone' ? 'mic' : a.kind) as keyof PermissionsDto],
  open_permission_settings: () => null,
  list_displays: () => [display],
  list_sources: () => sources,
  list_devices: () => devices,
  show_mic_modes: () => null,
  get_settings: () => settings,
  set_settings: (a) => {
    settings = a.settings as RecorderSettings;
    return settings;
  },
  get_capabilities: () => capabilities,
  preview_still: () => 'data:image/gif;base64,R0lGODlhAQABAAAAACw=',
  choose_background_image: () => null,
  begin_region_select: () => null,
  begin_pick_mode: () => null,
  open_system_picker: () => null,
  start_recording: () => null,
  stop_recording: () => null,
  discard_recording: () => null,
  switch_source: () => null,
  quick_export: () => 'export-1',
  cancel_export: () => null,
  reveal: () => null,
  recovery_report: () => recovery,
};

export const MOCK_COMMANDS = Object.keys(handlers);

export async function mockInvoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const h = handlers[cmd];
  if (!h) throw new Error(`mock: unknown command ${cmd}`);
  return h(args) as T;
}
