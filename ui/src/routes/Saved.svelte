<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { onEvent } from '../lib/events.ts';
  import { isExportProgress, isExportState, isNotice, isRecordingState } from '../lib/guards.ts';
  import type { DisplayInfo, ExportProgressDto, ExportSizeDto, ExportStateDto, RecorderSettings, SourceList } from '../lib/types.ts';
  import { basename, formatElapsed } from '../lib/format.ts';
  import { aspectRatio, ratioOf, referenceCanvasSize, type Size } from '../lib/geometry.ts';
  import { sourceSizePx } from '../lib/sources.ts';

  let projectDir = $state<string | null>(null);
  let duration = $state(0);
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
          duration = lastElapsed;
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
    <h1>Recording failed</h1>
    <p class="warn" role="alert">{failed}</p>
  {:else if !projectDir}
    <h1>Recording saved</h1>
    <p class="muted">Nothing recorded yet.</p>
  {:else}
    <h1>Recording saved</h1>
    {#if stoppedFor}
      <p class="warn" role="status">
        {stoppedFor === 'sleep' ? 'Recording stopped because the Mac went to sleep.' : 'Recording stopped because the disk is full.'}
        Everything up to that point is saved.
      </p>
    {/if}
    <dl>
      <dt>Duration</dt>
      <dd>{formatElapsed(duration)}</dd>
      {#if size}
        <dt>Size</dt>
        <dd>{size.w}×{size.h}</dd>
      {/if}
      <dt>Project</dt>
      <dd class="path" title={projectDir}>{basename(projectDir)}</dd>
    </dl>

    <div class="export" role="group" aria-label="Export">
      {#if exporting}
        <progress max="100" value={pct} aria-label="Export progress">{pct}%</progress>
        <span class="muted small">
          {exportState?.state === 'finalizing' ? 'Finishing…' : `${pct}%`}{progress?.eta_s != null ? ` · about ${Math.ceil(progress.eta_s)} s left` : ''}
        </span>
        <button onclick={() => job && run(() => api.cancelExport(job!))}>Cancel</button>
      {:else}
        <label class="row">
          <span class="muted">Size</span>
          <select bind:value={exportSize}>
            {#each SIZES as s (s.value)}<option value={s.value}>{s.label}</option>{/each}
          </select>
        </label>
        <button class="primary" onclick={startExport}>Export MP4</button>
      {/if}
    </div>

    {#if exportState?.state === 'done'}
      <p class="done" role="status">
        Exported {basename(exportState.path)}
        <button class="link" onclick={() => exportState?.state === "done" && run(() => api.reveal((exportState as { path: string }).path))}>Reveal</button>
      </p>
    {:else if exportState?.state === 'failed'}
      <p class="warn" role="alert">Export failed: {exportState.error}</p>
    {:else if exportState?.state === 'canceled'}
      <p class="muted" role="status">Export cancelled.</p>
    {/if}

    {#if error}<p class="warn" role="alert">{error}</p>{/if}

    <footer>
      <button onclick={() => run(() => api.reveal(projectDir!))}>Reveal in Finder</button>
    </footer>
  {/if}
</div>

<style>
  .saved {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  p {
    margin: 0;
  }
  dl {
    display: grid;
    grid-template-columns: 70px 1fr;
    gap: 4px 10px;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .export {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  progress {
    flex: 1;
    min-width: 120px;
  }
  .done {
    color: var(--ok);
  }
  footer {
    display: flex;
    justify-content: flex-end;
  }
</style>
