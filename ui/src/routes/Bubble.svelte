<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import type { BubbleGeom, CameraKey, CameraShape, RecorderSettings } from '../lib/types.ts';
  import { CAMERA_SIZE_PRESETS, CAMERA_ZOOM_MAX } from '../lib/geometry.ts';
  import { LiveView } from '../lib/live.ts';
  import { CAMERA_SHAPES } from '../lib/camera.ts';


  let canvas: HTMLCanvasElement | undefined = $state();
  let geom = $state<BubbleGeom | null>(null);
  let key = $state<CameraKey | null>(null);
  let hasFrame = $state(false);
  let hover = $state(false);
  let error = $state<string | null>(null);
  let drag: { pan: boolean; x: number; y: number; pan0: { x: number; y: number } } | null = null;
  let pending: { x: number; y: number } | null = null;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  async function refresh() {
    try {
      const st = await api.bubbleState();
      geom = st.geom;
      key = st.key;
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    if (!canvas) return;
    const view = new LiveView(canvas, 'bubble', () => (hasFrame = true));
    refresh();
    const timer = setInterval(refresh, 1000);
    return () => {
      view.stop();
      clearInterval(timer);
    };
  });

  function flush() {
    if (!pending) return;
    const p = pending;
    pending = null;
    api
      .bubbleDrag('move', p.x, p.y)
      .then((g) => {
        if (g) geom = g;
      })
      .catch((e) => (error = errorText(e)));
  }

  function down(e: PointerEvent, corner: [number, number] | null) {
    if (e.button !== 0 || !key) return;
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    const pan = e.altKey && corner === null;
    drag = { pan, x: e.screenX, y: e.screenY, pan0: { ...key.pan_norm } };
    if (!pan) api.bubbleDrag('begin', e.screenX, e.screenY, corner).catch((err) => (error = errorText(err)));
  }

  function move(e: PointerEvent) {
    if (!drag || !key || !geom) return;
    if (drag.pan) {
      const sign = key.mirror ? -1 : 1;
      const dx = ((e.screenX - drag.x) / geom.shape.w) * 2 * sign;
      const dy = ((e.screenY - drag.y) / geom.shape.h) * 2;
      edit({ pan_norm: { x: clamp(drag.pan0.x - dx, -1, 1), y: clamp(drag.pan0.y - dy, -1, 1) } });
      return;
    }
    const first = pending === null;
    pending = { x: e.screenX, y: e.screenY };
    if (first) requestAnimationFrame(flush);
  }

  function up() {
    if (!drag) return;
    const wasPan = drag.pan;
    drag = null;
    if (!wasPan) {
      flush();
      api
        .bubbleDrag('end', 0, 0)
        .then(refresh)
        .catch((e) => (error = errorText(e)));
    }
  }

  const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi);

  function edit(patch: Partial<CameraKey>, extra: Partial<RecorderSettings> = {}) {
    if (!key) return;
    key = { ...key, ...patch };
    const next = key;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      try {
        const s = await api.getSettings();
        await api.setSettings({ ...s, ...extra, camera_key: next });
        await refresh();
      } catch (e) {
        error = errorText(e);
      }
    }, 120);
  }

  function onwheel(e: WheelEvent) {
    if (!key) return;
    e.preventDefault();
    const zoom = clamp(key.zoom * Math.exp(-e.deltaY / 400), 1, CAMERA_ZOOM_MAX);
    edit({ zoom });
  }

  function onkeydown(e: KeyboardEvent) {
    if (!geom || !key || !e.key.startsWith('Arrow')) return;
    e.preventDefault();
    const step = e.shiftKey ? 10 : 1;
    const dx = e.key === 'ArrowLeft' ? -step : e.key === 'ArrowRight' ? step : 0;
    const dy = e.key === 'ArrowUp' ? -step : e.key === 'ArrowDown' ? step : 0;
    const at = { x: geom.window.x, y: geom.window.y };
    api
      .bubbleDrag('begin', at.x, at.y)
      .then(() => api.bubbleDrag('move', at.x + dx, at.y + dy))
      .then(() => api.bubbleDrag('end', 0, 0))
      .then(refresh)
      .catch((err) => (error = errorText(err)));
  }

  const shapeStyle = $derived(
    geom ? `left:${geom.shape.x}px;top:${geom.shape.y}px;width:${geom.shape.w}px;height:${geom.shape.h}px` : '',
  );
  const sizeIndex = $derived(key ? CAMERA_SIZE_PRESETS.findIndex((s) => Math.abs(s - key!.size_norm) < 0.005) : -1);
