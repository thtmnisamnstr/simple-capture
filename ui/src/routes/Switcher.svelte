<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import type { CapabilitiesDto, DisplayInfo, PermissionsDto, SourceList, SourceSpecDto } from '../lib/types.ts';
  import { liveRecent, loadRecent, rememberSource, type RecentEntry } from '../lib/recent.ts';
  import { sourceKey, sourceLabel, windowTitle } from '../lib/sources.ts';
  import { beginPick, beginRegion, hideSelf, systemPicker } from '../lib/windows.ts';

  let perms = $state<PermissionsDto | null>(null);
  let caps = $state<CapabilitiesDto | null>(null);
  let displays = $state<DisplayInfo[]>([]);
  let list = $state<SourceList | null>(null);
  let recent = $state<RecentEntry[]>([]);
  let query = $state('');
  let error = $state<string | null>(null);
  let searchEl = $state<HTMLInputElement | undefined>();

  const all = $derived({ displays, windows: list?.windows ?? [], apps: list?.apps ?? [] });
  const shownRecent = $derived(
    liveRecent(recent, list ? new Set(list.windows.map((w) => w.id)) : null, new Set(displays.map((d) => d.id))),
  );
  const windows = $derived(
    all.windows.filter((w) => w.on_screen && windowTitle(w).toLowerCase().includes(query.trim().toLowerCase())),
  );

  async function load() {
    error = null;
    try {
      const [p, c, d] = await Promise.all([api.getPermissions(), api.getCapabilities(), api.listDisplays()]);
      perms = p;
      caps = c;
      displays = d;
      list = p.screen === 'granted' ? await api.listSources() : null;
    } catch (e) {
      error = errorText(e);
    }
    recent = loadRecent();
  }

  $effect(() => {
    load();
    const onFocus = () => {
      load();
      searchEl?.focus();
    };
    const onVisible = () => document.visibilityState === 'visible' && load();
    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onVisible);
    };
  });

  async function pick(spec: SourceSpecDto) {
    error = null;
    try {
      await api.switchSource(spec);
      recent = rememberSource(spec, sourceLabel(spec, all));
      query = '';
      await hideSelf();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function run(fn: () => Promise<unknown>) {
    error = null;
    try {
      await fn();
      await hideSelf();
    } catch (e) {
      error = errorText(e);
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') hideSelf();
  }
</script>

<svelte:window {onkeydown} />

<div class="window switcher" role="dialog" aria-label="Switch source">
  <input type="search" placeholder="Search windows" aria-label="Search windows" bind:value={query} bind:this={searchEl} />

  <div class="scroll">
    {#if shownRecent.length && !query}
      <h2>Recent</h2>
      <ul>
        {#each shownRecent as r (sourceKey(r.spec))}
          <li><button class="item" onclick={() => pick(r.spec)}>{sourceLabel(r.spec, all) || r.label}</button></li>
        {/each}
      </ul>
    {/if}

    {#if !query}
      <h2>Displays</h2>
      <ul>
        {#each displays as d (d.id)}
          <li><button class="item" onclick={() => pick({ kind: 'display', display_id: d.id })}>{d.name}</button></li>
        {/each}
      </ul>
    {/if}

    <h2>Windows</h2>
    {#if perms && perms.screen !== 'granted'}
      <p class="muted small">Grant Screen Recording to list windows, or use the system picker.</p>
    {:else}
      <ul>
        {#each windows as w (w.id)}
          <li><button class="item" onclick={() => pick({ kind: 'window', window_id: w.id })}>{windowTitle(w)}</button></li>
        {:else}
          <li class="muted small">{query ? 'No matching windows.' : 'No windows.'}</li>
        {/each}
      </ul>
    {/if}
  </div>

  {#if error}<p class="warn" role="alert">{error}</p>{/if}

  <div class="actions">
    <button onclick={() => run(() => beginRegion('switch', null))}>Select region…</button>
    <button onclick={() => run(() => beginPick('switch'))}>Pick by clicking</button>
    {#if caps?.system_picker}<button onclick={() => run(() => systemPicker('switch'))}>System picker…</button>{/if}
  </div>
</div>

<style>
  .switcher {
    height: 100vh;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: 10px;
  }
  input[type='search'] {
    width: 100%;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  h2 {
    margin-top: 6px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .item {
    width: 100%;
    text-align: left;
    border: none;
    background: transparent;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item:hover,
  .item:focus-visible {
    background: var(--accent);
    color: var(--accent-text);
  }
  p {
    margin: 0;
  }
  .actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .actions button {
    font-size: 12px;
  }
</style>
