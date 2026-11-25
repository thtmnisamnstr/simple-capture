import { test } from 'node:test';
import assert from 'node:assert/strict';
import { framePlanes } from '../src/lib/live.ts';

test('frame planes: RGBA is one plane, I420 three, odd I420 is refused', () => {
  assert.deepEqual(framePlanes(0, 5, 3), [[5, 3, 4]]);
  assert.deepEqual(framePlanes(1, 4, 2), [
    [4, 2, 1],
    [2, 1, 1],
    [2, 1, 1],
  ]);
  assert.equal(framePlanes(1, 5, 2), null);
  assert.equal(framePlanes(7, 4, 2), null);
});
