// Global shortcut keys as the app stores them ("Alt+Shift+KeyR") and as people read them.

import { call } from './appinfo.ts';

export interface ShortcutRow {
  id: string;
  label: string;
  keys: string;
  default: string;
  error: string | null;
}

export const shortcutsApi = {
  list: () => call<ShortcutRow[]>('shortcuts'),
  set: (id: string, keys: string) => call<ShortcutRow[]>('shortcut_set', { id, keys }),
  pause: (paused: boolean) => call<ShortcutRow[]>('shortcuts_pause', { paused }),
};

const MODS = ['Control', 'Alt', 'Shift', 'Super'] as const;

/** The keys a keydown names, or `null` while only modifiers are held. */
export function keysFromEvent(e: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>): string | null {
  if (!e.code || /^(Shift|Control|Alt|Meta|OS|Fn|CapsLock)/.test(e.code)) return null;
  const held = [e.ctrlKey, e.altKey, e.shiftKey, e.metaKey];
  return [...MODS.filter((_, i) => held[i]), e.code].join('+');
}

const ARROWS: Record<string, string> = { ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→' };

function keyName(code: string): string {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  return ARROWS[code] ?? code;
}

/** "⌃⌥⇧⌘R" on a Mac, "Ctrl+Alt+Shift+Win+R" elsewhere; "None" for no keys. */
export function keysLabel(keys: string, mac: boolean): string {
  if (!keys) return 'None';
  const parts = keys.split('+');
  const key = keyName(parts[parts.length - 1]);
  const has = (m: string) => parts.slice(0, -1).some((p) => p.toLowerCase() === m.toLowerCase());
  if (mac) {
    const glyphs: [string, string][] = [['Control', '⌃'], ['Alt', '⌥'], ['Shift', '⇧'], ['Super', '⌘']];
    return glyphs.filter(([m]) => has(m)).map(([, g]) => g).join('') + key;
  }
  const names: [string, string][] = [['Control', 'Ctrl'], ['Alt', 'Alt'], ['Shift', 'Shift'], ['Super', 'Win']];
  return [...names.filter(([m]) => has(m)).map(([, n]) => n), key].join('+');
}
