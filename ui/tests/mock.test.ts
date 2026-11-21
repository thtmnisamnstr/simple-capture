import { test } from 'node:test';
import assert from 'node:assert/strict';
import { MOCK_COMMANDS, mockInvoke } from '../src/lib/mock.ts';

const COMMANDS = [
  'get_permissions', 'request_permission', 'open_permission_settings', 'list_displays', 'list_sources',
  'list_devices', 'show_mic_modes', 'get_settings', 'set_settings', 'get_capabilities', 'preview_still',
  'choose_background_image', 'begin_region_select', 'begin_pick_mode', 'open_system_picker',
  'start_recording', 'stop_recording', 'discard_recording', 'switch_source', 'quick_export',
  'cancel_export', 'reveal', 'recovery_report', 'camera_preview', 'preview_state', 'camera_key_live', 'show_video_effects', 'bar_drag',
];

test('the mock covers every app command', () => {
  assert.deepEqual([...MOCK_COMMANDS].sort(), [...COMMANDS].sort());
});

test('mock permissions never prompt and report statuses', async () => {
  const p = await mockInvoke<{ screen: string }>('get_permissions');
  assert.equal(p.screen, 'granted');
});
