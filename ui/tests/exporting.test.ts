import { test } from 'node:test';
import assert from 'node:assert/strict';

import { DESTINATIONS, destinationOf, forFormat } from '../src/lib/exporting.ts';

test('every destination round-trips through destinationOf', () => {
  for (const d of DESTINATIONS) assert.equal(destinationOf(d.settings()), d.id, d.id);
});

test('hand-made settings match no destination', () => {
  assert.equal(destinationOf({ ...forFormat('mp4'), quality: { tier: 'custom', crf: 30, kbps: null } }), null);
});

test('email stays under the attachment limit and vertical asks for 9:16', () => {
  const email = DESTINATIONS.find((d) => d.id === 'email')!.settings();
  assert.deepEqual(email.quality, { tier: 'target_size', mb: 18 });
  assert.equal(DESTINATIONS.find((d) => d.id === 'vertical')!.frame, '9:16');
});
