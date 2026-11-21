// The editor's document (mirrors sc_core::edit::Doc), timeline maths that match the Rust edit
// model, and the `editor` command.

import { invoke } from '@tauri-apps/api/core';
import { inTauri } from './api.ts';
import { mockInvoke } from './mock.ts';
import type { Background, CameraKey, PointF } from './types.ts';

export const NS = 1_000_000_000;

export interface Speed { num: number; den: number }
export interface Clip {
  id: string;
  src_in_rec_ns: number;
  src_out_rec_ns: number;
  speed: Speed;
  muted: boolean;
}
export interface CameraOverlay { id: string; track_id: string; keyframes: CameraKey[] }
export interface AudioMixEntry { gain_db: number; muted: boolean }
export interface ProjectCanvas {
  aspect: 'original' | '16:9' | '4:3' | '1:1' | '9:16';
  ratio: { num: number; den: number };
  fit: 'contain' | 'cover';
  background: Background;
}
export interface TextItem {
  id: string;
  start_tl_ns: number;
  end_tl_ns: number;
  layer: number;
  content: string;
  style: Record<string, unknown>;
  transform: { center_norm: PointF; scale: number; rotation_deg: number };
  anim: Record<string, unknown>;
}
export interface Doc {
  edl: { clips: Clip[] };
  text: TextItem[];
  camera_layout: { overlays: CameraOverlay[] };
  audio_mix: Record<string, AudioMixEntry>;
  output: { canvas: ProjectCanvas };
}
export interface EditorTrack { id: string; kind: 'screen' | 'camera' | 'mic' | 'system_audio' }
export interface EditorState {
  project_dir: string;
  title: string;
  rec_len_ns: number;
  fps: number;
  tracks: EditorTrack[];
  segments: { id: string; start_rec_ns: number; end_rec_ns: number | null }[];
  doc: Doc;
  revision: number;
  can_undo: boolean;
  can_redo: boolean;
  playhead_tl_ns: number;
  playing: boolean;
  clipboard_ns: number;
}
export interface DocUpdate {
  doc?: Doc;
  revision: number;
  can_undo?: boolean;
  can_redo?: boolean;
  playhead_tl_ns?: number;
  clipboard_ns?: number;
}
export interface EditorFrame {
  text?: { id: string; quad: [number, number][]; unresolved: number }[];
  native: boolean;
  canvas_px: { w: number; h: number };
  rect: { x: number; y: number; w: number; h: number };
  camera: { x: number; y: number; w: number; h: number } | null;
  rec_ns: number;
}

export type EditOp =
  | { op: 'split'; at_tl_ns: number }
  | { op: 'trim'; clip_id: string; edge: 'start' | 'end'; rec_ns: number }
  | { op: 'delete_range'; a_tl_ns: number; b_tl_ns: number }
  | { op: 'delete_clips'; ids: string[] }
  | { op: 'duplicate'; ids: string[] }
  | { op: 'move_clip'; id: string; to_index: number }
  | { op: 'set_speed'; ids: string[]; speed: Speed }
  | { op: 'set_muted'; ids: string[]; muted: boolean }
  | { op: 'set_camera_key'; overlay_id: string; key: CameraKey }
  | { op: 'delete_camera_key'; overlay_id: string; t_rec_ns: number }
  | { op: 'set_camera_static'; overlay_id: string; key: CameraKey }
  | { op: 'set_track_mix'; track_id: string; gain_db: number; muted: boolean }
  | { op: 'set_canvas'; canvas: ProjectCanvas }
  | { op: 'upsert_text'; item: TextItem }
  | { op: 'delete_text'; ids: string[] };

export interface Selection { ids?: string[]; a_tl_ns?: number; b_tl_ns?: number }

function call<T>(action: string, args: Record<string, unknown> = {}): Promise<T> {
  return inTauri() ? invoke<T>('editor', { action, args }) : mockInvoke<T>('editor', { action, ...args });
}

