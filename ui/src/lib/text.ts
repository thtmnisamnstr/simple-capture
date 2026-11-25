// Text items (mirrors sc_core::project::TextItem), their defaults, style presets and the
// animation lists the text engine knows (crates/text/src/anim.rs).

import type { PointF, Rgba } from './types.ts';

export type TextFill = { solid: Rgba } | { linear: { stops: [Rgba, Rgba]; angle_deg: number } };
export interface TextStyle {
  font_family: string;
  size_1080: number;
  weight: number;
  italic: boolean;
  uppercase: boolean;
  align: 'left' | 'center' | 'right';
  line_height: number;
  letter_spacing: number;
  opacity: number;
  fill: TextFill;
  outline?: { width_1080: number; color: Rgba } | null;
  shadow?: { blur_1080: number; offset_1080: PointF; color: Rgba } | null;
  glow?: { radius_1080: number; color: Rgba } | null;
  background?: { kind: 'per_line' | 'merged' | 'inverted'; color: Rgba; radius_1080: number; padding_1080: PointF } | null;
}
export interface Anim { kind: string; duration_s: number; easing: string }

/** Seconds the in and out animations run on an item `len_s` long; mirrors sc_text::anim::durations_ns. */
export function animDurations(anim: { in?: Anim | null; out?: Anim | null }, len_s: number): { in: number; out: number } {
  const i = anim.in ? Math.max(anim.in.duration_s, 0.01) : 0;
  const o = anim.out ? Math.max(anim.out.duration_s, 0.01) : 0;
  const k = i + o > len_s && len_s > 0 ? len_s / (i + o) : 1;
  return { in: i * k, out: o * k };
}
export interface TextAnim {
  in?: Anim | null;
  out?: Anim | null;
  emphasis?: { kind: 'pulse' | 'wiggle' | 'float'; period_s: number } | null;
}
export interface TextItemFull {
  id: string;
  start_tl_ns: number;
  end_tl_ns: number;
  layer: number;
  content: string;
  style: TextStyle;
  transform: { center_norm: PointF; scale: number; rotation_deg: number };
  anim: TextAnim;
}
export interface FontInfo {
  name: string;
  kind: string;
  default_weight: number;
  weights: [number, number];
  static_weights: number[];
  italic: boolean;
  variable: boolean;
  faces: { file: string; italic: boolean; weights: [number, number] }[];
}

/** `@font-face` rules for the bundled fonts, served by the app, so inline editing uses them. */
export function fontFaceCss(fonts: FontInfo[], base: string): string {
  return fonts
    .flatMap((f) =>
      f.faces.map(
        (x) =>
          `@font-face{font-family:'${f.name}';src:url('${base}/${encodeURIComponent(x.file)}');font-weight:${x.weights[0]} ${x.weights[1]};font-style:${x.italic ? 'italic' : 'normal'};}`,
      ),
    )
    .join('\n');
}

export const DEFAULT_TEXT_NS = 3_000_000_000;

const WHITE: Rgba = [1, 1, 1, 1];
const BLACK: Rgba = [0, 0, 0, 1];

export function fillColor(f: TextFill): Rgba {
  return 'solid' in f ? f.solid : f.linear.stops[0];
}

