import { test } from 'node:test';
import assert from 'node:assert/strict';
import { liveRecent, parseRecent, pushRecent, RECENT_MAX, type RecentEntry } from '../src/lib/recent.ts';
import { sourceKey, sourceSizePx, topmostWindowAt } from '../src/lib/sources.ts';
import type { SourceSpecDto, WindowInfo } from '../src/lib/types.ts';

const entry = (spec: SourceSpecDto, at = 0): RecentEntry => ({ spec, label: sourceKey(spec), at });
const win = (id: number) => entry({ kind: 'window', window_id: id }, id);

test('recent list: newest first, deduplicated, capped at five', () => {
  let list: RecentEntry[] = [];
  for (const id of [1, 2, 3, 4, 5, 6]) list = pushRecent(list, win(id));
  assert.deepEqual(list.map((e) => e.at), [6, 5, 4, 3, 2]);
  list = pushRecent(list, win(4));
  assert.deepEqual(list.map((e) => e.at), [4, 6, 5, 3, 2]);
  assert.equal(list.length, RECENT_MAX);
});

test('recent list never keeps system-picker picks', () => {
  const list = pushRecent([win(1)], entry({ kind: 'picked', pick_id: 'p1', region_pt: null }));
  assert.deepEqual(list.map((e) => sourceKey(e.spec)), ['window:1']);
});

test('recent list parsing tolerates junk', () => {
  assert.deepEqual(parseRecent('nope'), []);
  assert.deepEqual(parseRecent('{}'), []);
  const ok = parseRecent(JSON.stringify([win(1), { spec: { kind: 'bogus' }, label: 'x', at: 1 }, win(2)]));
  assert.deepEqual(ok.map((e) => e.at), [1, 2]);
});

test('recent list drops sources that are gone', () => {
  const list = [win(1), win(2), entry({ kind: 'display', display_id: '9' }), entry({ kind: 'display', display_id: '1' })];
  assert.deepEqual(liveRecent(list, new Set([2]), new Set(['1'])).map((e) => sourceKey(e.spec)), ['window:2', 'display:1']);
  assert.deepEqual(liveRecent(list, null, new Set(['1'])).map((e) => sourceKey(e.spec)), ['display:1']);
});

const w = (id: number, z: number, x: number, y: number, ww: number, hh: number, on_screen = true): WindowInfo => ({
  id,
  title: `w${id}`,
  app_name: 'App',
  app: { pid: id, bundle_id: null },
  frame_pt: { x, y, w: ww, h: hh },
  display: '1',
  on_screen,
  layer: 0,
  z_index: z,
});

test('z-order hit test picks the frontmost window under the point', () => {
  const windows = [w(1, 2, 0, 0, 500, 500), w(2, 0, 100, 100, 100, 100), w(3, 1, 50, 50, 300, 300), w(4, 0, 0, 0, 1000, 1000, false)];
  assert.equal(topmostWindowAt(windows, { x: 150, y: 150 })?.id, 2);
  assert.equal(topmostWindowAt(windows, { x: 60, y: 60 })?.id, 3);
  assert.equal(topmostWindowAt(windows, { x: 10, y: 10 })?.id, 1);
  assert.equal(topmostWindowAt(windows, { x: 700, y: 700 }), null);
  assert.equal(topmostWindowAt(windows, { x: 200, y: 200 })?.id, 3);
});

test('source pixel sizes', () => {
  const displays = [{ id: '1', name: 'D', frame_pt: { x: 0, y: 0, w: 1512, h: 982 }, scale: 2, size_px: { w: 3024, h: 1964 }, is_main: true }];
  const list = { displays, windows: [w(7, 0, 10, 10, 640.25, 360)], apps: [] };
  assert.deepEqual(sourceSizePx({ kind: 'display', display_id: '1' }, list), { w: 3024, h: 1964 });
  assert.deepEqual(sourceSizePx({ kind: 'window', window_id: 7 }, list), { w: 1281, h: 720 });
  assert.deepEqual(sourceSizePx({ kind: 'region', display_id: '1', region_pt: { x: 0, y: 0, w: 640, h: 360 } }, list), { w: 1280, h: 720 });
  assert.equal(sourceSizePx({ kind: 'picked', pick_id: 'x', region_pt: null }, list), null);
});
