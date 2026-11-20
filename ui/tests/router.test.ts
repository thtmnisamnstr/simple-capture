import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ROUTES, routeFromHash } from '../src/lib/router.ts';

test('every window label has a route', () => {
  assert.deepEqual([...ROUTES], ['panel', 'onboarding', 'bar', 'switcher', 'region', 'pick', 'saved', 'bubble']);
  for (const r of ROUTES) assert.equal(routeFromHash(`#/${r}`), r);
});

test('unknown or empty hash falls back to the panel', () => {
  assert.equal(routeFromHash(''), 'panel');
  assert.equal(routeFromHash('#/nope'), 'panel');
  assert.equal(routeFromHash('#/bar?x=1'), 'bar');
});