export const IN_ANIMS: [string, string][] = [
  ['fade', 'Fade'],
  ['pop', 'Pop'],
  ['slide_up', 'Slide up'],
  ['slide_down', 'Slide down'],
  ['slide_left', 'Slide from left'],
  ['slide_right', 'Slide from right'],
  ['typewriter', 'Typewriter'],
  ['words', 'Word by word'],
  ['bounce', 'Bounce'],
];
export const OUT_ANIMS: [string, string][] = [
  ['fade', 'Fade'],
  ['pop', 'Pop'],
  ['slide_up', 'Slide up'],
  ['slide_down', 'Slide down'],
  ['slide_left', 'Slide left'],
  ['slide_right', 'Slide right'],
];
export const EMPHASIS: [string, string][] = [
  ['pulse', 'Pulse'],
  ['wiggle', 'Wiggle'],
  ['float', 'Float'],
];
export const EASINGS: [string, string][] = [
  ['ease_out', 'Ease out'],
  ['ease_in_out', 'Ease in and out'],
  ['ease_in', 'Ease in'],
  ['linear', 'Linear'],
  ['back', 'Overshoot'],
];
export const WEIGHTS: [number, string][] = [
  [100, 'Thin'],
  [200, 'Extra Light'],
  [300, 'Light'],
  [400, 'Regular'],
  [500, 'Medium'],
  [600, 'Semibold'],
  [700, 'Bold'],
  [800, 'Heavy'],
  [900, 'Black'],
];

export function defaultStyle(): TextStyle {
  return {
    font_family: 'Inter',
    size_1080: 72,
    weight: 800,
    italic: false,
    uppercase: false,
    align: 'center',
    line_height: 1.15,
    letter_spacing: 0,
    opacity: 1,
    fill: { solid: WHITE },
    outline: null,
    shadow: { blur_1080: 6, offset_1080: { x: 0, y: 3 }, color: [0, 0, 0, 0.45] },
    glow: null,
    background: null,
  };
}

/** A deep copy of plain JSON data that also accepts Svelte's reactive proxies, which
 * `structuredClone` rejects with a DataCloneError. */
export function plainCopy<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

export function newTextItem(id: string, start: number, style?: TextStyle): TextItemFull {
  return {
    id,
    start_tl_ns: Math.round(start),
    end_tl_ns: Math.round(start + DEFAULT_TEXT_NS),
    layer: 0,
    content: 'Your text',
    style: style ? plainCopy(style) : defaultStyle(),
    transform: { center_norm: { x: 0.5, y: 0.78 }, scale: 1, rotation_deg: 0 },
    anim: { in: { kind: 'pop', duration_s: 0.35, easing: 'ease_out' }, out: { kind: 'fade', duration_s: 0.25, easing: 'ease_out' } },
  };
}

export interface Preset { name: string; style: Partial<TextStyle>; anim?: TextAnim; css: string }

const shadow = { blur_1080: 6, offset_1080: { x: 0, y: 3 }, color: [0, 0, 0, 0.45] as Rgba };

