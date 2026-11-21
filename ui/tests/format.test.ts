import { test } from 'node:test';
import assert from 'node:assert/strict';

import { hostOs, revealLabel } from '../src/lib/format.ts';

test('reveal label follows the host OS', () => {
  assert.equal(revealLabel(hostOs('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)')), 'Show in Finder');
  assert.equal(revealLabel(hostOs('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')), 'Show in Explorer');
  assert.equal(revealLabel(hostOs('Mozilla/5.0 (X11; Linux x86_64)')), 'Show in Files');
});
