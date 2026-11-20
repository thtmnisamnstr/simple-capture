import { test } from 'node:test';
import assert from 'node:assert/strict';
import { satisfies, check } from './npm-licenses.mjs';

test('SPDX expressions', () => {
  assert.equal(satisfies('MIT'), true);
  assert.equal(satisfies('Apache-2.0 OR MIT'), true);
  assert.equal(satisfies('(MIT OR GPL-3.0-only)'), true);
  assert.equal(satisfies('MIT AND GPL-3.0-only'), false);
  assert.equal(satisfies('GPL-2.0-or-later'), false);
  assert.equal(satisfies('MPL-2.0'), false);
  assert.equal(satisfies('Apache-2.0 WITH LLVM-exception'), true);
  assert.equal(satisfies('(MIT AND Zlib) OR AGPL-3.0'), true);
  assert.throws(() => satisfies('MIT OR'));
});

test('prod is strict, all allows only named dev tools', () => {
  const report = {
    MIT: [{ name: 'svelte', versions: ['5'], license: 'MIT' }],
    'MPL-2.0': [{ name: 'lightningcss-linux-x64-gnu', versions: ['1'], license: 'MPL-2.0' }],
  };
  assert.equal(check(report, 'prod').bad.length, 1);
  assert.equal(check(report, 'all').bad.length, 0);
  const gpl = { 'GPL-3.0': [{ name: 'x', versions: ['1'], license: 'GPL-3.0' }] };
  assert.equal(check(gpl, 'all').bad.length, 1);
  assert.equal(check({}, 'prod').count, 0);
});
