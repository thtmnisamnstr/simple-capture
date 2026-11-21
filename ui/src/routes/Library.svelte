<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { appApi, folderName, posterUrl, whenLabel, type Library as LibraryData, type LibraryItem } from '../lib/appinfo.ts';
  import { editor } from '../lib/editor.ts';
  import { onEvent } from '../lib/events.ts';
  import { formatBytes, formatElapsed, revealLabel } from '../lib/format.ts';
  import { isObject } from '../lib/guards.ts';
  import Icon from '../components/Icon.svelte';

  let lib = $state<LibraryData | null>(null);
  let error: string | null = $state(null);
  let query = $state('');
  let selected: string | null = $state(null);
  let confirming: LibraryItem | null = $state(null);
  let busy = $state(false);

  const items = $derived(
    (lib?.items ?? []).filter((i) => !query.trim() || i.title.toLowerCase().includes(query.trim().toLowerCase())),
  );
  const total = $derived((lib?.items ?? []).reduce((n, i) => n + i.bytes, 0));
  const current = $derived(items.find((i) => i.dir === selected) ?? null);

  async function load() {
    try {
      lib = await appApi.library();
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    load();
    const onFocus = () => load();
    window.addEventListener('focus', onFocus);
    const offs = [
      onEvent('library-changed', isObject, () => load()),
      onEvent('recording-state', isObject, () => load()),
    ];
    return () => {
      window.removeEventListener('focus', onFocus);
      offs.forEach((off) => off());
    };
  });

  async function run(f: () => Promise<unknown>) {
    error = null;
    busy = true;
    try {
      await f();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  const editable = (i: LibraryItem) => i.state !== 'recording';
  const length = (i: LibraryItem) => (i.edited_ns > 0 ? i.edited_ns : (i.duration_ns ?? 0));

  function onKey(e: KeyboardEvent) {
    if (!current || confirming || (e.target as HTMLElement)?.tagName === 'INPUT') return;
    if (e.key === 'Enter' && editable(current)) run(() => editor.open(current!.dir));
    if ((e.key === 'Backspace' || e.key === 'Delete') && (e.metaKey || e.ctrlKey)) confirming = current;
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="window library">
  <header class="toolbar" data-tauri-drag-region>
    <h1 data-tauri-drag-region>Library</h1>
    <span class="caption tnum" data-tauri-drag-region>
      {lib ? `${lib.items.length} recording${lib.items.length === 1 ? '' : 's'} · ${formatBytes(total)}` : ''}
      {lib?.free_bytes != null ? ` · ${formatBytes(lib.free_bytes)} free` : ''}
    </span>
    <span class="spacer" data-tauri-drag-region></span>
    <input class="search" type="search" placeholder="Search" aria-label="Search recordings" bind:value={query} />
    <button onclick={() => lib && api.reveal(lib.root)} title={revealLabel()}><Icon name="folder" size={14} />Open Folder</button>
  </header>

  {#if error}<p class="warn" role="alert"><Icon name="warning" size={14} /><span>{error}</span></p>{/if}

  {#if lib && !lib.items.length}
    <div class="empty">
      <span class="badge-icon"><Icon name="film" size={22} /></span>
      <p>No recordings yet.</p>
      <p class="caption">Recordings you make appear here.</p>
    </div>
  {:else}
    <ul class="grid" role="listbox" aria-label="Recordings">
      {#each items as i (i.dir)}
        <li
          role="option"
          aria-selected={selected === i.dir}
          class:selected={selected === i.dir}
          tabindex="0"
          onclick={() => (selected = i.dir)}
          ondblclick={() => editable(i) && run(() => editor.open(i.dir))}
          onkeydown={(e) => e.key === ' ' && (selected = i.dir)}
        >
          <div class="poster">
            <img src={posterUrl(i.dir)} alt="" loading="lazy" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = 'hidden')} />
            <span class="len tnum">{formatElapsed(length(i))}</span>
            {#if i.state === 'recording'}<span class="state live">Recording</span>
            {:else if i.state === 'recovered'}<span class="state">Recovered</span>
            {:else if i.state === 'damaged'}<span class="state bad">Damaged</span>{/if}
          </div>
          <div class="meta">
            <span class="name" title={folderName(i.dir)}>{i.title}</span>
            <span class="caption tnum">{whenLabel(i.created_at)} · {formatBytes(i.bytes)}{i.has_camera ? ' · camera' : ''}</span>
          </div>
        </li>
      {:else}
        <p class="caption muted">No recording matches “{query}”.</p>
      {/each}
    </ul>
  {/if}

  <footer>
    {#if current}
      <span class="caption sel">{current.title}</span>
      <button disabled={busy} onclick={() => (confirming = current)}><Icon name="trash" size={14} />Move to Trash</button>
      <button onclick={() => api.reveal(current!.dir)}>{revealLabel()}</button>
      <button disabled={busy || !editable(current)} onclick={() => run(() => appApi.exportFrom(folderName(current!.dir)))}><Icon name="export" size={14} />Export…</button>
      <button class="primary" disabled={busy || !editable(current)} onclick={() => run(() => editor.open(current!.dir))}>Edit</button>
    {:else}
      <span class="caption muted">Select a recording. Double-click to edit it.</span>
    {/if}
  </footer>

  {#if confirming}
    <div class="scrim" role="presentation" onclick={() => (confirming = null)}></div>
    <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="trash-h">
      <h2 id="trash-h">Move “{confirming.title}” to the Trash?</h2>
      <p class="caption">Its recording and edits go to the Trash, where you can still restore them. Exported videos are kept.</p>
      <div class="row">
        <button onclick={() => (confirming = null)}>Cancel</button>
        <button
          class="primary danger"
          onclick={() => {
            const c = confirming!;
            confirming = null;
            selected = null;
            run(() => appApi.trash(folderName(c.dir)).then(load));
          }}>Move to Trash</button
        >
      </div>
    </div>
  {/if}
</div>

<style>
  .library {
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 30px 16px 10px;
    border-bottom: 1px solid var(--separator);
  }
  .toolbar h1 {
    margin: 0;
    font-size: 15px;
  }
  .spacer {
    flex: 1;
  }
  .search {
    width: 180px;
  }
  .grid {
    flex: 1;
    min-height: 0;
    overflow: auto;
    list-style: none;
    margin: 0;
    padding: 14px 16px;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 14px;
    align-content: start;
  }
  li {
    border-radius: 10px;
    padding: 6px;
    cursor: default;
    outline: none;
  }
  li:focus-visible,
  li.selected {
    background: var(--fill-2);
  }
  li.selected .poster {
    box-shadow: 0 0 0 2px var(--accent, #0a84ff);
  }
  .poster {
    position: relative;
    aspect-ratio: 16 / 10;
    border-radius: 7px;
    overflow: hidden;
    background: var(--fill-3, #0002);
  }
  .poster img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: #000;
  }
  .len {
    position: absolute;
    right: 6px;
    bottom: 6px;
    padding: 1px 6px;
    border-radius: 5px;
    background: #000a;
    color: #fff;
    font-size: 11px;
  }
  .state {
    position: absolute;
    left: 6px;
    top: 6px;
    padding: 1px 6px;
    border-radius: 5px;
    background: var(--orange);
    color: #fff;
    font-size: 10.5px;
    font-weight: 600;
  }
  .state.live {
    background: var(--red, #d93025);
  }
  .state.bad {
    background: var(--label-2);
  }
  .meta {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 6px 2px 0;
    min-width: 0;
  }
  .name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .empty {
    flex: 1;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 4px;
  }
  .empty p {
    margin: 0;
  }
  .badge-icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    color: #fff;
    background: var(--label-3);
    margin-bottom: 6px;
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 16px 12px;
    border-top: 1px solid var(--separator);
  }
  .sel {
    margin-right: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .warn {
    margin: 8px 16px 0;
  }
  .scrim {
    position: fixed;
    inset: 0;
    background: #0004;
  }
  .dialog {
    position: fixed;
    left: 50%;
    top: 30%;
    transform: translate(-50%, -30%);
    width: min(380px, 90vw);
    padding: 16px;
    border-radius: 12px;
    background: var(--window-bg, var(--bg));
    box-shadow: 0 12px 40px #0006;
  }
  .dialog h2 {
    margin: 0 0 6px;
    font-size: 14px;
  }
  .dialog .row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 12px;
  }
  .danger {
    background: var(--red, #d93025);
  }
</style>
