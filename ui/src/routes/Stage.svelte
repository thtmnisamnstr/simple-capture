<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { onEvent } from '../lib/events.ts';
  import { isObject } from '../lib/guards.ts';
  import type { CameraKey, RecorderSettings } from '../lib/types.ts';
  import {
    CAMERA_SIZE_MAX,
    CAMERA_SIZE_MIN,
    CAMERA_ZOOM_MAX,
    cameraRects,
    clampCenter,
    shapeSize,
    snapCenter,
  } from '../lib/geometry.ts';
  import { shapeAspect } from '../lib/camera.ts';
  import { LiveView } from '../lib/live.ts';

  let canvas: HTMLCanvasElement | undefined = $state();
  let frame = $state({ w: 0, h: 0 });
  let key = $state<CameraKey | null>(null);
  let error = $state<string | null>(null);
  let drag: { corner: [number, number] | null; x: number; y: number; start: CameraKey } | null = null;
  let queued = false;

  let view = $state({ w: window.innerWidth, h: window.innerHeight });
  const css = $derived.by(() => {
    if (frame.w === 0 || frame.h === 0) return { w: 0, h: 0 };
    const k = Math.min(view.w / frame.w, view.h / frame.h);
    return { w: frame.w * k, h: frame.h * k };
  });
  const rect = $derived(
    key && css.w > 0 ? cameraRects(css, { w: 1, h: 1 }, shapeAspect(key.shape), poseOf(key)).dst : null,
  );

  function poseOf(k: CameraKey) {
    return { center: k.center_norm, size: k.size_norm, zoom: k.zoom, pan: k.pan_norm, radius: k.shape === 'circle' ? 1 : k.corner_radius };
  }

  async function load() {
    try {
      key = (await api.getSettings()).camera_key;
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    if (!canvas) return;
    const stream = new LiveView(canvas, 'stage', (w, h) => {
      if (w !== frame.w || h !== frame.h) frame = { w, h };
    });
    load();
    const off = onEvent('settings-changed', isObject, (s) => {
      if (!drag) key = (s as unknown as RecorderSettings).camera_key;
    });
    const onResize = () => (view = { w: window.innerWidth, h: window.innerHeight });
    window.addEventListener('resize', onResize);
    return () => {
      stream.stop();
      off();
      window.removeEventListener('resize', onResize);
    };
  });

  function push() {
    queued = false;
    if (key) api.cameraKeyLive($state.snapshot(key) as CameraKey).catch((e) => (error = errorText(e)));
  }

  function live(next: CameraKey) {
    key = next;
    if (!queued) {
      queued = true;
      requestAnimationFrame(push);
    }
  }

  async function save() {
    if (!key) return;
    try {
      const s = await api.getSettings();
      await api.setSettings({ ...s, camera_key: $state.snapshot(key) as CameraKey });
    } catch (e) {
      error = errorText(e);
    }
  }

  function down(e: PointerEvent, corner: [number, number] | null) {
    if (e.button !== 0 || !key) return;
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    drag = { corner, x: e.clientX, y: e.clientY, start: $state.snapshot(key) as CameraKey };
  }

  function move(e: PointerEvent) {
    if (!drag || !key || css.w === 0) return;
    const s = drag.start;
    const aspect = shapeAspect(s.shape);
    const canvasPx = { w: frame.w, h: frame.h };
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    if (drag.corner) {
      const [sx, sy] = drag.corner;
      const { w: w0, h: h0 } = shapeSize(Math.min(css.w, css.h) * s.size_norm, aspect);
      const size = Math.min(Math.max(s.size_norm * Math.max(1 + (dx * sx) / w0 + (dy * sy) / h0, 0.05), CAMERA_SIZE_MIN), CAMERA_SIZE_MAX);
      live({ ...key, size_norm: size, center_norm: clampCenter(canvasPx, aspect, size, s.center_norm) });
      return;
    }
    const c = { x: s.center_norm.x + dx / css.w, y: s.center_norm.y + dy / css.h };
    live({ ...key, center_norm: snapCenter(canvasPx, aspect, s.size_norm, c, (12 * frame.w) / css.w) });
  }

  function up() {
    if (!drag) return;
    drag = null;
    push();
    save();
  }

  function onwheel(e: WheelEvent) {
    if (!key) return;
    e.preventDefault();
    live({ ...key, zoom: Math.min(Math.max(key.zoom * Math.exp(-e.deltaY / 400), 1), CAMERA_ZOOM_MAX) });
    clearTimeout(wheelTimer);
    wheelTimer = setTimeout(save, 300);
  }
  let wheelTimer: ReturnType<typeof setTimeout> | undefined;
</script>

<svelte:window onpointermove={move} onpointerup={up} />

<div class="stage window">
  <div class="frame" style:width="{css.w}px" style:height="{css.h}px">
    <canvas bind:this={canvas} style:width="{css.w}px" style:height="{css.h}px"></canvas>
    {#if rect && key?.visible}
      <div
        class="cam"
        class:round={key.shape === 'circle'}
        style:left="{rect.x}px"
        style:top="{rect.y}px"
        style:width="{rect.w}px"
        style:height="{rect.h}px"
        role="presentation"
        onpointerdown={(e) => down(e, null)}
        {onwheel}
      >
        {#each [[-1, -1], [1, -1], [-1, 1], [1, 1]] as c (c.join())}
          <span
            class="handle"
            style:left={c[0] < 0 ? '0' : '100%'}
            style:top={c[1] < 0 ? '0' : '100%'}
            role="presentation"
            onpointerdown={(e) => down(e, [c[0], c[1]])}
          ></span>
        {/each}
      </div>
    {/if}
  </div>
  {#if frame.w === 0}<p class="muted hint">Starting the preview…</p>{/if}
  {#if error}<p class="warn err" role="alert">{error}</p>{/if}
</div>

<style>
  .stage {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #111;
    overflow: hidden;
  }
  .frame {
    position: relative;
  }
  canvas {
    display: block;
  }
  .cam {
    position: absolute;
    cursor: grab;
    outline: 1px dashed rgba(255, 255, 255, 0.6);
  }
  .cam.round {
    border-radius: 50%;
  }
  .handle {
    position: absolute;
    width: 10px;
    height: 10px;
    margin: -5px 0 0 -5px;
    border-radius: 50%;
    background: #fff;
    border: 1px solid rgba(0, 0, 0, 0.4);
    cursor: nwse-resize;
  }
  .hint {
    position: absolute;
  }
  .err {
    position: absolute;
    left: 8px;
    bottom: 8px;
  }
</style>
