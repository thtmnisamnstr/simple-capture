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
  assert.deepEqual(sourceSizePx({ kind: 'window', window_id: 7 }, list), { w: 1280, h: 720 });
  assert.deepEqual(sourceSizePx({ kind: 'region', display_id: '1', region_pt: { x: 0, y: 0, w: 640, h: 360 } }, list), { w: 1280, h: 720 });
  assert.equal(sourceSizePx({ kind: 'picked', pick_id: 'x', region_pt: null }, list), null);
});

test('source pixel sizes at 1x match the 1x capture size', () => {
  const displays = [{ id: '1', name: 'D', frame_pt: { x: 0, y: 0, w: 1511, h: 982 }, scale: 2, size_px: { w: 3022, h: 1964 }, is_main: true }];
  const list = { displays, windows: [w(7, 0, 10, 10, 641, 361)], apps: [] };
  assert.deepEqual(sourceSizePx({ kind: 'display', display_id: '1' }, list, '1x'), { w: 1512, h: 982 });
  assert.deepEqual(sourceSizePx({ kind: 'window', window_id: 7 }, list, '1x'), { w: 642, h: 362 });
  assert.deepEqual(sourceSizePx({ kind: 'window', window_id: 7 }, list, 'native'), { w: 1282, h: 722 });
  assert.deepEqual(sourceSizePx({ kind: 'region', display_id: '1', region_pt: { x: 0, y: 0, w: 640, h: 360 } }, list, '1x'), { w: 640, h: 360 });
});

test('application sources are sized to the union of their windows on the display', () => {
  const displays = [
    { id: '1', name: 'D', frame_pt: { x: 0, y: 0, w: 1512, h: 982 }, scale: 2, size_px: { w: 3024, h: 1964 }, is_main: true },
    { id: '2', name: 'E', frame_pt: { x: 1512, y: 0, w: 1920, h: 1080 }, scale: 1, size_px: { w: 1920, h: 1080 }, is_main: false },
  ];
  const aw = (id: number, pid: number, x: number, y: number, ww: number, hh: number, extra: Partial<WindowInfo> = {}): WindowInfo => ({
    ...w(id, id, x, y, ww, hh),
    app: { pid, bundle_id: null },
    ...extra,
  });
  const app = (pid: number, display_id = '1'): SourceSpecDto => ({ kind: 'application', display_id, pid, bundle_id: null });
  const size = (windows: WindowInfo[], spec = app(42)) => sourceSizePx(spec, { displays, windows, apps: [] });

  assert.deepEqual(size([aw(1, 42, 100, 50, 800, 600)]), { w: 1600, h: 1200 }, 'one window');
  assert.deepEqual(size([aw(1, 42, 100, 50, 400, 300), aw(2, 42, 600, 200, 300, 500)]), { w: 1600, h: 1300 }, 'union');
  assert.deepEqual(
    size([
      aw(1, 42, 100, 50, 800, 600),
      aw(2, 7, 0, 0, 1512, 982),
      aw(3, 42, 0, 0, 1512, 982, { on_screen: false }),
      aw(4, 42, 0, 0, 1512, 982, { layer: 25 }),
      aw(5, 42, 0, 0, 1, 982),
    ]),
    { w: 1600, h: 1200 },
    'other apps, off-screen, non-zero layer and tiny windows are ignored',
  );
  assert.deepEqual(size([]), { w: 3024, h: 1964 }, 'no windows: whole display');
  assert.deepEqual(size([aw(1, 42, 2000, 0, 500, 400)]), { w: 3024, h: 1964 }, 'only on another display: whole display');
  assert.deepEqual(size([aw(1, 42, -100, 900, 400, 300)]), { w: 600, h: 164 }, 'clipped to the display');
  assert.deepEqual(size([aw(1, 42, 2000, 100, 500, 400)], app(42, '2')), { w: 500, h: 400 }, 'secondary display at 1x');
  const small = size([aw(1, 42, 100, 50, 800, 600)]);
  assert.notDeepEqual(small, { w: 3024, h: 1964 }, 'a smaller app is not display-sized');
  assert.equal(sourceSizePx(app(42, '9'), { displays, windows: [], apps: [] }), null, 'unknown display');
  assert.deepEqual(size([aw(1, 42, 10.25, 20, 300.5, 201)]), { w: 600, h: 402 }, 'odd points snap at native scale');
});

test('application sources at 1x match the 1x capture size', () => {
  const displays = [{ id: '1', name: 'D', frame_pt: { x: 0, y: 0, w: 1512, h: 982 }, scale: 2, size_px: { w: 3024, h: 1964 }, is_main: true }];
  const spec: SourceSpecDto = { kind: 'application', display_id: '1', pid: 42, bundle_id: null };
  const at = (frames: [number, number, number, number][], res: 'native' | '1x') =>
    sourceSizePx(spec, { displays, windows: frames.map(([x, y, ww, hh], i) => ({ ...w(i + 1, i, x, y, ww, hh), app: { pid: 42, bundle_id: null } })), apps: [] }, res);

  assert.deepEqual(at([[100, 50, 800, 600]], '1x'), { w: 800, h: 600 }, 'one window at 1x');
  assert.deepEqual(at([[100, 50, 800, 600]], 'native'), { w: 1600, h: 1200 }, 'same window at native');
  assert.deepEqual(at([[0, 0, 301, 201]], '1x'), { w: 300, h: 200 }, 'odd-point window floors to even at 1x');
  assert.deepEqual(at([[0, 0, 301, 201]], 'native'), { w: 602, h: 402 }, 'odd-point window at native');
  assert.deepEqual(at([], '1x'), { w: 1512, h: 982 }, 'no windows at 1x: whole display in points');
  const odd = [{ ...displays[0], frame_pt: { x: 0, y: 0, w: 1511, h: 981 }, size_px: { w: 3021, h: 1961 } }];
  assert.deepEqual(sourceSizePx(spec, { displays: odd, windows: [], apps: [] }, '1x'), { w: 1512, h: 982 }, 'whole display at 1x is even');
  assert.deepEqual(sourceSizePx(spec, { displays: odd, windows: [], apps: [] }), { w: 3022, h: 1962 }, 'whole display at native is even');
});
