import { test } from 'node:test';
import assert from 'node:assert/strict';
import { COMMANDS } from '../src/lib/api.ts';
import { MOCK_COMMANDS, mockInvoke } from '../src/lib/mock.ts';

test('the mock covers every app command', () => {
  assert.deepEqual([...MOCK_COMMANDS].sort(), [...COMMANDS].sort());
});

test('mock permissions never prompt and report statuses', async () => {
  const p = await mockInvoke<{ screen: string }>('get_permissions');
  assert.equal(p.screen, 'granted');
});
