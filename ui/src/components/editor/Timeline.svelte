<script lang="ts">
  import {
    NS,
    formatTime,
    locate,
    recToTl,
    snapTo,
    stackRows,
    speedLabel,
    starts as clipStarts,
    tickStep,
    type Clip,
    onTimeline,
    type Redaction,
    type TextItem,
  } from '../../lib/editor.ts';
  import { scUrl } from '../../lib/format.ts';
  import type { CameraKey } from '../../lib/types.ts';
  import Icon from '../Icon.svelte';

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
    ontextmove,
    selectedText = null,
    marks = [],
    blurs = [],
    selectedBlur = null,
    onselectblur,
    onblurmove,
    cameraClips = null,
    cameraPeaks = null,
    linked = true,
    onlink,
    onslide,
    height = $bindable(150),
  }: {
    cameraClips?: Clip[] | null;
    cameraPeaks?: Uint8Array | null;
    linked?: boolean;
    onlink?: (linked: boolean) => void;
    onslide?: (id: string, byNs: number) => void;
    height?: number;
    clips: Clip[];
    text: TextItem[];
    keys: CameraKey[];
    playhead: number;
    selected: string[];
    range: [number, number] | null;
    marks?: [number, number][];
    blurs?: Redaction[];
    selectedBlur?: string | null;
    onselectblur?: (id: string) => void;
    onblurmove?: (id: string, start: number, end: number) => void;
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
    ontextmove?: (id: string, start: number, end: number) => void;
    selectedText?: string | null;
  } = $props();

  const RULER_H = 24;
  const TRACK_H = 64;
  const CAM_H = TRACK_H;
  const TEXT_H = 26;
  const BLUR_H = 20;
  const PAD = 16;
  const MIN_CLIP_NS = 10_000_000;

  let scroller: HTMLDivElement | undefined = $state();
  let canvas: HTMLCanvasElement | undefined = $state();
  let camCanvas: HTMLCanvasElement | undefined = $state();
  let viewW = $state(800);
  let scrollX = $state(0);
  type Lane = 'screen' | 'camera';
  const st = $derived(clipStarts(clips));
  const camSt = $derived(cameraClips ? clipStarts(cameraClips) : []);
  const laneClips = (l: Lane) => (l === 'camera' ? (cameraClips ?? []) : clips);
  const laneSt = (l: Lane) => (l === 'camera' ? camSt : st);
  const duration = $derived(st[st.length - 1] ?? 0);
  const contentW = $derived(PAD * 2 + (duration / NS) * pxPerSec);
  const x = (t: number) => PAD + (t / NS) * pxPerSec;
  const t = (px: number) => ((px - PAD) / pxPerSec) * NS;

  type Drag =
    | { kind: 'scrub' }
    | { kind: 'range'; from: number }
    | { kind: 'trim'; lane: Lane; id: string; edge: 'start' | 'end'; startX: number; index: number }
    | { kind: 'move'; lane: Lane; id: string; startX: number; index: number; moved: boolean }
    | { kind: 'text'; lane: 'text' | 'blur'; id: string; part: 'body' | 'start' | 'end'; startX: number; s0: number; e0: number; moved: boolean };
  let drag = $state<Drag | null>(null);
  let ghost = $state<{ lane: Lane; index: number; start: number; end: number } | null>(null);
  let slide = $state<{ id: string; by: number } | null>(null);
  let textGhost = $state<{ id: string; start: number; end: number } | null>(null);
  const MIN_TEXT_NS = 100_000_000;

  type BlurItem = { id: string; blur: string; style: Redaction['style']; start_tl_ns: number; end_tl_ns: number };
  const blurItems = $derived<BlurItem[]>(
    blurs.flatMap((b) =>
      onTimeline(clips, b).map(([s, e], k) => ({ id: `${b.id}#${k}`, blur: b.id, style: b.style, start_tl_ns: s, end_tl_ns: e })),
    ),
  );

  const textStack = $derived(stackRows(text));
  const blurStack = $derived(stackRows(blurItems));
  const textLaneH = $derived(textStack.rows * TEXT_H);
  const blurLaneH = $derived(blurStack.rows * BLUR_H);
  $effect(() => {
    height =
      RULER_H + 6 + TRACK_H + (cameraClips ? 6 + CAM_H : 0) + 6 + textLaneH + (blurItems.length ? 6 + blurLaneH : 0) + 24;
  });

  function onTextDown(e: PointerEvent, it: TextItem | BlurItem, part: 'body' | 'start' | 'end', lane: 'text' | 'blur' = 'text') {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    if (lane === 'blur') onselectblur?.((it as BlurItem).blur);
    else onselecttext?.(it.id);
    drag = { kind: 'text', lane, id: it.id, part, startX: e.clientX, s0: it.start_tl_ns, e0: it.end_tl_ns, moved: false };
  }
  let dropAt = $state<number | null>(null);
  let pointerX = 0;

  const images = new Map<number, HTMLImageElement>();
  const failures = new Map<number, number>();
  let imageTick = $state(0);

  function thumbImage(i: number): HTMLImageElement | null {
    if (!thumbs || !thumbs.ready.has(i)) return null;
    let img = images.get(i);
    if (!img) {
      img = new Image();
      img.onload = () => imageTick++;
      img.onerror = () => {
        const n = (failures.get(i) ?? 0) + 1;
        failures.set(i, n);
        if (n < 3) {
          images.delete(i);
          setTimeout(() => imageTick++, 500);
        }
      };
      img.src = `${scUrl('thumb')}/${thumbs.step_ms}/${i}.png`;
      images.set(i, img);
    }
    return img.complete && img.naturalWidth > 0 ? img : null;
  }

  $effect(() => {
    if (thumbs === null) {
      images.clear();
      failures.clear();
    }
  });

  function snapTargets(): number[] {
    const out = [...st, ...camSt, playhead];
    for (const it of [...text, ...blurItems]) out.push(it.start_tl_ns, it.end_tl_ns);
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

  function onClipDown(e: PointerEvent, c: Clip, index: number, lane: Lane) {
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
    drag = { kind: 'move', lane, id: c.id, startX: e.clientX, index, moved: false };
  }

  function onEdgeDown(e: PointerEvent, c: Clip, index: number, edge: 'start' | 'end', lane: Lane) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    onselect([c.id]);
    drag = { kind: 'trim', lane, id: c.id, edge, startX: e.clientX, index };
    const lst = laneSt(lane);
    ghost = { lane, index, start: lst[index], end: lst[index + 1] };
  }

  function trimRec(c: Clip, index: number, edge: 'start' | 'end', dxPx: number, lane: Lane): number {
    const st = laneSt(lane);
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
      const { lane, index } = drag;
      const c = laneClips(lane)[index];
      const lst = laneSt(lane);
      const rec = trimRec(c, index, drag.edge, e.clientX - drag.startX, lane);
      const len = (r0: number, r1: number) => ((r1 - r0) * c.speed.den) / c.speed.num;
      ghost =
        drag.edge === 'end'
          ? { lane, index, start: lst[index], end: lst[index] + len(c.src_in_rec_ns, rec) }
          : linked
            ? { lane, index, start: lst[index], end: lst[index] + len(rec, c.src_out_rec_ns) }
            : { lane, index, start: lst[index] + len(c.src_in_rec_ns, rec), end: lst[index + 1] };
    } else if (drag.kind === 'text') {
      if (!drag.moved && Math.abs(e.clientX - drag.startX) < 3) return;
      drag.moved = true;
      const dt = ((e.clientX - drag.startX) / pxPerSec) * NS;
      let s = drag.s0;
      let en = drag.e0;
      if (drag.part === 'body') {
        const len = drag.e0 - drag.s0;
        s = Math.min(Math.max(snapped(drag.s0 + dt), 0), Math.max(0, duration - len));
        en = s + len;
      } else if (drag.part === 'start') s = Math.min(Math.max(snapped(drag.s0 + dt), 0), drag.e0 - MIN_TEXT_NS);
      else en = Math.max(Math.min(snapped(drag.e0 + dt), duration), drag.s0 + MIN_TEXT_NS);
      textGhost = { id: drag.id, start: s, end: en };
    } else if (drag.kind === 'move') {
      if (!drag.moved && Math.abs(e.clientX - drag.startX) < 5) return;
      drag.moved = true;
      if (!linked) {
        const s0 = laneSt(drag.lane)[drag.index];
        slide = { id: drag.id, by: Math.max(0, snapped(s0 + ((e.clientX - drag.startX) / pxPerSec) * NS)) - s0 };
        return;
      }
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
    if (d.kind === 'text') {
      const g = textGhost;
      textGhost = null;
      if (d.moved && g) (d.lane === 'blur' ? onblurmove : ontextmove)?.(g.id, Math.round(g.start), Math.round(g.end));
      return;
    }
    if (d.kind === 'scrub') onseek(playhead, true);
    else if (d.kind === 'range' && range && Math.abs(range[1] - range[0]) < (3 / pxPerSec) * NS) onrange(null);
    else if (d.kind === 'trim') {
      const c = laneClips(d.lane)[d.index];
      const dx = e.clientX - d.startX;
      ghost = null;
      if (Math.abs(dx) >= 1) ontrim(c.id, d.edge, trimRec(c, d.index, d.edge, dx, d.lane));
    } else if (d.kind === 'move') {
      const target = dropAt;
      const slid = slide;
      dropAt = null;
      slide = null;
      if (d.moved && slid) {
        if (Math.round(slid.by) !== 0) onslide?.(d.id, Math.round(slid.by));
      } else if (d.moved && target !== null) {
        const to = target > d.index ? target - 1 : target;
        if (to !== d.index) onmove(d.id, to);
      } else if (!d.moved) {
        const lst = laneSt(d.lane);
        const at = Math.min(Math.max(t(localX(e)), lst[d.index]), Math.min(lst[d.index + 1], duration) - 1);
        onseek(Math.max(0, at), true);
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

  const keyMarks = $derived(
    cameraClips ? keys.flatMap((k) => recToTl(cameraClips, camSt, k.t_rec_ns)) : keys.flatMap((k) => recToTl(clips, st, k.t_rec_ns)),
  );

  function draw() {
    void imageTick;
    if (canvas) paint(canvas, 'screen', TRACK_H);
    if (camCanvas && cameraClips) paint(camCanvas, 'camera', CAM_H);
  }

  function paint(cv: HTMLCanvasElement, lane: Lane, H: number) {
    const clips = laneClips(lane);
    const st = laneSt(lane);
    const wave = lane === 'camera' ? cameraPeaks : peaks;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, viewW);
    const pw = Math.round(w * dpr);
    const ph = Math.round(H * dpr);
    if (cv.width !== pw) cv.width = pw;
    if (cv.height !== ph) cv.height = ph;
    const g = cv.getContext('2d');
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, H);
    const thumbH = 42;
    const tileW = thumbs ? Math.max(24, (thumbs.w / thumbs.h) * thumbH) : 76;
    clips.forEach((c, i) => {
      if (c.hidden) return;
      const x0 = x(st[i]) - scrollX;
      const x1 = x(Math.min(st[i + 1], lane === 'camera' ? duration : Infinity)) - scrollX;
      if (x1 < 0 || x0 > w || x1 <= x0) return;
      g.save();
      g.beginPath();
      g.roundRect(x0 + 1, 2, Math.max(1, x1 - x0 - 2), H - 4, 6);
      g.clip();
      g.fillStyle = lane === 'camera' ? 'rgba(48,176,199,0.32)' : 'rgba(128,128,140,0.28)';
      g.fillRect(x0, 0, x1 - x0, H);
      for (let tx = Math.max(x0, x0 + Math.floor((0 - x0) / tileW) * tileW); lane === 'screen' && tx < Math.min(x1, w); tx += tileW) {
        const mid = st[i] + ((tx + tileW / 2 - x0) / pxPerSec) * NS;
        const loc = locate(clips, st, Math.min(mid, st[i + 1] - 1));
        if (!loc || !thumbs) continue;
        const idx = Math.min(thumbs.count - 1, Math.max(0, Math.round(loc.rec_ns / (thumbs.step_ms * 1e6))));
        const img = thumbImage(idx);
        if (img) g.drawImage(img, tx, 2, tileW, thumbH);
      }
      if (wave) {
        const peaks = wave;
        const TRACK_H = H;
        const base = TRACK_H - 3;
        const hMax = 16;
        g.fillStyle = c.muted ? 'rgba(160,160,170,0.35)' : 'rgba(47,160,111,0.85)';
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
    void [clips, cameraClips, camCanvas, scrollX, viewW, pxPerSec, thumbs, peaks, cameraPeaks, imageTick];
    draw();
  });
</script>

{#snippet boxes(lane: Lane, list: Clip[], lst: number[])}
  {#each list as c, i (c.id)}
    {@const g = ghost && ghost.lane === lane && ghost.index === i ? ghost : null}
    {@const by = slide && slide.id === c.id ? slide.by : 0}
    {@const s0 = (g ? g.start : lst[i]) + by}
    {@const e0 = Math.min((g ? g.end : lst[i + 1]) + by, lane === 'camera' ? Math.max(duration, s0) : Infinity)}
    {#if e0 > s0}
      <div
        class="clip"
        class:gap={c.hidden}
        class:sel={selected.includes(c.id)}
        class:muted={c.muted}
        class:dragging={drag?.kind === 'move' && drag.id === c.id && drag.moved}
        style:left="{x(s0)}px"
        style:width="{Math.max(2, x(e0) - x(s0))}px"
        role="button"
        tabindex="-1"
        aria-label={c.hidden ? 'Gap' : `${lane === 'camera' ? 'Camera' : 'Screen'} clip ${i + 1}`}
        title={c.hidden ? 'Gap: nothing plays here. Delete it to close it up.' : undefined}
        onpointerdown={(e) => onClipDown(e, c, i, lane)}
      >
        {#if c.hidden}
          <span class="gaplabel">Gap</span>
        {:else}
          {#if c.speed.num !== c.speed.den}<span class="badge chip">{speedLabel(c.speed)}</span>{/if}
          {#if c.muted}<span class="badge chip">Muted</span>{/if}
          <span class="edge l" role="presentation" onpointerdown={(e) => onEdgeDown(e, c, i, 'start', lane)}></span>
          <span class="edge r" role="presentation" onpointerdown={(e) => onEdgeDown(e, c, i, 'end', lane)}></span>
        {/if}
      </div>
    {/if}
  {/each}
{/snippet}

<svelte:window onpointermove={onMove} onpointerup={onUp} />

<div class="tl">
  <div class="heads" aria-hidden={!cameraClips}>
    <div style:height="{RULER_H}px"></div>
    <div class="head" style:height="{TRACK_H}px" style:margin-top="6px">
      <span>{cameraClips ? 'Screen' : 'Video'}</span>
      <span class="sub">{cameraClips ? 'System audio' : 'and sound'}</span>
    </div>
    {#if cameraClips}
      <button
        class="link"
        class:off={!linked}
        style:top="{RULER_H + 6 + TRACK_H - 8}px"
        aria-pressed={linked}
        aria-label={linked ? 'Screen and camera are linked' : 'Screen and camera are separate'}
        title={linked
          ? 'Linked: cuts and moves apply to the screen and the camera together. Click to edit them separately.'
          : 'Separate: cuts and moves apply to one track. Click to link them again (gaps in the screen close up).'}
        onclick={() => onlink?.(!linked)}
      >
        <Icon name={linked ? 'link' : 'unlink'} size={13} />
      </button>
      <div class="head" style:height="{CAM_H}px" style:margin-top="6px">
        <span>Camera</span>
        <span class="sub">Microphone</span>
      </div>
    {/if}
    <div class="head" style:height="{textLaneH}px" style:margin-top="6px"><span>Text</span></div>
    {#if blurItems.length}
      <div class="head" style:height="{blurLaneH}px" style:margin-top="6px"><span>Blur</span></div>
    {/if}
  </div>
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
      {@render boxes('screen', clips, st)}
      {#if !cameraClips}
        {#each keyMarks as k, i (i)}
          <span class="key" style:left="{x(k)}px" title="Camera keyframe"></span>
        {/each}
      {/if}
      {#if dropAt !== null}<span class="drop" style:left="{x(st[dropAt])}px"></span>{/if}
    </div>
    {#if cameraClips}
      <div class="track cam" style:height="{CAM_H}px" role="presentation" onpointerdown={onTrackDown}>
        <canvas class="paint" bind:this={camCanvas} style:left="{scrollX}px" style:width="{viewW}px" style:height="{CAM_H}px"></canvas>
        {@render boxes('camera', cameraClips, camSt)}
        {#each keyMarks as k, i (i)}
          <span class="key" style:left="{x(k)}px" title="Camera keyframe"></span>
        {/each}
      </div>
    {/if}
    <div class="track text" style:height="{textLaneH}px" role="presentation" onpointerdown={onTrackDown}>
      {#each text as it (it.id)}
        {@const g = textGhost && textGhost.id === it.id ? textGhost : null}
        <div
          class="titem"
          class:sel={selectedText === it.id}
          style:top="{(textStack.row.get(it.id) ?? 0) * TEXT_H + 2}px"
          style:height="{TEXT_H - 4}px"
          style:left="{x(g ? g.start : it.start_tl_ns)}px"
          style:width="{Math.max(6, x(g ? g.end : it.end_tl_ns) - x(g ? g.start : it.start_tl_ns))}px"
          role="button"
          tabindex="-1"
          aria-label="Text {it.content}"
          onpointerdown={(e) => onTextDown(e, it, 'body')}
        >
          <span class="tlabel2">{it.content.split('\n')[0]}</span>
          <span class="tedge l" role="presentation" onpointerdown={(e) => onTextDown(e, it, 'start')}></span>
          <span class="tedge r" role="presentation" onpointerdown={(e) => onTextDown(e, it, 'end')}></span>
        </div>
      {/each}
    </div>
    {#if blurItems.length}
      <div class="track text blurs" style:height="{blurLaneH}px" role="presentation" onpointerdown={onTrackDown}>
        {#each blurItems as it (it.id)}
          {@const g = textGhost && textGhost.id === it.id ? textGhost : null}
          <div
            class="titem blur"
            class:sel={selectedBlur === it.blur}
            style:top="{(blurStack.row.get(it.id) ?? 0) * BLUR_H + 2}px"
            style:height="{BLUR_H - 4}px"
            style:left="{x(g ? g.start : it.start_tl_ns)}px"
            style:width="{Math.max(6, x(g ? g.end : it.end_tl_ns) - x(g ? g.start : it.start_tl_ns))}px"
            role="button"
            tabindex="-1"
            aria-label={it.style === 'black' ? 'Black box' : 'Blur box'}
            onpointerdown={(e) => onTextDown(e, it, 'body', 'blur')}
          >
            <span class="tlabel2">{it.style === 'black' ? 'Black' : 'Blur'}</span>
            <span class="tedge l" role="presentation" onpointerdown={(e) => onTextDown(e, it, 'start', 'blur')}></span>
            <span class="tedge r" role="presentation" onpointerdown={(e) => onTextDown(e, it, 'end', 'blur')}></span>
          </div>
        {/each}
      </div>
    {/if}
    {#each marks as m (m[0])}
      <div class="mark" style:left="{x(m[0])}px" style:width="{Math.max(1, x(m[1]) - x(m[0]))}px"></div>
    {/each}
    {#if range}
      <div class="range" style:left="{x(Math.min(...range))}px" style:width="{Math.abs(x(range[1]) - x(range[0]))}px"></div>
    {/if}
    <div class="playhead" style:left="{x(playhead)}px"><span class="phead"></span></div>
  </div>
</div>
</div>

<style>
  .tl {
    display: flex;
    min-height: 100%;
    min-width: 0;
  }
  .heads {
    position: relative;
    flex: none;
    width: 84px;
    padding: 0 6px 0 12px;
    border-right: 1px solid var(--separator);
  }
  .head {
    display: flex;
    flex-direction: column;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    color: var(--label);
    line-height: 1.2;
  }
  .head .sub {
    font-weight: 400;
    font-size: 10px;
    color: var(--label-2);
  }
  .link {
    position: absolute;
    right: 6px;
    width: 22px;
    min-width: 22px;
    height: 22px;
    padding: 0;
    border-radius: 50%;
    z-index: 1;
    color: var(--accent-ink);
    background: var(--control, rgba(128, 128, 128, 0.18));
  }
  .link.off {
    color: var(--label-2);
  }
  .scroller {
    flex: 1;
    min-width: 0;
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
  .clip.gap {
    cursor: default;
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--separator);
    background-image: repeating-linear-gradient(135deg, transparent 0 6px, color-mix(in srgb, var(--label-3) 25%, transparent) 6px 7px);
    align-items: center;
    justify-content: center;
  }
  .clip.gap.sel {
    box-shadow: inset 0 0 0 2px var(--yellow);
  }
  .gaplabel {
    font-size: 10px;
    color: var(--label-2);
    pointer-events: none;
  }
  .track.text {
    border-radius: 6px;
    background: var(--fill);
  }
  .titem {
    position: absolute;
    display: flex;
    align-items: center;
    padding: 0 8px;
    border-radius: 5px;
    background: color-mix(in srgb, var(--accent) 70%, transparent);
    color: #fff;
    font-size: 11px;
    overflow: hidden;
    cursor: grab;
  }
  .titem.blur {
    background: color-mix(in srgb, var(--label-2) 55%, transparent);
  }
  .titem.sel {
    box-shadow: inset 0 0 0 2px var(--yellow);
  }
  .tlabel2 {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none;
  }
  .tedge {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 7px;
    cursor: ew-resize;
  }
  .tedge.l {
    left: 0;
  }
  .tedge.r {
    right: 0;
  }
  .mark {
    position: absolute;
    top: 0;
    bottom: 0;
    background: color-mix(in srgb, var(--red, #d93025) 28%, transparent);
    pointer-events: none;
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
  .phead {
    position: absolute;
    top: 0;
    left: -5px;
    width: 12px;
    height: 12px;
    border-radius: 2px 2px 6px 6px;
    background: var(--red);
  }
</style>
