import { test } from 'node:test';
import assert from 'node:assert/strict';

import { hostOs, revealLabel } from '../src/lib/format.ts';

test('reveal label follows the host OS', () => {
  assert.equal(revealLabel(hostOs('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)')), 'Show in Finder');
  assert.equal(revealLabel(hostOs('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')), 'Show in Explorer');
  assert.equal(revealLabel(hostOs('Mozilla/5.0 (X11; Linux x86_64)')), 'Show in Files');
});

test('library posters and dates', async () => {
  const { posterUrl, whenLabel, folderName } = await import('../src/lib/appinfo.ts');
  assert.equal(posterUrl('/Users/x/Movies/Simple Capture/2026-11-21 10.47.00 Recording.sc', 'Macintosh'), 'sc://poster/2026-11-21%2010.47.00%20Recording.sc.png');
  assert.equal(posterUrl('C:\\Videos\\Simple Capture\\a.sc', 'Windows NT'), 'http://sc.localhost/poster/a.sc.png');
  assert.equal(folderName('/a/b/c.sc/'), 'c.sc');
  const now = new Date(2026, 10, 21, 12, 0);
  assert.match(whenLabel(new Date(2026, 10, 21, 9, 5).toISOString(), now), /^Today /);
  assert.match(whenLabel(new Date(2026, 10, 20, 9, 5).toISOString(), now), /^Yesterday /);
  assert.match(whenLabel(new Date(2026, 9, 3, 9, 5).toISOString(), now), /2026/);
});
