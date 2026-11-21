// Mock backend for the UI outside Tauri (Vite dev server, Browser pane, node tests).
// Browser scenarios via the query string, e.g. `?screen=denied`, `?state=recording`,
// `?notice=source_lost`, `?saved=1`, `?camera=1` (a camera chosen).

import type {
  Background,
  GradientStop,
  CapabilitiesDto,
  DevicesDto,
  DisplayInfo,
  ExportSizeDto,
  NoticeDto,
  PermissionStatus,
  PermissionsDto,
  RecorderSettings,
  RecordingState,
  RecoveryReportDto,
  SourceList,
  SourceSpecDto,
  CanvasDto,
  WindowInfo,
} from './types.ts';
import { busEmit } from './bus.ts';
import { aspectRatio, containFit, ratioOf, referenceCanvasSize } from './geometry.ts';
import { rgbaToCss } from './backgrounds.ts';
import { sourceSizePx } from './sources.ts';

function scenario(): URLSearchParams {
  try {
    return new URLSearchParams(globalThis.location?.search ?? '');
  } catch {
    return new URLSearchParams();
  }
}

const q = scenario();
const status = (name: string, fallback: PermissionStatus): PermissionStatus => {
  const v = q.get(name);
  return v === 'granted' || v === 'denied' || v === 'not_determined' || v === 'restricted' || v === 'unknown' ? v : fallback;
};

const displays: DisplayInfo[] = [
  { id: '1', name: 'Built-in Retina Display', frame_pt: { x: 0, y: 0, w: 1512, h: 982 }, scale: 2, size_px: { w: 3024, h: 1964 }, is_main: true },
  { id: '2', name: 'Samsung Odyssey G9', frame_pt: { x: 1512, y: 0, w: 3840, h: 1080 }, scale: 2, size_px: { w: 7680, h: 2160 }, is_main: false },
];

const app = (pid: number, bundle_id: string) => ({ pid, bundle_id });
const windows: WindowInfo[] = [
  { id: 101, title: 'Simple Capture — README.md', app_name: 'Code', app: app(501, 'com.microsoft.VSCode'), frame_pt: { x: 60, y: 60, w: 1100, h: 760 }, display: '1', on_screen: true, layer: 0, z_index: 0 },
  { id: 102, title: 'Docs', app_name: 'Safari', app: app(502, 'com.apple.Safari'), frame_pt: { x: 300, y: 140, w: 1100, h: 800 }, display: '1', on_screen: true, layer: 0, z_index: 1 },
  { id: 103, title: '', app_name: 'Terminal', app: app(503, 'com.apple.Terminal'), frame_pt: { x: 1700, y: 120, w: 900, h: 600 }, display: '2', on_screen: true, layer: 0, z_index: 2 },
  { id: 104, title: 'Notes', app_name: 'Notes', app: app(504, 'com.apple.Notes'), frame_pt: { x: 2800, y: 80, w: 220, h: 900 }, display: '2', on_screen: true, layer: 0, z_index: 3 },
  { id: 105, title: 'Hidden', app_name: 'Mail', app: app(505, 'com.apple.mail'), frame_pt: { x: 0, y: 0, w: 800, h: 600 }, display: '1', on_screen: false, layer: 0, z_index: 4 },
];

const sources: SourceList = {
  displays,
  windows,
  apps: [
    { app: app(501, 'com.microsoft.VSCode'), name: 'Code' },
    { app: app(502, 'com.apple.Safari'), name: 'Safari' },
    { app: app(503, 'com.apple.Terminal'), name: 'Terminal' },
    { app: app(504, 'com.apple.Notes'), name: 'Notes' },
  ],
};

const devices: DevicesDto = {
  cameras: [
    { id: 'cam-1', name: 'FaceTime HD Camera', model_id: null, is_default: true, kind_hint: 'builtin' },
    { id: 'cam-2', name: 'iPhone Camera', model_id: null, is_default: false, kind_hint: 'continuity' },
  ],
  mics: [
    { id: 'mic-1', name: 'MacBook Pro Microphone', model_id: null, is_default: true, kind_hint: 'builtin' },
    { id: 'mic-2', name: 'Shure MV7', model_id: null, is_default: false, kind_hint: 'usb' },
  ],
};

