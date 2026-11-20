// Source helpers: stable keys, labels, pixel sizes, display lookup and the pick-mode hit test.

import type {
  CapabilitiesDto,
  DisplayInfo,
  DisplayProbeDto,
  PointF,
  RecorderSettings,
  RectPt,
  SourceList,
  SourceSpecDto,
  WindowInfo,
} from './types.ts';
import { appBoundsPt, evenRound, rectContains, snappedRegionSizePx, type Size } from './geometry.ts';

export type Resolution = RecorderSettings['resolution'];

export function sourceKey(s: SourceSpecDto): string {
  switch (s.kind) {
    case 'display': return `display:${s.display_id}`;
    case 'window': return `window:${s.window_id}`;
    case 'application': return `app:${s.bundle_id ?? s.pid}@${s.display_id}`;
    case 'region': {
      const r = s.region_pt;
      return `region:${s.display_id}:${r.x},${r.y},${r.w},${r.h}`;
    }
    case 'picked': return `picked:${s.pick_id}`;
  }
}

export const sameSource = (a: SourceSpecDto | null, b: SourceSpecDto | null): boolean =>
  !!a && !!b && sourceKey(a) === sourceKey(b);

export function displayName(displays: DisplayInfo[], id: string): string {
  return displays.find((d) => d.id === id)?.name ?? `Display ${id}`;
}

export function sourceLabel(s: SourceSpecDto, list: Partial<SourceList>): string {
  const displays = list.displays ?? [];
  switch (s.kind) {
    case 'display': return displayName(displays, s.display_id);
    case 'window': {
      const w = list.windows?.find((x) => x.id === s.window_id);
      return w ? windowTitle(w) : `Window ${s.window_id}`;
    }
    case 'application': {
      const a = list.apps?.find((x) => x.app.pid === s.pid || (s.bundle_id && x.app.bundle_id === s.bundle_id));
      return `${a?.name ?? s.bundle_id ?? `App ${s.pid}`} on ${displayName(displays, s.display_id)}`;
    }
    case 'region':
      return `Region ${Math.round(s.region_pt.w)}×${Math.round(s.region_pt.h)} on ${displayName(displays, s.display_id)}`;
    case 'picked': return s.region_pt ? 'Picked region' : 'Picked with the system picker';
  }
}

export function windowTitle(w: WindowInfo): string {
  return w.title ? `${w.app_name} — ${w.title}` : w.app_name;
}

export function displayOfSource(s: SourceSpecDto, list: Partial<SourceList>): DisplayInfo | null {
  const displays = list.displays ?? [];
  switch (s.kind) {
    case 'display':
    case 'application':
    case 'region':
      return displays.find((d) => d.id === s.display_id) ?? null;
    case 'window': {
      const w = list.windows?.find((x) => x.id === s.window_id);
      return displays.find((d) => d.id === w?.display) ?? null;
    }
    case 'picked': return null;
  }
}

export function sourceSizePx(s: SourceSpecDto, list: Partial<SourceList>, resolution: Resolution = 'native'): Size | null {
  const d = displayOfSource(s, list);
  const oneX = resolution === '1x';
  switch (s.kind) {
    case 'display': {
      if (!d) return null;
      const full = oneX ? d.frame_pt : d.size_px;
      return { w: evenRound(full.w), h: evenRound(full.h) };
    }
    case 'application':
      return d ? appSizePx(s.pid, d, list.windows ?? [], resolution) : null;
    case 'region':
      return d ? snappedRegionSizePx(s.region_pt, oneX ? 1 : d.scale) : null;
    case 'window': {
      const w = list.windows?.find((x) => x.id === s.window_id);
      if (!w) return null;
      const scale = oneX ? 1 : (d?.scale ?? 2);
      return { w: evenRound(w.frame_pt.w * scale), h: evenRound(w.frame_pt.h * scale) };
    }
    case 'picked': return null;
  }
}

/** An application source is captured as the snapped union of its on-screen, layer-0 windows on the
 *  display, or the whole display when it has none there or they cover it (ADR-039). At 1x the snap
 *  and the whole-display size use 1 px per point, as sc-capture-macos's app_crop does. */
export function appSizePx(pid: number, d: DisplayInfo, windows: WindowInfo[], resolution: Resolution = 'native'): Size {
  const frames = windows
    .filter((w) => w.app.pid === pid && w.on_screen && w.layer === 0 && w.frame_pt.w >= 2 && w.frame_pt.h >= 2)
    .map((w) => w.frame_pt);
  const b = appBoundsPt(frames, d.frame_pt);
  const oneX = resolution === '1x';
  if (!b || (b.w >= d.frame_pt.w && b.h >= d.frame_pt.h)) {
    const full = oneX ? d.frame_pt : d.size_px;
    return { w: evenRound(full.w), h: evenRound(full.h) };
  }
  return snappedRegionSizePx(b, oneX ? 1 : d.scale);
}

export function probeFor(caps: CapabilitiesDto | null, displayId: string | null): DisplayProbeDto | null {
  if (!caps || !displayId) return null;
  return caps.displays.find((p) => p.display_id === displayId) ?? null;
}

export function globalToLocal(r: RectPt, d: DisplayInfo): RectPt {
  return { x: r.x - d.frame_pt.x, y: r.y - d.frame_pt.y, w: r.w, h: r.h };
}

/** Frontmost (lowest `z_index`) on-screen window containing `p` (global top-left points). */
export function topmostWindowAt(windows: WindowInfo[], p: PointF): WindowInfo | null {
  let best: WindowInfo | null = null;
  for (const w of windows) {
    if (!w.on_screen || w.frame_pt.w <= 0 || w.frame_pt.h <= 0) continue;
    if (!rectContains(w.frame_pt, p)) continue;
    if (!best || w.z_index < best.z_index) best = w;
  }
  return best;
}

export function displayAt(displays: DisplayInfo[], p: PointF): DisplayInfo | null {
  return displays.find((d) => rectContains(d.frame_pt, p)) ?? null;
}

export function mainDisplay(displays: DisplayInfo[]): DisplayInfo | null {
  return displays.find((d) => d.is_main) ?? displays[0] ?? null;
}
