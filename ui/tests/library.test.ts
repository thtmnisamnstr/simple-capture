import { test } from 'node:test';
import assert from 'node:assert/strict';
import { clickSelect, sortItems, type LibraryItem } from '../src/lib/appinfo.ts';

const item = (dir: string, created: string, mins: number, mb: number, title = dir): LibraryItem => ({
  dir,
  title,
  created_at: created,
  state: 'finalized',
  duration_ns: mins * 60e9,
  edited_ns: 0,
  bytes: mb * 1e6,
  has_camera: false,
  modified_s: 0,
});

const items = [
  item('a', '2026-11-19T09:00:00Z', 1, 300, 'Demo 10'),
  item('b', '2026-11-21T09:00:00Z', 5, 100, 'demo 2'),
  item('c', '2026-11-20T09:00:00Z', 3, 200, 'Alpha'),
];

test('library sorts by date, length, size and name', () => {
  const order = (by: Parameters<typeof sortItems>[1]) => sortItems(items, by).map((i) => i.dir).join('');
  assert.equal(order('newest'), 'bca');
  assert.equal(order('oldest'), 'acb');
  assert.equal(order('longest'), 'bca');
  assert.equal(order('largest'), 'acb');
  assert.equal(order('name'), 'cba');
  assert.equal(items.map((i) => i.dir).join(''), 'abc', 'sorting copies');
});

test('library selection: click, command-click and shift-click', () => {
  const order = ['a', 'b', 'c', 'd', 'e'];
  const none = { toggle: false, extend: false };
  let r = clickSelect(order, [], null, 'b', none);
  assert.deepEqual(r, { selected: ['b'], anchor: 'b' });
  r = clickSelect(order, r.selected, r.anchor, 'd', { toggle: false, extend: true });
  assert.deepEqual(r.selected, ['b', 'c', 'd']);
  assert.equal(r.anchor, 'b');
  r = clickSelect(order, r.selected, r.anchor, 'a', { toggle: false, extend: true });
  assert.deepEqual(r.selected, ['a', 'b']);
  r = clickSelect(order, r.selected, r.anchor, 'e', { toggle: true, extend: false });
  assert.deepEqual(r, { selected: ['a', 'b', 'e'], anchor: 'e' });
  r = clickSelect(order, r.selected, r.anchor, 'a', { toggle: true, extend: false });
  assert.deepEqual(r.selected, ['b', 'e']);
  r = clickSelect(order, r.selected, 'gone', 'c', { toggle: false, extend: true });
  assert.deepEqual(r, { selected: ['c'], anchor: 'c' });
});