let settings: RecorderSettings = {
  source: { kind: 'display', display_id: '1' },
  canvas: { aspect: 'original', background: { kind: 'solid', color: [0.07, 0.07, 0.08, 1] }, background_image: null },
  camera: q.get('camera') ? 'cam-1' : null,
  mic: 'mic-1',
  system_audio: 'off',
  fps: 30,
  resolution: 'native',
  cursor: true,
  click_highlight: false,
  camera_key: {
    t_rec_ns: 0,
    visible: true,
    shape: 'circle',
    corner_radius: 0,
    center_norm: { x: 0.91, y: 0.84 },
    size_norm: 0.26,
    zoom: 1,
    pan_norm: { x: 0, y: 0 },
    mirror: false,
    border: null,
    shadow: null,
    easing: 'linear',
  },
  preview_shown: true,
  countdown_s: 3,
  delete_raw_after_export: false,
  debug_overlay: false,
};

const permissions: PermissionsDto = {
  screen: status('screen', 'granted'),
  camera: status('camera', 'not_determined'),
  mic: status('mic', 'not_determined'),
};

const capabilities: CapabilitiesDto = {
  system_picker: true,
  application_source: true,
  app_audio: true,
  show_clicks: q.get('clicks') !== 'off',
  mic_modes: true,
  switch_source: true,
  exclusion: 'per_window',
  displays: [
    { display_id: '1', can_60fps_native: true, can_60fps_1x: true },
    { display_id: '2', can_60fps_native: false, can_60fps_1x: true },
  ],
};

const recovery: RecoveryReportDto[] =
  q.get('recovered') === '1'
    ? [{ dir: '~/Movies/Simple Capture/Recording 2026-11-18 at 21.04.11.sc', outcome: 'recovered', segments: 2, dropped_segments: [], notes: [] }]
    : [];

const PROJECT_DIR = '~/Movies/Simple Capture/Recording 2026-11-19 at 10.00.00.sc';
const NS = 1_000_000_000;

let recState: RecordingState = { state: 'idle' };
let elapsedNs = 0;
let ticker: ReturnType<typeof setInterval> | null = null;

function setState(s: RecordingState) {
  recState = s;
  busEmit('recording-state', s);
}

function startTicker() {
  stopTicker();
  ticker = setInterval(() => {
    elapsedNs += NS;
    if (recState.state === 'recording' || recState.state === 'switching') setState({ state: recState.state, elapsed_ns: elapsedNs });
    busEmit('health', {
      rec_ns: elapsedNs,
      free_disk_bytes: 182_000_000_000,
      tracks: [{ track_id: 'screen', frames: (elapsedNs / NS) * 30, dropped: 0, writer_not_ready: 0, gaps: 0, overlaps: 0, fps: 30, drift_ns: 0 }],
    });
  }, 1000);
}

function stopTicker() {
  if (ticker) clearInterval(ticker);
  ticker = null;
}

const later = (ms: number, fn: () => void) => setTimeout(fn, ms);

function startRecording() {
  elapsedNs = 0;
  setState({ state: 'preparing' });
  later(300, () => {
    setState({ state: 'recording', elapsed_ns: 0 });
    startTicker();
  });
}

function stopRecording() {
  stopTicker();
  setState({ state: 'stopping' });
  later(500, () => setState({ state: 'finalized', project_dir: PROJECT_DIR }));
}

const exports = new Map<string, ReturnType<typeof setInterval>>();
let exportSeq = 0;

function quickExport(size: ExportSizeDto): string {
  const id = `export-${++exportSeq}`;
  const total = 30 * Math.max(1, Math.round(elapsedNs / NS) || 12);
  let done = 0;
  busEmit('export-state', { id, state: 'preparing' });
  const t = setInterval(() => {
    done = Math.min(total, done + Math.ceil(total / 12));
    busEmit('export-state', { id, state: 'rendering' });
    busEmit('export-progress', { id, frames_done: done, frames_total: total, fps: 240, eta_s: (total - done) / 240 });
    if (done >= total) {
      clearInterval(t);
      exports.delete(id);
      busEmit('export-state', { id, state: 'finalizing' });
      later(300, () => busEmit('export-state', { id, state: 'done', path: `${PROJECT_DIR.replace(/\.sc$/, '')} ${size}.mp4` }));
    }
  }, 250);
  exports.set(id, t);
  return id;
}

