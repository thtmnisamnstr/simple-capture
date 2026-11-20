import { test } from 'node:test';
import assert from 'node:assert/strict';
import { overlayDisplay, parsePurpose } from '../src/lib/overlay.ts';
import { hashParams } from '../src/lib/router.ts';
import type { DisplayInfo } from '../src/lib/types.ts';

const d = (id: string, x: number, main: boolean): DisplayInfo => ({
  id,
  name: id,
  frame_pt: { x, y: 0, w: 1000, h: 800 },
  scale: 2,
  size_px: { w: 2000, h: 1600 },
  is_main: main,
});
const displays = [d('1', 0, true), d('2', 1000, false)];

test('overlay display: explicit id, then window position, then main', () => {
  assert.equal(overlayDisplay(displays, '2', null)?.id, '2');
  assert.equal(overlayDisplay(displays, null, { x: 1000, y: 0 })?.id, '2');
  assert.equal(overlayDisplay(displays, 'gone', { x: -5000, y: 0 })?.id, '1');
  assert.equal(overlayDisplay([], null, null), null);
});

test('overlay purpose falls back to the recording state', () => {
  assert.equal(parsePurpose('switch', false), 'switch');
  assert.equal(parsePurpose(null, true), 'switch');
  assert.equal(parsePurpose('bogus', false), 'start');
});

test('hash parameters', () => {
  const q = hashParams('#/region?purpose=switch&display=2');
  assert.equal(q.get('purpose'), 'switch');
  assert.equal(q.get('display'), '2');
  assert.equal(hashParams('#/panel').get('purpose'), null);
});
