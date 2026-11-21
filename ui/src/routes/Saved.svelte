<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { onEvent } from '../lib/events.ts';
  import { isExportProgress, isExportState, isNotice, isRecordingState } from '../lib/guards.ts';
  import type { DisplayInfo, ExportProgressDto, ExportSizeDto, ExportStateDto, RecorderSettings, SourceList } from '../lib/types.ts';
  import { basename, formatElapsed } from '../lib/format.ts';
  import { aspectRatio, ratioOf, referenceCanvasSize, type Size } from '../lib/geometry.ts';
  import { sourceSizePx } from '../lib/sources.ts';
  import Icon from '../components/Icon.svelte';
  import { editor } from '../lib/editor.ts';

  let projectDir = $state<string | null>(null);
  let duration = $state(0);
  let tracks = $state<string[]>([]);
  const TRACK_NAMES: Record<string, string> = { screen: 'screen', camera: 'camera', mic: 'microphone', system_audio: 'system audio' };
  const contents = $derived(tracks.map((t) => TRACK_NAMES[t] ?? t).join(', '));
  let size = $state<Size | null>(null);
  let stoppedFor = $state<'sleep' | 'disk' | null>(null);
  let failed = $state<string | null>(null);
  let exportSize = $state<ExportSizeDto>('reference');
  let job = $state<string | null>(null);
  let progress = $state<ExportProgressDto | null>(null);
  let exportState = $state<ExportStateDto | null>(null);
  let error = $state<string | null>(null);
  let lastElapsed = 0;

  const SIZES: { value: ExportSizeDto; label: string }[] = [
    { value: 'reference', label: 'Full size (long side up to 3840)' },
    { value: '2160', label: '2160p' },
    { value: '1440', label: '1440p' },
    { value: '1080', label: '1080p' },
    { value: '720', label: '720p' },
    { value: '480', label: '480p' },
  ];

  const exporting = $derived(
    !!job && !!exportState && (exportState.state === 'preparing' || exportState.state === 'rendering' || exportState.state === 'finalizing'),
  );
  const pct = $derived(progress && progress.frames_total > 0 ? Math.round((progress.frames_done / progress.frames_total) * 100) : 0);

  async function measure() {
    try {
      const [s, d]: [RecorderSettings, DisplayInfo[]] = await Promise.all([api.getSettings(), api.listDisplays()]);
      const perms = await api.getPermissions();
      const list: Partial<SourceList> = perms.screen === 'granted' ? await api.listSources() : { displays: d };
      const content = s.source ? sourceSizePx(s.source, list, s.resolution) : null;
      if (!content) return;
      const fixed = aspectRatio(s.canvas.aspect);
      size = referenceCanvasSize(s.canvas.aspect, fixed ?? ratioOf(content), [content]);
    } catch {
      size = null;
    }
  }

  $effect(() => {
    const offs = [
      onEvent('recording-state', isRecordingState, (s) => {
        if (s.state === 'recording' || s.state === 'switching') lastElapsed = s.elapsed_ns;
        if (s.state === 'preparing') {
          lastElapsed = 0;
          stoppedFor = null;
          failed = null;
        }
        if (s.state === 'finalized') {
          projectDir = s.project_dir;
          duration = s.duration_ns ?? lastElapsed;
          tracks = s.tracks ?? [];
          job = null;
          progress = null;
          exportState = null;
          error = null;
          measure();
        }
        if (s.state === 'failed') failed = s.error;
      }),
      onEvent('notice', isNotice, (n) => {
        if (n.kind === 'stopped_for_sleep') stoppedFor = 'sleep';
        if (n.kind === 'stopped_for_disk') stoppedFor = 'disk';
      }),
      onEvent('export-progress', isExportProgress, (p) => {
        if (p.id === job) progress = p;
      }),
      onEvent('export-state', isExportState, (s) => {
        if (s.id === job) exportState = s;
      }),
    ];
    return () => offs.forEach((off) => off());
  });

  async function startExport() {
    if (!projectDir) return;
    error = null;
    progress = null;
    exportState = { id: '', state: 'preparing' };
    try {
      job = await api.quickExport(projectDir, exportSize);
      if (exportState?.id === '') exportState = { id: job, state: 'preparing' };
    } catch (e) {
      exportState = null;
      error = errorText(e);
    }
  }

  async function run(fn: () => Promise<unknown>) {
    error = null;
    try {
      await fn();
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<div class="window saved">
  {#if failed && !projectDir}
    <header>
      <span class="badge-icon bad"><Icon name="warning" size={20} /></span>
      <div><h1>Recording failed</h1><p class="caption">{failed}</p></div>
    </header>
  {:else if !projectDir}
    <header>
      <span class="badge-icon"><Icon name="film" size={20} /></span>
      <div><h1>Recording saved</h1><p class="caption">Nothing recorded yet.</p></div>
    </header>
  {:else}
    <header>
      <span class="badge-icon ok"><Icon name="check" size={22} /></span>
      <div class="title">
        <h1>Recording saved</h1>
        <p class="caption tnum">{formatElapsed(duration)}{size ? ` · ${size.w} × ${size.h}` : ''}</p>
      </div>
    </header>

    {#if stoppedFor}
      <p class="warn" role="status">
        <Icon name="warning" size={14} />
        <span>
          {stoppedFor === 'sleep' ? 'Recording stopped because the Mac went to sleep.' : 'Recording stopped because the disk is full.'}
          Everything up to that point is saved.
        </span>
      </p>
    {/if}

    <div class="group">
      {#if contents}
        <div class="grow"><span class="glabel muted">Recorded</span><span class="gcontrol value">{contents}</span></div>
      {/if}
      <div class="grow">
        <span class="glabel muted">Project</span>
        <span class="gcontrol value">
          <span class="path" title={projectDir}>{basename(projectDir)}</span>
          <button class="plain icon-btn" title="Show in Finder" aria-label="Show in Finder" onclick={() => run(() => api.reveal(projectDir!))}
            ><Icon name="folder" size={14} /></button
          >
        </span>
      </div>
      <div class="grow">
        <label class="glabel muted" for="export-size">Export size</label>
        <span class="gcontrol">
          <select id="export-size" bind:value={exportSize} disabled={exporting}>
            {#each SIZES as s (s.value)}<option value={s.value}>{s.label}</option>{/each}
          </select>
        </span>
      </div>
    </div>
    {#if contents}<p class="caption note">One video with all of it: the screen in its frame, the camera where you placed it, and the audio mixed.</p>{/if}

    {#if exportState?.state === 'done'}
      <p class="done" role="status">
        <Icon name="check" size={14} />
        <span class="path">Exported {basename(exportState.path)}</span>
        <button class="link" onclick={() => exportState?.state === "done" && run(() => api.reveal((exportState as { path: string }).path))}>Show in Finder</button>
      </p>
    {:else if exportState?.state === 'failed'}
      <p class="warn" role="alert"><Icon name="warning" size={14} /><span>Export failed: {exportState.error}</span></p>
    {:else if exportState?.state === 'canceled'}
      <p class="caption note" role="status">Export cancelled.</p>
    {/if}
    {#if error}<p class="warn" role="alert"><Icon name="warning" size={14} /><span>{error}</span></p>{/if}

    <footer role="group" aria-label="Export">
      {#if exporting}
        <div class="progress">
          <progress max="100" value={pct} aria-label="Export progress">{pct}%</progress>
          <span class="caption tnum">
            {exportState?.state === 'finalizing' ? 'Finishing…' : `Exporting ${pct}%`}{progress?.eta_s != null ? ` · about ${Math.ceil(progress.eta_s)} s left` : ''}
          </span>
        </div>
        <button onclick={() => job && run(() => api.cancelExport(job!))}>Cancel</button>
      {:else}
        <button onclick={() => run(() => api.reveal(projectDir!))}>Show in Finder</button>
        <button onclick={() => run(() => editor.open(projectDir!).then(() => undefined))}>Edit…</button>
        <button class="primary" onclick={startExport}><Icon name="export" size={14} />Export Video</button>
      {/if}
    </footer>
  {/if}
</div>

<style>
  .saved {
    padding: 16px 18px 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .title {
    min-width: 0;
  }
  .badge-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 50%;
    color: #fff;
    background: var(--label-3);
  }
  .badge-icon.ok {
    background: var(--green);
  }
  .badge-icon.bad {
    background: var(--orange);
  }
  .value {
    max-width: 70%;
    justify-content: flex-end;
    text-align: right;
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .icon-btn {
    padding: 2px 4px;
    color: var(--label-2);
  }
  .note {
    margin: -4px 10px 0;
  }
  .done {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--green);
    font-size: 12px;
    min-width: 0;
  }
  footer {
    margin-top: auto;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 8px;
  }
  footer button {
    min-width: 92px;
  }
  .progress {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  progress {
    appearance: none;
    width: 100%;
    height: 6px;
  }
  progress::-webkit-progress-bar {
    border-radius: 3px;
    background: var(--fill-2);
  }
  progress::-webkit-progress-value {
    border-radius: 3px;
    background: var(--accent);
  }
</style>
