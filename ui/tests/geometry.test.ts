import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  appBoundsPt,
  aspectLockedResize,
  aspectRatio,
  containFit,
  coverSrcRect,
  evenRound,
  flipY,
  HANDLES,
  presetSize,
  ptToPx,
  ratio,
  ratioF,
  ratioOf,
  referenceCanvasSize,
  smallContent,
  snappedRegionSizePx,
  snapRegionEvenPx,
  type GeoAspect,
  type SizeClass,
} from '../src/lib/geometry.ts';

test('contain fit worked examples (ARCHITECTURE §8)', () => {
  assert.deepEqual(containFit({ w: 1920, h: 1080 }, { w: 7680, h: 2160 }), { x: 0, y: 270, w: 1920, h: 540 });
  assert.deepEqual(containFit({ w: 1080, h: 1920 }, { w: 7680, h: 2160 }), { x: 0, y: 808, w: 1080, h: 304 });
});

test('reference canvas size examples', () => {
  const big = { w: 7680, h: 2160 };
  const r = ratioOf(big);
  assert.deepEqual(referenceCanvasSize('16:9', r, [big]), { w: 3840, h: 2160 });
  assert.deepEqual(referenceCanvasSize('9:16', r, [big]), { w: 2160, h: 3840 });
  assert.deepEqual(referenceCanvasSize('original', r, [big]), { w: 7680, h: 2160 });
  const win = { w: 3024, h: 1964 };
  assert.deepEqual(referenceCanvasSize('16:9', ratioOf(win), [win]), { w: 3492, h: 1964 });
  assert.deepEqual(referenceCanvasSize('original', ratio(16, 9), [{ w: 1281, h: 721 }]), { w: 1280, h: 720 });
});

test('Original with several segments caps the long side at 8192', () => {
  const s = referenceCanvasSize('original', ratio(32, 9), [{ w: 7680, h: 2160 }, { w: 3024, h: 1964 }]);
  assert.ok(Math.max(s.w, s.h) <= 8192);
  assert.equal(s.w % 2, 0);
  assert.equal(s.h % 2, 0);
});

test('preset table (ARCHITECTURE §10)', () => {
  const table: Record<number, string[]> = {
    2160: ['3840×2160', '2880×2160', '2160×2160', '2160×3840', '2160×2700'],
    1440: ['2560×1440', '1920×1440', '1440×1440', '1440×2560', '1440×1800'],
    1080: ['1920×1080', '1440×1080', '1080×1080', '1080×1920', '1080×1350'],
    720: ['1280×720', '960×720', '720×720', '720×1280', '720×900'],
    480: ['854×480', '640×480', '480×480', '480×854', '480×600'],
    360: ['640×360', '480×360', '360×360', '360×640', '360×450'],
    240: ['427×240', '320×240', '240×240', '240×427', '240×300'],
  };
  const aspects: GeoAspect[] = ['16:9', '4:3', '1:1', '9:16', '4:5'];
  for (const [cls, row] of Object.entries(table)) {
    const gif = Number(cls) <= 360;
    aspects.forEach((a, i) => {
      const s = presetSize(Number(cls) as SizeClass, aspectRatio(a)!, gif);
      assert.equal(`${s.w}×${s.h}`, row[i], `${cls} ${a}`);
    });
  }
  assert.deepEqual(presetSize(240, ratio(16, 9), false), { w: 426, h: 240 });
});

test('even rounding', () => {
  assert.equal(evenRound(853.33), 854);
  assert.equal(evenRound(3491.56), 3492);
  assert.equal(evenRound(0.2), 2);
  assert.equal(evenRound(5), 6);
});

function xorshift(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s >>>= 0;
    s ^= s >>> 17;
    s ^= s << 5;
    s >>>= 0;
    return s / 0x100000000;
  };
}

test('contain fit properties over 100k seeded cases', () => {
  const rnd = xorshift(0x5eed);
  const dim = (max: number) => 2 + Math.floor(rnd() * max);
  for (let i = 0; i < 100_000; i++) {
    const canvas = { w: dim(4000), h: dim(4000) };
    const content = { w: dim(8000), h: dim(8000) };
    const d = containFit(canvas, content);
    assert.ok(d.x >= 0 && d.y >= 0 && d.x + d.w <= canvas.w && d.y + d.h <= canvas.h, 'inside');
    assert.ok(Number.isInteger(d.x) && Number.isInteger(d.w), 'snapped');
    const s = Math.min(canvas.w / content.w, canvas.h / content.h);
    assert.ok(Math.abs(d.w - content.w * s) <= 1 && Math.abs(d.h - content.h * s) <= 1, 'aspect error ≤ 1 px');
    assert.ok(Math.abs(d.x + d.w / 2 - canvas.w / 2) <= 0.5 && Math.abs(d.y + d.h / 2 - canvas.h / 2) <= 0.5, 'centred');
  }
});

test('small content rule', () => {
  assert.equal(smallContent({ w: 1080, h: 1920 }, { w: 7680, h: 2160 }), true);
  assert.equal(smallContent({ w: 1920, h: 1080 }, { w: 7680, h: 2160 }), false);
  assert.equal(smallContent({ w: 3840, h: 2160 }, { w: 7680, h: 2160 }), false);
  assert.equal(smallContent({ w: 2160, h: 3840 }, { w: 7680, h: 2160 }), true);
  assert.equal(smallContent({ w: 1920, h: 1080 }, { w: 2000, h: 4000 }), false);
  assert.equal(smallContent({ w: 1920, h: 1080 }, { w: 100, h: 4000 }), true);
});

