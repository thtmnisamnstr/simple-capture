<script lang="ts">
  import { untrack } from 'svelte';
  import { editor, formatTime, type Silences } from '../../lib/editor.ts';

  let {
    onpreview,
    onremove,
    onclose,
  }: {
    onpreview: (ranges: [number, number][]) => void;
    onremove: (ranges: [number, number][]) => Promise<void>;
    onclose: () => void;
  } = $props();

  const LENGTHS = [
    { ms: 500, label: '0.5 s' },
    { ms: 1000, label: '1 s' },
    { ms: 2000, label: '2 s' },
  ];

  let threshold = $state<number | null>(null);
  let minMs = $state(1000);
  let found = $state<Silences | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(true);
  let seq = 0;

  $effect(() => {
    const t = threshold;
    const m = minMs;
    const mine = ++seq;
    const timer = setTimeout(
      () => {
        editor
          .silences(t, m)
          .then((s) => {
            if (mine !== seq) return;
            found = s;
            error = null;
            if (threshold === null) threshold = Math.round(s.threshold_db);
            onpreview(s.ranges);
          })
          .catch((e) => {
            if (mine !== seq) return;
            error = String(e);
            onpreview([]);
          })
          .finally(() => mine === seq && (busy = false));
      },
      untrack(() => found) ? 120 : 0,
    );
    return () => clearTimeout(timer);
  });

  async function remove() {
    if (!found?.ranges.length) return;
    busy = true;
    try {
      await onremove(found.ranges);
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Escape') onclose();
    if (e.key === 'Enter' && !busy) void remove();
  }
</script>

<div class="panel" role="dialog" aria-label="Remove Silences" tabindex="-1" {onkeydown}>
  <header>
    <h2>Remove Silences</h2>
    <button class="plain close" aria-label="Close" onclick={onclose}>✕</button>
  </header>
  <div class="row">
    <label for="quiet">Quieter than</label>
    <input
      id="quiet"
      class="slider"
      type="range"
      min="-60"
      max="-20"
      step="1"
      value={threshold ?? -45}
      disabled={threshold === null}
      oninput={(e) => (threshold = Number(e.currentTarget.value))}
    />
    <span class="tnum value">{threshold === null ? '…' : `${threshold} dB`}</span>
    {#if found && threshold !== null && Math.round(found.suggested_db) !== threshold}
      <button class="link" onclick={() => (threshold = Math.round(found!.suggested_db))}>Auto</button>
    {/if}
  </div>
  <div class="row">
    <span class="lbl">For at least</span>
    <div class="segmented" role="radiogroup" aria-label="Shortest silence">
      {#each LENGTHS as l (l.ms)}
        <button role="radio" aria-checked={minMs === l.ms} onclick={() => (minMs = l.ms)}>{l.label}</button>
      {/each}
    </div>
  </div>
  <p class="caption summary tnum" role="status">
    {#if error}
      <span class="err">{error}</span>
    {:else if !found}
      Listening…
    {:else if !found.ranges.length}
      No silences that long at this level.
    {:else if found.removed_ns >= found.duration_ns}
      Everything is quieter than this. Move the level to the left.
    {:else}
      {found.ranges.length} {found.ranges.length === 1 ? 'silence' : 'silences'}, {formatTime(found.removed_ns)} of {formatTime(found.duration_ns)}. Marked on the timeline.
    {/if}
  </p>
  <div class="actions">
    <button onclick={onclose}>Cancel</button>
    <button class="primary" disabled={busy || !found?.ranges.length || found.removed_ns >= found.duration_ns} onclick={remove}>Remove</button>
  </div>
</div>

<style>
  .panel {
    position: absolute;
    right: 12px;
    bottom: calc(100% + 6px);
    width: 340px;
    padding: 10px 12px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--window);
    border-radius: 10px;
    box-shadow: var(--shadow-pop);
    z-index: 20;
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 13px;
  }
  .close {
    min-height: 18px;
    padding: 0 4px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .row label,
  .lbl {
    width: 84px;
    flex: none;
  }
  .slider {
    flex: 1;
    min-width: 0;
  }
  .value {
    width: 46px;
    text-align: right;
  }
  .summary {
    margin: 0;
    min-height: 2.6em;
  }
  .err {
    color: var(--red, #d93025);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
