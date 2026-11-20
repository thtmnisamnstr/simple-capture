import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  isExportProgress,
  isExportState,
  isHealth,
  isNotice,
  isPermissions,
  isPicked,
  isRecordingState,
  isRegionSelected,
  isSourceSpec,
} from '../src/lib/guards.ts';

test('permissions', () => {
  assert.ok(isPermissions({ screen: 'granted', camera: 'denied', mic: 'not_determined' }));
  assert.ok(!isPermissions({ screen: 'yes', camera: 'denied', mic: 'denied' }));
  assert.ok(!isPermissions(null));
});

test('source specs', () => {
  assert.ok(isSourceSpec({ kind: 'display', display_id: '1' }));
  assert.ok(isSourceSpec({ kind: 'window', window_id: 7 }));
  assert.ok(isSourceSpec({ kind: 'application', display_id: '1', pid: 3, bundle_id: null }));
  assert.ok(isSourceSpec({ kind: 'region', display_id: '1', region_pt: { x: 0, y: 0, w: 10, h: 10 } }));
  assert.ok(isSourceSpec({ kind: 'picked', pick_id: 'p', region_pt: null }));
  assert.ok(!isSourceSpec({ kind: 'region', display_id: '1', region_pt: { x: 0, y: 0, w: -1, h: 10 } }));
  assert.ok(!isSourceSpec({ kind: 'window', window_id: '7' }));
  assert.ok(!isSourceSpec({ kind: 'tab' }));
});

test('recording states', () => {
  assert.ok(isRecordingState({ state: 'idle' }));
  assert.ok(isRecordingState({ state: 'recording', elapsed_ns: 5 }));
  assert.ok(isRecordingState({ state: 'finalized', project_dir: '/x.sc' }));
  assert.ok(!isRecordingState({ state: 'recording' }));
  assert.ok(!isRecordingState({ state: 'failed' }));
  assert.ok(!isRecordingState({ state: 'paused' }));
});

test('notices', () => {
  assert.ok(isNotice({ kind: 'source_lost', segment: 's2' }));
  assert.ok(isNotice({ kind: 'frames_dropping', track: 'screen', pct: 3 }));
  assert.ok(isNotice({ kind: 'stopped_for_sleep' }));
  assert.ok(isNotice({ kind: 'switch_failed', error: 'x', restored: true }));
  assert.ok(!isNotice({ kind: 'switch_failed', error: 'x' }));
  assert.ok(!isNotice({ kind: 'device_lost', device: 'speaker', name: 'x' }));
  assert.ok(!isNotice('source_lost'));
});

test('picks and regions', () => {
  assert.ok(isPicked({ purpose: 'switch', source: { kind: 'window', window_id: 1 }, via: 'own_chooser' }));
  assert.ok(!isPicked({ purpose: 'later', source: { kind: 'window', window_id: 1 }, via: 'own_chooser' }));
  assert.ok(isRegionSelected({ purpose: 'start', display_id: '2', region_pt: { x: 1, y: 2, w: 3, h: 4 } }));
  assert.ok(!isRegionSelected({ purpose: 'start', display_id: 2, region_pt: { x: 1, y: 2, w: 3, h: 4 } }));
});

test('health and export', () => {
  assert.ok(isHealth({ rec_ns: 1, free_disk_bytes: 2, tracks: [{ track_id: 'screen', frames: 1, dropped: 0, fps: 30 }] }));
  assert.ok(!isHealth({ rec_ns: 1, tracks: [] }));
  assert.ok(isExportProgress({ id: 'e', frames_done: 1, frames_total: 2, fps: 3, eta_s: null }));
  assert.ok(!isExportProgress({ id: 'e', frames_done: 1, frames_total: 2, fps: 3 }));
  assert.ok(isExportState({ id: 'e', state: 'done', path: '/a.mp4' }));
  assert.ok(isExportState({ id: 'e', state: 'canceled' }));
  assert.ok(!isExportState({ id: 'e', state: 'done' }));
});