function svgBackground(bg: Background, w: number, h: number): string {
  const stops = (list: GradientStop[]) => list.map((x) => `<stop offset="${x.at}" stop-color="${rgbaToCss(x.color)}"/>`).join('');
  switch (bg.kind) {
    case 'solid':
      return `<rect width="${w}" height="${h}" fill="${rgbaToCss(bg.color)}"/>`;
    case 'linear': {
      const t = (bg.angle_deg * Math.PI) / 180;
      const dx = Math.sin(t) / 2;
      const dy = -Math.cos(t) / 2;
      return `<defs><linearGradient id="g" x1="${0.5 - dx}" y1="${0.5 - dy}" x2="${0.5 + dx}" y2="${0.5 + dy}">${stops(bg.stops)}</linearGradient></defs><rect width="${w}" height="${h}" fill="url(#g)"/>`;
    }
    case 'radial':
      return `<defs><radialGradient id="g" cx="${bg.center_norm.x}" cy="${bg.center_norm.y}" r="${bg.radius_norm}">${stops(bg.stops)}</radialGradient></defs><rect width="${w}" height="${h}" fill="url(#g)"/>`;
    case 'image':
      return `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#d9b98a"/><stop offset="1" stop-color="#8a5a3c"/></linearGradient></defs><rect width="${w}" height="${h}" fill="url(#g)"/>`;
  }
}

function stillUrl(source: SourceSpecDto, canvas: CanvasDto): string {
  const content = sourceSizePx(source, sources, settings.resolution) ?? { w: 1600, h: 1000 };
  const fixed = aspectRatio(canvas.aspect);
  const size = referenceCanvasSize(canvas.aspect, fixed ?? ratioOf(content), [content]);
  const k = 320 / Math.max(size.w, size.h);
  const cw = Math.round(size.w * k);
  const ch = Math.round(size.h * k);
  const dst = containFit({ w: cw, h: ch }, content);
  const html =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${cw}" height="${ch}">` +
    svgBackground(canvas.background, cw, ch) +
    `<rect x="${dst.x}" y="${dst.y}" width="${dst.w}" height="${dst.h}" fill="#dfe3ea"/>` +
    `<rect x="${dst.x}" y="${dst.y}" width="${dst.w}" height="${Math.max(2, dst.h * 0.06)}" fill="#b8bfcc"/>` +
    `<rect x="${dst.x + dst.w * 0.08}" y="${dst.y + dst.h * 0.2}" width="${dst.w * 0.5}" height="${dst.h * 0.08}" rx="2" fill="#9aa3b2"/>` +
    `<rect x="${dst.x + dst.w * 0.08}" y="${dst.y + dst.h * 0.36}" width="${dst.w * 0.75}" height="${dst.h * 0.05}" rx="2" fill="#c4cad4"/>` +
    `<rect x="${dst.x + dst.w * 0.08}" y="${dst.y + dst.h * 0.46}" width="${dst.w * 0.65}" height="${dst.h * 0.05}" rx="2" fill="#c4cad4"/>` +
    `</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(html)}`;
}

function bootScenario() {
  const state = q.get('state');
  const notice = q.get('notice');
  if (state === 'recording' || notice) {
    elapsedNs = 83 * NS;
    later(50, () => {
      setState({ state: 'recording', elapsed_ns: elapsedNs });
      startTicker();
    });
  }
  if (notice) {
    const n: Record<string, NoticeDto> = {
      source_lost: { kind: 'source_lost', segment: 'seg-2' },
      frames_dropping: { kind: 'frames_dropping', track: 'screen', pct: 7.5 },
      switch_failed: { kind: 'switch_failed', error: 'The window closed before the switch', restored: true },
      disk_low: { kind: 'disk_low', bytes_left: 1_800_000_000 },
      stopped_for_sleep: { kind: 'stopped_for_sleep' },
      stopped_for_disk: { kind: 'stopped_for_disk' },
    };
    const dto = n[notice];
    if (dto) later(120, () => busEmit('notice', dto));
  }
  if (q.get('saved') === '1' || notice === 'stopped_for_sleep' || notice === 'stopped_for_disk') {
    later(80, () => {
      setState({ state: 'recording', elapsed_ns: 754 * NS });
      stopTicker();
      setState({ state: 'finalized', project_dir: PROJECT_DIR });
    });
  }
  if (q.get('overlay') === 'switch') later(40, () => busEmit('overlay-context', { purpose: 'switch', display_id: '1' }));
}

type Handler = (args: Record<string, unknown>) => unknown;

