<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { emitEvent, onEvent } from '../lib/events.ts';
  import { isRecordingState } from '../lib/guards.ts';
  import type { DisplayInfo, Purpose, RectPt, SourceSpecDto, WindowInfo } from '../lib/types.ts';
  import { aspectLockedResize, snapRegionEvenPx } from '../lib/geometry.ts';
  import { globalToLocal, topmostWindowAt, windowTitle } from '../lib/sources.ts';
  import { hashParams } from '../lib/router.ts';
  import { overlayDisplay, parsePurpose } from '../lib/overlay.ts';
  import { hideSelf, isOverlayContext, OVERLAY_CONTEXT_EVENT, systemPicker } from '../lib/windows.ts';

  let display = $state<DisplayInfo | null>(null);
  let windows = $state<WindowInfo[]>([]);
  let canList = $state(true);
  let purpose = $state<Purpose>('start');
  let recording = $state(false);
  let hover = $state<WindowInfo | null>(null);
  let region = $state<RectPt | null>(null);
  let error = $state<string | null>(null);
  let pressAt: { x: number; y: number } | null = null;

  const hoverRect = $derived(hover && display ? globalToLocal(hover.frame_pt, display) : null);
  const bounds = $derived<RectPt>({ x: 0, y: 0, w: display?.frame_pt.w ?? window.innerWidth, h: display?.frame_pt.h ?? window.innerHeight });

  async function setup(explicitDisplay: string | null, explicitPurpose: string | null) {
    error = null;
    hover = null;
    region = null;
    pressAt = null;
    try {
      const [p, d] = await Promise.all([api.getPermissions(), api.listDisplays()]);
      display = overlayDisplay(d, explicitDisplay, { x: window.screenX, y: window.screenY });
      canList = p.screen === 'granted';
      windows = canList ? (await api.listSources()).windows : [];
    } catch (e) {
      error = errorText(e);
    }
    purpose = parsePurpose(explicitPurpose, recording);
  }

  $effect(() => {
    const q = hashParams(window.location.hash);
    setup(q.get('display'), q.get('purpose'));
    const offs = [
      onEvent(OVERLAY_CONTEXT_EVENT, isOverlayContext, (ctx) => setup(ctx.display_id, ctx.purpose)),
      onEvent('recording-state', isRecordingState, (s) => {
        recording = s.state === 'recording' || s.state === 'switching';
      }),
    ];
    return () => offs.forEach((off) => off());
  });

  function toGlobal(x: number, y: number) {
    return { x: x + (display?.frame_pt.x ?? 0), y: y + (display?.frame_pt.y ?? 0) };
  }

  function onpointermove(e: PointerEvent) {
    if (pressAt) {
      const moved = Math.abs(e.clientX - pressAt.x) > 4 || Math.abs(e.clientY - pressAt.y) > 4;
      if (moved || region) {
        region = aspectLockedResize({ ...pressAt, w: 0, h: 0 }, null, 'se', { x: e.clientX, y: e.clientY }, bounds);
        hover = null;
        return;
      }
    }
    hover = topmostWindowAt(windows, toGlobal(e.clientX, e.clientY));
  }

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    pressAt = { x: e.clientX, y: e.clientY };
  }

  function onpointerup(e: PointerEvent) {
    if (!pressAt) return;
    pressAt = null;
    if (region && display) {
      const r = snapRegionEvenPx(region, display.scale);
      region = null;
      finish({ kind: 'region', display_id: display.id, region_pt: r });
      return;
    }
    const w = topmostWindowAt(windows, toGlobal(e.clientX, e.clientY));
    if (w) finish({ kind: 'window', window_id: w.id });
  }

  async function finish(source: SourceSpecDto) {
    try {
      await emitEvent('source-picked', { purpose, source, via: 'own_chooser' });
      if (purpose === 'switch') await api.switchSource(source);
      await hideSelf();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function cancel() {
    region = null;
    pressAt = null;
    try {
      await emitEvent('pick-cancelled', { purpose });
    } finally {
      await hideSelf();
    }
  }

  async function useSystemPicker() {
    try {
      await hideSelf();
      await systemPicker(purpose);
    } catch (e) {
      error = errorText(e);
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key.toLowerCase();
    if (k === 'escape') cancel();
    else if (k === 'd' && display) finish({ kind: 'display', display_id: display.id });
    else if (k === 'p') useSystemPicker();
    else return;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} {onpointermove} {onpointerup} />

<div class="overlay" role="application" aria-label="Pick a source on {display?.name ?? 'this display'}" {onpointerdown}>
  {#if hoverRect && hover}
    <div class="hl" style:left="{hoverRect.x}px" style:top="{hoverRect.y}px" style:width="{hoverRect.w}px" style:height="{hoverRect.h}px">
      <span class="tag">{windowTitle(hover)}</span>
    </div>
  {/if}
  {#if region}
    <div class="region" style:left="{region.x}px" style:top="{region.y}px" style:width="{region.w}px" style:height="{region.h}px">
      <span class="tag">{Math.round(region.w)}×{Math.round(region.h)}</span>
    </div>
  {/if}

  <div class="card" role="status">
    <strong>{purpose === 'switch' ? 'Switch to…' : 'Record…'}</strong>
    <span>{canList ? 'Click a window' : 'Grant Screen Recording to pick windows'}</span>
    <span><kbd>D</kbd> whole display</span>
    <span>Drag for a region</span>
    <span><kbd>P</kbd> system picker</span>
    <span><kbd>Esc</kbd> cancel</span>
  </div>
  {#if error}<p class="warn err" role="alert">{error}</p>{/if}
</div>

<style>
  :global(body:has(.overlay)) {
    overflow: hidden;
  }
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.12);
    cursor: crosshair;
    overflow: hidden;
  }
  .hl,
  .region {
    position: absolute;
    border: 3px solid #0a84ff;
    background: rgba(10, 132, 255, 0.16);
    border-radius: 6px;
    pointer-events: none;
  }
  .region {
    border-style: dashed;
    border-radius: 0;
    border-width: 2px;
  }
  .tag {
    position: absolute;
    left: 6px;
    top: 6px;
    max-width: calc(100% - 12px);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 2px 8px;
    border-radius: 5px;
    background: #0a84ff;
    color: #fff;
    font-size: 12px;
  }
  .card {
    position: absolute;
    left: 50%;
    bottom: 48px;
    transform: translateX(-50%);
    display: flex;
    gap: 14px;
    align-items: center;
    padding: 10px 16px;
    border-radius: 12px;
    background: rgba(30, 30, 32, 0.88);
    color: #fff;
    font-size: 13px;
    white-space: nowrap;
    pointer-events: none;
  }
  kbd {
    font: inherit;
    border: 1px solid rgba(255, 255, 255, 0.5);
    border-radius: 4px;
    padding: 0 5px;
  }
  .err {
    position: absolute;
    left: 10px;
    top: 10px;
  }
</style>
