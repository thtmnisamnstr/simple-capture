import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { cameraRects, cornerCenter, ratio, snapCenter, type Corner } from '../src/lib/geometry.ts';

const cases = JSON.parse(readFileSync(new URL('../../crates/geometry/tests/camera_cases.json', import.meta.url), 'utf8'));
const size = ([w, h]: number[]) => ({ w, h });
const pt = ([x, y]: number[]) => ({ x, y });
const close = (a: number, b: number) => Math.abs(a - b) <= 1e-9 * Math.max(Math.abs(b), 1);
const closeRect = (r: { x: number; y: number; w: number; h: number }, [x, y, w, h]: number[]) =>
  close(r.x, x) && close(r.y, y) && close(r.w, w) && close(r.h, h);

test('cameraRects matches the shared Rust cases', () => {
  for (const c of cases.rects) {
    const p = c.pose;
    const r = cameraRects(size(c.canvas), size(c.camera), ratio(c.aspect[0], c.aspect[1]), {
      center: pt(p.center), size: p.size, zoom: p.zoom, pan: pt(p.pan), radius: p.radius,
    });
    assert.ok(closeRect(r.dst, c.dst), `${c.name}: dst ${JSON.stringify(r.dst)}`);
    assert.ok(closeRect(r.src, c.src), `${c.name}: src ${JSON.stringify(r.src)}`);
    assert.ok(close(r.radius_px, c.radius_px), `${c.name}: radius ${r.radius_px}`);
  }
});

test('corner presets and snapping match the shared Rust cases', () => {
  for (const c of cases.corners) {
    const got = cornerCenter(size(c.canvas), ratio(c.aspect[0], c.aspect[1]), c.size, c.corner as Corner);
    assert.ok(close(got.x, c.center[0]) && close(got.y, c.center[1]), JSON.stringify(got));
  }
  for (const c of cases.snaps) {
    const got = snapCenter(size(c.canvas), ratio(c.aspect[0], c.aspect[1]), c.size, pt(c.center), c.threshold_px);
    assert.ok(close(got.x, c.snapped[0]) && close(got.y, c.snapped[1]), `${c.name}: ${JSON.stringify(got)}`);
  }
});
