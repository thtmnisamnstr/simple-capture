import { test } from 'node:test';
import assert from 'node:assert/strict';
import { uiLogLine } from '../src/lib/uilog.ts';
import { isObject } from '../src/lib/guards.ts';

test('ui log lines carry the route', () => {
  assert.equal(uiLogLine('panel', 'csp', 'script-src-elem blocked inline'), '[panel] csp: script-src-elem blocked inline');
});

test('object guard', () => {
  assert.equal(isObject({ recovery: [] }), true);
  assert.equal(isObject([]), false);
  assert.equal(isObject(null), false);
});
