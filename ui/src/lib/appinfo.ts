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

export function call<T>(action: string, args: Record<string, unknown> = {}): Promise<T> {
  return inTauri() ? invoke<T>('app', { action, args }) : mockInvoke<T>('app', { action, ...args });
}

export const appApi = {
  about: () => call<AboutInfo>('about'),
  licenseText: (path: string) => call<string>('license_text', { path }),
  runDiagnostics: () => call<null>('diagnostics_run'),
};

export type LineKind = 'pass' | 'fail' | 'skip' | 'head' | 'info';

export function lineKind(line: string): LineKind {
  const t = line.trimStart();
  if (t.startsWith('PASS ')) return 'pass';
  if (t.startsWith('FAIL ')) return 'fail';
  if (t.startsWith('SKIP ')) return 'skip';
  return line.startsWith(' ') ? 'info' : 'head';
}
