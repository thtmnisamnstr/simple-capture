<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { onEvent } from '../lib/events.ts';
  import { isNotice, isRecordingState } from '../lib/guards.ts';
  import type { NoticeDto, RecordingState } from '../lib/types.ts';
  import { formatBytes, formatElapsed } from '../lib/format.ts';
  import { beginPick, showWindow } from '../lib/windows.ts';

  let rec = $state<RecordingState>({ state: 'idle' });
  let elapsed = $state(0);
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
      case 'hotkey_unavailable': return `⌥⇧S is unavailable: ${n.error}`;
      case 'stopped_for_sleep': return 'Stopped because the Mac went to sleep';
      case 'stopped_for_disk': return 'Stopped because the disk is full';
      case 'source_lost': return 'Source closed';
    }
  }

  $effect(() => {
    const offs = [
      onEvent('recording-state', isRecordingState, (s) => {
        rec = s;
        if (s.state === 'recording' || s.state === 'switching') elapsed = s.elapsed_ns;
        if (s.state === 'preparing') {
          elapsed = 0;
          sourceLost = false;
          notice = null;
          confirming = false;
        }
        if (s.state === 'switching') sourceLost = false;
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
    return () => {
      offs.forEach((off) => off());
      clearTimeout(dismissTimer);
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
  }
</script>

<svelte:window {onkeydown} />

<div class="bar" role="toolbar" aria-label="Recording controls">
  <span class="rec" class:live aria-hidden="true"></span>
  <span class="timer" role="timer" aria-label="Elapsed">{formatElapsed(elapsed)}</span>

  {#if confirming}
    <div class="msg" role="alertdialog" aria-labelledby="discard-q">
      <span id="discard-q">Discard this recording?</span>
    </div>
    <button onclick={() => (confirming = false)}>Keep</button>
    <button class="danger" onclick={() => act(async () => { confirming = false; await api.discardRecording(); })}>Discard</button>
  {:else}
    <div class="msg" aria-live="polite">
      {#if sourceLost}
        <span class="lost">Source closed —</span>
        <button class="link" onclick={() => act(switchSource)}>Switch source</button>
        <span class="muted">·</span>
        <button class="link" onclick={() => act(() => api.stopRecording())}>Stop</button>
      {:else if error}
        <span class="err" title={error}>{error}</span>
      {:else if notice}
        <span class="notice" title={noticeText(notice)}>{noticeText(notice)}</span>
      {:else}
        <span class="muted">{label}</span>
      {/if}
    </div>
    {#if !sourceLost}
      <button onclick={() => act(switchSource)} disabled={!live} title="Switch source (⌥⇧S)">Switch</button>
      <button class="stop" onclick={() => act(() => api.stopRecording())} disabled={!live} aria-label="Stop recording">
        <span class="square" aria-hidden="true"></span>Stop
      </button>
    {/if}
    <button class="icon" onclick={() => (confirming = true)} disabled={!live && rec.state !== 'failed'} aria-label="Discard recording" title="Discard">✕</button>
  {/if}
</div>

<style>
  :global(body:has(.bar)) {
    overflow: hidden;
  }
  .bar {
    height: 100vh;
    min-height: 40px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    background: var(--surface);
    border-radius: 10px;
    border: 1px solid var(--border);
  }
  .rec {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--muted);
    flex: none;
  }
  .rec.live {
    background: #ff3b30;
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .rec.live {
      animation: none;
    }
  }
  .timer {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
    min-width: 44px;
  }
  .msg {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
    overflow: hidden;
    font-size: 12px;
  }
  .msg span {
    overflow: hidden;
    text-overflow: ellipsis;
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
    color: var(--warn-text);
  }
  .lost,
  .err {
    color: var(--danger);
  }
  .stop {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .square {
    width: 8px;
    height: 8px;
    background: currentColor;
    border-radius: 1px;
  }
  .icon {
    width: 26px;
    padding: 0;
  }
</style>