</script>

<svelte:window onpointermove={move} onpointerup={up} {onkeydown} />

<div class="bubble" role="application" aria-label="Camera bubble" onpointerenter={() => (hover = true)} onpointerleave={() => (hover = false)}>
  <canvas bind:this={canvas}></canvas>
  {#if geom}
    <div
      class="shape"
      class:round={key?.shape === 'circle'}
      style={shapeStyle}
      role="presentation"
      onpointerdown={(e) => down(e, null)}
      {onwheel}
    >
      {#if !hasFrame}<span class="hint">Starting camera…</span>{/if}
      {#if hover && key}
        <div class="tools" role="toolbar" aria-label="Camera" tabindex="-1" onpointerdown={(e) => e.stopPropagation()}>
          <select aria-label="Shape" value={key.shape} onchange={(e) => edit({ shape: e.currentTarget.value as CameraShape })}>
            {#each CAMERA_SHAPES as s (s.value)}<option value={s.value}>{s.label}</option>{/each}
          </select>
          {#each ['S', 'M', 'L'] as label, i (label)}
            <button aria-pressed={sizeIndex === i} onclick={() => edit({ size_norm: CAMERA_SIZE_PRESETS[i] })}>{label}</button>
          {/each}
          <button aria-pressed={key.mirror} title="Mirror" onclick={() => edit({ mirror: !key!.mirror })}>⇋</button>
          {#if key.zoom > 1 || key.pan_norm.x !== 0 || key.pan_norm.y !== 0}
            <button title="Reset framing" onclick={() => edit({ zoom: 1, pan_norm: { x: 0, y: 0 } })}>⟲</button>
          {/if}
          <button title="Hide the bubble" onclick={() => edit({}, { bubble_shown: false })}>✕</button>
        </div>
      {/if}
      {#each [[-1, -1], [1, -1], [-1, 1], [1, 1]] as c (c.join())}
        <span
          class="handle"
          class:show={hover}
          style:left={c[0] < 0 ? '0' : '100%'}
          style:top={c[1] < 0 ? '0' : '100%'}
          role="presentation"
          onpointerdown={(e) => down(e, [c[0], c[1]])}
        ></span>
      {/each}
    </div>
  {/if}
  {#if error}<p class="err" role="alert">{error}</p>{/if}
</div>

<style>
  .bubble {
    position: fixed;
    inset: 0;
    overflow: hidden;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .shape {
    position: absolute;
    cursor: grab;
  }
  .shape.round {
    border-radius: 50%;
  }
  .hint {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #fff;
    font-size: 12px;
    background: rgba(20, 20, 22, 0.7);
    border-radius: inherit;
  }
  .tools {
    position: absolute;
    left: 50%;
    bottom: 8px;
    transform: translateX(-50%);
    display: flex;
    gap: 3px;
    padding: 3px;
    border-radius: 8px;
    background: rgba(20, 20, 22, 0.82);
    cursor: default;
  }
  .tools button,
  .tools select {
    min-height: 22px;
    padding: 0 6px;
    font-size: 11px;
    color: #fff;
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.25);
    border-radius: 5px;
  }
  .tools button[aria-pressed='true'] {
    background: var(--accent);
    border-color: var(--accent);
  }
  .handle {
    position: absolute;
    width: 12px;
    height: 12px;
    margin: -6px 0 0 -6px;
    border-radius: 50%;
    background: #fff;
    border: 1px solid rgba(0, 0, 0, 0.35);
    opacity: 0;
    cursor: nwse-resize;
  }
  .handle.show {
    opacity: 1;
  }
  .err {
    position: absolute;
    left: 4px;
    right: 4px;
    bottom: 0;
    margin: 0;
    font-size: 10px;
    color: #fff;
    background: rgba(160, 30, 30, 0.85);
  }
</style>
