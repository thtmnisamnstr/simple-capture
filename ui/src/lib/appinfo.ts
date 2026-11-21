// The `app` command: About, licenses, the self-test and the Library.

import { invoke } from '@tauri-apps/api/core';
import { inTauri } from './api.ts';
import { mockInvoke } from './mock.ts';

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
}
export interface Library { root: string; free_bytes: number | null; items: LibraryItem[] }

export function call<T>(action: string, args: Record<string, unknown> = {}): Promise<T> {
  return inTauri() ? invoke<T>('app', { action, args }) : mockInvoke<T>('app', { action, ...args });
}

export const appApi = {
  about: () => call<AboutInfo>('about'),
  licenseText: (path: string) => call<string>('license_text', { path }),
  runDiagnostics: () => call<null>('diagnostics_run'),
  library: () => call<Library>('library'),
  trash: (name: string) => call<null>('library_trash', { name }),
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
  const name = dir.split(/[\\/]/).filter(Boolean).pop() ?? '';
  const base = /Windows|Android/.test(ua) ? 'http://sc.localhost/poster' : 'sc://poster';
  return `${base}/${encodeURIComponent(name)}.png`;
}

export function folderName(dir: string): string {
  return dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;
}

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
