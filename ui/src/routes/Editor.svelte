<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { onEvent } from '../lib/events.ts';
  import { isExportProgress, isExportState, isObject } from '../lib/guards.ts';
  import {
    NS,
    editor,
    formatTime,
    keyAt,
    locate,
    starts,
    type Doc,
    type DocUpdate,
    type EditOp,
    type EditorFrame,
    type EditorState,
  } from '../lib/editor.ts';
  import { CAMERA_SIZE_MAX, CAMERA_SIZE_MIN, CAMERA_ZOOM_MAX, clampCenter, shapeSize, snapCenter } from '../lib/geometry.ts';
  import { shapeAspect } from '../lib/camera.ts';
  import { LiveView } from '../lib/live.ts';
  import type { CameraKey, ExportProgressDto, ExportStateDto } from '../lib/types.ts';
  import Icon from '../components/Icon.svelte';
  import Timeline, { type Thumbs } from '../components/editor/Timeline.svelte';
  import Inspector from '../components/editor/Inspector.svelte';

  let st = $state<EditorState | null>(null);
  let doc = $state<Doc | null>(null);
  let revision = $state(0);
  let canUndo = $state(false);
  let canRedo = $state(false);
  let playhead = $state(0);
  let playing = $state(false);
  let selected = $state<string[]>([]);
  let range = $state<[number, number] | null>(null);
  let clipboardNs = $state(0);
  let frame = $state<EditorFrame | null>(null);
  let thumbs = $state<Thumbs | null>(null);
  let peaks = $state<Uint8Array | null>(null);
  let pxPerSec = $state(40);
  let fitted = false;
  let tab = $state<'clip' | 'camera' | 'audio' | 'frame'>('clip');
  let error = $state<string | null>(null);
  let toast = $state<string | null>(null);
  let exportJob = $state<string | null>(null);
  let exportProgress = $state<ExportProgressDto | null>(null);
  let exportState = $state<ExportStateDto | null>(null);
  let exportMenu = $state(false);
  let previewEl: HTMLDivElement | undefined = $state();
  let liveCanvas: HTMLCanvasElement | undefined = $state();
  let timelineEl: HTMLDivElement | undefined = $state();

  const clips = $derived(doc?.edl.clips ?? []);
  const st0 = $derived(starts(clips));
  const duration = $derived(st0[st0.length - 1] ?? 0);
  const fps = $derived(st?.fps ?? 30);
  const overlay = $derived(doc?.camera_layout.overlays[0] ?? null);
  const recNs = $derived(frame?.rec_ns ?? locate(clips, st0, Math.min(playhead, duration - 1))?.rec_ns ?? 0);
  const exporting = $derived(!!exportJob && !!exportState && ['preparing', 'rendering', 'finalizing'].includes(exportState.state));
  const pct = $derived(exportProgress && exportProgress.frames_total > 0 ? Math.round((exportProgress.frames_done / exportProgress.frames_total) * 100) : 0);

  function absorb(u: DocUpdate) {
    if (u.doc) doc = u.doc;
    if (u.revision !== undefined) revision = Math.max(revision, u.revision);
    if (u.can_undo !== undefined) canUndo = u.can_undo;
    if (u.can_redo !== undefined) canRedo = u.can_redo;
    if (u.playhead_tl_ns !== undefined && !playing) playhead = u.playhead_tl_ns;
    if (u.clipboard_ns !== undefined) clipboardNs = u.clipboard_ns;
    if (doc) selected = selected.filter((id) => doc!.edl.clips.some((c) => c.id === id));
  }

  async function run<T>(p: Promise<T>): Promise<T | null> {
    error = null;
    try {
      return await p;
    } catch (e) {
      error = errorText(e);
      return null;
    }
  }

  async function edit(op: EditOp, coalesce?: string) {
    const u = await run(editor.edit(op, coalesce));
    if (u) absorb(u);
  }

  async function load() {
    const s = await run(editor.state());
    if (!s) return;
    st = s;
    doc = s.doc;
    revision = s.revision;
    canUndo = s.can_undo;
    canRedo = s.can_redo;
    playhead = s.playhead_tl_ns;
    playing = s.playing;
    clipboardNs = s.clipboard_ns;
    selected = [];
    range = null;
    thumbs = null;
    peaks = null;
    fitted = false;
    frame = null;
    sendRect();
  }

  function fitZoom() {
    if (!timelineEl || duration <= 0) return;
    pxPerSec = Math.min(Math.max((timelineEl.clientWidth - 40) / (duration / NS), 2), 400);
    fitted = true;
  }

  $effect(() => {
    if (!fitted && duration > 0 && timelineEl) fitZoom();
  });

  function sendRect() {
    if (!previewEl) return;
    const r = previewEl.getBoundingClientRect();
    void editor.rect({ x: r.left, y: r.top, w: r.width, h: r.height }).catch(() => {});
  }

  async function loadPeaks() {
    try {
      const base = /Windows|Android/.test(navigator.userAgent) ? 'http://sc.localhost/peaks' : 'sc://peaks';
      const r = await fetch(`${base}/v1`, { cache: 'no-store' });
      if (r.ok) peaks = new Uint8Array(await r.arrayBuffer());
    } catch {
      peaks = null;
    }
  }

  $effect(() => {
    const offs = [
      onEvent('editor-doc', isObject, (u) => absorb(u as unknown as DocUpdate)),
      onEvent('editor-playhead', isObject, (p) => {
        const v = p as { tl_ns: number; playing: boolean };
        playing = v.playing;
        if (!scrubbing) playhead = v.tl_ns;
      }),
      onEvent('editor-frame', isObject, (f) => (frame = f as unknown as EditorFrame)),
      onEvent('editor-media', isObject, (m) => {
        const v = m as { peaks?: boolean; thumbs?: { step_ms: number; count: number; w: number; h: number; ready: number[] } };
        if (v.peaks) void loadPeaks();
        if (v.thumbs) {
          const t = v.thumbs;
          const ready = thumbs && thumbs.step_ms === t.step_ms ? new Set(thumbs.ready) : new Set<number>();
          for (const i of t.ready) ready.add(i);
          thumbs = { step_ms: t.step_ms, count: t.count, w: t.w, h: t.h, ready };
        }
      }),
      onEvent('editor-open', isObject, () => void load()),
      onEvent('export-progress', isExportProgress, (p) => {
        if (p.id === exportJob) exportProgress = p;
      }),
      onEvent('export-state', isExportState, (s) => {
        if (s.id !== exportJob) return;
        exportState = s;
        if (s.state === 'done') toast = s.path;
        if (s.state === 'failed') error = s.error;
      }),
    ];
    void load();
    const ro = new ResizeObserver(() => sendRect());
    if (previewEl) ro.observe(previewEl);
    const onResize = () => sendRect();
    window.addEventListener('resize', onResize);
    const onFocus = () => void load();
    window.addEventListener('focus', onFocus);
    return () => {
      offs.forEach((f) => f());
      ro.disconnect();
      window.removeEventListener('resize', onResize);
      window.removeEventListener('focus', onFocus);
    };
  });

  $effect(() => {
    if (!liveCanvas) return;
    const v = new LiveView(liveCanvas, 'editor', () => {});
    return () => v.stop();
  });

  let scrubbing = false;
  let seekQueued: number | null = null;
  function seek(t: number, done: boolean) {
    scrubbing = !done;
    playhead = Math.min(Math.max(t, 0), duration);
    if (seekQueued === null) {
      seekQueued = requestAnimationFrame(() => {
        seekQueued = null;
        void editor.seek(playhead).catch(() => {});
      });
    }
  }

  async function togglePlay() {
    const r = await run(playing ? editor.pause() : editor.play(playhead));
    if (r) {
      playing = r.playing;
      playhead = r.playhead_tl_ns;
    }
  }

  function stepFrames(n: number) {
    if (playing) void editor.pause();
    seek(playhead + (n * NS) / fps, true);
  }

  function jumpCut(dir: 1 | -1) {
    const cuts = [...st0, duration];
    const eps = NS / fps / 2;
    const next = dir > 0 ? cuts.find((c) => c > playhead + eps) : [...cuts].reverse().find((c) => c < playhead - eps);
    if (next !== undefined) seek(next, true);
  }

  const selection = () =>
    range && Math.abs(range[1] - range[0]) > 0
      ? { a_tl_ns: Math.min(...range), b_tl_ns: Math.max(...range) }
      : selected.length
        ? { ids: selected }
        : null;

  async function deleteSelection() {
    const s = selection();
    if (!s) return;
    if (s.ids) await edit({ op: 'delete_clips', ids: s.ids });
    else await edit({ op: 'delete_range', a_tl_ns: s.a_tl_ns!, b_tl_ns: s.b_tl_ns! });
    if (s.a_tl_ns !== undefined) seek(s.a_tl_ns, true);
    range = null;
  }

  async function copy(cut: boolean) {
    const s = selection();
    if (!s) return;
    const u = await run(cut ? editor.cut(s) : editor.copy(s));
    if (u) {
      absorb(u);
      if (cut) range = null;
      toast = cut ? 'Cut' : 'Copied';
    }
  }

  async function paste() {
    const u = await run(editor.paste(playhead));
    if (u) absorb(u);
  }

  async function undo() {
    const u = await run(editor.undo());
    if (u) absorb(u);
  }

  async function redo() {
    const u = await run(editor.redo());
    if (u) absorb(u);
  }

  function split() {
    void edit({ op: 'split', at_tl_ns: Math.round(playhead) });
  }

  function duplicate() {
    if (selected.length) void edit({ op: 'duplicate', ids: selected });
  }

  async function startExport(size: string) {
    exportMenu = false;
    exportProgress = null;
    exportState = null;
    toast = null;
    const id = await run(editor.exportVideo(size));
    if (id) {
      exportJob = id;
      exportState = { id, state: 'preparing' };
    }
  }

  function typing(e: KeyboardEvent): boolean {
    const t = e.target as HTMLElement | null;
    return !!t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable);
  }

  function onkeydown(e: KeyboardEvent) {
    if (typing(e) && e.key !== 'Escape') return;
    const mod = e.metaKey || e.ctrlKey;
    const k = e.key.toLowerCase();
    const handled = () => {
      e.preventDefault();
      e.stopPropagation();
    };
    if (mod && k === 'z') return handled(), e.shiftKey ? void redo() : void undo();
    if (mod && k === 'x') return handled(), void copy(true);
    if (mod && k === 'c') return handled(), void copy(false);
    if (mod && k === 'v') return handled(), void paste();
    if (mod && k === 'd') return handled(), duplicate();
    if (mod && k === 'a') return handled(), (selected = clips.map((c) => c.id));
    if (mod && (k === '=' || k === '+')) return handled(), (pxPerSec = Math.min(pxPerSec * 1.4, 600));
    if (mod && k === '-') return handled(), (pxPerSec = Math.max(pxPerSec / 1.4, 2));
    if (mod) return;
    switch (e.key) {
      case ' ':
        handled();
        void togglePlay();
        break;
      case 'k':
      case 'K':
        handled();
        if (playing) void togglePlay();
        break;
      case 'l':
      case 'L':
        handled();
        if (!playing) void togglePlay();
        break;
      case 'j':
      case 'J':
        handled();
        seek(playhead - 2 * NS, true);
        if (playing) void editor.play(Math.max(0, playhead));
        break;
      case 'ArrowLeft':
        handled();
        if (e.shiftKey) seek(playhead - NS, true);
        else stepFrames(-1);
        break;
      case 'ArrowRight':
        handled();
        if (e.shiftKey) seek(playhead + NS, true);
        else stepFrames(1);
        break;
      case 'ArrowUp':
        handled();
        jumpCut(-1);
        break;
      case 'ArrowDown':
        handled();
        jumpCut(1);
        break;
      case 's':
      case 'S':
        handled();
        split();
        break;
      case 'i':
      case 'I':
        handled();
        range = [playhead, range ? Math.max(range[1], playhead) : duration];
        break;
      case 'o':
      case 'O':
        handled();
        range = [range ? Math.min(range[0], playhead) : 0, playhead];
        break;
      case 'Delete':
      case 'Backspace':
        handled();
        void deleteSelection();
        break;
      case 'Escape':
        selected = [];
        range = null;
        exportMenu = false;
        break;
      case 'Home':
        handled();
        seek(0, true);
        break;
      case 'End':
        handled();
        seek(duration, true);
        break;
    }
  }

  type CamDrag = { corner: [number, number] | null; x: number; y: number; start: CameraKey };
  let camDrag: CamDrag | null = null;
  const camKey = $derived(overlay ? keyAt(overlay.keyframes, recNs) : null);

  function camDown(e: PointerEvent, corner: [number, number] | null) {
    if (e.button !== 0 || !camKey || playing) return;
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    camDrag = { corner, x: e.clientX, y: e.clientY, start: $state.snapshot(camKey) as CameraKey };
  }

  let camQueued: CameraKey | null = null;
  function camSend(k: CameraKey) {
    if (!overlay) return;
    const first = camQueued === null;
    camQueued = k;
    if (first)
      requestAnimationFrame(() => {
        const next = camQueued;
        camQueued = null;
        if (next) void edit({ op: 'set_camera_key', overlay_id: overlay.id, key: next }, 'cam-drag');
      });
  }

  function camMove(e: PointerEvent) {
    if (!camDrag || !frame) return;
    const s = camDrag.start;
    const aspect = shapeAspect(s.shape);
    const css = frame.rect;
    const canvasPx = frame.canvas_px;
    const dx = e.clientX - camDrag.x;
    const dy = e.clientY - camDrag.y;
    if (camDrag.corner) {
      const [sx, sy] = camDrag.corner;
      const { w: w0, h: h0 } = shapeSize(Math.min(css.w, css.h) * s.size_norm, aspect);
      const size = Math.min(Math.max(s.size_norm * Math.max(1 + (dx * sx) / w0 + (dy * sy) / h0, 0.05), CAMERA_SIZE_MIN), CAMERA_SIZE_MAX);
      camSend({ ...s, size_norm: size, center_norm: clampCenter(canvasPx, aspect, size, s.center_norm) });
      return;
    }
    const c = { x: s.center_norm.x + dx / css.w, y: s.center_norm.y + dy / css.h };
    camSend({ ...s, center_norm: snapCenter(canvasPx, aspect, s.size_norm, c, (12 * canvasPx.w) / css.w) });
  }

  function camUp() {
    if (!camDrag) return;
    camDrag = null;
    void editor.seal();
  }

  function camWheel(e: WheelEvent) {
    if (!camKey || !overlay) return;
    e.preventDefault();
    void edit(
      { op: 'set_camera_key', overlay_id: overlay.id, key: { ...camKey, zoom: Math.min(Math.max(camKey.zoom * Math.exp(-e.deltaY / 400), 1), CAMERA_ZOOM_MAX) } },
      'cam-wheel',
    );
  }

  const camRect = $derived(frame?.camera && previewEl ? frame.camera : null);