export const PRESETS: Preset[] = [
  { name: 'Classic', style: { font_family: 'Inter', weight: 800, fill: { solid: WHITE }, shadow, outline: null, glow: null, background: null, uppercase: false }, css: 'font: 800 13px Inter, system-ui; color: #fff; text-shadow: 0 1px 3px #0008' },
  {
    name: 'Outline',
    style: { font_family: 'Montserrat', weight: 900, fill: { solid: WHITE }, outline: { width_1080: 6, color: BLACK }, shadow: null, glow: null, background: null },
    css: 'font: 900 13px Montserrat, system-ui; color: #fff; -webkit-text-stroke: 1px #000',
  },
  {
    name: 'Box',
    style: {
      font_family: 'Inter',
      weight: 700,
      fill: { solid: BLACK },
      outline: null,
      shadow: null,
      glow: null,
      background: { kind: 'merged', color: WHITE, radius_1080: 16, padding_1080: { x: 22, y: 6 } },
    },
    css: 'font: 700 13px Inter, system-ui; color: #000; background: #fff; border-radius: 4px',
  },
  {
    name: 'Inverted',
    style: {
      font_family: 'Inter',
      weight: 800,
      fill: { solid: WHITE },
      outline: null,
      shadow: null,
      glow: null,
      background: { kind: 'merged', color: [1, 0.18, 0.33, 1], radius_1080: 16, padding_1080: { x: 22, y: 6 } },
    },
    css: 'font: 800 13px Inter, system-ui; color: #fff; background: #ff2d55; border-radius: 4px',
  },
  {
    name: 'Neon',
    style: { font_family: 'Tilt Neon', weight: 400, fill: { solid: [1, 0.36, 0.85, 1] }, glow: { radius_1080: 14, color: [1, 0.36, 0.85, 1] }, outline: null, shadow: null, background: null },
    css: "font: 400 13px 'Tilt Neon', system-ui; color: #ff5cd9; text-shadow: 0 0 6px #ff5cd9",
  },
  {
    name: 'Headline',
    style: { font_family: 'Bebas Neue', weight: 400, size_1080: 110, uppercase: true, fill: { solid: WHITE }, shadow, outline: null, glow: null, background: null },
    css: "font: 400 15px 'Bebas Neue', system-ui; color: #fff; letter-spacing: .5px",
  },
  {
    name: 'Typewriter',
    style: {
      font_family: 'Courier Prime',
      weight: 700,
      fill: { solid: WHITE },
      shadow: null,
      outline: null,
      glow: null,
      background: { kind: 'per_line', color: [0, 0, 0, 0.7], radius_1080: 6, padding_1080: { x: 14, y: 4 } },
    },
    anim: { in: { kind: 'typewriter', duration_s: 1.2, easing: 'linear' }, out: { kind: 'fade', duration_s: 0.25, easing: 'ease_out' } },
    css: "font: 700 13px 'Courier Prime', monospace; color: #fff; background: #000b",
  },
  {
    name: 'Handwritten',
    style: { font_family: 'Caveat', weight: 700, size_1080: 90, fill: { solid: [1, 0.84, 0.04, 1] }, shadow, outline: null, glow: null, background: null },
    css: 'font: 700 16px Caveat, cursive; color: #ffd60a',
  },
  {
    name: 'Elegant',
    style: { font_family: 'Playfair Display', weight: 700, italic: true, fill: { solid: WHITE }, shadow, outline: null, glow: null, background: null },
    css: "font: italic 700 13px 'Playfair Display', serif; color: #fff",
  },
  {
    name: 'Marker',
    style: { font_family: 'Sedgwick Ave Display', weight: 400, fill: { solid: [1, 0.84, 0.04, 1] }, outline: { width_1080: 4, color: BLACK }, shadow: null, glow: null, background: null },
    css: "font: 400 14px 'Sedgwick Ave Display', cursive; color: #ffd60a; -webkit-text-stroke: .6px #000",
  },
];

export function applyPreset(style: TextStyle, p: Preset): TextStyle {
  return { ...style, ...plainCopy(p.style) };
}

const RECENT_KEY = 'sc.text.recent';

export function recentStyles(): TextStyle[] {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]');
    return Array.isArray(v) ? v.slice(0, 6) : [];
  } catch {
    return [];
  }
}

export function rememberStyle(s: TextStyle): void {
  try {
    const key = JSON.stringify(s);
    const list = recentStyles().filter((x) => JSON.stringify(x) !== key);
    list.unshift(s);
    localStorage.setItem(RECENT_KEY, JSON.stringify(list.slice(0, 6)));
  } catch {
    return;
  }
}

/** Snap an angle to 0°, ±15°, ±90° and 180° within `within` degrees. */
export function snapAngle(deg: number, within = 3): number {
  const norm = ((((deg + 180) % 360) + 360) % 360) - 180;
  for (const t of [0, 15, -15, 90, -90, 180, -180]) if (Math.abs(norm - t) <= within) return t === -180 ? 180 : t;
  return norm;
}

/** Snap a normalized position to the centre, thirds and 5% safe margins within `within`. */
export function snapPos(v: number, within: number): { v: number; guide: number | null } {
  for (const g of [0.5, 1 / 3, 2 / 3, 0.05, 0.95]) if (Math.abs(v - g) <= within) return { v: g, guide: g };
  return { v, guide: null };
}

export function rgbaCss(c: Rgba): string {
  return `rgba(${Math.round(c[0] * 255)}, ${Math.round(c[1] * 255)}, ${Math.round(c[2] * 255)}, ${c[3]})`;
}
