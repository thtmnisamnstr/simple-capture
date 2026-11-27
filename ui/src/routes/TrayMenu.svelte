<script lang="ts">
  import { appApi } from '../lib/appinfo.ts';
  import { hideSelf } from '../lib/windows.ts';

  let paused = $state(false);

  const items = $derived([
    { id: 'start-stop', label: 'Stop Recording' },
    { id: 'pause', label: paused ? 'Resume Recording' : 'Pause Recording' },
    { id: 'switch', label: 'Switch Source…' },
    null,
    { id: 'open', label: 'Open Recorder' },
    { id: 'quit', label: 'Quit' },
  ]);

  async function refresh() {
    paused = (await appApi.recStatus().catch(() => null))?.paused ?? false;
  }

  async function choose(id: string) {
    await hideSelf();
    await appApi.trayItem(id).catch((e) => console.warn('tray item', e));
  }

  $effect(() => {
    void refresh();
    const onFocus = () => void refresh();
    const onBlur = () => void hideSelf();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') void hideSelf();
    };
    window.addEventListener('focus', onFocus);
    window.addEventListener('blur', onBlur);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('keydown', onKey);
    };
  });
</script>

<div class="menu" role="menu" aria-label="simple-capture">
  {#each items as it, i (it?.id ?? `sep${i}`)}
    {#if it}
      <button class="item" role="menuitem" onclick={() => choose(it.id)}>{it.label}</button>
    {:else}
      <div class="sep" role="separator"></div>
    {/if}
  {/each}
</div>

<style>
  .menu {
    display: flex;
    flex-direction: column;
    padding: 5px;
    height: 100vh;
    box-sizing: border-box;
  }
  .item {
    all: unset;
    padding: 4px 10px;
    border-radius: 5px;
    font-size: 13px;
    line-height: 18px;
    color: var(--label);
    cursor: default;
  }
  .item:hover,
  .item:focus-visible {
    background: var(--accent);
    color: #fff;
  }
  .sep {
    height: 1px;
    margin: 5px 10px;
    background: var(--separator);
  }
</style>