</script>

<svelte:window {onkeydown} onpointermove={camMove} onpointerup={camUp} />

<div class="editor">
  <header class="toolbar" data-tauri-drag-region>
    <div class="title" data-tauri-drag-region>
      <span class="name">{st?.title ?? 'Editor'}</span>
      <span class="muted tnum small">{formatTime(duration)}</span>
    </div>
    <div class="transport">
      <button class="plain icon" title="Go to start (Home)" onclick={() => seek(0, true)} aria-label="Go to start"><Icon name="start" size={15} /></button>
      <button class="play" onclick={togglePlay} aria-label={playing ? 'Pause' : 'Play'} title="Play/Pause (Space)">
        <Icon name={playing ? 'pause' : 'play'} size={16} />
      </button>
      <span class="time tnum">{formatTime(playhead)} <span class="muted">/ {formatTime(duration)}</span></span>
    </div>
    <div class="right">
      <button class="plain icon tool" disabled={!canUndo} onclick={undo} title="Undo (⌘Z)" aria-label="Undo"><Icon name="undo" size={15} /></button>
      <button class="plain icon tool" disabled={!canRedo} onclick={redo} title="Redo (⇧⌘Z)" aria-label="Redo"><Icon name="redo" size={15} /></button>
      {#if exporting}
        <div class="progress" title="Exporting {pct}%">
          <span class="bar"><span style:width="{pct}%"></span></span>
          <span class="tnum small">{pct}%</span>
          <button class="plain small" onclick={() => exportJob && api.cancelExport(exportJob)}>Cancel</button>
        </div>
      {:else}
        <div class="menu-anchor">
          <button class="primary" onclick={() => (exportMenu = !exportMenu)} disabled={!doc}>
            <Icon name="export" size={14} /> Export
          </button>
          {#if exportMenu}
            <div class="menu" role="menu">
              {#each [['reference', 'Full Size (MP4)'], ['1080', '1080p (MP4)'], ['720', '720p (MP4)']] as [v, label] (v)}
                <button role="menuitem" class="plain" onclick={() => startExport(v)}>{label}</button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </header>

  <main class="middle">
    <div class="stagewrap">
      <div class="preview" bind:this={previewEl} class:native={frame?.native ?? true}>
        {#if frame && !frame.native}
          <canvas
            bind:this={liveCanvas}
            class="live"
            style:left="{frame.rect.x - (previewEl?.getBoundingClientRect().left ?? 0)}px"
            style:top="{frame.rect.y - (previewEl?.getBoundingClientRect().top ?? 0)}px"
            style:width="{frame.rect.w}px"
            style:height="{frame.rect.h}px"
          ></canvas>
        {:else}
          <canvas bind:this={liveCanvas} class="live hidden"></canvas>
        {/if}
        {#if camRect && camKey?.visible && !playing && previewEl}
          {@const origin = previewEl.getBoundingClientRect()}
          <div
            class="cam"
            class:round={camKey.shape === 'circle'}
            style:left="{frame!.rect.x - origin.left + camRect.x}px"
            style:top="{frame!.rect.y - origin.top + camRect.y}px"
            style:width="{camRect.w}px"
            style:height="{camRect.h}px"
            role="presentation"
            onpointerdown={(e) => camDown(e, null)}
            onwheel={camWheel}
          >
            {#each [[-1, -1], [1, -1], [-1, 1], [1, 1]] as c (c.join())}
              <span
                class="handle"
                style:left={c[0] < 0 ? '0' : '100%'}
                style:top={c[1] < 0 ? '0' : '100%'}
                role="presentation"
                onpointerdown={(e) => camDown(e, [c[0], c[1]])}
              ></span>
            {/each}
          </div>
        {/if}
      </div>
    </div>
    <aside class="side">
      {#if doc && st}
        <Inspector
          {doc}
          tracks={st.tracks}
          {selected}
          {recNs}
          canvasPx={frame?.canvas_px ?? { w: 1920, h: 1080 }}
          bind:tab
          onedit={(op, c) => void edit(op, c)}
          onsplit={split}
          onduplicate={duplicate}
          ondelete={deleteSelection}
        />
      {/if}
    </aside>
  </main>

  <section class="bottom">
    <div class="tlbar">
      <button class="plain" onclick={split} title="Split at playhead (S)"><Icon name="scissors" size={14} /> Split</button>
      <button class="plain" onclick={() => void deleteSelection()} disabled={!selected.length && !range} title="Delete selection (⌫)"><Icon name="trash" size={14} /> Delete</button>
      <button class="plain" onclick={() => void copy(true)} disabled={!selected.length && !range} title="Cut (⌘X)">Cut</button>
      <button class="plain" onclick={() => void copy(false)} disabled={!selected.length && !range} title="Copy (⌘C)">Copy</button>
      <button class="plain" onclick={paste} disabled={clipboardNs <= 0} title="Paste at playhead (⌘V)">Paste</button>
      <span class="spacer"></span>
      {#if range}<span class="badge">Range {formatTime(Math.abs(range[1] - range[0]))}</span>{/if}
      {#if selected.length}<span class="badge">{selected.length} selected</span>{/if}
      <span class="spacer"></span>
      <span class="caption muted hint">Shift-drag to select a range · I/O set it</span>
      <button class="plain icon" onclick={() => (pxPerSec = Math.max(pxPerSec / 1.4, 2))} aria-label="Zoom out">−</button>
      <input type="range" class="zoom" min="0" max="1" step="0.001" aria-label="Timeline zoom"
        value={Math.log(pxPerSec / 2) / Math.log(300)}
        oninput={(e) => (pxPerSec = 2 * Math.pow(300, Number(e.currentTarget.value)))} />
      <button class="plain icon" onclick={() => (pxPerSec = Math.min(pxPerSec * 1.4, 600))} aria-label="Zoom in">+</button>
      <button class="plain" onclick={fitZoom}>Fit</button>
    </div>
    <div class="timeline" bind:this={timelineEl}>
      {#if doc}
        <Timeline
          {clips}
          text={doc.text}
          keys={overlay?.keyframes ?? []}
          {playhead}
          {selected}
          {range}
          {thumbs}
          {peaks}
          bind:pxPerSec
          onseek={seek}
          onselect={(ids) => {
            selected = ids;
            if (ids.length) tab = 'clip';
          }}
          onrange={(r) => (range = r)}
          ontrim={(id, edge, rec) => void edit({ op: 'trim', clip_id: id, edge, rec_ns: rec })}
          onmove={(id, to) => void edit({ op: 'move_clip', id, to_index: to })}
        />
      {/if}
    </div>
    {#if error || toast}
      <div class="status" role="status">
        {#if error}<span class="err"><Icon name="warning" size={13} /> {error}</span>{/if}
        {#if toast && !error}
          {#if toast.startsWith('/') || /^[A-Z]:\\/.test(toast)}
            <span>Exported.</span>
            <button class="link" onclick={() => api.reveal(toast!)}>Show in Finder</button>
          {:else}
            <span>{toast}</span>
          {/if}
        {/if}
        <button class="plain icon" aria-label="Dismiss" onclick={() => ((error = null), (toast = null))}><Icon name="close" size={12} /></button>
      </div>
    {/if}
  </section>
</div>

<style>
  .editor {
    position: fixed;
    inset: 0;
    display: grid;
    grid-template-rows: 52px minmax(0, 1fr) auto;
    grid-template-columns: minmax(0, 1fr);
    background: transparent;
  }
  .toolbar {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 12px;
    padding: 0 12px 0 84px;
    background: var(--window);
    border-bottom: 1px solid var(--separator);
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }
  .name {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .transport {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .play {
    width: 34px;
    height: 34px;
    min-height: 34px;
    padding: 0;
    border-radius: 50%;
  }
  .tool {
    width: 30px;
    height: 26px;
  }
  .time {
    font-size: 13px;
    min-width: 130px;
  }
  .right {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 6px;
  }
  .menu-anchor {
    position: relative;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 10;
    display: flex;
    flex-direction: column;
    min-width: 170px;
    padding: 5px;
    border-radius: 8px;
    background: var(--window);
    box-shadow: var(--shadow-pop);
  }
  .menu button {
    justify-content: flex-start;
    padding: 4px 8px;
  }
  .menu button:hover {
    background: var(--accent);
    color: var(--on-accent);
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .bar {
    width: 110px;
    height: 6px;
    border-radius: 3px;
    background: var(--fill-2);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .middle {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 300px;
    min-height: 0;
    min-width: 0;
  }
  .stagewrap {
    position: relative;
    padding: 0;
    min-width: 0;
    min-height: 0;
  }
  .preview {
    position: absolute;
    inset: 0;
    background: #000;
  }
  .preview.native {
    background: transparent;
  }
  .live {
    position: absolute;
  }
  .live.hidden {
    visibility: hidden;
    width: 1px;
    height: 1px;
  }
  .cam {
    position: absolute;
    cursor: grab;
    outline: 1px dashed rgba(255, 255, 255, 0.7);
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
  .side {
    background: var(--window);
    border-left: 1px solid var(--separator);
    min-height: 0;
  }
  .bottom {
    min-width: 0;
    background: var(--window);
    border-top: 1px solid var(--separator);
    display: flex;
    flex-direction: column;
  }
  .tlbar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 5px 10px;
    border-bottom: 1px solid var(--separator);
  }
  .spacer {
    flex: 1;
  }
  .zoom {
    width: 110px;
  }
  .hint {
    margin-right: 8px;
  }
  .timeline {
    min-width: 0;
    height: 150px;
    padding: 0 0 6px;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 12px;
    border-top: 1px solid var(--separator);
    font-size: 12px;
  }
  .err {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--red);
  }
  .status .icon {
    margin-left: auto;
  }
</style>
