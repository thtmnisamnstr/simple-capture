<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import type { CapabilitiesDto, CaptureDevice, DisplayInfo, PermissionsDto, SourceList, SourceSpecDto } from '../lib/types.ts';
  import { liveRecent, loadRecent, rememberSource, type RecentEntry } from '../lib/recent.ts';
  import { sourceKey, sourceLabel, windowTitle } from '../lib/sources.ts';
  import { beginPick, beginRegion, hideSelf, systemPicker } from '../lib/windows.ts';
  import Icon from '../components/Icon.svelte';

  let perms = $state<PermissionsDto | null>(null);
  let caps = $state<CapabilitiesDto | null>(null);
  let displays = $state<DisplayInfo[]>([]);
  let list = $state<SourceList | null>(null);
  let cameras = $state<CaptureDevice[]>([]);
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
      const [p, c, d, dev] = await Promise.all([
        api.getPermissions(),
        api.getCapabilities(),
        api.listDisplays(),
        api.listDevices().catch(() => ({ cameras: [], mics: [] })),
      ]);
      perms = p;
      caps = c;
      displays = d;
      cameras = p.camera === 'granted' ? dev.cameras : [];
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
      recent = rememberSource(spec, sourceLabel(spec, all, cameras));
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
  <div class="top">
    <input type="search" placeholder="Search windows" aria-label="Search windows" bind:value={query} bind:this={searchEl} />
  </div>

  <div class="scroll">
    {#if shownRecent.length && !query}
      <h2>Recent</h2>
      <ul>
        {#each shownRecent as r (sourceKey(r.spec))}
          <li>
            <button class="item" onclick={() => pick(r.spec)}>
              <Icon name={r.spec.kind === 'display' ? 'display' : r.spec.kind === 'application' ? 'app' : r.spec.kind === 'region' ? 'region' : r.spec.kind === 'camera' ? 'camera' : r.spec.kind === 'device' ? 'phone' : 'window'} />
              <span class="name">{sourceLabel(r.spec, all, cameras) || r.label}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    {#if !query}
      <h2>Displays</h2>
      <ul>
        {#each displays as d (d.id)}
          <li>
            <button class="item" onclick={() => pick({ kind: 'display', display_id: d.id })}>
              <Icon name="display" /><span class="name">{d.name}</span>
            </button>
          </li>
        {/each}
      </ul>
      {#if cameras.length}
        <h2>Camera</h2>
        <ul>
          {#each cameras as c (c.id)}
            <li>
              <button class="item" onclick={() => pick({ kind: 'camera', device_id: c.id })} title="The camera fills the frame">
                <Icon name="camera" /><span class="name">{c.name}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}

    <h2>Windows</h2>
    {#if perms && perms.screen !== 'granted'}
      <p class="caption pad">Allow Screen Recording to list windows, or use the System Picker.</p>
    {:else}
      <ul>
        {#each windows as w (w.id)}
          <li>
            <button class="item" onclick={() => pick({ kind: 'window', window_id: w.id })}>
              <Icon name="window" /><span class="name">{windowTitle(w)}</span>
            </button>
          </li>
        {:else}
          <li class="caption pad">{query ? 'No matching windows.' : 'No windows.'}</li>
        {/each}
      </ul>
    {/if}
  </div>

  {#if error}<p class="warn" role="alert"><Icon name="warning" size={14} /><span>{error}</span></p>{/if}

  <div class="actions">
    <button onclick={() => run(() => beginRegion('switch', null))}><Icon name="region" size={14} />Region…</button>
    <button onclick={() => run(() => beginPick('switch'))}><Icon name="pointer" size={14} />Click a Window</button>
    {#if caps?.system_picker}<button onclick={() => run(() => systemPicker('switch'))} title="Choose with the system picker"><Icon name="picker" size={14} />System…</button>{/if}
  </div>
</div>

<style>
  .switcher {
    height: 100vh;
    display: flex;
    flex-direction: column;
    border-radius: 12px;
    background: var(--window);
    box-shadow: inset 0 0 0 0.5px var(--separator);
    overflow: hidden;
  }
  :global(:root[data-native]) .switcher {
    background: transparent;
  }
  .top {
    padding: 10px 10px 6px;
  }
  input[type='search'] {
    width: 100%;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 6px 6px;
  }
  h2 {
    margin: 10px 0 3px 8px;
    font-size: 11px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .item {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 8px;
    min-height: 24px;
    padding: 2px 8px;
    border-radius: 5px;
    background: transparent;
    box-shadow: none;
    text-align: left;
  }
  .item :global(.icon) {
    color: var(--accent);
  }
  .item:hover,
  .item:focus-visible {
    background: var(--accent);
    color: var(--on-accent);
    box-shadow: none;
  }
  .item:hover :global(.icon),
  .item:focus-visible :global(.icon) {
    color: inherit;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pad {
    padding: 2px 8px;
  }
  .warn {
    margin: 0 10px 6px;
  }
  .actions {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(0, 1fr));
    gap: 6px;
    padding: 8px 10px 10px;
    box-shadow: 0 -0.5px 0 var(--separator);
  }
  .actions button {
    min-width: 0;
    font-size: 12px;
  }
  .actions :global(.icon) {
    color: var(--label-2);
  }
</style>
