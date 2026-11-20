// Mirror of sc_geometry (plan §4.1, ARCHITECTURE §8 and §10); tests use the same tables as the crate.

import type { PointF, RectPt } from './types.ts';

export interface Size { w: number; h: number }
export interface RectPx { x: number; y: number; w: number; h: number }
export interface Ratio { num: number; den: number }

export type GeoAspect = 'original' | '16:9' | '4:3' | '1:1' | '9:16' | '4:5';
export type SizeClass = 2160 | 1440 | 1080 | 720 | 480 | 360 | 240;
export type Handle = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw' | 'move';

export const HANDLES: readonly Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];
export const SHORT_SIDE_CAP = 2160;
export const ORIGINAL_LONG_CAP = 8192;
export const MIN_REGION_PT = 4;

/** f64::round semantics: half away from zero. */
export function roundHalfAway(x: number): number {
  return Math.sign(x) * Math.round(Math.abs(x));
}

function gcd(a: number, b: number): number {
  while (b) [a, b] = [b, a % b];
  return a;
}

export function ratio(num: number, den: number): Ratio {
  if (num <= 0 || den <= 0) return { num: 1, den: 1 };
  const g = gcd(num, den);
  return { num: num / g, den: den / g };
}

export const ratioOf = (s: Size): Ratio => ratio(Math.round(s.w), Math.round(s.h));
export const ratioF = (r: Ratio): number => r.num / r.den;

export function aspectRatio(a: GeoAspect): Ratio | null {
  switch (a) {
    case '16:9': return { num: 16, den: 9 };
    case '4:3': return { num: 4, den: 3 };
    case '1:1': return { num: 1, den: 1 };
    case '9:16': return { num: 9, den: 16 };
    case '4:5': return { num: 4, den: 5 };
    default: return null;
  }
}

export function evenRound(x: number): number {
  return Math.max(2, 2 * roundHalfAway(x / 2));
}

