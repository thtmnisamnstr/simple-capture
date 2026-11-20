<script lang="ts">
  import type { Background, CanvasDto } from '../lib/types.ts';
  import {
    GRADIENT_PRESETS,
    SOLID_PALETTE,
    hexToRgba,
    linear,
    radial,
    rgbaToHex,
    backgroundCss,
  } from '../lib/backgrounds.ts';
  import { api, errorText } from '../lib/api.ts';
  import { basename } from '../lib/format.ts';

  let { canvas = $bindable(), onclose }: { canvas: CanvasDto; onclose: () => void } = $props();

  type Mode = 'solid' | 'gradient' | 'image';
  let mode = $state<Mode>(canvas.background.kind === 'solid' ? 'solid' : canvas.background.kind === 'image' ? 'image' : 'gradient');
  let error = $state<string | null>(null);

  const bg = $derived(canvas.background);
  const solidHex = $derived(bg.kind === 'solid' ? rgbaToHex(bg.color) : '#121214');
  const stops = $derived(
    bg.kind === 'linear' || bg.kind === 'radial'
      ? [rgbaToHex(bg.stops[0]?.color ?? [0, 0, 0, 1]), rgbaToHex(bg.stops[bg.stops.length - 1]?.color ?? [1, 1, 1, 1])]
      : [GRADIENT_PRESETS[0].from, GRADIENT_PRESETS[0].to],
  );
  const angle = $derived(bg.kind === 'linear' ? bg.angle_deg : 135);

  function set(b: Background, image: string | null = null) {
    canvas = { ...canvas, background: b, background_image: image };
  }

  function setGradient(from: string, to: string, kind: 'linear' | 'radial' = bg.kind === 'radial' ? 'radial' : 'linear') {
    set(kind === 'linear' ? linear(from, to, angle) : radial(from, to));
  }

  function setMode(m: Mode) {
    mode = m;
    if (m === 'solid' && bg.kind !== 'solid') set({ kind: 'solid', color: hexToRgba(SOLID_PALETTE[0]) });
    if (m === 'gradient' && bg.kind !== 'linear' && bg.kind !== 'radial') setGradient(stops[0], stops[1], 'linear');
  }

  async function chooseImage() {
    error = null;
    try {
      const path = await api.chooseBackgroundImage();
      if (path) set({ kind: 'image', asset: path, sha256: '', fit: 'cover', focus_norm: { x: 0.5, y: 0.5 } }, path);
    } catch (e) {
      error = errorText(e);
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose();
    }
  }
</script>

<div class="popover" role="dialog" aria-label="Background" tabindex="-1" {onkeydown}>
  <div class="segmented" role="radiogroup" aria-label="Background kind">
    {#each [['solid', 'Solid'], ['gradient', 'Gradient'], ['image', 'Image']] as [m, label] (m)}
      <button role="radio" aria-checked={mode === m} onclick={() => setMode(m as Mode)}>{label}</button>
    {/each}
  </div>

  {#if mode === 'solid'}
    <div class="swatches" role="radiogroup" aria-label="Colour">
      {#each SOLID_PALETTE as hex (hex)}
        <button
          class="swatch"
          role="radio"
          aria-checked={bg.kind === 'solid' && solidHex === hex}
          aria-label={hex}
          style:background={hex}
          onclick={() => set({ kind: 'solid', color: hexToRgba(hex) })}
        ></button>
      {/each}
      <label class="custom">
        <input type="color" value={solidHex} oninput={(e) => set({ kind: 'solid', color: hexToRgba(e.currentTarget.value) })} />
        Custom
      </label>
    </div>
  {:else if mode === 'gradient'}
    <div class="swatches" role="radiogroup" aria-label="Gradient preset">
      {#each GRADIENT_PRESETS as p (p.name)}
        <button
          class="swatch"
          role="radio"
          aria-checked={stops[0] === p.from && stops[1] === p.to}
          aria-label={p.name}
          title={p.name}
          style:background={backgroundCss(linear(p.from, p.to, 135))}
          onclick={() => setGradient(p.from, p.to)}
        ></button>
      {/each}
    </div>
    <div class="row">
      <label>From <input type="color" value={stops[0]} oninput={(e) => setGradient(e.currentTarget.value, stops[1])} /></label>
      <label>To <input type="color" value={stops[1]} oninput={(e) => setGradient(stops[0], e.currentTarget.value)} /></label>
      <div class="segmented" role="radiogroup" aria-label="Gradient type">
        <button role="radio" aria-checked={bg.kind === 'linear'} onclick={() => setGradient(stops[0], stops[1], 'linear')}>Linear</button>
        <button role="radio" aria-checked={bg.kind === 'radial'} onclick={() => setGradient(stops[0], stops[1], 'radial')}>Radial</button>
      </div>
    </div>
    {#if bg.kind === 'linear'}
      <label class="row">
        Angle
        <input
          type="range"
          min="0"
          max="360"
          step="15"
          value={angle}
          oninput={(e) => set(linear(stops[0], stops[1], Number(e.currentTarget.value)))}
        />
        <span class="muted">{angle}°</span>
      </label>
    {/if}
  {:else}
    <div class="row">
      <button onclick={chooseImage}>Choose…</button>
      <span class="muted ellipsis">{canvas.background_image ? basename(canvas.background_image) : 'PNG or JPEG, fills the frame'}</span>
    </div>
  {/if}

  {#if error}<p class="warn" role="alert">{error}</p>{/if}
  <div class="row end"><button onclick={onclose}>Done</button></div>
</div>

<style>
  .popover {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.12);
  }
  .swatches { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .swatch {
    width: 24px;
    height: 24px;
    min-height: 24px;
    padding: 0;
    border-radius: 6px;
    border: 1px solid var(--border);
  }
  .swatch[aria-checked='true'] { box-shadow: 0 0 0 2px var(--accent); }
  .custom { display: inline-flex; align-items: center; gap: 4px; }
  input[type='color'] { width: 28px; height: 24px; padding: 0; border: 1px solid var(--border); border-radius: 5px; background: none; }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .row.end { justify-content: flex-end; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; flex: 1; }
</style>
