<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { emitEvent, onEvent } from '../lib/events.ts';
  import { isRecordingState } from '../lib/guards.ts';
  import type { DisplayInfo, Purpose, RectPt, RecorderSettings } from '../lib/types.ts';
  import {
    aspectLockedResize,
    aspectRatio,
    fitRatioInside,
    HANDLES,
    ptToPx,
    snapRegionEvenPx,
    type Handle,
    type Ratio,
  } from '../lib/geometry.ts';
  import { hashParams } from '../lib/router.ts';
  import { loadLastRegion, overlayDisplay, parsePurpose, saveLastRegion } from '../lib/overlay.ts';
  import { hideSelf, isOverlayContext, OVERLAY_CONTEXT_EVENT } from '../lib/windows.ts';

  type Preset = 'free' | '16:9' | '9:16' | '4:3' | '1:1' | 'match';
  const PRESETS: { value: Preset; label: string }[] = [
    { value: 'free', label: 'Free' },
    { value: '16:9', label: '16:9' },
    { value: '9:16', label: '9:16' },
    { value: '4:3', label: '4:3' },
    { value: '1:1', label: '1:1' },
    { value: 'match', label: 'Match frame' },
  ];

  let displays = $state<DisplayInfo[]>([]);
  let display = $state<DisplayInfo | null>(null);
  let purpose = $state<Purpose>('start');
  let recording = $state(false);
  let settings = $state<RecorderSettings | null>(null);
  let rect = $state<RectPt | null>(null);
  let preset = $state<Preset>('free');
  let error = $state<string | null>(null);
  let vw = $state(window.innerWidth);
  let vh = $state(window.innerHeight);
  let drag: { handle: Handle | 'new'; start: { x: number; y: number }; orig: RectPt | null } | null = null;

  const bounds = $derived<RectPt>({
    x: 0,
    y: 0,
    w: Math.min(display?.frame_pt.w ?? vw, vw),
    h: Math.min(display?.frame_pt.h ?? vh, vh),
  });
  const matchRatio = $derived(settings ? aspectRatio(settings.canvas.aspect) : null);
  const lock = $derived<Ratio | null>(preset === 'free' ? null : preset === 'match' ? matchRatio : aspectRatio(preset));
  const px = $derived(rect && display ? ptToPx(snapRegionEvenPx(rect, display.scale), display.scale) : null);
  const toolbarTop = $derived(rect ? (rect.y + rect.h + 56 < bounds.h ? rect.y + rect.h + 10 : Math.max(10, rect.y - 46)) : 0);
  const toolbarLeft = $derived(rect ? Math.min(Math.max(10, rect.x), Math.max(10, bounds.w - 700)) : 0);

  async function setup(explicitDisplay: string | null, explicitPurpose: string | null) {
    error = null;
    try {
      const [d, s] = await Promise.all([api.listDisplays(), api.getSettings()]);
      displays = d;
      settings = s;
    } catch (e) {
      error = errorText(e);
    }
    display = overlayDisplay(displays, explicitDisplay, { x: window.screenX, y: window.screenY });
    purpose = parsePurpose(explicitPurpose, recording);
    const last = display ? loadLastRegion(display.id) : null;
    rect = last ? clampRect(last) : null;
    drag = null;
  }

  function clampRect(r: RectPt): RectPt | null {
    const w = Math.min(r.w, bounds.w);
    const h = Math.min(r.h, bounds.h);
    if (w < 4 || h < 4) return null;
    return { x: Math.min(Math.max(r.x, 0), bounds.w - w), y: Math.min(Math.max(r.y, 0), bounds.h - h), w, h };
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
    const onResize = () => ((vw = window.innerWidth), (vh = window.innerHeight));
    window.addEventListener('resize', onResize);
    return () => {
      offs.forEach((off) => off());
      window.removeEventListener('resize', onResize);
    };
  });

  function local(e: PointerEvent) {
    return { x: e.clientX, y: e.clientY };
  }

  function down(e: PointerEvent, handle: Handle | 'new') {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    drag = { handle, start: local(e), orig: rect ? { ...rect } : null };
  }

  function move(e: PointerEvent) {
    if (!drag) return;
    const p = local(e);
    if (drag.handle === 'new') {
      if (Math.abs(p.x - drag.start.x) < 3 && Math.abs(p.y - drag.start.y) < 3) return;
      const seed = { x: drag.start.x, y: drag.start.y, w: 0, h: 0 };
      rect = aspectLockedResize(seed, lock, 'se', p, bounds);
    } else if (drag.handle === 'move' && drag.orig) {
      rect = aspectLockedResize(drag.orig, null, 'move', { x: drag.orig.x + p.x - drag.start.x, y: drag.orig.y + p.y - drag.start.y }, bounds);
    } else if (drag.orig) {
      rect = aspectLockedResize(drag.orig, lock, drag.handle as Handle, p, bounds);
    }
  }

  function up() {
    drag = null;
  }

  function choosePreset(p: Preset) {
    preset = p;
    const r = p === 'free' ? null : p === 'match' ? matchRatio : aspectRatio(p);
    if (rect && r) rect = fitRatioInside(rect, r);
  }

  function setSize(axis: 'w' | 'h', value: number) {
    if (!rect || !Number.isFinite(value) || value < 4) return;
    let w = axis === 'w' ? value : rect.w;
    let h = axis === 'h' ? value : rect.h;
    if (lock) {
      if (axis === 'w') h = (w * lock.den) / lock.num;
      else w = (h * lock.num) / lock.den;
    }
    const scale = Math.min(1, bounds.w / w, bounds.h / h);
    rect = clampRect({ x: rect.x, y: rect.y, w: w * scale, h: h * scale }) ?? rect;
  }

  async function confirm() {
    if (!rect || !display) return;
    const region = snapRegionEvenPx(rect, display.scale);
    saveLastRegion(display.id, region);
    try {
      await emitEvent('region-selected', { purpose, display_id: display.id, region_pt: region });
      if (purpose === 'switch') await api.switchSource({ kind: 'region', display_id: display.id, region_pt: region });
      rect = null;
      await hideSelf();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function cancel() {
    rect = null;
    drag = null;
    await hideSelf();
  }

  function onkeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    const typing = target?.tagName === 'INPUT';
    if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
    } else if (e.key === 'Enter' && rect) {
      e.preventDefault();
      confirm();
    } else if (!typing && rect && e.key.startsWith('Arrow')) {
      e.preventDefault();
      const step = e.shiftKey ? 10 : 1;
      const dx = e.key === 'ArrowLeft' ? -step : e.key === 'ArrowRight' ? step : 0;
      const dy = e.key === 'ArrowUp' ? -step : e.key === 'ArrowDown' ? step : 0;
      rect = aspectLockedResize(rect, null, 'move', { x: rect.x + dx, y: rect.y + dy }, bounds);
    }
  }