const handlers: Record<string, Handler> = {
  get_permissions: () => ({ ...permissions }),
  request_permission: (a) => {
    const key = (a.kind === 'microphone' ? 'mic' : a.kind) as keyof PermissionsDto;
    if (permissions[key] === 'not_determined') permissions[key] = 'granted';
    return permissions[key];
  },
  open_permission_settings: () => null,
  list_displays: () => displays,
  list_sources: () => {
    if (permissions.screen !== 'granted') throw new Error('list_sources needs the Screen Recording grant');
    return sources;
  },
  list_devices: () => devices,
  show_mic_modes: () => null,
  get_settings: () => settings,
  set_settings: (a) => {
    settings = a.settings as RecorderSettings;
    return settings;
  },
  get_capabilities: () => capabilities,
  preview_still: (a) => stillUrl(a.source as SourceSpecDto, a.canvas as CanvasDto),
  choose_background_image: () => '~/Pictures/Wallpapers/Dunes.jpg',
  begin_region_select: () => null,
  begin_pick_mode: () => null,
  open_system_picker: () => null,
  start_recording: () => {
    startRecording();
    return null;
  },
  stop_recording: () => {
    stopRecording();
    return null;
  },
  discard_recording: () => {
    stopTicker();
    setState({ state: 'idle' });
    return null;
  },
  switch_source: () => {
    if (recState.state === 'recording') {
      setState({ state: 'switching', elapsed_ns: elapsedNs });
      later(250, () => setState({ state: 'recording', elapsed_ns: elapsedNs }));
    }
    return null;
  },
  quick_export: (a) => quickExport(a.size as ExportSizeDto),
  cancel_export: (a) => {
    const id = String(a.id);
    const t = exports.get(id);
    if (t) clearInterval(t);
    exports.delete(id);
    busEmit('export-state', { id, state: 'canceled' });
    return null;
  },
  reveal: () => null,
  recovery_report: () => recovery,
  camera_preview: () => null,
  preview_state: () => ({ shown: false, key: settings.camera_key, frame: 0, camera_open: null }),
  camera_key_live: () => null,
  show_video_effects: () => null,
  bar_drag: () => null,
  editor: (a) => mockEditor(String(a.action ?? 'state'), a),
  app: (a) => mockApp(String(a.action ?? 'about'), a),
};

const S = 1_000_000_000;
const LIB_ROOT = '/Users/me/Movies/Simple Capture';
const libItem = (stamp: string, title: string, mins: number, mb: number, camera: boolean, state = 'finalized') => ({
  dir: `${LIB_ROOT}/${stamp} ${title}.sc`,
  title,
  created_at: `${stamp.slice(0, 10)}T${stamp.slice(11).replace(/\./g, ':')}-08:00`,
  state,
  duration_ns: mins * 60 * S,
  edited_ns: 0,
  bytes: mb * 1_000_000,
  has_camera: camera,
  modified_s: 0,
});
const library = [
  libItem('2026-11-21 10.00.00', 'Recording', 1.25, 182, true),
  libItem('2026-11-20 16.12.40', 'Q4 roadmap walkthrough', 7.5, 911, true),
  libItem('2026-11-19 09.03.11', 'Bug repro: export stalls', 0.6, 64, false),
  libItem('2026-11-12 14.44.02', 'Onboarding demo', 3.2, 402, true, 'recovered'),
];
const folderOf = (a: Record<string, unknown>) => library.find((i) => i.dir.endsWith(`/${String(a.name)}`));

const SHORTCUT_ACTIONS: [string, string, string][] = [
  ['record', 'Start or stop recording', 'Alt+Shift+KeyR'],
  ['pause', 'Pause or resume', 'Alt+Shift+KeyP'],
  ['switch', 'Switch source', 'Alt+Shift+KeyS'],
  ['camera', 'Show or hide the camera', 'Alt+Shift+KeyC'],
  ['open', 'Open the recorder', 'Alt+Shift+KeyO'],
];
const mockShortcuts: Record<string, string> = Object.fromEntries(SHORTCUT_ACTIONS.map(([id, , k]) => [id, k]));
function shortcutRows() {
  const seen = new Map<string, string>();
  return SHORTCUT_ACTIONS.map(([id, label, def]) => {
    const keys = mockShortcuts[id];
    const other = keys ? seen.get(keys) : undefined;
    if (keys && !other) seen.set(keys, label);
    return { id, label, keys, default: def, error: other ? `Also used for “${other}”` : null };
  });
}