export function containFit(canvas: Size, content: Size): RectPx {
  if (content.w <= 0 || content.h <= 0) return { x: 0, y: 0, w: 0, h: 0 };
  const s = Math.min(canvas.w / content.w, canvas.h / content.h);
  const x = (canvas.w - content.w * s) / 2;
  const y = (canvas.h - content.h * s) / 2;
  const x0 = roundHalfAway(x);
  const y0 = roundHalfAway(y);
  const x1 = roundHalfAway(x + content.w * s);
  const y1 = roundHalfAway(y + content.h * s);
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

export function coverSrcRect(image: Size, dst: Size, focus: PointF): RectPt {
  const s = Math.max(dst.w / image.w, dst.h / image.h);
  const w = dst.w / s;
  const h = dst.h / s;
  const clampTo = (v: number, hi: number) => Math.min(Math.max(v, 0), Math.max(hi, 0));
  return { x: clampTo(focus.x * image.w - w / 2, image.w - w), y: clampTo(focus.y * image.h - h / 2, image.h - h), w, h };
}

export function naturalCanvas(r: Ratio, content: Size): [number, number] {
  const a = ratioF(r);
  return content.w / content.h >= a ? [content.w, content.w / a] : [content.h * a, content.h];
}

function sidesFromShort(short: number, a: number): [number, number] {
  return a >= 1 ? [short * a, short] : [short, short / a];
}

/** `r` is only used for Original, where it is the first source's shape. */
export function referenceCanvasSize(aspect: GeoAspect, r: Ratio, segments: Size[]): Size {
  const segs = segments.filter((s) => s.w > 0 && s.h > 0);
  if (segs.length === 0) return { w: 2, h: 2 };
  const fixed = aspectRatio(aspect);
  if (!fixed && segs.length === 1) {
    return { w: Math.max(2, Math.floor(segs[0].w / 2) * 2), h: Math.max(2, Math.floor(segs[0].h / 2) * 2) };
  }
  const lock = fixed ?? r;
  const a = ratioF(lock);
  let short = Math.max(...segs.map((s) => Math.min(...naturalCanvas(lock, s))));
  if (fixed) short = Math.min(short, SHORT_SIDE_CAP);
  let [w, h] = sidesFromShort(short, a);
  if (!fixed) {
    const long = Math.max(w, h);
    if (long > ORIGINAL_LONG_CAP) {
      const k = ORIGINAL_LONG_CAP / long;
      w *= k;
      h *= k;
    }
  }
  return { w: evenRound(w), h: evenRound(h) };
}

export function presetSize(cls: SizeClass, r: Ratio, gif: boolean): Size {
  const [w, h] = sidesFromShort(cls, ratioF(r));
  const round = gif ? roundHalfAway : evenRound;
  return { w: round(w), h: round(h) };
}

export function ptToPx(r: RectPt, scale: number): RectPx {
  const x0 = roundHalfAway(r.x * scale);
  const y0 = roundHalfAway(r.y * scale);
  const x1 = roundHalfAway((r.x + r.w) * scale);
  const y1 = roundHalfAway((r.y + r.h) * scale);
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

export function flipY(r: RectPt, containerHPt: number): RectPt {
  return { x: r.x, y: containerHPt - r.y - r.h, w: r.w, h: r.h };
}

export function snapRegionEvenPx(r: RectPt, scale: number): RectPt {
  return {
    x: roundHalfAway(r.x * scale) / scale,
    y: roundHalfAway(r.y * scale) / scale,
    w: evenRound(r.w * scale) / scale,
    h: evenRound(r.h * scale) / scale,
  };
}

/** Pixel size of a region snapped like the capture does (sc_geometry::snap_region_even_px): both
 *  edges rounded to whole pixels, then the size floored to even, at least 2. */
export function snappedRegionSizePx(r: RectPt, scale: number): Size {
  const side = (o: number, len: number) => {
    const p0 = roundHalfAway(o * scale);
    const p1 = roundHalfAway((o + len) * scale);
    return Math.max(2, Math.floor((p1 - p0) / 2) * 2);
  };
  return { w: side(r.x, r.w), h: side(r.y, r.h) };
}

const finiteRect = (r: RectPt) => [r.x, r.y, r.w, r.h].every(Number.isFinite);

/** Union of an app's window rects (global points), each clipped to the display, as display-local
 *  points (ADR-039); null when nothing is left or a side is under 2 pt. Mirrors
 *  sc_geometry::app_bounds_pt. */
export function appBoundsPt(windows: RectPt[], display: RectPt): RectPt | null {
  if (!finiteRect(display)) return null;
  let acc: [number, number, number, number] | null = null;
  for (const w of windows) {
    if (!finiteRect(w)) continue;
    const x0 = Math.max(w.x, display.x);
    const y0 = Math.max(w.y, display.y);
    const x1 = Math.min(w.x + w.w, display.x + display.w);
    const y1 = Math.min(w.y + w.h, display.y + display.h);
    if (x1 <= x0 || y1 <= y0) continue;
    acc = acc ? [Math.min(acc[0], x0), Math.min(acc[1], y0), Math.max(acc[2], x1), Math.max(acc[3], y1)] : [x0, y0, x1, y1];
  }
  if (!acc) return null;
  const [x0, y0, x1, y1] = acc;
  const w = x1 - x0;
  const h = y1 - y0;
  return w >= 2 && h >= 2 ? { x: x0 - display.x, y: y0 - display.y, w, h } : null;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi);

/** For `move`, `to` is the new top-left corner; otherwise the dragged handle's position. */
export function aspectLockedResize(r: RectPt, lock: Ratio | null, handle: Handle, to: PointF, bounds: RectPt): RectPt {
  const bx1 = bounds.x + bounds.w;
  const by1 = bounds.y + bounds.h;
  if (handle === 'move') {
    const w = Math.min(r.w, bounds.w);
    const h = Math.min(r.h, bounds.h);
    return { x: clamp(to.x, bounds.x, bx1 - w), y: clamp(to.y, bounds.y, by1 - h), w, h };
  }
  const p = { x: clamp(to.x, bounds.x, bx1), y: clamp(to.y, bounds.y, by1) };
  const movesW = handle.includes('w');
  const movesE = handle.includes('e');
  const movesN = handle.includes('n');
  const movesS = handle.includes('s');
  let x0 = r.x;
  let y0 = r.y;
  let x1 = r.x + r.w;
  let y1 = r.y + r.h;
  if (movesW) x0 = p.x;
  if (movesE) x1 = p.x;
  if (movesN) y0 = p.y;
  if (movesS) y1 = p.y;
  let left = Math.min(x0, x1);
  let right = Math.max(x0, x1);
  let top = Math.min(y0, y1);
  let bottom = Math.max(y0, y1);
  if (!lock) {
    if (right - left < MIN_REGION_PT) (right = Math.min(left + MIN_REGION_PT, bx1)), (left = right - MIN_REGION_PT);
    if (bottom - top < MIN_REGION_PT) (bottom = Math.min(top + MIN_REGION_PT, by1)), (top = bottom - MIN_REGION_PT);
    return { x: left, y: top, w: right - left, h: bottom - top };
  }
  const a = ratioF(lock);
  const ax = movesW ? r.x + r.w : r.x;
  const ay = movesN ? r.y + r.h : r.y;
  const minW = Math.max(MIN_REGION_PT, MIN_REGION_PT * a);
  const fitW = (want: number, maxW: number, maxH: number) => {
    const cap = Math.min(maxW, maxH * a);
    return Math.max(Math.min(want, cap), Math.min(minW, cap));
  };
  if ((movesW || movesE) && (movesN || movesS)) {
    const dirX = p.x >= ax ? 1 : -1;
    const dirY = p.y >= ay ? 1 : -1;
    const dw = Math.abs(p.x - ax);
    const dh = Math.abs(p.y - ay);
    const want = dw / Math.max(dh, 1e-9) > a ? dw : dh * a;
    const w = fitW(want, dirX > 0 ? bx1 - ax : ax - bounds.x, dirY > 0 ? by1 - ay : ay - bounds.y);
    const h = w / a;
    return { x: dirX > 0 ? ax : ax - w, y: dirY > 0 ? ay : ay - h, w, h };
  }
  if (movesN || movesS) {
    const cx = r.x + r.w / 2;
    const dirY = p.y >= ay ? 1 : -1;
    const maxH = dirY > 0 ? by1 - ay : ay - bounds.y;
    const w = fitW(Math.abs(p.y - ay) * a, 2 * Math.min(cx - bounds.x, bx1 - cx), maxH);
    const h = w / a;
    return { x: cx - w / 2, y: dirY > 0 ? ay : ay - h, w, h };
  }
  const cy = r.y + r.h / 2;
  const dirX = p.x >= ax ? 1 : -1;
  const maxW = dirX > 0 ? bx1 - ax : ax - bounds.x;
  const w = fitW(Math.abs(p.x - ax), maxW, 2 * Math.min(cy - bounds.y, by1 - cy));
  const h = w / a;
  return { x: dirX > 0 ? ax : ax - w, y: cy - h / 2, w, h };
}

/** Largest rect of ratio `lock` centred in `r`. */
export function fitRatioInside(r: RectPt, lock: Ratio): RectPt {
  const a = ratioF(lock);
  let w = r.w;
  let h = w / a;
  if (h > r.h) (h = r.h), (w = h * a);
  return { x: r.x + (r.w - w) / 2, y: r.y + (r.h - h) / 2, w, h };
}

/** The largest box with `shape`'s aspect that fits inside a `max`×`max` square. */
export function fitBox(shape: Size, max: number): Size {
  if (!(shape.w > 0 && shape.h > 0)) return { w: max, h: max };
  const k = max / Math.max(shape.w, shape.h);
  return { w: Math.max(1, Math.round(shape.w * k)), h: Math.max(1, Math.round(shape.h * k)) };
}

export function smallContent(canvas: Size, content: Size): boolean {
  const d = containFit(canvas, content);
  return d.w < canvas.w * 0.25 || d.h < canvas.h * 0.25;
}

export function rectContains(r: RectPt, p: PointF): boolean {
  return p.x >= r.x && p.y >= r.y && p.x < r.x + r.w && p.y < r.y + r.h;
}

// Camera overlay geometry: mirror of sc_geometry::camera (M2_PLAN §3.2), same case table.

export const CAMERA_SIZE_PRESETS = [0.18, 0.26, 0.36] as const;
export const CAMERA_SIZE_MIN = 0.08;
export const CAMERA_SIZE_MAX = 0.9;
export const CAMERA_MARGIN = 0.03;
export const CAMERA_ZOOM_MAX = 4;

export interface CameraPose { center: PointF; size: number; zoom: number; pan: PointF; radius: number }
export interface CameraRects { dst: RectPt; src: RectPt; radius_px: number }
export type Corner = 'top_left' | 'top_right' | 'bottom_left' | 'bottom_right';

export function shapeSize(short: number, aspect: Ratio): Size {
  const a = ratioF(aspect);
  return a >= 1 ? { w: short * a, h: short } : { w: short, h: short / a };
}

export function cameraRects(canvas: Size, camera: Size, aspect: Ratio, pose: CameraPose): CameraRects {
  const short = Math.min(canvas.w, canvas.h) * clamp(pose.size, CAMERA_SIZE_MIN, CAMERA_SIZE_MAX);
  const { w, h } = shapeSize(short, aspect);
  const dst = { x: pose.center.x * canvas.w - w / 2, y: pose.center.y * canvas.h - h / 2, w, h };
  const fw = Math.max(camera.w, 1);
  const fh = Math.max(camera.h, 1);
  const a = ratioF(aspect);
  const [fillW, fillH] = fw / fh > a ? [fh * a, fh] : [fw, fw / a];
  const zoom = clamp(pose.zoom, 1, CAMERA_ZOOM_MAX);
  const sw = fillW / zoom;
  const sh = fillH / zoom;
  const src = {
    x: ((fw - sw) / 2) * (1 + clamp(pose.pan.x, -1, 1)),
    y: ((fh - sh) / 2) * (1 + clamp(pose.pan.y, -1, 1)),
    w: sw,
    h: sh,
  };
  return { dst, src, radius_px: (clamp(pose.radius, 0, 1) * Math.min(w, h)) / 2 };
}

export function cornerCenter(canvas: Size, aspect: Ratio, size: number, corner: Corner): PointF {
  const short = Math.min(canvas.w, canvas.h);
  const { w, h } = shapeSize(short * size, aspect);
  const m = short * CAMERA_MARGIN;
  const x = corner.endsWith('left') ? m + w / 2 : canvas.w - m - w / 2;
  const y = corner.startsWith('top') ? m + h / 2 : canvas.h - m - h / 2;
  return { x: x / canvas.w, y: y / canvas.h };
}

export function clampCenter(canvas: Size, aspect: Ratio, size: number, center: PointF): PointF {
  const { w, h } = shapeSize(Math.min(canvas.w, canvas.h) * size, aspect);
  const fx = Math.min(w / 2 / canvas.w, 0.5);
  const fy = Math.min(h / 2 / canvas.h, 0.5);
  return { x: clamp(center.x, fx, 1 - fx), y: clamp(center.y, fy, 1 - fy) };
}

export function snapCenter(canvas: Size, aspect: Ratio, size: number, center: PointF, thresholdPx: number): PointF {
  const short = Math.min(canvas.w, canvas.h);
  const { w, h } = shapeSize(short * size, aspect);
  const m = short * CAMERA_MARGIN;
  const snap = (v: number, targets: number[]) => {
    let best: number | null = null;
    let bestD = Infinity;
    for (const t of targets) {
      const d = Math.abs(t - v);
      if (d <= thresholdPx && d < bestD) [best, bestD] = [t, d];
    }
    return best ?? v;
  };
  const x = snap(center.x * canvas.w, [m + w / 2, canvas.w / 2, canvas.w - m - w / 2]);
  const y = snap(center.y * canvas.h, [m + h / 2, canvas.h / 2, canvas.h - m - h / 2]);
  return clampCenter(canvas, aspect, size, { x: x / canvas.w, y: y / canvas.h });
}
