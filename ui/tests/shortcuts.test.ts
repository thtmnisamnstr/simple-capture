import { test } from 'node:test';
import assert from 'node:assert/strict';
import { keysFromEvent, keysLabel } from '../src/lib/shortcuts.ts';

const ev = (code: string, mods: Partial<Record<'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey', boolean>> = {}) => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

test('shortcut keys from a keydown, in a fixed modifier order', () => {
  assert.equal(keysFromEvent(ev('KeyR', { shiftKey: true, altKey: true })), 'Alt+Shift+KeyR');
  assert.equal(keysFromEvent(ev('Digit5', { metaKey: true, ctrlKey: true })), 'Control+Super+Digit5');
  assert.equal(keysFromEvent(ev('F5')), 'F5');
  assert.equal(keysFromEvent(ev('ShiftLeft', { shiftKey: true })), null);
  assert.equal(keysFromEvent(ev('MetaRight', { metaKey: true })), null);
});

test('shortcut labels read like the OS writes them', () => {
  assert.equal(keysLabel('Alt+Shift+KeyR', true), '⌥⇧R');
  assert.equal(keysLabel('Super+Control+Digit1', true), '⌃⌘1');
  assert.equal(keysLabel('Alt+Shift+KeyR', false), 'Alt+Shift+R');
  assert.equal(keysLabel('Control+Super+ArrowUp', false), 'Ctrl+Win+↑');
  assert.equal(keysLabel('', true), 'None');
});