function mockApp(action: string, a: Record<string, unknown> = {}): unknown {
  switch (action) {
    case 'about':
      return {
        version: '0.1.0',
        os: 'macos',
        arch: 'aarch64',
        licenses: [
          { title: 'Simple Capture', license: 'MIT', note: 'This app.', files: [{ name: 'LICENSE', path: '/LICENSE', html: false }] },
        ],
      };
    case 'license_text':
      return 'MIT License';
    case 'shortcuts':
    case 'shortcuts_pause':
      return shortcutRows();
    case 'shortcut_set': {
      const id = String(a.id);
      const keys = String(a.keys ?? '');
      if (keys && !/(Alt|Control|Super)\+/.test(keys)) throw new Error('Add ⌘, ⌃ or ⌥ so typing doesn’t trigger it');
      mockShortcuts[id] = keys;
      return shortcutRows();
    }
    case 'library':
      return { root: LIB_ROOT, free_bytes: 212_000_000_000, items: library.map((i) => ({ ...i })) };
    case 'library_item':
      return folderOf(a) ?? null;
    case 'library_rename': {
      const it = folderOf(a);
      const title = String(a.title ?? '').trim();
      if (!it) throw new Error('no such recording in the library');
      if (!title) throw new Error('A recording needs a name.');
      it.title = title;
      it.dir = `${LIB_ROOT}/${it.dir.split('/').pop()!.slice(0, 19)} ${title}.sc`;
      return { dir: it.dir };
    }
    case 'library_trash': {
      const i = library.findIndex((x) => x.dir.endsWith(`/${String(a.name)}`));
      if (i >= 0) library.splice(i, 1);
      return null;
    }
    default:
      return null;
  }
}

const mockDoc = {
  edl: {
    clips: [
      { id: 'c1', src_in_rec_ns: 0, src_out_rec_ns: 24 * S, speed: { num: 1, den: 1 }, muted: false },
      { id: 'c2', src_in_rec_ns: 30 * S, src_out_rec_ns: 50 * S, speed: { num: 2, den: 1 }, muted: false },
      { id: 'c3', src_in_rec_ns: 52 * S, src_out_rec_ns: 75 * S, speed: { num: 1, den: 1 }, muted: true },
    ],
  },
  text: [
    {
      id: 't1',
      start_tl_ns: 2 * S,
      end_tl_ns: 8 * S,
      layer: 0,
      content: 'Welcome to the demo',
      style: {
        font_family: 'Montserrat',
        size_1080: 72,
        weight: 900,
        italic: false,
        uppercase: false,
        align: 'center',
        line_height: 1.15,
        letter_spacing: 0,
        opacity: 1,
        fill: { solid: [1, 1, 1, 1] },
        outline: { width_1080: 6, color: [0, 0, 0, 1] },
      },
      transform: { center_norm: { x: 0.5, y: 0.78 }, scale: 1, rotation_deg: 0 },
      anim: { in: { kind: 'pop', duration_s: 0.35, easing: 'ease_out' } },
    },
  ],
  camera_layout: { overlays: [{ id: 'cam', track_id: 'camera', keyframes: [{ ...settings.camera_key, t_rec_ns: 0 }, { ...settings.camera_key, t_rec_ns: 40 * S }] }] },
  audio_mix: { mic: { gain_db: -3, muted: false } },
  output: { canvas: { aspect: '16:9', ratio: { num: 16, den: 9 }, fit: 'contain', background: { kind: 'solid', color: [0.07, 0.07, 0.08, 1] } } },
};

let editorTitle = 'Recording';
let mockRevision = 3;

