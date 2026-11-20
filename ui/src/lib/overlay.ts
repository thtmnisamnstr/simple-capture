// Which display and purpose an overlay window serves (region select and pick mode).

import type { DisplayInfo, Purpose } from './types.ts';
import { displayAt, mainDisplay } from './sources.ts';

export function overlayDisplay(displays: DisplayInfo[], explicitId: string | null, screenPos: { x: number; y: number } | null): DisplayInfo | null {
  if (explicitId) {
    const d = displays.find((x) => x.id === explicitId);
    if (d) return d;
  }
  if (screenPos) {
    const d = displayAt(displays, { x: screenPos.x + 1, y: screenPos.y + 1 });
    if (d) return d;
  }
  return mainDisplay(displays);
}

export function parsePurpose(v: string | null, recording: boolean): Purpose {
  if (v === 'start' || v === 'switch') return v;
  return recording ? 'switch' : 'start';
}

const REGION_KEY = 'sc.last-region.';

export function loadLastRegion(displayId: string): { x: number; y: number; w: number; h: number } | null {
  try {
    const v = JSON.parse(localStorage.getItem(REGION_KEY + displayId) ?? 'null');
    if (v && [v.x, v.y, v.w, v.h].every((n) => typeof n === 'number' && Number.isFinite(n)) && v.w > 0 && v.h > 0) return v;
  } catch {
    return null;
  }
  return null;
}

export function saveLastRegion(displayId: string, r: { x: number; y: number; w: number; h: number }): void {
  try {
    localStorage.setItem(REGION_KEY + displayId, JSON.stringify(r));
  } catch {
    return;
  }
}
