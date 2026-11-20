// Background presets and conversions between the DTO's 0..1 RGBA and CSS.

import type { Background, GradientStop, Rgba } from './types.ts';

export function hexToRgba(hex: string): Rgba {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return [0, 0, 0, 1];
  const n = parseInt(m[1], 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, 1];
}

export function rgbaToHex(c: Rgba): string {
  const h = (v: number) => Math.round(Math.min(Math.max(v, 0), 1) * 255).toString(16).padStart(2, '0');
  return `#${h(c[0])}${h(c[1])}${h(c[2])}`;
}

export function rgbaToCss(c: Rgba): string {
  const b = (v: number) => Math.round(Math.min(Math.max(v, 0), 1) * 255);
  return `rgba(${b(c[0])}, ${b(c[1])}, ${b(c[2])}, ${c[3]})`;
}

export const SOLID_PALETTE = ['#121214', '#ffffff', '#1f2937', '#2563eb', '#7c3aed', '#db2777', '#ea580c', '#16a34a', '#f5f0e6'];

export interface GradientPreset { name: string; from: string; to: string }
export const GRADIENT_PRESETS: GradientPreset[] = [
  { name: 'Dusk', from: '#3b2667', to: '#bc78ec' },
  { name: 'Ocean', from: '#0f2027', to: '#2c5364' },
  { name: 'Sunset', from: '#ff512f', to: '#f09819' },
  { name: 'Mint', from: '#11998e', to: '#38ef7d' },
  { name: 'Slate', from: '#232526', to: '#414345' },
  { name: 'Peach', from: '#ffecd2', to: '#fcb69f' },
];

export function twoStops(from: string, to: string): GradientStop[] {
  return [
    { at: 0, color: hexToRgba(from) },
    { at: 1, color: hexToRgba(to) },
  ];
}

export function linear(from: string, to: string, angleDeg = 135): Background {
  return { kind: 'linear', stops: twoStops(from, to), angle_deg: angleDeg };
}

export function radial(from: string, to: string): Background {
  return { kind: 'radial', stops: twoStops(from, to), center_norm: { x: 0.5, y: 0.5 }, radius_norm: 0.75 };
}

export function backgroundCss(bg: Background): string {
  const stops = (s: GradientStop[]) => s.map((x) => `${rgbaToCss(x.color)} ${Math.round(x.at * 100)}%`).join(', ');
  switch (bg.kind) {
    case 'solid': return rgbaToCss(bg.color);
    case 'linear': return `linear-gradient(${bg.angle_deg}deg, ${stops(bg.stops)})`;
    case 'radial':
      return `radial-gradient(circle at ${bg.center_norm.x * 100}% ${bg.center_norm.y * 100}%, ${stops(bg.stops)})`;
    case 'image': return 'repeating-conic-gradient(#8884 0% 25%, #0000 0% 50%) 50% / 12px 12px';
  }
}

export function backgroundSummary(bg: Background): string {
  switch (bg.kind) {
    case 'solid': return rgbaToHex(bg.color);
    case 'linear': return `Linear gradient, ${Math.round(bg.angle_deg)}°`;
    case 'radial': return 'Radial gradient';
    case 'image': return 'Image';
  }
}
