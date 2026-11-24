import { test } from 'node:test';
import assert from 'node:assert/strict';
import { NS, formatTime, keyAt, locate, recToTl, snapTo, starts, tickStep, tlLen, wholeNs, stackRows, type Clip } from '../src/lib/editor.ts';
import type { CameraKey } from '../src/lib/types.ts';

const clip = (a: number, b: number, num = 1, den = 1): Clip => ({
  id: `${a}-${b}`,
  src_in_rec_ns: a * NS,
  src_out_rec_ns: b * NS,
  speed: { num, den },
  muted: false,
});

test('timeline maths match the Rust edit model', () => {
  const clips = [clip(0, 4), clip(10, 14, 2, 1), clip(20, 21, 1, 2)];
  assert.deepEqual(clips.map(tlLen), [4 * NS, 2 * NS, 2 * NS]);
  const st = starts(clips);
  assert.deepEqual(st, [0, 4 * NS, 6 * NS, 8 * NS]);
  assert.equal(locate(clips, st, 3 * NS)?.rec_ns, 3 * NS);
  assert.deepEqual(locate(clips, st, 5 * NS), { index: 1, start: 4 * NS, end: 6 * NS, rec_ns: 12 * NS });
  assert.equal(locate(clips, st, 7 * NS)?.rec_ns, 20.5 * NS);
  assert.equal(locate(clips, st, 8 * NS), null);
  assert.deepEqual(recToTl(clips, st, 12 * NS), [5 * NS]);
  assert.deepEqual(recToTl([clip(0, 5), clip(0, 5)], starts([clip(0, 5), clip(0, 5)]), 2 * NS), [2 * NS, 7 * NS]);
});

test('formatting, snapping and ticks', () => {
  assert.equal(formatTime(0), '0:00.0');
  assert.equal(formatTime(61.25 * NS), '1:01.2');
  assert.equal(formatTime(3725 * NS), '1:02:05.0');
  assert.equal(formatTime(1.5 * NS, 30), '0:01:15');
  assert.equal(snapTo(1.01 * NS, [NS, 2 * NS], 0.05 * NS), NS);
  assert.equal(snapTo(1.2 * NS, [NS, 2 * NS], 0.05 * NS), 1.2 * NS);
  assert.equal(tickStep(40), 2 * NS);
  assert.equal(tickStep(400), 0.25 * NS);
});

test('the key in effect is the last at or before the time', () => {
  const k = (t: number) => ({ t_rec_ns: t }) as CameraKey;
  assert.equal(keyAt([], 5), null);
  assert.equal(keyAt([k(0), k(10)], 5)?.t_rec_ns, 0);
  assert.equal(keyAt([k(0), k(10)], 10)?.t_rec_ns, 10);
  assert.equal(keyAt([k(3)], 1)?.t_rec_ns, 3);
});

test('edit ops send whole nanoseconds, however deep', () => {
  const op = { kind: 'set_text', item: { start_tl_ns: 1.2 * 1e9 + 0.4, style: { size: 12.5 }, keys: [{ t_tl_ns: 3.7 }] } };
  const w = wholeNs(op);
  assert.equal(w.item.start_tl_ns, 1_200_000_000);
  assert.equal(w.item.keys[0].t_tl_ns, 4);
  assert.equal(w.item.style.size, 12.5, 'only times are rounded');
});

test('overlapping items stack; the rest share the first row', () => {
  const it = (id: string, s: number, e: number) => ({ id, start_tl_ns: s, end_tl_ns: e });
  const { rows, row } = stackRows([it('a', 0, 10), it('b', 5, 15), it('c', 10, 20), it('d', 6, 8), it('e', 30, 40)]);
  assert.equal(rows, 3);
  assert.deepEqual(['a', 'b', 'c', 'd', 'e'].map((k) => row.get(k)), [0, 1, 0, 2, 0]);
  assert.equal(stackRows([]).rows, 1);
});
