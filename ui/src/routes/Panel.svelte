<script lang="ts">
  import { api, errorText } from '../lib/api.ts';
  import { onEvent } from '../lib/events.ts';
  import { isPicked, isRecordingState, isRegionSelected } from '../lib/guards.ts';
  import type {
    Aspect,
    CapabilitiesDto,
    DevicesDto,
    DisplayInfo,
    PermissionsDto,
    PickedDto,
    RecorderSettings,
    RecoveryReportDto,
    SourceList,
    SourceSpecDto,
    Via,
  } from '../lib/types.ts';
  import {
    displayOfSource,
    mainDisplay,
    probeFor,
    sameSource,
    sourceLabel,
    sourceSizePx,
    windowTitle,
  } from '../lib/sources.ts';
  import { aspectRatio, ratioOf, referenceCanvasSize, smallContent } from '../lib/geometry.ts';
  import { backgroundCss, backgroundSummary } from '../lib/backgrounds.ts';
  import { rememberSource } from '../lib/recent.ts';
  import { beginPick, beginRegion, hideSelf, showWindow, systemPicker } from '../lib/windows.ts';
  import BackgroundEditor from '../components/BackgroundEditor.svelte';

  let perms = $state<PermissionsDto | null>(null);
  let caps = $state<CapabilitiesDto | null>(null);
  let displays = $state<DisplayInfo[]>([]);
  let list = $state<SourceList | null>(null);
  let devices = $state<DevicesDto>({ cameras: [], mics: [] });
  let settings = $state<RecorderSettings | null>(null);
  let recovery = $state<RecoveryReportDto[]>([]);
  let via = $state<Via>('own_chooser');
  let tab = $state<'displays' | 'windows' | 'apps'>('displays');
  let appDisplay = $state<string | null>(null);
  let still = $state<string | null>(null);
  let bgOpen = $state(false);
  let error = $state<string | null>(null);
  let recording = $state(false);
  let starting = $state(false);
  let pickedDisplay = $state<PickedDto | null>(null);
  let loaded = $state(false);

  const ASPECTS: { value: Aspect; label: string }[] = [
    { value: 'original', label: 'Original' },
    { value: '16:9', label: '16:9' },
    { value: '4:3', label: '4:3' },
    { value: '1:1', label: '1:1' },
    { value: '9:16', label: '9:16' },
  ];

  const screenGranted = $derived(perms?.screen === 'granted');
  const all = $derived({ displays, windows: list?.windows ?? [], apps: list?.apps ?? [] });
  const source = $derived(settings?.source ?? null);
  const sourceDisplay = $derived(source ? displayOfSource(source, all) : null);
  const probe = $derived(probeFor(caps, sourceDisplay?.id ?? null));
  const contentPx = $derived(source ? sourceSizePx(source, all) : null);
  const canvasPx = $derived.by(() => {
    if (!settings || !contentPx) return null;
    const fixed = aspectRatio(settings.canvas.aspect);
    return referenceCanvasSize(settings.canvas.aspect, fixed ?? ratioOf(contentPx), [contentPx]);
  });
  const isSmall = $derived(!!canvasPx && !!contentPx && smallContent(canvasPx, contentPx));
  const missingPerms = $derived(perms ? (['screen', 'camera', 'mic'] as const).filter((k) => perms![k] !== 'granted') : []);

  const fpsChoice = $derived(!settings ? '30' : settings.fps === 30 ? '30' : settings.resolution === '1x' ? '60-1x' : '60');
  const notMeasured = 'Not measured for this source’s display yet';
  const why60 = $derived(
    !probe ? notMeasured : probe.can_60fps_native ? null : 'This Mac can’t keep up with 60 fps at native resolution on this display',
  );
  const why60x1 = $derived(
    !probe ? notMeasured : probe.can_60fps_1x ? null : 'This Mac can’t keep up with 60 fps on this display, even at 1×',
  );
  const whyAppAudio = $derived(
    !caps?.app_audio
      ? 'Not supported on this system'
      : source?.kind !== 'application'
        ? 'Choose an app as the source to record only its audio'
        : null,
  );

  async function load() {
    error = null;
    try {
      const [p, c, d, s, dev, rec] = await Promise.all([
        api.getPermissions(),
        api.getCapabilities(),
        api.listDisplays(),
        api.getSettings(),
        api.listDevices(),
        api.recoveryReport(),
      ]);
      perms = p;
      caps = c;
      displays = d;
      devices = dev;
      recovery = rec.filter((r) => r.outcome === 'recovered' || r.outcome === 'damaged');
      const main = mainDisplay(d);
      settings = { ...s, source: s.source ?? (main ? { kind: 'display', display_id: main.id } : null) };
      appDisplay = main?.id ?? null;
      await loadSources();
      loaded = true;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function loadSources() {
    if (perms?.screen !== 'granted') {
      list = null;
      return;
    }
    try {
      list = await api.listSources();
    } catch (e) {
      list = null;
      error = errorText(e);
    }
  }

  $effect(() => {
    load();
    const offs = [
      onEvent('source-picked', isPicked, (p) => {
        if (p.purpose !== 'start') return;
        choose(p.source, p.via);
        if (p.via === 'system_picker' && p.source.kind === 'display') pickedDisplay = p;
      }),
      onEvent('region-selected', isRegionSelected, (r) => {
        if (r.purpose !== 'start') return;
        choose({ kind: 'region', display_id: r.display_id, region_pt: r.region_pt }, 'own_chooser');
      }),
      onEvent('recording-state', isRecordingState, (s) => {
        recording = s.state === 'preparing' || s.state === 'recording' || s.state === 'switching' || s.state === 'stopping';
        if (s.state === 'failed') error = s.error;
      }),
    ];
    const onFocus = () =>
      api
        .getPermissions()
        .then((p) => {
          const changed = p.screen !== perms?.screen;
          perms = p;
          if (changed) loadSources();
        })
        .catch(() => undefined);
    window.addEventListener('focus', onFocus);
    return () => {
      offs.forEach((off) => off());
      window.removeEventListener('focus', onFocus);
    };
  });

  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    if (!loaded || !settings) return;
    const snap = $state.snapshot(settings) as RecorderSettings;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => api.setSettings(snap).catch((e) => (error = errorText(e))), 300);
  });

  let stillTimer: ReturnType<typeof setTimeout> | undefined;
  let stillSeq = 0;
  $effect(() => {
    if (!settings?.source) return;
    const src = $state.snapshot(settings.source) as SourceSpecDto;
    const canvas = $state.snapshot(settings.canvas) as RecorderSettings['canvas'];
    clearTimeout(stillTimer);
    const seq = ++stillSeq;
    stillTimer = setTimeout(() => {
      api
        .previewStill(src, canvas)
        .then((url) => {
          if (seq === stillSeq) still = url;
        })
        .catch(() => {
          if (seq === stillSeq) still = null;
        });
    }, 250);
  });

  $effect(() => {
    if (!settings || !loaded) return;
    if ((fpsChoice === '60' && why60) || (fpsChoice === '60-1x' && why60x1)) settings.fps = 30;
    if (settings.system_audio === 'app' && whyAppAudio) settings.system_audio = 'off';
    if (caps && !caps.show_clicks && settings.click_highlight) settings.click_highlight = false;
  });

  function choose(spec: SourceSpecDto, how: Via = 'own_chooser') {
    if (!settings) return;
    settings.source = spec;
    via = how;
    pickedDisplay = null;
  }

  function setFps(choice: string) {
    if (!settings) return;
    if (choice === '30') settings.fps = 30;
    else {
      settings.fps = 60;
      settings.resolution = choice === '60-1x' ? '1x' : 'native';
    }
  }

  async function record() {
    if (!settings?.source) return;
    starting = true;
    error = null;
    const snap = $state.snapshot(settings) as RecorderSettings;
    const src = snap.source!;
    try {
      await api.setSettings(snap);
      await api.startRecording({ source: src, via, title: null, settings: snap });
      rememberSource(src, sourceLabel(src, all));
      await hideSelf();
    } catch (e) {
      error = errorText(e);
    } finally {
      starting = false;
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

  function regionOfPicked() {
    const d = pickedDisplay?.source.kind === 'display' ? pickedDisplay.source.display_id : null;
    pickedDisplay = null;
    run(() => beginRegion('start', d));
  }
</script>

<div class="window panel">
  <header>
    <h1>Simple Capture</h1>
    {#if missingPerms.length}
      <button class="link" onclick={() => showWindow('onboarding')}>Permissions…</button>
    {/if}
  </header>

  {#if recovery.length}
    <p class="warn" role="status">
      {recovery.length === 1 ? 'A recording from a previous session was recovered.' : `${recovery.length} recordings from a previous session were recovered.`}
      <button class="link" onclick={() => run(() => api.reveal(recovery[0].dir))}>Reveal</button>
    </p>
  {/if}

  <section aria-labelledby="src-h">
    <h2 id="src-h">Source</h2>
    <div class="segmented tabs" role="tablist" aria-label="Source type">
      <button role="tab" aria-selected={tab === 'displays'} onclick={() => (tab = 'displays')}>Displays</button>
      <button role="tab" aria-selected={tab === 'windows'} onclick={() => (tab = 'windows')}>Windows</button>
      {#if caps?.application_source !== false}
        <button role="tab" aria-selected={tab === 'apps'} onclick={() => (tab = 'apps')}>Apps</button>
      {/if}
    </div>

    <div class="list" role="tabpanel" aria-label="Sources">
      {#if tab === 'displays'}
        {#each displays as d (d.id)}
          {@const spec: SourceSpecDto = { kind: 'display', display_id: d.id }}
          <button class="item" aria-pressed={sameSource(source, spec)} onclick={() => choose(spec)}>
            <span class="name">{d.name}{d.is_main ? ' (main)' : ''}</span>
            <span class="muted small">{d.size_px.w}×{d.size_px.h}</span>
          </button>
        {/each}
      {:else if !screenGranted}
        <p class="muted empty">Grant Screen Recording to list windows and apps.</p>
        <div class="row empty">
          <button onclick={() => showWindow('onboarding')}>Grant…</button>
          {#if caps?.system_picker}<button onclick={() => run(() => systemPicker('start'))}>System picker…</button>{/if}
        </div>
      {:else if tab === 'windows'}
        {#each all.windows.filter((w) => w.on_screen) as w (w.id)}
          {@const spec: SourceSpecDto = { kind: 'window', window_id: w.id }}
          <button class="item" aria-pressed={sameSource(source, spec)} onclick={() => choose(spec)}>
            <span class="name">{windowTitle(w)}</span>
            <span class="muted small">{Math.round(w.frame_pt.w)}×{Math.round(w.frame_pt.h)} pt</span>
          </button>
        {:else}
          <p class="muted empty">No windows to record.</p>
        {/each}
      {:else}
        {#if displays.length > 1}
          <label class="row small app-display">
            Record on
            <select bind:value={appDisplay}>
              {#each displays as d (d.id)}<option value={d.id}>{d.name}</option>{/each}
            </select>
          </label>
        {/if}
        {#each all.apps as a (a.app.pid)}
          {@const spec: SourceSpecDto = { kind: 'application', display_id: appDisplay ?? displays[0]?.id ?? '', pid: a.app.pid, bundle_id: a.app.bundle_id }}
          <button class="item" aria-pressed={sameSource(source, spec)} onclick={() => choose(spec)}>
            <span class="name">{a.name}</span>
            <span class="muted small">all its windows</span>
          </button>
        {:else}
          <p class="muted empty">No apps to record.</p>
        {/each}
      {/if}
    </div>

    <div class="row actions">
      <button onclick={() => run(() => beginRegion('start', sourceDisplay?.id ?? null))}>Select region…</button>
      <button onclick={() => run(() => beginPick('start'))}>Pick window by clicking</button>
      {#if caps?.system_picker}<button onclick={() => run(() => systemPicker('start'))}>System picker…</button>{/if}
    </div>

    {#if pickedDisplay}
      <div class="choice" role="group" aria-label="Picked display">
        <span>Record the whole display or a region of it?</span>
        <button onclick={() => (pickedDisplay = null)}>Whole display</button>
        <button onclick={regionOfPicked}>Select region…</button>
      </div>
    {/if}

    {#if source}
      <p class="current small">
        <span class="muted">Source:</span>
        {sourceLabel(source, all)}{via === 'system_picker' ? ' (system picker)' : ''}
      </p>
    {/if}
  </section>

  {#if settings}
    <section aria-labelledby="frame-h">
      <h2 id="frame-h">Frame</h2>
      <div class="preview-row">
        <div
          class="preview"
          style:aspect-ratio={canvasPx ? `${canvasPx.w} / ${canvasPx.h}` : '16 / 10'}
          style:background={backgroundCss(settings.canvas.background)}
        >
          {#if still}<img src={still} alt="Preview of the frame" />{/if}
        </div>
        <div class="frame-controls">
          <div class="segmented" role="radiogroup" aria-label="Frame shape">
            {#each ASPECTS as a (a.value)}
              <button role="radio" aria-checked={settings.canvas.aspect === a.value} onclick={() => (settings!.canvas.aspect = a.value)}>
                {a.label}
              </button>
            {/each}
          </div>
          <p class="muted small">Original = the first source’s shape.</p>
          {#if canvasPx}<p class="muted small">Video size {canvasPx.w}×{canvasPx.h}</p>{/if}
        </div>
      </div>
      {#if isSmall}
        <p class="warn" role="status">
          The source will look small in this frame (under a quarter of its width or height). Try a frame closer to its shape, or
          record a region.
        </p>
      {/if}
      <div class="row">
        <span class="label">Background</span>
        <button class="bg-button" aria-expanded={bgOpen} aria-haspopup="dialog" onclick={() => (bgOpen = !bgOpen)}>
          <span class="chip" style:background={backgroundCss(settings.canvas.background)}></span>
          {backgroundSummary(settings.canvas.background)}
        </button>
      </div>
      {#if bgOpen}
        <BackgroundEditor bind:canvas={settings.canvas} onclose={() => (bgOpen = false)} />
      {/if}
    </section>

    <section aria-labelledby="av-h">
      <h2 id="av-h">Camera and audio</h2>
      <div class="row nowrap">
        <label class="label" for="camera">Camera</label>
        <select id="camera" bind:value={settings.camera}>
          <option value={null}>None</option>
          {#each devices.cameras as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
        </select>
      </div>
      {#if settings.camera}<p class="muted small indent">Recorded now, shown in M2.</p>{/if}
      <div class="row nowrap">
        <label class="label" for="mic">Microphone</label>
        <select id="mic" bind:value={settings.mic}>
          <option value={null}>None</option>
          {#each devices.mics as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
        </select>
        {#if caps?.mic_modes}
          <button onclick={() => run(() => api.showMicModes())} disabled={!settings.mic}>Mic Mode…</button>
        {/if}
      </div>
      <div class="row">
        <span class="label" id="sa-l">System audio</span>
        <div class="segmented" role="radiogroup" aria-labelledby="sa-l">
          <button role="radio" aria-checked={settings.system_audio === 'off'} onclick={() => (settings!.system_audio = 'off')}>Off</button>
          <button role="radio" aria-checked={settings.system_audio === 'all'} onclick={() => (settings!.system_audio = 'all')}>All</button>
          <button
            role="radio"
            aria-checked={settings.system_audio === 'app'}
            disabled={!!whyAppAudio}
            title={whyAppAudio ?? ''}
            onclick={() => (settings!.system_audio = 'app')}>App</button
          >
        </div>
      </div>
      {#if whyAppAudio && source?.kind !== 'application'}<p class="muted small indent">App: {whyAppAudio}.</p>{/if}
    </section>

    <section aria-labelledby="vid-h">
      <h2 id="vid-h">Video</h2>
      <div class="row">
        <span class="label" id="fps-l">Frame rate</span>
        <div class="segmented" role="radiogroup" aria-labelledby="fps-l">
          <button role="radio" aria-checked={fpsChoice === '30'} onclick={() => setFps('30')}>30 fps</button>
          <button role="radio" aria-checked={fpsChoice === '60'} disabled={!!why60} title={why60 ?? ''} onclick={() => setFps('60')}>60 fps</button>
          <button role="radio" aria-checked={fpsChoice === '60-1x'} disabled={!!why60x1} title={why60x1 ?? ''} onclick={() => setFps('60-1x')}>
            60 fps at 1×
          </button>
        </div>
      </div>
      {#if why60}<p class="muted small indent">60 fps: {why60}.</p>{/if}
      {#if why60x1 && why60x1 !== why60}<p class="muted small indent">60 fps at 1×: {why60x1}.</p>{/if}
      <div class="row">
        <span class="label" id="res-l">Resolution</span>
        <div class="segmented" role="radiogroup" aria-labelledby="res-l">
          <button
            role="radio"
            aria-checked={settings.resolution === 'native'}
            disabled={settings.fps === 60 && !!why60}
            onclick={() => (settings!.resolution = 'native')}>Native</button
          >
          <button role="radio" aria-checked={settings.resolution === '1x'} onclick={() => (settings!.resolution = '1x')}>1×</button>
        </div>
      </div>
      <label class="row check"><input type="checkbox" bind:checked={settings.cursor} /> Show the cursor</label>
      {#if caps?.show_clicks}
        <label class="row check">
          <input type="checkbox" bind:checked={settings.click_highlight} disabled={!settings.cursor} /> Highlight clicks
        </label>
      {/if}
    </section>
  {/if}

  {#if error}<p class="warn" role="alert">{error}</p>{/if}

  <footer>
    <button class="primary record" onclick={record} disabled={!settings?.source || recording || starting}>
      <span class="dot" aria-hidden="true"></span>
      {recording ? 'Recording…' : starting ? 'Starting…' : 'Record'}
    </button>
  </footer>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px 14px 0;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .tabs {
    align-self: flex-start;
  }
  .tabs button[aria-selected='true'] {
    background: var(--accent);
    color: var(--accent-text);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 132px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: 3px;
  }
  .item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    border: none;
    text-align: left;
    background: transparent;
    flex: none;
  }
  .item[aria-pressed='true'] {
    background: var(--accent);
    color: var(--accent-text);
  }
  .item[aria-pressed='true'] .muted {
    color: inherit;
    opacity: 0.85;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .empty {
    margin: 6px;
  }
  .app-display {
    padding: 2px 6px 4px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .nowrap {
    flex-wrap: nowrap;
  }
  .nowrap select {
    min-width: 0;
    flex: 1;
  }
  .actions button {
    font-size: 12px;
  }
  .choice {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    padding: 6px 8px;
    border-radius: 6px;
    background: var(--surface-2);
  }
  .current {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preview-row {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }
  .preview {
    width: 112px;
    max-height: 112px;
    flex: none;
    border-radius: 5px;
    border: 1px solid var(--border);
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .preview img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }
  .frame-controls {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .frame-controls p {
    margin: 0;
  }
  .label {
    width: 86px;
    flex: none;
    color: var(--muted);
  }
  .indent {
    margin: -3px 0 0 94px;
  }
  .bg-button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .chip {
    width: 14px;
    height: 14px;
    border-radius: 4px;
    border: 1px solid var(--border);
  }
  .check {
    gap: 6px;
  }
  p.warn {
    margin: 0;
  }
  footer {
    position: sticky;
    bottom: 0;
    padding: 10px 0 12px;
    background: linear-gradient(transparent, var(--bg) 30%);
    display: flex;
    justify-content: flex-end;
  }
  .record {
    min-width: 120px;
    min-height: 30px;
    font-weight: 600;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    justify-content: center;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: #ff3b30;
    box-shadow: 0 0 0 1.5px rgba(255, 255, 255, 0.9);
  }
</style>
