import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PRESETS, applyPreset, defaultStyle, fontFaceCss, newTextItem, snapAngle, snapPos } from '../src/lib/text.ts';

test('angles snap to 0, ±15, ±90 and 180', () => {
  assert.equal(snapAngle(2), 0);
  assert.equal(snapAngle(13), 15);
  assert.equal(snapAngle(-88), -90);
  assert.equal(snapAngle(179), 180);
  assert.equal(snapAngle(-179), 180);
  assert.equal(snapAngle(40), 40);
  assert.equal(snapAngle(362), 0);
});

test('positions snap to the centre, thirds and safe margins', () => {
  assert.deepEqual(snapPos(0.505, 0.01), { v: 0.5, guide: 0.5 });
  assert.equal(snapPos(0.34, 0.01).v, 1 / 3);
  assert.equal(snapPos(0.052, 0.01).v, 0.05);
  assert.deepEqual(snapPos(0.42, 0.01), { v: 0.42, guide: null });
});

test('new items last 3 s from the playhead and presets keep other fields', () => {
  const it = newTextItem('x', 1_500_000_000);
  assert.equal(it.end_tl_ns - it.start_tl_ns, 3_000_000_000);
  const base = { ...defaultStyle(), size_1080: 99 };
  const neon = applyPreset(base, PRESETS.find((p) => p.name === 'Neon')!);
  assert.equal(neon.font_family, 'Tilt Neon');
  assert.equal(neon.size_1080, 99);
  assert.ok(neon.glow);
});

test('font faces become @font-face rules with weight ranges', () => {
  const css = fontFaceCss(
    [{ name: 'Inter', kind: '', default_weight: 700, weights: [100, 900], static_weights: [], italic: false, variable: true, faces: [{ file: 'Inter[opsz,wght].ttf', italic: false, weights: [100, 900] }] }],
    'sc://font',
  );
  assert.match(css, /font-family:'Inter'/);
  assert.match(css, /Inter%5Bopsz%2Cwght%5D\.ttf/);
  assert.match(css, /font-weight:100 900/);
});
