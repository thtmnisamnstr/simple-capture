// The `app` command: About, licenses, the self-test and the Library.

import { invoke } from '@tauri-apps/api/core';
import { inTauri, viaMock } from './api.ts';
import { basename, scUrl } from './format.ts';

export interface LicenseFile { name: string; path: string; html: boolean }
export interface LicenseEntry { title: string; license: string; note: string; files: LicenseFile[] }
export interface AboutInfo { version: string; os: string; arch: string; licenses: LicenseEntry[] }
export interface DiagnosticsDone {
  passed: number;
  failed: number;
  skipped: number;
  text: string;
  report: string | null;
  error: string | null;
}

export interface LibraryItem {
  dir: string;
  title: string;
  created_at: string;
  state: 'recording' | 'finalized' | 'recovered' | 'damaged' | string;
  duration_ns: number | null;
  edited_ns: number;
  bytes: number;
  has_camera: boolean;
  modified_s: number;
  recovery?: { dropped?: { track_id?: string; segment_id?: string; reason?: string }[]; notes?: string[] };
}
export interface Library { root: string; free_bytes: number | null; items: LibraryItem[] }

export function call<T>(action: string, args: Record<string, unknown> = {}): Promise<T> {
  return inTauri() ? invoke<T>('app', { action, args }) : viaMock<T>('app', { action, ...args });
}

export const appApi = {
  about: () => call<AboutInfo>('about'),
  recStatus: () => call<{ paused: boolean; elapsed_ns: number }>('rec_status'),
  trayItem: (id: string) => call<unknown>('tray_item', { id }),
  licenseText: (path: string) => call<string>('license_text', { path }),
  runDiagnostics: () => call<null>('diagnostics_run'),
  library: () => call<Library>('library'),
  trash: (name: string) => call<null>('library_trash', { name }),
  item: (name: string) => call<LibraryItem | null>('library_item', { name }),
  rename: (name: string, title: string) => call<{ dir: string }>('library_rename', { name, title }),
  exportFrom: (name: string) => call<null>('library_export', { name }),
};

export type LineKind = 'pass' | 'fail' | 'skip' | 'head' | 'info';

export function lineKind(line: string): LineKind {
  const t = line.trimStart();
  if (t.startsWith('PASS ')) return 'pass';
  if (t.startsWith('FAIL ')) return 'fail';
  if (t.startsWith('SKIP ')) return 'skip';
  return line.startsWith(' ') ? 'info' : 'head';
}

export function posterUrl(dir: string, ua: string = typeof navigator === 'undefined' ? '' : navigator.userAgent): string {
  return `${scUrl('poster', ua)}/${encodeURIComponent(basename(dir))}.png`;
}

export const folderName = basename;

/** "Today 10:47", "Yesterday 18:02" or "Nov 3, 2026 09:15" from an RFC 3339 time. */
export function whenLabel(rfc3339: string, now: Date = new Date()): string {
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return rfc3339;
  const time = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(now) - day(d)) / 86_400_000);
  if (diff === 0) return `Today ${time}`;
  if (diff === 1) return `Yesterday ${time}`;
  return `${d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })} ${time}`;
}

export type LibrarySort = 'newest' | 'oldest' | 'longest' | 'largest' | 'name';

export const LIBRARY_SORTS: { value: LibrarySort; label: string }[] = [
  { value: 'newest', label: 'Newest First' },
  { value: 'oldest', label: 'Oldest First' },
  { value: 'longest', label: 'Longest First' },
  { value: 'largest', label: 'Largest First' },
  { value: 'name', label: 'Name' },
];

export const itemLength = (i: LibraryItem): number => (i.edited_ns > 0 ? i.edited_ns : (i.duration_ns ?? 0));

export function sortItems(items: LibraryItem[], by: LibrarySort): LibraryItem[] {
  const newest = (a: LibraryItem, b: LibraryItem) => b.created_at.localeCompare(a.created_at);
  const cmp: Record<LibrarySort, (a: LibraryItem, b: LibraryItem) => number> = {
    newest,
    oldest: (a, b) => -newest(a, b),
    longest: (a, b) => itemLength(b) - itemLength(a) || newest(a, b),
    largest: (a, b) => b.bytes - a.bytes || newest(a, b),
    name: (a, b) => a.title.localeCompare(b.title, undefined, { numeric: true, sensitivity: 'base' }) || newest(a, b),
  };
  return [...items].sort(cmp[by]);
}

/** Finder-style selection: a plain click picks one, ⌘ toggles one, ⇧ extends from the anchor. */
export function clickSelect(
  order: string[],
  selected: string[],
  anchor: string | null,
  target: string,
  mods: { toggle: boolean; extend: boolean },
): { selected: string[]; anchor: string } {
  if (mods.extend && anchor && order.includes(anchor)) {
    const [a, b] = [order.indexOf(anchor), order.indexOf(target)].sort((x, y) => x - y);
    const range = order.slice(a, b + 1);
    const base = mods.toggle ? selected.filter((d) => !range.includes(d)) : [];
    return { selected: [...base, ...range], anchor };
  }
  if (mods.toggle) {
    const on = selected.includes(target);
    return { selected: on ? selected.filter((d) => d !== target) : [...selected, target], anchor: target };
  }
  return { selected: [target], anchor: target };
}
