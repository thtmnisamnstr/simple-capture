<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import {
    appApi,
    clickSelect,
    folderName,
    itemLength,
    LIBRARY_SORTS,
    posterUrl,
    sortItems,
    whenLabel,
    type Library as LibraryData,
    type LibraryItem,
    type LibrarySort,
  } from '../lib/appinfo.ts';
  import { editor } from '../lib/editor.ts';
  import { onEvent } from '../lib/events.ts';
  import { formatBytes, formatElapsed, revealLabel } from '../lib/format.ts';
  import { isObject } from '../lib/guards.ts';
  import Icon from '../components/Icon.svelte';

  const SORT_KEY = 'sc.library-sort';

  let lib = $state<LibraryData | null>(null);
  let error: string | null = $state(null);
  let query = $state('');
  let selected = $state<string[]>([]);
  let anchor: string | null = null;
  let confirming = $state<LibraryItem[] | null>(null);
  let busy = $state(false);
  let sort = $state<LibrarySort>(loadSort());
  let renaming: string | null = $state(null);
  let draft = $state('');
  let renameTimer: ReturnType<typeof setTimeout> | undefined;

  function open(i: LibraryItem) {
    clearTimeout(renameTimer);
    if (editable(i) && !renaming) run(() => editor.open(i.dir));
  }

  const items = $derived(
    sortItems(
      (lib?.items ?? []).filter((i) => !query.trim() || i.title.toLowerCase().includes(query.trim().toLowerCase())),
      sort,
    ),
  );
  const total = $derived((lib?.items ?? []).reduce((n, i) => n + i.bytes, 0));
  const chosen = $derived(items.filter((i) => selected.includes(i.dir)));
  const current = $derived(chosen.length === 1 ? chosen[0] : null);

  function loadSort(): LibrarySort {
    try {
      const v = localStorage.getItem(SORT_KEY);
      return LIBRARY_SORTS.some((s) => s.value === v) ? (v as LibrarySort) : 'newest';
    } catch {
      return 'newest';
    }
  }

  $effect(() => {
    try {
      localStorage.setItem(SORT_KEY, sort);
    } catch {
      /* private mode */
    }
  });

  async function load() {
    try {
      lib = await appApi.library();
      const dirs = new Set(lib.items.map((i) => i.dir));
      selected = selected.filter((d) => dirs.has(d));
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
      onEvent('project-renamed', isObject, (r) => {
        if (typeof r.from === 'string' && typeof r.to === 'string') selected = selected.map((d) => (d === r.from ? (r.to as string) : d));
      }),
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

  function pick(e: MouseEvent, i: LibraryItem) {
    if (renaming) return;
    const r = clickSelect(
      items.map((x) => x.dir),
      selected,
      anchor,
      i.dir,
      { toggle: e.metaKey || e.ctrlKey, extend: e.shiftKey },
    );
    selected = r.selected;
    anchor = r.anchor;
  }

  function startRename(i: LibraryItem) {
    if (!editable(i)) return;
    selected = [i.dir];
    renaming = i.dir;
    draft = i.title;
  }

  function commitRename() {
    const dir = renaming;
    const item = items.find((i) => i.dir === dir);
    renaming = null;
    if (!dir || !item) return;
    const title = draft.trim();
    if (!title || title === item.title) return;
    run(async () => {
      const r = await appApi.rename(folderName(dir), title);
      selected = [r.dir];
      anchor = r.dir;
      await load();
    });
  }

  function trashAll(list: LibraryItem[]) {
    confirming = null;
    selected = [];
    run(async () => {
      for (const c of list) await appApi.trash(folderName(c.dir));
      await load();
    });
  }

  function focusSelect(el: HTMLInputElement) {
    el.focus();
    el.select();
  }

  function onKey(e: KeyboardEvent) {
    if (confirming || renaming || (e.target as HTMLElement)?.tagName === 'INPUT') return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      selected = items.map((i) => i.dir);
      return;
    }
    if (e.key === 'Escape') selected = [];
    if (!chosen.length) return;
    if ((e.key === 'Backspace' || e.key === 'Delete') && (e.metaKey || e.ctrlKey)) {
      const list = chosen.filter(editable);
      if (list.length) confirming = list;
    }
    if (!current) return;
    if (e.key === 'Enter' && editable(current)) run(() => editor.open(current!.dir));
    if (e.key === 'F2') startRename(current);
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
    <select class="sort" aria-label="Sort recordings" bind:value={sort}>
      {#each LIBRARY_SORTS as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
    </select>
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
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
    <ul class="grid" role="listbox" aria-label="Recordings" aria-multiselectable="true" onclick={(e) => e.target === e.currentTarget && (selected = [])}>
      {#each items as i (i.dir)}
        {@const on = selected.includes(i.dir)}
        <li
          role="option"
          aria-selected={on}
          class:selected={on}
          tabindex="0"
          onclick={(e) => pick(e, i)}
          ondblclick={() => open(i)}
          onkeydown={(e) => e.key === ' ' && (selected = [i.dir])}
        >
          <div class="poster">
            {#if i.state !== 'recording'}
              {#key `${i.state}-${i.duration_ns ?? 0}`}
                <img src="{posterUrl(i.dir)}?v={i.state}-{i.duration_ns ?? 0}" alt="" loading="lazy" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = 'hidden')} />
              {/key}
            {/if}
            <span class="len tnum">{formatElapsed(itemLength(i))}</span>
            {#if i.state === 'recording'}<span class="state live">Recording</span>
            {:else if i.state === 'recovered'}<span class="state">Recovered</span>
            {:else if i.state === 'damaged'}<span class="state bad">Damaged</span>{/if}
          </div>
          <div class="meta">
            {#if renaming === i.dir}
              <input
                class="rename"
                aria-label="Name"
                maxlength="100"
                bind:value={draft}
                use:focusSelect
                onclick={(e) => e.stopPropagation()}
                ondblclick={(e) => e.stopPropagation()}
                onkeydown={(e) => {
                  e.stopPropagation();
                  if (e.key === 'Enter') commitRename();
                  if (e.key === 'Escape') renaming = null;
                }}
                onblur={commitRename}
              />
            {:else}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <span
                class="name"
                title={folderName(i.dir)}
                onclick={(e) => {
                  if (on && selected.length === 1 && !e.metaKey && !e.shiftKey) {
                    e.stopPropagation();
                    clearTimeout(renameTimer);
                    renameTimer = setTimeout(() => startRename(i), 500);
                  }
                }}>{i.title}</span
              >
            {/if}
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
      <button disabled={busy || !editable(current)} onclick={() => (confirming = [current!])}><Icon name="trash" size={14} />Move to Trash</button>
      <button disabled={busy || !editable(current)} onclick={() => startRename(current!)}>Rename</button>
      <button onclick={() => api.reveal(current!.dir)}>{revealLabel()}</button>
      <button disabled={busy || !editable(current)} onclick={() => run(() => appApi.exportFrom(folderName(current!.dir)))}><Icon name="export" size={14} />Export…</button>
      <button class="primary" disabled={busy || !editable(current)} onclick={() => run(() => editor.open(current!.dir))}>Edit</button>
    {:else if chosen.length > 1}
      <span class="caption sel tnum">{chosen.length} recordings · {formatBytes(chosen.reduce((n, i) => n + i.bytes, 0))}</span>
      <button disabled={busy || !chosen.some(editable)} onclick={() => (confirming = chosen.filter(editable))}><Icon name="trash" size={14} />Move {chosen.filter(editable).length} to Trash</button>
    {:else}
      <span class="caption muted">Select a recording. Double-click to edit it; click its name to rename it.</span>
    {/if}
  </footer>

  {#if confirming && confirming.length}
    {@const list = confirming}
    <div class="scrim" role="presentation" onclick={() => (confirming = null)}></div>
    <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="trash-h">
      <h2 id="trash-h">{list.length === 1 ? `Move “${list[0].title}” to the Trash?` : `Move ${list.length} recordings to the Trash?`}</h2>
      <p class="caption">
        {list.length === 1 ? 'Its recording and edits go' : `Their recordings and edits (${formatBytes(list.reduce((n, i) => n + i.bytes, 0))}) go`} to the Trash, where you
        can still restore them. Exported videos are kept.
      </p>
      <div class="row">
        <button onclick={() => (confirming = null)}>Cancel</button>
        <button class="primary danger" onclick={() => trashAll(list)}>Move to Trash</button>
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
  .sort {
    width: auto;
  }
  .rename {
    width: 100%;
    font-weight: 600;
    min-height: 20px;
    padding: 0 4px;
    box-sizing: border-box;
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