export const editor = {
  open: (projectDir: string) => call<EditorState>('open', { project_dir: projectDir }),
  state: () => call<EditorState>('state'),
  edit: (op: EditOp, coalesce?: string) => call<DocUpdate>('edit', { op, coalesce: coalesce ?? null }),
  undo: () => call<DocUpdate>('undo'),
  redo: () => call<DocUpdate>('redo'),
  seal: () => call<DocUpdate>('seal'),
  copy: (range: Selection) => call<DocUpdate>('copy', { range }),
  cut: (range: Selection) => call<DocUpdate>('cut', { range }),
  paste: (at: number) => call<DocUpdate>('paste', { at_tl_ns: at }),
  play: (from?: number) => call<{ playhead_tl_ns: number; playing: boolean }>('play', { from_tl_ns: from ?? null }),
  pause: () => call<{ playhead_tl_ns: number; playing: boolean }>('pause'),
  seek: (t: number) => call<void>('seek', { tl_ns: Math.round(t) }),
  rect: (rect: { x: number; y: number; w: number; h: number } | null) => call<void>('rect', { rect }),
  exportVideo: (size: string) => call<string>('export', { size }),
  backgroundImage: (path: string) => call<Background>('background_image', { path }),
  fonts: () => call<{ bundled: import('./text.ts').FontInfo[]; system: string[] }>('fonts'),
  pickColor: () => call<[number, number, number, number] | null>('pick_color'),
  close: () => call<void>('close'),
};

export const SPEEDS: { speed: Speed; label: string }[] = [
  { speed: { num: 1, den: 2 }, label: '0.5×' },
  { speed: { num: 1, den: 1 }, label: '1×' },
  { speed: { num: 3, den: 2 }, label: '1.5×' },
  { speed: { num: 2, den: 1 }, label: '2×' },
  { speed: { num: 4, den: 1 }, label: '4×' },
];

export const sameSpeed = (a: Speed, b: Speed): boolean => a.num * b.den === b.num * a.den;
export const speedLabel = (s: Speed): string =>
  SPEEDS.find((x) => sameSpeed(x.speed, s))?.label ?? `${(s.num / s.den).toFixed(2)}×`;

export function tlLen(c: Clip): number {
  return Math.floor(((c.src_out_rec_ns - c.src_in_rec_ns) * c.speed.den) / c.speed.num);
}

export function starts(clips: Clip[]): number[] {
  const out = [0];
  let t = 0;
  for (const c of clips) {
    t += tlLen(c);
    out.push(t);
  }
  return out;
}

export interface Located { index: number; start: number; end: number; rec_ns: number }

export function locate(clips: Clip[], st: number[], tl: number): Located | null {
  const total = st[st.length - 1] ?? 0;
  if (tl < 0 || tl >= total || clips.length === 0) return null;
  let lo = 0;
  let hi = clips.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (st[mid] <= tl) lo = mid;
    else hi = mid - 1;
  }
  const c = clips[lo];
  const rec = Math.min(c.src_in_rec_ns + Math.floor(((tl - st[lo]) * c.speed.num) / c.speed.den), c.src_out_rec_ns);
  return { index: lo, start: st[lo], end: st[lo + 1], rec_ns: rec };
}

/** Timeline positions where a recording time is shown (one per clip that contains it). */
export function recToTl(clips: Clip[], st: number[], rec: number): number[] {
  const out: number[] = [];
  clips.forEach((c, i) => {
    if (rec >= c.src_in_rec_ns && rec < c.src_out_rec_ns) {
      out.push(st[i] + Math.floor(((rec - c.src_in_rec_ns) * c.speed.den) / c.speed.num));
    }
  });
  return out;
}

export function keyAt(keys: CameraKey[], rec: number): CameraKey | null {
  if (keys.length === 0) return null;
  let k = keys[0];
  for (const x of keys) if (x.t_rec_ns <= rec) k = x;
  return k;
}

export function snapTo(t: number, targets: number[], thresholdNs: number): number {
  let best = t;
  let dist = thresholdNs;
  for (const x of targets) {
    const d = Math.abs(x - t);
    if (d <= dist) {
      dist = d;
      best = x;
    }
  }
  return best;
}

/** `m:ss.t` (tenths), or `m:ss:ff` (frames) when `fps` is given; hours when needed. */
export function formatTime(ns: number, fps = 0): string {
  const neg = ns < 0;
  const t = Math.abs(ns) / NS;
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = Math.floor(t % 60);
  const frac = fps > 0 ? String(Math.floor((t % 1) * fps)).padStart(2, '0') : String(Math.floor((t % 1) * 10));
  const sep = fps > 0 ? ':' : '.';
  const body = h ? `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}` : `${m}:${String(s).padStart(2, '0')}`;
  return `${neg ? '−' : ''}${body}${sep}${frac}`;
}

/** Ruler tick spacing (ns) for a zoom, so labels stay at least `minPx` apart. */
export function tickStep(pxPerSec: number, minPx = 70): number {
  const steps = [0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600];
  for (const s of steps) if (s * pxPerSec >= minPx) return s * NS;
  return 3600 * NS;
}

export function trackName(kind: EditorTrack['kind']): string {
  return { screen: 'Screen', camera: 'Camera', mic: 'Microphone', system_audio: 'System audio' }[kind];
}
