<script lang="ts">
  import {
    NS,
    formatTime,
    locate,
    recToTl,
    snapTo,
    speedLabel,
    starts as clipStarts,
    tickStep,
    type Clip,
    type TextItem,
  } from '../../lib/editor.ts';
  import type { CameraKey } from '../../lib/types.ts';

  export interface Thumbs { step_ms: number; count: number; w: number; h: number; ready: Set<number> }

  let {
    clips,
    text,
    keys,
    playhead,
    selected,
    range,
    thumbs,
    peaks,
    snapping = true,
    pxPerSec = $bindable(40),
    onseek,
    onselect,
    onrange,
    ontrim,
    onmove,
    onselecttext,
  }: {
    clips: Clip[];
    text: TextItem[];
    keys: CameraKey[];
    playhead: number;
    selected: string[];
    range: [number, number] | null;
    thumbs: Thumbs | null;
    peaks: Uint8Array | null;
    snapping?: boolean;
    pxPerSec?: number;
    onseek: (t: number, done: boolean) => void;
    onselect: (ids: string[]) => void;
    onrange: (r: [number, number] | null) => void;
    ontrim: (id: string, edge: 'start' | 'end', recNs: number) => void;
    onmove: (id: string, toIndex: number) => void;
    onselecttext?: (id: string) => void;
  } = $props();

  const RULER_H = 24;
  const TRACK_H = 64;
  const TEXT_H = 26;
  const PAD = 16;
  const MIN_CLIP_NS = 10_000_000;

  let scroller: HTMLDivElement | undefined = $state();
  let canvas: HTMLCanvasElement | undefined = $state();
  let viewW = $state(800);
  let scrollX = $state(0);
  const st = $derived(clipStarts(clips));
  const duration = $derived(st[st.length - 1] ?? 0);
  const contentW = $derived(PAD * 2 + (duration / NS) * pxPerSec);
  const x = (t: number) => PAD + (t / NS) * pxPerSec;
  const t = (px: number) => ((px - PAD) / pxPerSec) * NS;

  type Drag =
    | { kind: 'scrub' }
    | { kind: 'range'; from: number }
    | { kind: 'trim'; id: string; edge: 'start' | 'end'; startX: number; index: number }
    | { kind: 'move'; id: string; startX: number; index: number; moved: boolean };
  let drag = $state<Drag | null>(null);
  let ghost = $state<{ index: number; start: number; end: number } | null>(null);
  let dropAt = $state<number | null>(null);
  let pointerX = 0;

  const images = new Map<number, HTMLImageElement>();
  let imageTick = $state(0);

  function thumbImage(i: number): HTMLImageElement | null {
    if (!thumbs || !thumbs.ready.has(i)) return null;
    let img = images.get(i);
    if (!img) {
      img = new Image();
      img.onload = () => imageTick++;
      const base = /Windows|Android/.test(navigator.userAgent) ? 'http://sc.localhost/thumb' : 'sc://thumb';
      img.src = `${base}/${thumbs.step_ms}/${i}.png`;
      images.set(i, img);
    }
    return img.complete && img.naturalWidth > 0 ? img : null;
  }

  $effect(() => {
    if (thumbs === null) images.clear();
  });

  function snapTargets(): number[] {
    const out = [...st, playhead];
    for (const it of text) out.push(it.start_tl_ns, it.end_tl_ns);
    return out;
  }

  function snapped(time: number): number {
    if (!snapping) return time;
    return snapTo(time, snapTargets(), (8 / pxPerSec) * NS);
  }

  function localX(e: PointerEvent): number {
    const r = scroller!.getBoundingClientRect();
    return e.clientX - r.left + scroller!.scrollLeft;
  }

  function onRulerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    const at = Math.min(Math.max(snapped(t(localX(e))), 0), duration);
    if (e.shiftKey) {
      drag = { kind: 'range', from: at };
      onrange([at, at]);
      return;
    }
    drag = { kind: 'scrub' };
    onseek(at, false);
  }

  function onTrackDown(e: PointerEvent) {
    if (e.button !== 0) return;
    const at = Math.min(Math.max(t(localX(e)), 0), duration);
    if (e.shiftKey) {
      e.preventDefault();
      (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
      const s = snapped(at);
      drag = { kind: 'range', from: s };
      onrange([s, s]);
      return;
    }
    onselect([]);
    onrange(null);
    onseek(at, true);
  }

  function onClipDown(e: PointerEvent, c: Clip, index: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    if (e.shiftKey) {
      onTrackDown(e);
      return;
    }
    e.preventDefault();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    const additive = e.metaKey || e.ctrlKey;
    if (additive) onselect(selected.includes(c.id) ? selected.filter((s) => s !== c.id) : [...selected, c.id]);
    else if (!selected.includes(c.id)) onselect([c.id]);
    onrange(null);
    drag = { kind: 'move', id: c.id, startX: e.clientX, index, moved: false };
  }

  function onEdgeDown(e: PointerEvent, c: Clip, index: number, edge: 'start' | 'end') {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    onselect([c.id]);
    drag = { kind: 'trim', id: c.id, edge, startX: e.clientX, index };
    ghost = { index, start: st[index], end: st[index + 1] };
  }

  function trimRec(c: Clip, index: number, edge: 'start' | 'end', dxPx: number): number {
    const dTl = (dxPx / pxPerSec) * NS;
    const dRec = (dTl * c.speed.num) / c.speed.den;
    if (edge === 'start') {
      let rec = c.src_in_rec_ns + dRec;
      const tl = snapped(st[index] + dTl);
      if (tl !== st[index] + dTl) rec = c.src_in_rec_ns + ((tl - st[index]) * c.speed.num) / c.speed.den;
      return Math.round(Math.min(Math.max(rec, 0), c.src_out_rec_ns - MIN_CLIP_NS));
    }
    let rec = c.src_out_rec_ns + dRec;
    const tl = snapped(st[index + 1] + dTl);
    if (tl !== st[index + 1] + dTl) rec = c.src_out_rec_ns + ((tl - st[index + 1]) * c.speed.num) / c.speed.den;
    return Math.round(Math.max(rec, c.src_in_rec_ns + MIN_CLIP_NS));
  }

  function onMove(e: PointerEvent) {
    if (!drag || !scroller) return;
    pointerX = localX(e);
    const at = Math.min(Math.max(t(pointerX), 0), duration);
    if (drag.kind === 'scrub') onseek(snapped(at), false);
    else if (drag.kind === 'range') onrange([drag.from, snapped(at)]);
    else if (drag.kind === 'trim') {
      const c = clips[drag.index];
      const rec = trimRec(c, drag.index, drag.edge, e.clientX - drag.startX);
      const len = (r0: number, r1: number) => ((r1 - r0) * c.speed.den) / c.speed.num;
      ghost =
        drag.edge === 'start'
          ? { index: drag.index, start: st[drag.index], end: st[drag.index] + len(rec, c.src_out_rec_ns) }
          : { index: drag.index, start: st[drag.index], end: st[drag.index] + len(c.src_in_rec_ns, rec) };
    } else if (drag.kind === 'move') {
      if (!drag.moved && Math.abs(e.clientX - drag.startX) < 5) return;
      drag.moved = true;
      let best = 0;
      let bestD = Infinity;
      st.forEach((s, i) => {
        const d = Math.abs(x(s) - pointerX);
        if (d < bestD) {
          bestD = d;
          best = i;
        }
      });
      dropAt = best;
    }
  }

  function onUp(e: PointerEvent) {
    if (!drag) return;
    const d = drag;
    drag = null;
    if (d.kind === 'scrub') onseek(playhead, true);
    else if (d.kind === 'range' && range && Math.abs(range[1] - range[0]) < (3 / pxPerSec) * NS) onrange(null);
    else if (d.kind === 'trim') {
      const c = clips[d.index];
      const dx = e.clientX - d.startX;
      ghost = null;
      if (Math.abs(dx) >= 1) ontrim(c.id, d.edge, trimRec(c, d.index, d.edge, dx));
    } else if (d.kind === 'move') {
      const target = dropAt;
      dropAt = null;
      if (d.moved && target !== null) {
        const to = target > d.index ? target - 1 : target;
        if (to !== d.index) onmove(d.id, to);
      } else if (!d.moved) {
        const c = clips[d.index];
        const at = Math.min(Math.max(t(localX(e)), st[d.index]), st[d.index + 1] - 1);
        if (c) onseek(at, true);
      }
    }
  }

  function onwheel(e: WheelEvent) {
    if (!(e.ctrlKey || e.metaKey) || !scroller) return;
    e.preventDefault();
    const anchor = localX(e as unknown as PointerEvent);
    const atT = t(anchor);
    const next = Math.min(Math.max(pxPerSec * Math.exp(-e.deltaY / 200), 2), 600);
    pxPerSec = next;
    requestAnimationFrame(() => {
      if (scroller) scroller.scrollLeft = Math.max(0, PAD + (atT / NS) * next - (anchor - scroller.scrollLeft));
    });
  }

  $effect(() => {
    if (!scroller) return;
    const ro = new ResizeObserver(() => (viewW = scroller!.clientWidth));
    ro.observe(scroller);
    viewW = scroller.clientWidth;
    return () => ro.disconnect();
  });

  $effect(() => {
    if (!scroller || drag) return;
    const px = x(playhead);
    const left = scroller.scrollLeft;
    if (px > left + viewW - 40) scroller.scrollLeft = px - 80;
    else if (px < left) scroller.scrollLeft = Math.max(0, px - 80);
  });

  const ticks = $derived.by(() => {
    const step = tickStep(pxPerSec);
    const first = Math.max(0, Math.floor(t(scrollX) / step) * step);
    const last = Math.min(duration, t(scrollX + viewW) + step);
    const out: { t: number; label: string }[] = [];
    for (let v = first; v <= last; v += step) out.push({ t: v, label: step >= NS ? formatTime(v).replace(/\.\d$/, '') : formatTime(v) });
    return out;
  });

  const keyMarks = $derived(keys.flatMap((k) => recToTl(clips, st, k.t_rec_ns)));

  function draw() {
    if (!canvas) return;
    void imageTick;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, viewW);
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(TRACK_H * dpr);
    const g = canvas.getContext('2d');
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, TRACK_H);
    const thumbH = 42;
    const tileW = thumbs ? Math.max(24, (thumbs.w / thumbs.h) * thumbH) : 76;
    clips.forEach((c, i) => {
      const x0 = x(st[i]) - scrollX;
      const x1 = x(st[i + 1]) - scrollX;
      if (x1 < 0 || x0 > w) return;
      g.save();
      g.beginPath();
      g.roundRect(x0 + 1, 2, Math.max(1, x1 - x0 - 2), TRACK_H - 4, 6);
      g.clip();
      g.fillStyle = 'rgba(128,128,140,0.28)';
      g.fillRect(x0, 0, x1 - x0, TRACK_H);
      for (let tx = Math.max(x0, x0 + Math.floor((0 - x0) / tileW) * tileW); tx < Math.min(x1, w); tx += tileW) {
        const mid = st[i] + ((tx + tileW / 2 - x0) / pxPerSec) * NS;
        const loc = locate(clips, st, Math.min(mid, st[i + 1] - 1));
        if (!loc || !thumbs) continue;
        const idx = Math.min(thumbs.count - 1, Math.max(0, Math.round(loc.rec_ns / (thumbs.step_ms * 1e6))));
        const img = thumbImage(idx);
        if (img) g.drawImage(img, tx, 2, tileW, thumbH);
      }
      if (peaks) {
        const base = TRACK_H - 3;
        const hMax = 16;
        g.fillStyle = c.muted ? 'rgba(160,160,170,0.35)' : 'rgba(64,156,255,0.85)';
        g.fillRect(x0, base - hMax - 1, x1 - x0, hMax + 1);
        g.fillStyle = c.muted ? 'rgba(220,220,230,0.5)' : 'rgba(255,255,255,0.9)';
        const from = Math.max(x0, 0);
        const to = Math.min(x1, w);
        for (let px = from; px < to; px += 2) {
          const tl = st[i] + ((px - x0) / pxPerSec) * NS;
          const loc = locate(clips, st, Math.min(tl, st[i + 1] - 1));
          if (!loc) continue;
          const bin = Math.floor(loc.rec_ns / 1e7);
          const span = Math.max(1, Math.ceil(((2 / pxPerSec) * 100 * c.speed.num) / c.speed.den));
          let m = 0;
          for (let b = bin; b < bin + span && b < peaks.length; b++) m = Math.max(m, peaks[b]);
          const h = (m / 255) * hMax;
          g.fillRect(px, base - h, 1.5, h);
        }
      }
      g.restore();
    });
  }

  $effect(() => {
    void [clips, scrollX, viewW, pxPerSec, thumbs, peaks, imageTick];
    draw();
  });