test('points, pixels and flips', () => {
  assert.deepEqual(ptToPx({ x: 10.25, y: 0, w: 100.5, h: 50 }, 2), { x: 21, y: 0, w: 201, h: 100 });
  assert.deepEqual(flipY({ x: 0, y: 10, w: 5, h: 20 }, 100), { x: 0, y: 70, w: 5, h: 20 });
  const s = snapRegionEvenPx({ x: 10.3, y: 4.1, w: 100.7, h: 33.3 }, 2);
  assert.equal((s.w * 2) % 2, 0);
  assert.equal((s.h * 2) % 2, 0);
  assert.ok(Number.isInteger(s.x * 2) && Number.isInteger(s.y * 2));
});

test('cover source rect stays inside the image and keeps the dst ratio', () => {
  const r = coverSrcRect({ w: 4000, h: 3000 }, { w: 1920, h: 1080 }, { x: 0.9, y: 0.5 });
  assert.ok(r.x >= 0 && r.x + r.w <= 4000 + 1e-9 && r.y >= 0 && r.y + r.h <= 3000 + 1e-9);
  assert.ok(Math.abs(r.w / r.h - 1920 / 1080) < 1e-9);
});

test('aspect-locked region resize keeps the ratio and stays in bounds', () => {
  const bounds = { x: 0, y: 0, w: 1512, h: 982 };
  const rnd = xorshift(42);
  const locks = [ratio(16, 9), ratio(9, 16), ratio(4, 3), ratio(1, 1)];
  for (let i = 0; i < 20_000; i++) {
    const lock = locks[i % locks.length];
    const w = 20 + rnd() * 600;
    const r = { x: rnd() * (bounds.w - w), y: rnd() * 400, w, h: w / ratioF(lock) };
    if (r.y + r.h > bounds.h) continue;
    const handle = HANDLES[Math.floor(rnd() * HANDLES.length)];
    const to = { x: -200 + rnd() * 1900, y: -200 + rnd() * 1400 };
    const out = aspectLockedResize(r, lock, handle, to, bounds);
    const eps = 1e-6;
    assert.ok(Math.abs(out.w / out.h - ratioF(lock)) < 1e-6, `ratio ${handle}`);
    assert.ok(out.x >= -eps && out.y >= -eps && out.x + out.w <= bounds.w + eps && out.y + out.h <= bounds.h + eps, `bounds ${handle}`);
    assert.ok(out.w > 0 && out.h > 0);
  }
});

test('free resize follows the handle and move clamps to bounds', () => {
  const bounds = { x: 0, y: 0, w: 1000, h: 800 };
  const r = { x: 100, y: 100, w: 200, h: 100 };
  assert.deepEqual(aspectLockedResize(r, null, 'se', { x: 500, y: 450 }, bounds), { x: 100, y: 100, w: 400, h: 350 });
  assert.deepEqual(aspectLockedResize(r, null, 'nw', { x: 400, y: 300 }, bounds), { x: 300, y: 200, w: 100, h: 100 });
  assert.deepEqual(aspectLockedResize(r, null, 'move', { x: 950, y: -20 }, bounds), { x: 800, y: 0, w: 200, h: 100 });
  const locked = aspectLockedResize(r, ratio(16, 9), 'se', { x: 420, y: 120 }, bounds);
  assert.deepEqual([locked.x, locked.y, Math.round(locked.w)], [100, 100, 320]);
});

test('app bounds: union of windows clipped to the display, display-local (mirrors Rust)', () => {
  const d = { x: 0, y: 0, w: 1512, h: 982 };
  const right = { x: 1512, y: -100, w: 1920, h: 1080 };
  const r = (x: number, y: number, w: number, h: number) => ({ x, y, w, h });
  const cases: [string, ReturnType<typeof r>[], ReturnType<typeof r>, ReturnType<typeof r> | null][] = [
    ['no windows', [], d, null],
    ['one window', [r(100, 50, 800, 600)], d, r(100, 50, 800, 600)],
    ['union of two', [r(100, 50, 400, 300), r(600, 200, 300, 500)], d, r(100, 50, 800, 650)],
    ['clipped to the display', [r(-50, 900, 300, 200)], d, r(0, 900, 250, 82)],
    ['window on another display is ignored', [r(100, 50, 200, 100), r(1600, 0, 800, 600)], d, r(100, 50, 200, 100)],
    ['display-local on a secondary display', [r(1612, 0, 400, 300)], right, r(100, 100, 400, 300)],
    ['sliver under 2 pt', [r(1511, 10, 100, 100)], d, null],
    ['empty window', [r(10, 10, 0, 0)], d, null],
    ['non-finite window skipped', [r(NaN, 0, 10, 10), r(10, 10, 20, 20)], d, r(10, 10, 20, 20)],
    ['covers the whole display', [r(-10, -10, 2000, 2000)], d, d],
  ];
  for (const [name, windows, display, want] of cases) assert.deepEqual(appBoundsPt(windows, display), want, name);
});

test('snapped region size matches the capture (edges rounded, floored to even)', () => {
  assert.deepEqual(snappedRegionSizePx({ x: 100, y: 50, w: 800, h: 600 }, 2), { w: 1600, h: 1200 });
  assert.deepEqual(snappedRegionSizePx({ x: 10.25, y: 20, w: 300.5, h: 201 }, 2), { w: 600, h: 402 });
  assert.deepEqual(snappedRegionSizePx({ x: 0, y: 0, w: 0.2, h: 0.2 }, 2), { w: 2, h: 2 });
});