</script>

<svelte:window {onkeydown} onpointermove={move} onpointerup={up} />

<div
  class="overlay"
  class:empty={!rect}
  role="application"
  aria-label="Select a region of {display?.name ?? 'the display'}"
  onpointerdown={(e) => down(e, 'new')}
>
  {#if rect}
    <div
      class="sel"
      style:left="{rect.x}px"
      style:top="{rect.y}px"
      style:width="{rect.w}px"
      style:height="{rect.h}px"
      role="presentation"
      onpointerdown={(e) => down(e, 'move')}
    >
      {#each HANDLES as h (h)}
        <span class="handle {h}" role="presentation" onpointerdown={(e) => down(e, h)}></span>
      {/each}
    </div>

    <div class="toolbar" style:left="{toolbarLeft}px" style:top="{toolbarTop}px" role="toolbar" aria-label="Region" tabindex="-1" onpointerdown={(e) => e.stopPropagation()}>
      <label>W <input type="number" min="4" value={Math.round(rect.w)} onchange={(e) => setSize('w', Number(e.currentTarget.value))} /></label>
      <span class="muted">×</span>
      <label>H <input type="number" min="4" value={Math.round(rect.h)} onchange={(e) => setSize('h', Number(e.currentTarget.value))} /></label>
      <span class="muted small" title="Pixels recorded">pt{px ? ` · ${px.w}×${px.h} px` : ''}</span>
      <div class="segmented" role="radiogroup" aria-label="Region shape">
        {#each PRESETS as p (p.value)}
          <button
            role="radio"
            aria-checked={preset === p.value}
            disabled={p.value === 'match' && !matchRatio}
            title={p.value === 'match' && !matchRatio ? 'The frame is Original: pick a frame shape first' : ''}
            onclick={() => choosePreset(p.value)}>{p.label}</button
          >
        {/each}
      </div>
      <button onclick={cancel}>Cancel</button>
      <button class="primary" onclick={confirm}>{purpose === 'switch' ? 'Switch to region' : 'Use region'}</button>
    </div>
  {:else}
    <div class="hint" role="status">
      Drag to select a region{display ? ` of ${display.name}` : ''} · <kbd>Esc</kbd> to cancel
    </div>
  {/if}
  {#if error}<p class="warn err" role="alert">{error}</p>{/if}
</div>

<style>
  :global(body:has(.overlay)) {
    overflow: hidden;
  }
  .overlay {
    position: fixed;
    inset: 0;
    cursor: crosshair;
    overflow: hidden;
  }
  .overlay.empty {
    background: var(--overlay-dim);
  }
  .sel {
    position: absolute;
    box-shadow: 0 0 0 100vmax var(--overlay-dim);
    outline: 1px solid rgba(255, 255, 255, 0.95);
    cursor: move;
  }
  .handle {
    position: absolute;
    width: 10px;
    height: 10px;
    background: #fff;
    border: 1px solid rgba(0, 0, 0, 0.4);
    border-radius: 2px;
    transform: translate(-50%, -50%);
  }
  .nw { left: 0; top: 0; cursor: nwse-resize; }
  .n { left: 50%; top: 0; cursor: ns-resize; }
  .ne { left: 100%; top: 0; cursor: nesw-resize; }
  .e { left: 100%; top: 50%; cursor: ew-resize; }
  .se { left: 100%; top: 100%; cursor: nwse-resize; }
  .s { left: 50%; top: 100%; cursor: ns-resize; }
  .sw { left: 0; top: 100%; cursor: nesw-resize; }
  .w { left: 0; top: 50%; cursor: ew-resize; }
  .toolbar {
    position: absolute;
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: nowrap;
    white-space: nowrap;
    max-width: calc(100vw - 20px);
    padding: 6px 8px;
    border-radius: 10px;
    background: var(--window);
    color: var(--label);
    box-shadow: var(--shadow-pop);
    cursor: default;
  }
  .toolbar label {
    display: inline-flex;
    gap: 4px;
    align-items: center;
  }
  .toolbar input {
    width: 64px;
  }
  .hint {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    padding: 10px 16px;
    border-radius: 12px;
    background: rgba(30, 30, 32, 0.82);
    -webkit-backdrop-filter: blur(20px) saturate(1.6);
    backdrop-filter: blur(20px) saturate(1.6);
    box-shadow: inset 0 0 0 0.5px rgba(255, 255, 255, 0.15), 0 8px 30px rgba(0, 0, 0, 0.35);
    color: #fff;
    font-size: 13px;
    white-space: nowrap;
    pointer-events: none;
  }
  kbd {
    font: inherit;
    font-size: 12px;
    padding: 1px 6px;
    border-radius: 5px;
    background: rgba(255, 255, 255, 0.16);
    box-shadow: inset 0 -1px 0 rgba(0, 0, 0, 0.3);
  }
  .err {
    position: absolute;
    left: 10px;
    bottom: 10px;
  }
</style>
