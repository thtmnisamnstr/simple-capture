<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { onEvent } from '../lib/events.ts';
  import { isHealth, isNotice, isRecordingState } from '../lib/guards.ts';
  import type { NoticeDto, RecordingState } from '../lib/types.ts';
  import { formatBytes, formatElapsed } from '../lib/format.ts';
  import { beginPick, showWindow } from '../lib/windows.ts';
  import { call as appCall } from '../lib/appinfo.ts';
  import { isObject } from '../lib/guards.ts';
  import type { HealthDto } from '../lib/types.ts';
  import Icon from '../components/Icon.svelte';
  import { hostOs } from '../lib/format.ts';
  import { keysLabel, shortcutsApi } from '../lib/shortcuts.ts';

  let rec = $state<RecordingState>({ state: 'idle' });
  let keys = $state<Record<string, string>>({});
  const keyHint = (id: string) => (keys[id] ? ` (${keysLabel(keys[id], hostOs() === 'macos')})` : '');

  $effect(() => {
    const load = () =>
      shortcutsApi
        .list()
        .then((rows) => (keys = Object.fromEntries(rows.map((r) => [r.id, r.keys]))))
        .catch(() => undefined);
    load();
    return onEvent('settings-changed', isObject, () => load());
  });
  let elapsed = $state(0);
  let base = { ns: 0, at: 0, running: false };
  let dragging = false;
  let pendingMove: { x: number; y: number } | null = null;

  function gripDown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    dragging = true;
    api.barDrag('begin', e.screenX, e.screenY).catch((err) => (error = errorText(err)));
  }

  function gripMove(e: PointerEvent) {
    if (!dragging) return;
    const first = pendingMove === null;
    pendingMove = { x: e.screenX, y: e.screenY };
    if (first)
      requestAnimationFrame(() => {
        const p = pendingMove;
        pendingMove = null;
        if (p) api.barDrag('move', p.x, p.y).catch((err) => (error = errorText(err)));
      });
  }

  function gripUp() {
    if (!dragging) return;
    dragging = false;
    api.barDrag('end', 0, 0).catch((err) => (error = errorText(err)));
  }
  let paused = $state(false);
  let countdown = $state(0);
  let restarting = $state(false);
  let debug = $state(false);
  let lastHealth = $state<HealthDto | null>(null);
  let micDb = $state<number | null>(null);
  let micTimer: ReturnType<typeof setTimeout> | undefined;
  let notice = $state<NoticeDto | null>(null);
  let sourceLost = $state(false);
  let confirming = $state(false);
  let error = $state<string | null>(null);
  let dismissTimer: ReturnType<typeof setTimeout> | undefined;

  const label = $derived(
    rec.state === 'preparing'
      ? 'Starting…'
      : rec.state === 'switching'
        ? 'Switching…'
        : rec.state === 'stopping'
          ? 'Saving…'
          : rec.state === 'failed'
            ? 'Recording failed'
            : paused
              ? 'Paused'
              : 'Recording',
  );
  const live = $derived(rec.state === 'recording' || rec.state === 'switching');

  function noticeText(n: NoticeDto): string {
    switch (n.kind) {
      case 'frames_dropping': return 'Frames are dropping: try 30 fps or 1× next time';
      case 'switch_failed': return n.restored ? `Switch failed, still recording the previous source (${n.error})` : `Switch failed: ${n.error}`;
      case 'device_lost': return `${n.device === 'camera' ? 'Camera' : 'Microphone'} “${n.name}” disconnected`;
      case 'disk_low': return `Disk almost full: ${formatBytes(n.bytes_left)} left`;
      case 'audio_gap': return `${n.track === 'mic' ? 'Microphone' : 'System audio'} dropped out for ${(n.dur_ns / 1e9).toFixed(1)} s`;
      case 'hotkey_unavailable': return `Some shortcuts are unavailable: ${n.error}`;
      case 'paused': return n.paused ? 'Paused' : 'Recording';
      case 'export_failed': return `Export failed: ${n.error}`;
      case 'stopped_for_sleep': return 'Stopped because the Mac went to sleep';
      case 'stopped_for_disk': return 'Stopped because the disk is full';
      case 'source_lost': return 'Source closed';
    }
  }

  $effect(() => {
    const offs = [
      onEvent('recording-state', isRecordingState, (s) => {
        rec = s;
        if (s.state === 'recording' || s.state === 'switching') {
          if (!base.running || s.elapsed_ns > elapsed) base = { ns: s.elapsed_ns, at: performance.now(), running: true };
        } else {
          base.running = false;
        }
        if (s.state === 'preparing') {
          elapsed = 0;
          paused = false;
          countdown = 0;
          restarting = false;
          sourceLost = false;
          notice = null;
          confirming = false;
        }
        if (s.state === 'switching') sourceLost = false;
      }),
      onEvent('health', isHealth, (h) => {
        lastHealth = h;
        if (base.running && !paused) base = { ns: h.elapsed_ns ?? h.rec_ns, at: performance.now(), running: true };
      }),
      onEvent('recording-paused', isObject, (p) => {
        paused = p.paused === true;
        const ns = typeof p.elapsed_ns === 'number' ? p.elapsed_ns : elapsed;
        elapsed = ns;
        base = { ns, at: performance.now(), running: true };
      }),
      onEvent('mic-level', isObject, (m) => {
        if (typeof m.peak_db !== 'number') return;
        micDb = micDb === null ? m.peak_db : Math.max(m.peak_db, micDb - 3);
        clearTimeout(micTimer);
        micTimer = setTimeout(() => (micDb = null), 1200);
      }),
      onEvent('countdown', isObject, (c) => {
        countdown = typeof c.left === 'number' ? c.left : 0;
      }),
      onEvent('settings-changed', isObject, (st) => {
        debug = st.debug_overlay === true;
      }),
      onEvent('notice', isNotice, (n) => {
        if (n.kind === 'source_lost') {
          sourceLost = true;
          return;
        }
        notice = n;
        clearTimeout(dismissTimer);
        dismissTimer = setTimeout(() => (notice = null), 8000);
      }),
    ];
    api
      .getSettings()
      .then((st) => (debug = st.debug_overlay === true))
      .catch(() => {});
    const tick = setInterval(() => {
      if (base.running && !paused) elapsed = base.ns + (performance.now() - base.at) * 1e6;
    }, 250);
    return () => {
      offs.forEach((off) => off());
      clearTimeout(dismissTimer);
      clearInterval(tick);
    };
  });

  async function act(fn: () => Promise<unknown>) {
    error = null;
    try {
      await fn();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function switchSource() {
    if (!(await showWindow('switcher'))) await beginPick('switch');
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && confirming) confirming = false;
    if (e.key === 'Escape' && restarting) restarting = false;
    if (e.key === 'Escape' && countdown > 0) act(() => appCall('cancel_countdown'));
  }

  const debugText = $derived.by(() => {
    const h = lastHealth;
    if (!debug || !h) return '';
    const screen = h.tracks.find((t) => t.track_id.startsWith('screen')) ?? h.tracks[0];
    if (!screen) return '';
    const dropped = h.tracks.reduce((n, t) => n + t.dropped, 0);
    const drift = Math.max(...h.tracks.map((t) => Math.abs(t.drift_ns))) / 1e6;
    return `${screen.fps.toFixed(1)} fps · ${dropped} dropped · ${drift.toFixed(0)} ms drift`;
  });
</script>

<svelte:window {onkeydown} onpointermove={gripMove} onpointerup={gripUp} />

<div class="bar" role="toolbar" aria-label="Recording controls">
  <span class="grip" role="presentation" title="Drag to move" onpointerdown={gripDown}>
    <svg width="8" height="18" viewBox="0 0 8 18" aria-hidden="true"
      >{#each [3, 9, 15] as y (y)}<circle cx="2" cy={y} r="1.1" /><circle cx="6" cy={y} r="1.1" />{/each}</svg
    >
  </span>
  <span class="rec" class:live={live && !paused} aria-hidden="true"></span>
  <span class="timer" role="timer" aria-label="Elapsed">{formatElapsed(elapsed)}</span>
  {#if micDb !== null && live && !paused}
    <span class="mic" role="meter" aria-label="Microphone level" aria-valuemin={-60} aria-valuemax={0} aria-valuenow={Math.round(micDb)}>
      {#each [-48, -36, -24, -12, -4] as step (step)}<i class:on={micDb >= step} class:hot={step === -4 && micDb >= step}></i>{/each}
    </span>
  {/if}

  {#if countdown > 0}
    <div class="msg count" role="timer" aria-live="assertive">
      <span class="big tnum">{countdown}</span><span class="state">Recording starts…</span>
    </div>
    <button class="hud" onclick={() => act(() => appCall('cancel_countdown'))}>Cancel</button>
  {:else if confirming}
    <div class="msg" role="alertdialog" aria-labelledby="discard-q">
      <span id="discard-q">Discard this recording?</span>
    </div>
    <button class="hud" onclick={() => (confirming = false)}>Keep</button>
    <button class="hud danger" onclick={() => act(async () => { confirming = false; await api.discardRecording(); })}>Discard</button>
  {:else if restarting}
    <div class="msg" role="alertdialog" aria-labelledby="restart-q">
      <span id="restart-q">Start over? This take is discarded.</span>
    </div>
    <button class="hud" onclick={() => (restarting = false)}>Keep</button>
    <button class="hud danger" onclick={() => act(async () => { restarting = false; await appCall('restart'); })}>Restart</button>
  {:else}
    <div class="msg" aria-live="polite">
      {#if sourceLost}
        <span class="lost">Source closed</span>
        <button class="hud" onclick={() => act(switchSource)}>Switch…</button>
      {:else if error}
        <span class="err" title={error}>{error}</span>
      {:else if notice}
        <span class="notice" title={noticeText(notice)}>{noticeText(notice)}</span>
      {:else}
        <span class="state" class:paused>{label}</span>
        {#if debugText}<span class="debug tnum">{debugText}</span>{/if}
      {/if}
    </div>
    {#if !sourceLost}
      <button class="hud round" onclick={() => act(switchSource)} disabled={!live} title="Switch Source{keyHint('switch')}" aria-label="Switch source">
        <Icon name="switch" size={16} />
      </button>
    {/if}
    <button
      class="hud round"
      onclick={() => act(() => appCall(paused ? 'resume' : 'pause'))}
      disabled={!live}
      title={(paused ? 'Resume' : 'Pause') + keyHint('pause')}
      aria-label={paused ? 'Resume recording' : 'Pause recording'}
    >
      {#if paused}<Icon name="play" size={14} />{:else}<Icon name="pause" size={14} />{/if}
    </button>
    <button class="stop" onclick={() => act(() => api.stopRecording())} disabled={!live && !sourceLost} aria-label="Stop recording" title="Stop">
      <span class="square" aria-hidden="true"></span>
    </button>
    <button class="hud round" onclick={() => (restarting = true)} disabled={!live} aria-label="Restart recording" title="Restart">
      <Icon name="undo" size={14} />
    </button>
    <button class="hud round" onclick={() => (confirming = true)} disabled={!live && rec.state !== 'failed'} aria-label="Discard recording" title="Discard">
      <Icon name="close" size={14} />
    </button>
  {/if}
</div>

<style>
  :global(body:has(.bar)) {
    overflow: hidden;
  }
  .bar {
    height: 100vh;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px 0 8px;
    color: rgba(255, 255, 255, 0.92);
    border-radius: 16px;
    background: rgba(30, 30, 32, 0.94);
    box-shadow: inset 0 0 0 0.5px rgba(255, 255, 255, 0.14);
  }
  :global(:root[data-native]) .bar {
    background: transparent;
  }
  .grip {
    display: grid;
    place-items: center;
    width: 14px;
    height: 32px;
    cursor: grab;
    fill: rgba(255, 255, 255, 0.35);
  }
  .rec {
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.35);
  }
  .rec.live {
    background: #ff453a;
    box-shadow: 0 0 8px rgba(255, 69, 58, 0.7);
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.4;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .rec.live {
      animation: none;
    }
  }
  .timer {
    font-size: 17px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.01em;
    min-width: 52px;
  }
  .mic {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 16px;
  }
  .mic i {
    width: 3px;
    border-radius: 1px;
    background: rgba(255, 255, 255, 0.2);
  }
  .mic i:nth-child(1) {
    height: 5px;
  }
  .mic i:nth-child(2) {
    height: 8px;
  }
  .mic i:nth-child(3) {
    height: 11px;
  }
  .mic i:nth-child(4) {
    height: 14px;
  }
  .mic i:nth-child(5) {
    height: 16px;
  }
  .mic i.on {
    background: #30d158;
  }
  .mic i.hot {
    background: #ff9f0a;
  }
  .msg {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    white-space: nowrap;
    overflow: hidden;
    font-size: 12px;
  }
  .msg span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .state {
    color: rgba(255, 255, 255, 0.55);
  }
  .state.paused {
    color: #ffd60a;
  }
  .debug {
    color: rgba(255, 255, 255, 0.45);
    font-size: 11px;
  }
  .count .big {
    font-size: 22px;
    font-weight: 700;
    min-width: 18px;
  }
  .notice,
  .err {
    white-space: normal;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    line-height: 1.25;
  }
  .notice {
    color: #ffd60a;
  }
  .lost,
  .err {
    color: #ff6961;
  }
  .hud {
    min-height: 26px;
    color: rgba(255, 255, 255, 0.92);
    background: rgba(255, 255, 255, 0.12);
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.08);
  }
  .hud:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.2);
  }
  .hud:active:not(:disabled) {
    background: rgba(255, 255, 255, 0.28);
  }
  .hud.danger {
    background: #ff453a;
    color: #fff;
  }
  .round {
    width: 30px;
    min-width: 30px;
    height: 30px;
    padding: 0;
    border-radius: 50%;
  }
  .stop {
    width: 34px;
    min-width: 34px;
    height: 34px;
    padding: 0;
    border-radius: 50%;
    background: #ff453a;
    box-shadow: 0 0 0 0.5px rgba(0, 0, 0, 0.3), 0 1px 3px rgba(0, 0, 0, 0.35);
  }
  .stop:hover:not(:disabled) {
    background: #ff5a50;
  }
  .stop:active:not(:disabled) {
    background: #e0352b;
  }
  .square {
    width: 11px;
    height: 11px;
    border-radius: 2.5px;
    background: #fff;
  }
</style>