function mockEditor(action: string, a: Record<string, unknown> = {}): unknown {
  if (action === 'silences') {
    const t = typeof a.threshold_db === 'number' ? a.threshold_db : -44;
    const min = typeof a.min_ms === 'number' ? a.min_ms : 1000;
    const all: [number, number][] = [
      [0, 1.4 * S],
      [9.2 * S, 10.6 * S],
      [21 * S, 23.5 * S],
      [31 * S, 31.8 * S],
      [44 * S, 47 * S],
    ];
    const ranges = all.filter(([x, y]) => y - x >= (min / 1000) * S * (1 + (t + 44) / 30));
    return { ranges, removed_ns: ranges.reduce((n, [x, y]) => n + y - x, 0), duration_ns: 57 * S, threshold_db: t, suggested_db: -44 };
  }
  if (action === 'rename') {
    const t = String(a.title ?? '').trim();
    if (!t) throw new Error('A recording needs a name.');
    editorTitle = t;
    return '/Users/me/Movies/Simple Capture/2026-11-21 10.00.00 Recording.sc';
  }
  if (action === 'export_options') {
    const ok = { best: true, fast: true };
    return {
      store: { last_format: 'mp4', per_format: {}, presets: [{ name: 'Web 720p', settings: { format: 'mp4' } }] },
      available: { video: { h264: ok, hevc: ok, av1: { best: true, fast: false }, vp9: { best: true, fast: false }, vp8: { best: true, fast: false }, prores: ok }, gifski: true },
      recording_fps: 30,
      reference: { w: 3024, h: 1964 },
      ratio: [16, 9],
      default_dir: '/Users/me/Movies/Simple Capture',
    };
  }
  if (action === 'export_estimate') {
    return {
      canvas: { w: 3492, h: 1964 },
      fps: 30,
      frames: 1710,
      duration_ns: 57 * S,
      file: '/Users/me/Movies/Simple Capture/2026-11-21 10.00.00 Recording.mp4',
      bytes_low: 48_000_000,
      bytes_high: 172_000_000,
      seconds: 26,
      audio: true,
      less_compatible: null,
      too_large_for_fast: false,
    };
  }
  if (action === 'fonts') {
    const face = (file: string, lo: number, hi: number) => ({ file, italic: false, weights: [lo, hi] });
    return {
      bundled: [
        { name: 'Inter', kind: 'Modern', default_weight: 700, weights: [100, 900], static_weights: [100, 900], italic: true, variable: true, faces: [face('Inter[opsz,wght].ttf', 100, 900)] },
        { name: 'Montserrat', kind: 'Geometric', default_weight: 800, weights: [100, 900], static_weights: [100, 900], italic: true, variable: true, faces: [face('Montserrat[wght].ttf', 100, 900)] },
        { name: 'Bebas Neue', kind: 'Poster', default_weight: 400, weights: [400, 400], static_weights: [400], italic: false, variable: false, faces: [face('BebasNeue-Regular.ttf', 400, 400)] },
      ],
      system: ['Helvetica Neue', 'Georgia'],
    };
  }
  if (action === 'state' || action === 'open') {
    return {
      project_dir: '/Users/me/Movies/Simple Capture/2026-11-21 10.00.00 Recording.sc',
      title: editorTitle,
      rec_len_ns: 75 * S,
      fps: 30,
      tracks: [
        { id: 'screen', kind: 'screen' },
        { id: 'camera', kind: 'camera' },
        { id: 'mic', kind: 'mic' },
        { id: 'system_audio', kind: 'system_audio' },
      ],
      segments: [{ id: 's1', start_rec_ns: 0, end_rec_ns: 75 * S }],
      doc: mockDoc,
      revision: 3,
      can_undo: true,
      can_redo: false,
      playhead_tl_ns: 12 * S,
      playing: false,
      clipboard_ns: 0,
    };
  }
  if (action === 'play' || action === 'pause') return { playhead_tl_ns: 12 * S, playing: action === 'play' };
  if (action === 'rect' && a.rect && typeof a.rect === 'object') {
    const r = a.rect as { x: number; y: number; w: number; h: number };
    const f0 = containFit(r, { w: 16, h: 9 });
    const fit = { x: r.x + f0.x, y: r.y + f0.y, w: f0.w, h: f0.h };
    busEmit('editor-frame', {
      text: [],
      native: false,
      canvas_px: { w: 1920, h: 1080 },
      rect: fit,
      camera: null,
      screen: { x: fit.w * 0.05, y: fit.h * 0.05, w: fit.w * 0.9, h: fit.h * 0.9 },
      rec_ns: 12 * S,
    });
    return null;
  }
  if (action === 'edit' && a.op && typeof a.op === 'object') {
    const op = JSON.parse(JSON.stringify(a.op)) as { op: string; item?: { id: string }; ids?: string[] };
    const list = (mockDoc as { redactions?: { id: string }[] }).redactions ?? [];
    if (op.op === 'upsert_redaction' && op.item) {
      const i = list.findIndex((r) => r.id === op.item!.id);
      if (i >= 0) list[i] = op.item;
      else list.push(op.item);
    }
    if (op.op === 'delete_redactions') list.splice(0, list.length, ...list.filter((r) => !op.ids?.includes(r.id)));
    (mockDoc as { redactions?: unknown[] }).redactions = list;
    mockRevision += 1;
    return { doc: JSON.parse(JSON.stringify(mockDoc)), revision: mockRevision, can_undo: true, can_redo: false };
  }
  return { revision: 3 };
}

export const MOCK_COMMANDS = Object.keys(handlers);

let booted = false;

export function bootMock(): void {
  if (booted) return;
  booted = true;
  bootScenario();
}

export async function mockInvoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const h = handlers[cmd];
  if (!h) throw new Error(`mock: unknown command ${cmd}`);
  return h(args) as T;
}
