// Recent sources for the switch popover: newest first, deduplicated, capped at five.

import type { SourceSpecDto } from './types.ts';
import { sourceKey } from './sources.ts';
import { isSourceSpec } from './guards.ts';

export const RECENT_MAX = 5;
export const RECENT_STORAGE_KEY = 'sc.recent-sources';

export interface RecentEntry { spec: SourceSpecDto; label: string; at: number }

/** System-picker picks do not survive a relaunch, so they are never remembered. */
export function pushRecent(list: RecentEntry[], entry: RecentEntry, max = RECENT_MAX): RecentEntry[] {
  if (entry.spec.kind === 'picked') return list.slice(0, max);
  const key = sourceKey(entry.spec);
  return [entry, ...list.filter((e) => sourceKey(e.spec) !== key)].slice(0, max);
}

export function parseRecent(raw: string | null): RecentEntry[] {
  if (!raw) return [];
  try {
    const v: unknown = JSON.parse(raw);
    if (!Array.isArray(v)) return [];
    return v
      .filter((e) => e && isSourceSpec(e.spec) && e.spec.kind !== 'picked' && typeof e.label === 'string' && typeof e.at === 'number')
      .slice(0, RECENT_MAX);
  } catch {
    return [];
  }
}

/** Drops windows and displays that no longer exist. */
export function liveRecent(list: RecentEntry[], windowIds: Set<number> | null, displayIds: Set<string>): RecentEntry[] {
  return list.filter((e) => {
    const s = e.spec;
    if (s.kind === 'window') return windowIds ? windowIds.has(s.window_id) : false;
    if (s.kind === 'picked') return false;
    return displayIds.has(s.display_id);
  });
}

function storage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

export function loadRecent(): RecentEntry[] {
  try {
    return parseRecent(storage()?.getItem(RECENT_STORAGE_KEY) ?? null);
  } catch {
    return [];
  }
}

export function rememberSource(spec: SourceSpecDto, label: string): RecentEntry[] {
  const next = pushRecent(loadRecent(), { spec, label, at: Date.now() });
  try {
    storage()?.setItem(RECENT_STORAGE_KEY, JSON.stringify(next));
  } catch {
    // Storage full or blocked: the list just is not remembered.
  }
  return next;
}
