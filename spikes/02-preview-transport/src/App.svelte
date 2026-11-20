<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { runBench, log, type ModeCfg, type LiveStats } from './lib/bench';

  let cfg = $state<ModeCfg | null>(null);
  let live = $state<LiveStats | null>(null);
  let status = $state('starting');
  let playing = $state(true);
  let scrub = $state(30);
  let canvas = $state<HTMLCanvasElement | null>(null);
  let box = $state<HTMLElement | null>(null);
  const dpr = window.devicePixelRatio;

  const isNative = $derived(cfg?.renderer === 'native');
  const usesCanvas = $derived(cfg?.renderer === 'gl' || cfg?.renderer === '2d');

  onMount(async () => {
    try {
      cfg = await invoke<ModeCfg>('get_config');
      if (cfg.transport === 'native') {
        document.documentElement.classList.add('native');
      }
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      status = 'running';
      await runBench(cfg, canvas, box!, (s) => (live = s));
      status = 'done';
    } catch (e) {
      status = `error: ${e}`;
      log(`fatal: ${e}`);
      await invoke('report', { js: { errors: [`fatal: ${e}`] } }).catch(() => {});
    }
  });

  const fmt = (n: number | undefined, d = 1) => (n === undefined || Number.isNaN(n) ? '–' : n.toFixed(d));
</script>

<main>
  <header>
    <strong>Simple Capture · preview transport spike</strong>
    <span class="mode">{cfg?.name ?? '…'}</span>
    <span class="status">{status}</span>
  </header>

  <section class="stage">
    <div
      class="preview"
      class:hole={isNative}
      bind:this={box}
      style:width="{(cfg?.width ?? 1920) / dpr}px"
      style:height="{(cfg?.height ?? 1080) / dpr}px"
    >
      {#if usesCanvas}
        <canvas bind:this={canvas}></canvas>
      {/if}
      <div class="overlay">
        <button onclick={() => (playing = !playing)}>{playing ? '❚❚' : '▶'}</button>
        <span class="tc">00:{String(Math.floor(scrub)).padStart(2, '0')}:12</span>
        <span class="chip">HTML over video</span>
      </div>
    </div>
  </section>

  <aside>
    <h2>Controls (HTML)</h2>
    <label>Scrub <input type="range" min="0" max="59" bind:value={scrub} /></label>
    <div class="spinner" aria-hidden="true"></div>
    <dl>
      <dt>phase</dt><dd>{live?.phase ?? '–'}</dd>
      <dt>rAF fps</dt><dd>{fmt(live?.rafFps)}</dd>
      <dt>new frames/s</dt><dd>{fmt(live?.presentedFps)}</dd>
      <dt>latency (last)</dt><dd>{fmt(live?.latencyP50)} ms</dd>
      <dt>id gaps</dt><dd>{live?.drops ?? '–'}</dd>
      <dt>dpr</dt><dd>{dpr}</dd>
      <dt>frame</dt><dd>{cfg ? `${cfg.width}×${cfg.height} @ ${cfg.fps}` : '–'}</dd>
    </dl>
  </aside>
</main>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    height: 100%;
    background: #18181c;
    color: #e8e8ee;
    font: 13px/1.4 -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    overflow: hidden;
  }
  :global(html.native),
  :global(html.native body) {
    background: transparent;
  }
  main {
    display: grid;
    grid-template-columns: 1fr 300px;
    grid-template-rows: 44px 1fr;
    height: 100vh;
  }
  header {
    grid-column: 1 / 3;
    display: flex;
    gap: 16px;
    align-items: center;
    padding: 0 16px;
    background: #222228;
    border-bottom: 1px solid #333;
  }
  .mode {
    font-family: ui-monospace, monospace;
    color: #9fd;
  }
  .status {
    margin-left: auto;
    color: #aaa;
  }
  .stage {
    display: flex;
    align-items: flex-start;
    justify-content: flex-start;
    padding: 24px;
    background: #18181c;
  }
  :global(html.native) .stage {
    background: transparent;
  }
  .preview {
    position: relative;
    background: #000;
    outline: 1px solid #444;
    flex: none;
  }
  .preview.hole {
    background: transparent;
  }
  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
  .overlay {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: 12px;
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 6px 10px;
    border-radius: 8px;
    background: rgba(10, 10, 14, 0.62);
    backdrop-filter: blur(6px);
  }
  .overlay button {
    width: 30px;
    height: 26px;
    border-radius: 6px;
    border: 0;
    background: #fff;
    color: #111;
  }
  .tc {
    font-family: ui-monospace, monospace;
  }
  .chip {
    margin-left: auto;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 99px;
    background: #3a6;
    color: #fff;
  }
  aside {
    padding: 16px;
    background: #202026;
    border-left: 1px solid #333;
  }
  h2 {
    font-size: 13px;
    margin: 0 0 12px;
  }
  label {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .spinner {
    width: 28px;
    height: 28px;
    margin: 16px 0;
    border: 3px solid #555;
    border-top-color: #9fd;
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 12px;
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }
  dt {
    color: #999;
  }
  dd {
    margin: 0;
  }
</style>