</script>

<svelte:window onpointermove={onMove} onpointerup={onUp} />

<div
  class="scroller"
  bind:this={scroller}
  onscroll={() => (scrollX = scroller!.scrollLeft)}
  {onwheel}
>
  <div class="content" style:width="{Math.max(contentW, viewW)}px">
    <div class="ruler" style:height="{RULER_H}px" role="slider" aria-label="Playhead" aria-valuenow={playhead} tabindex="-1" onpointerdown={onRulerDown}>
      {#each ticks as tk (tk.t)}
        <span class="tick" style:left="{x(tk.t)}px"><span class="tlabel">{tk.label}</span></span>
      {/each}
    </div>
    <div class="track main" style:height="{TRACK_H}px" role="presentation" onpointerdown={onTrackDown}>
      <canvas class="paint" bind:this={canvas} style:left="{scrollX}px" style:width="{viewW}px" style:height="{TRACK_H}px"></canvas>
      {#each clips as c, i (c.id)}
        {@const g = ghost && ghost.index === i ? ghost : null}
        <div
          class="clip"
          class:sel={selected.includes(c.id)}
          class:muted={c.muted}
          class:dragging={drag?.kind === 'move' && drag.id === c.id && drag.moved}
          style:left="{x(g ? g.start : st[i])}px"
          style:width="{Math.max(2, x(g ? g.end : st[i + 1]) - x(g ? g.start : st[i]))}px"
          role="button"
          tabindex="-1"
          aria-label="Clip {i + 1}"
          onpointerdown={(e) => onClipDown(e, c, i)}
        >
          {#if c.speed.num !== c.speed.den}<span class="badge chip">{speedLabel(c.speed)}</span>{/if}
          {#if c.muted}<span class="badge chip">Muted</span>{/if}
          <span class="edge l" role="presentation" onpointerdown={(e) => onEdgeDown(e, c, i, 'start')}></span>
          <span class="edge r" role="presentation" onpointerdown={(e) => onEdgeDown(e, c, i, 'end')}></span>
        </div>
      {/each}
      {#each keyMarks as k, i (i)}
        <span class="key" style:left="{x(k)}px" title="Camera keyframe"></span>
      {/each}
      {#if dropAt !== null}<span class="drop" style:left="{x(st[dropAt])}px"></span>{/if}
    </div>
    <div class="track text" style:height="{TEXT_H}px" role="presentation" onpointerdown={onTrackDown}>
      {#if text.length === 0}<span class="lane-hint" style:left="{scrollX + 10}px">Text</span>{/if}
      {#each text as it (it.id)}
        <button
          class="titem"
          style:left="{x(it.start_tl_ns)}px"
          style:width="{Math.max(6, x(it.end_tl_ns) - x(it.start_tl_ns))}px"
          onpointerdown={(e) => e.stopPropagation()}
          onclick={() => onselecttext?.(it.id)}
        >{it.content}</button>
      {/each}
    </div>
    {#if range}
      <div class="range" style:left="{x(Math.min(...range))}px" style:width="{Math.abs(x(range[1]) - x(range[0]))}px"></div>
    {/if}
    <div class="playhead" style:left="{x(playhead)}px"><span class="head"></span></div>
  </div>
</div>

<style>
  .scroller {
    position: relative;
    overflow-x: auto;
    overflow-y: hidden;
    height: 100%;
  }
  .content {
    position: relative;
    height: 100%;
    min-height: 100%;
  }
  .ruler {
    position: relative;
    border-bottom: 1px solid var(--separator);
    cursor: col-resize;
  }
  .tick {
    position: absolute;
    bottom: 0;
    height: 7px;
    border-left: 1px solid var(--label-3);
  }
  .tlabel {
    position: absolute;
    left: 4px;
    bottom: 5px;
    font-size: 10px;
    color: var(--label-2);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .track {
    position: relative;
    margin-top: 6px;
  }
  .paint {
    position: absolute;
    top: 0;
    pointer-events: none;
  }
  .clip {
    position: absolute;
    top: 2px;
    bottom: 2px;
    border-radius: 6px;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.25);
    cursor: grab;
    display: flex;
    align-items: flex-start;
    gap: 3px;
    padding: 3px 6px;
  }
  .clip.sel {
    box-shadow: inset 0 0 0 2px var(--yellow), 0 0 0 1px rgba(0, 0, 0, 0.35);
  }
  .clip.dragging {
    opacity: 0.6;
    cursor: grabbing;
  }
  .chip {
    font-size: 10px;
    padding: 0 5px;
    background: rgba(0, 0, 0, 0.55);
    color: #fff;
  }
  .edge {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 8px;
    cursor: ew-resize;
  }
  .edge.l {
    left: -2px;
    border-radius: 6px 0 0 6px;
  }
  .edge.r {
    right: -2px;
    border-radius: 0 6px 6px 0;
  }
  .clip.sel .edge {
    background: var(--yellow);
    width: 6px;
  }
  .clip.sel .edge.l {
    left: 0;
  }
  .clip.sel .edge.r {
    right: 0;
  }
  .key {
    position: absolute;
    bottom: -4px;
    width: 8px;
    height: 8px;
    margin-left: -4px;
    transform: rotate(45deg);
    background: var(--orange);
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.3);
    pointer-events: none;
  }
  .drop {
    position: absolute;
    top: -4px;
    bottom: -4px;
    width: 3px;
    margin-left: -1.5px;
    border-radius: 2px;
    background: var(--accent);
    pointer-events: none;
  }
  .track.text {
    border-radius: 6px;
    background: var(--fill);
  }
  .lane-hint {
    position: absolute;
    top: 5px;
    font-size: 11px;
    color: var(--label-3);
    pointer-events: none;
  }
  .titem {
    position: absolute;
    top: 2px;
    bottom: 2px;
    min-height: 0;
    padding: 0 6px;
    border-radius: 5px;
    background: color-mix(in srgb, var(--accent) 70%, transparent);
    color: #fff;
    font-size: 11px;
    justify-content: flex-start;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .range {
    position: absolute;
    top: 0;
    bottom: 0;
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border-left: 1px solid var(--accent);
    border-right: 1px solid var(--accent);
    pointer-events: none;
  }
  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--red);
    pointer-events: none;
  }
  .head {
    position: absolute;
    top: 0;
    left: -5px;
    width: 12px;
    height: 12px;
    border-radius: 2px 2px 6px 6px;
    background: var(--red);
  }
</style>
