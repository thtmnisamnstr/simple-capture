<script lang="ts">
  import { hostOs, revealLabel } from '../lib/format.ts';
  import { keysFromEvent, keysLabel, shortcutsApi, type ShortcutRow } from '../lib/shortcuts.ts';
  import { editor } from '../lib/editor.ts';
  import { api, errorText } from '../lib/api.ts';
  import { onEvent } from '../lib/events.ts';
  import { isObject, isPicked, isRecordingState, isRegionSelected } from '../lib/guards.ts';
  import type {
    Aspect,
    CapabilitiesDto,
    DevicesDto,
    DisplayInfo,
    PermissionsDto,
    CameraKey,
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
  import {
    aspectRatio,
    CAMERA_SIZE_PRESETS,
    cornerCenter,
    ratioOf,
    referenceCanvasSize,
    smallContent,
    type Corner,
  } from '../lib/geometry.ts';
  import { CAMERA_SHAPES, shapeAspect, shapeChange } from '../lib/camera.ts';
  import { backgroundCss, backgroundSummary } from '../lib/backgrounds.ts';
  import { rememberSource } from '../lib/recent.ts';
  import { beginPick, beginRegion, hideSelf, showWindow, systemPicker } from '../lib/windows.ts';
  import BackgroundEditor from '../components/BackgroundEditor.svelte';
  import Icon from '../components/Icon.svelte';
  import { call as appCall } from '../lib/appinfo.ts';

  let perms = $state<PermissionsDto | null>(null);
  let caps = $state<CapabilitiesDto | null>(null);
  let displays = $state<DisplayInfo[]>([]);
  let list = $state<SourceList | null>(null);
  let devices = $state<DevicesDto>({ cameras: [], mics: [] });
  let settings = $state<RecorderSettings | null>(null);
  let recovery = $state<RecoveryReportDto[]>([]);
  let via = $state<Via>('own_chooser');
  let tab = $state<'displays' | 'windows' | 'apps' | 'iphone'>('displays');
  const MIRRORING = 'com.apple.ScreenContinuity';
  const mirrorWindows = $derived((list?.windows ?? []).filter((w) => w.app.bundle_id === MIRRORING && w.on_screen));

  $effect(() => {
    if (tab !== 'iphone') return;
    const refresh = async () => {
      const d = await api.listDevices().catch(() => null);
      if (d) devices = d;
      await loadSources();
    };
    void refresh();
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  });
  let appDisplay = $state<string | null>(null);
  const PREVIEW_W = 316;
  const PREVIEW_H = 138;
  let still = $state<string | null>(null);
  let bgOpen = $state(false);
  let error = $state<string | null>(null);
  let recording = $state(false);
  let starting = $state(false);
  let pickedDisplay = $state<PickedDto | null>(null);
  let loaded = $state(false);
  let lastScreen = $state<SourceSpecDto | null>(null);
  let shortcuts = $state<ShortcutRow[]>([]);
  let capturing = $state<string | null>(null);
  const mac = hostOs() === 'macos';

  async function startCapture(e: MouseEvent, id: string) {
    (e.currentTarget as HTMLElement).focus();
    if (capturing === id) return;
    if (capturing) await endCapture(capturing, null);
    capturing = id;
    await shortcutsApi.pause(true).catch(() => undefined);
  }

  async function endCapture(id: string, keys: string | null) {
    if (capturing !== id) return;
    capturing = null;
    try {
      shortcuts = await shortcutsApi.pause(false);
      if (keys !== null) shortcuts = await shortcutsApi.set(id, keys);
    } catch (e) {
      error = errorText(e);
      shortcuts = await shortcutsApi.list().catch(() => shortcuts);
    }
  }

  function captureKey(e: KeyboardEvent, id: string) {
    if (capturing !== id) return;
    e.preventDefault();
    e.stopPropagation();
    const plain = !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey;
    if (e.key === 'Escape' && plain) return void endCapture(id, null);
    if ((e.key === 'Backspace' || e.key === 'Delete') && plain) return void endCapture(id, '');
    const keys = keysFromEvent(e);
    if (keys) void endCapture(id, keys);
  }

  type Mode = 'screen' | 'both' | 'camera';
  const mode = $derived<Mode>(settings?.source?.kind === 'camera' ? 'camera' : settings?.camera ? 'both' : 'screen');
  const noCamera = $derived(
    perms && perms.camera !== 'granted'
      ? 'Allow the camera first'
      : loaded && !devices.cameras.length
        ? 'No camera is connected'
        : null,
  );

  const ASPECTS: { value: Aspect; label: string }[] = [
    { value: 'original', label: 'Original' },
    { value: '16:9', label: '16:9' },
    { value: '4:3', label: '4:3' },
    { value: '1:1', label: '1:1' },
    { value: '4:5', label: '4:5' },
    { value: '9:16', label: '9:16' },
  ];

  const screenGranted = $derived(perms?.screen === 'granted');
  const all = $derived({ displays, windows: list?.windows ?? [], apps: list?.apps ?? [] });
  const source = $derived(settings?.source ?? null);
  const sourceDisplay = $derived(source ? displayOfSource(source, all) : null);
  const probe = $derived(probeFor(caps, sourceDisplay?.id ?? null));
  const contentPx = $derived(source ? sourceSizePx(source, all, settings?.resolution) : null);
  const canvasPx = $derived.by(() => {
    if (!settings || !contentPx) return null;
    const fixed = aspectRatio(settings.canvas.aspect);
    return referenceCanvasSize(settings.canvas.aspect, fixed ?? ratioOf(contentPx), [contentPx]);
  });
  const previewBox = $derived.by(() => {
    const c = canvasPx ?? { w: 16, h: 10 };
    const k = Math.min(PREVIEW_W / c.w, PREVIEW_H / c.h);
    return { w: Math.max(1, Math.round(c.w * k)), h: Math.max(1, Math.round(c.h * k)) };
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

  let micDb: number | null = $state(null);
  let micTimer: ReturnType<typeof setTimeout> | undefined;

  const portalOnly = $derived(!!caps?.system_picker && loaded && displays.length === 0);

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
      shortcuts = await shortcutsApi.list().catch(() => []);
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
      onEvent('devices-changed', isObject, () => {
        void api
          .listDevices()
          .then((d) => (devices = d))
          .catch(() => undefined);
      }),
      onEvent('source-picked', isPicked, (p) => {
        if (p.purpose !== 'start') return;
        choose(p.source, p.via);
        if (p.via === 'system_picker' && p.source.kind === 'display') pickedDisplay = p;
      }),
      onEvent('region-selected', isRegionSelected, (r) => {
        if (r.purpose !== 'start') return;
        choose({ kind: 'region', display_id: r.display_id, region_pt: r.region_pt }, 'own_chooser');
      }),
      onEvent('launch-tasks-done', isObject, () => {
        Promise.all([api.getCapabilities(), api.recoveryReport()])
          .then(([c, rec]) => {
            caps = c;
            recovery = rec.filter((r) => r.outcome === 'recovered' || r.outcome === 'damaged');
          })
          .catch((e) => (error = errorText(e)));
      }),
      onEvent('settings-changed', isObject, (next) => {
        if (settings && JSON.stringify(next) !== JSON.stringify($state.snapshot(settings))) {
          settings = next as unknown as RecorderSettings;
        }
      }),
      onEvent('mic-level', isObject, (m) => {
        if (typeof m.peak_db !== 'number') return;
        micDb = micDb === null ? m.peak_db : Math.max(m.peak_db, micDb - 3);
        clearTimeout(micTimer);
        micTimer = setTimeout(() => (micDb = null), 1500);
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

  function setKey(patch: Partial<CameraKey>) {
    if (settings) settings.camera_key = { ...settings.camera_key, ...patch };
  }

  function placeCorner(corner: Corner) {
    if (!settings) return;
    const k = settings.camera_key;
    const frame = canvasPx ?? { w: 1920, h: 1080 };
    setKey({ center_norm: cornerCenter(frame, shapeAspect(k.shape), k.size_norm, corner) });
  }

  function defaultCamera(): string | null {
    return settings?.camera ?? devices.cameras.find((c) => c.is_default)?.id ?? devices.cameras[0]?.id ?? null;
  }

  function screenSource(): SourceSpecDto | null {
    const main = mainDisplay(displays);
    return lastScreen ?? (main ? { kind: 'display', display_id: main.id } : null);
  }

  function setMode(next: Mode) {
    if (!settings) return;
    const cur = settings.source;
    if (next === 'camera') {
      const cam = defaultCamera();
      if (!cam) return;
      if (cur && cur.kind !== 'camera') lastScreen = cur;
      settings.camera = cam;
      settings.source = { kind: 'camera', device_id: cam };
      via = 'own_chooser';
      return;
    }
    if (!cur || cur.kind === 'camera') settings.source = screenSource();
    settings.camera = next === 'both' ? defaultCamera() : null;
  }

  $effect(() => {
    if (!settings || settings.source?.kind !== 'camera') return;
    if (!settings.camera) settings.source = screenSource();
    else if (settings.source.device_id !== settings.camera) settings.source = { kind: 'camera', device_id: settings.camera };
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
    const key = $state.snapshot(settings.camera_key) as CameraKey;
    void settings.camera;
    clearTimeout(stillTimer);
    const seq = ++stillSeq;
    const fetchStill = () =>
      api
        .previewStill(src, canvas, key)
        .then((url) => {
          if (seq === stillSeq) still = url;
        })
        .catch(() => {
          if (seq === stillSeq) still = null;
        });
    stillTimer = setTimeout(() => {
      fetchStill();
      if (src.kind === 'camera' || src.kind === 'device') stillTimer = setTimeout(fetchStill, 1500);
    }, 250);
  });

  $effect(() => {
    if (!settings || !loaded) return;
    if (probe && ((fpsChoice === '60' && why60) || (fpsChoice === '60-1x' && why60x1))) settings.fps = 30;
    if (settings.system_audio === 'app' && whyAppAudio) settings.system_audio = 'off';
    if (caps && !caps.show_clicks && !caps.own_clicks && settings.click_highlight) settings.click_highlight = false;
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
      rememberSource(src, sourceLabel(src, all, devices.cameras));
      await api.cameraPreview(false).catch(() => undefined);
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

<svelte:window
  onkeydown={(e) => capturing && captureKey(e, capturing)}
  onblur={() => capturing && endCapture(capturing, null)}
/>

<div class="window panel">
  <div class="scroll">
    {#if missingPerms.length}
      <button class="banner" onclick={() => showWindow('onboarding')}>
        <span class="banner-icon"><Icon name="lock" size={15} /></span>
        <span class="banner-text">
          <strong>Permissions needed</strong>
          <span class="caption">Simple Capture can’t use {missingPerms.length === 1 ? 'one device' : `${missingPerms.length} devices`} yet.</span>
        </span>
        <Icon name="chevron" size={13} />
      </button>
    {/if}

    {#if recovery.length}
      <p class="warn" role="status">
        <Icon name="warning" size={14} />
        <span>
          {recovery.length === 1 ? 'A recording from a previous session was recovered.' : `${recovery.length} recordings from a previous session were recovered.`}
          <button class="link" onclick={() => run(() => editor.open(recovery[0].dir).then(() => undefined))}>Edit</button>
          <button class="link" onclick={() => run(() => api.reveal(recovery[0].dir))}>{revealLabel()}</button>
          {#if recovery.length > 1}<button class="link" onclick={() => showWindow('library')}>Library…</button>{/if}
        </span>
      </p>
    {/if}

    {#if settings}
      <div class="segmented modes" role="radiogroup" aria-label="What to record">
        <button role="radio" aria-checked={mode === 'screen'} onclick={() => setMode('screen')}>Screen</button>
        <button role="radio" aria-checked={mode === 'both'} disabled={!!noCamera} title={noCamera ?? 'The screen, with the camera in a corner'} onclick={() => setMode('both')}>Screen + Camera</button>
        <button role="radio" aria-checked={mode === 'camera'} disabled={!!noCamera} title={noCamera ?? 'The camera fills the frame'} onclick={() => setMode('camera')}>Camera Only</button>
      </div>
    {/if}

    {#if mode !== 'camera'}
    <section aria-labelledby="src-h">
      <div class="head">
        <h2 id="src-h">Source</h2>
        {#if !portalOnly}
        <div class="segmented" role="tablist" aria-label="Source type">
          <button role="tab" aria-selected={tab === 'displays'} onclick={() => (tab = 'displays')}>Displays</button>
          <button role="tab" aria-selected={tab === 'windows'} onclick={() => (tab = 'windows')}>Windows</button>
          {#if caps?.application_source !== false}
            <button role="tab" aria-selected={tab === 'apps'} onclick={() => (tab = 'apps')}>Apps</button>
          {/if}
          {#if mac}
            <button role="tab" aria-selected={tab === 'iphone'} onclick={() => (tab = 'iphone')}>iPhone</button>
          {/if}
        </div>
        {/if}
      </div>

      {#if portalOnly}
        <div class="group list" role="group" aria-label="Source">
          <div class="empty">
            <p class="muted">{source ? 'The screen or window you chose will be recorded.' : 'Your system asks which screen or window to share.'}</p>
            <div class="row">
              <button class="primary" onclick={() => run(() => systemPicker('start'))}>{source ? 'Choose Again…' : 'Choose Screen or Window…'}</button>
            </div>
          </div>
        </div>
      {:else}

      <div class="group list" role="tabpanel" aria-label="Sources">
        {#if tab === 'displays'}
          {#each displays as d (d.id)}
            {@const spec: SourceSpecDto = { kind: 'display', display_id: d.id }}
            <button class="item" aria-pressed={sameSource(source, spec)} onclick={() => choose(spec)}>
              <Icon name="display" />
              <span class="name">{d.name}</span>
              {#if d.is_main}<span class="tag">Main</span>{/if}
              <span class="dim tnum">{d.size_px.w} × {d.size_px.h}</span>
            </button>
          {/each}
        {:else if tab === 'iphone'}
          {#each devices.screens ?? [] as d (d.id)}
            {@const spec: SourceSpecDto = { kind: 'device', device_id: d.id }}
            <button class="item" aria-pressed={sameSource(source, spec)} onclick={() => choose(spec)} title="The screen of the iPhone or iPad on the cable">
              <Icon name="phone" />
              <span class="name">{d.name}</span>
              <span class="tag">USB</span>
            </button>
          {/each}
          {#each mirrorWindows as w (w.id)}
            {@const spec: SourceSpecDto = { kind: 'window', window_id: w.id }}
            <button class="item" aria-pressed={sameSource(source, spec)} onclick={() => choose(spec)} title="The iPhone Mirroring window: your iPhone over Wi-Fi">
              <Icon name="phone" />
              <span class="name">iPhone Mirroring</span>
              <span class="tag">Wireless</span>
            </button>
          {/each}
          <div class="empty">
            {#if !(devices.screens?.length) && !mirrorWindows.length}
              <p class="muted">Plug in your iPhone or iPad with a cable, unlock it and tap Trust; it shows up here in a few seconds. For wireless, open iPhone Mirroring (macOS 15 or later).</p>
            {:else}
              <p class="muted">Wireless goes through iPhone Mirroring; a cable gives the sharpest picture.</p>
            {/if}
            {#if !mirrorWindows.length}
              <div class="row">
                <button onclick={() => run(() => appCall('open_iphone_mirroring'))}>Open iPhone Mirroring</button>
              </div>
            {/if}
            {#if !screenGranted && !(devices.screens?.length)}<p class="muted">Wireless needs Screen Recording permission.</p>{/if}
          </div>
        {:else if !screenGranted}
          <div class="empty">
            <p class="muted">Allow Screen Recording to list windows and apps.</p>
            <div class="row">
              <button onclick={() => showWindow('onboarding')}>Allow…</button>
              {#if caps?.system_picker}<button onclick={() => run(() => systemPicker('start'))}>Use the System Picker…</button>{/if}
            </div>
          </div>
        {:else if tab === 'windows'}
          {#each all.windows.filter((w) => w.on_screen) as w (w.id)}
            {@const spec: SourceSpecDto = { kind: 'window', window_id: w.id }}
            <button class="item" aria-pressed={sameSource(source, spec)} onclick={() => choose(spec)}>
              <Icon name="window" />
              <span class="name">{windowTitle(w)}</span>
              <span class="dim tnum">{Math.round(w.frame_pt.w)} × {Math.round(w.frame_pt.h)}</span>
            </button>
          {:else}
            <p class="muted empty">No windows to record.</p>
          {/each}
        {:else}
          {#if displays.length > 1}
            <label class="app-display caption">
              Record on
              <select bind:value={appDisplay}>
                {#each displays as d (d.id)}<option value={d.id}>{d.name}</option>{/each}
              </select>
            </label>
          {/if}
          {#each all.apps as a (a.app.pid)}
            {@const spec: SourceSpecDto = { kind: 'application', display_id: appDisplay ?? displays[0]?.id ?? '', pid: a.app.pid, bundle_id: a.app.bundle_id }}
            <button class="item" aria-pressed={sameSource(source, spec)} onclick={() => choose(spec)}>
              <Icon name="app" />
              <span class="name">{a.name}</span>
              <span class="dim">All windows</span>
            </button>
          {:else}
            <p class="muted empty">No apps to record.</p>
          {/each}
        {/if}
      </div>

      <div class="tools">
        <button onclick={() => run(() => beginRegion('start', sourceDisplay?.id ?? null))}><Icon name="region" size={14} />Region…</button>
        <button onclick={() => run(() => beginPick('start'))}><Icon name="pointer" size={14} />Click a Window</button>
        {#if caps?.system_picker}<button onclick={() => run(() => systemPicker('start'))} title="Choose with the system picker"><Icon name="picker" size={14} />System…</button>{/if}
      </div>
      {/if}

      {#if pickedDisplay}
        <div class="choice" role="group" aria-label="Picked display">
          <span>Record the whole display, or a region of it?</span>
          <div class="row">
            <button onclick={() => (pickedDisplay = null)}>Whole Display</button>
            <button class="primary" onclick={regionOfPicked}>Select Region…</button>
          </div>
        </div>
      {/if}
    </section>
    {/if}

    {#if settings}
      <section aria-labelledby="frame-h">
        <h2 id="frame-h">Frame</h2>
        <div class="group">
          <div class="grow stage-row">
            <div class="stage-well">
              <div
                class="preview"
                style:width="{previewBox.w}px"
                style:height="{previewBox.h}px"
                style:background={backgroundCss(settings.canvas.background)}
              >
                {#if still}<img src={still} alt="Preview of the frame" />{/if}
              </div>
            </div>
            {#if canvasPx && mode !== 'camera'}<p class="caption center tnum">{canvasPx.w} × {canvasPx.h} video</p>{/if}
          </div>
          <div class="grow">
            <span class="glabel">Shape</span>
            <div class="gcontrol wide">
              <div class="segmented" role="radiogroup" aria-label="Frame shape">
                {#each ASPECTS as a (a.value)}
                  <button
                    role="radio"
                    aria-checked={settings.canvas.aspect === a.value}
                    title={a.value === 'original' ? 'The first source’s shape' : a.label}
                    onclick={() => (settings!.canvas.aspect = a.value)}>{a.label}</button
                  >
                {/each}
              </div>
            </div>
          </div>
          {#if settings.canvas.aspect !== 'original'}
            <div class="grow">
              <span class="glabel">{mode === 'camera' ? 'Camera' : 'Picture'}</span>
              <div class="gcontrol">
                <div class="segmented" role="radiogroup" aria-label="How the picture fits the frame">
                  <button role="radio" aria-checked={(settings.canvas.fit ?? 'contain') === 'contain'} title="All of the picture, with the background around it" onclick={() => (settings!.canvas.fit = 'contain')}>Fit</button>
                  <button role="radio" aria-checked={settings.canvas.fit === 'cover'} title="Fill the frame, cropping the edges (for vertical video from a wide camera)" onclick={() => (settings!.canvas.fit = 'cover')}>Fill</button>
                </div>
              </div>
            </div>
          {/if}
          <div class="grow">
            <span class="glabel">Background</span>
            <div class="gcontrol">
              <button class="bg-button" aria-expanded={bgOpen} aria-haspopup="dialog" onclick={() => (bgOpen = !bgOpen)}>
                <span class="chip" style:background={backgroundCss(settings.canvas.background)}></span>
                <span class="bg-name">{backgroundSummary(settings.canvas.background)}</span>
              </button>
            </div>
          </div>
          <label class="grow">
            <span class="glabel">Show the Preview window</span>
            <input class="switch" type="checkbox" bind:checked={settings.preview_shown} />
          </label>
        </div>
        {#if isSmall}
          <p class="warn" role="status">
            <Icon name="warning" size={14} />
            <span>The source will look small in this frame. Try a shape closer to the source, or record a region.</span>
          </p>
        {/if}
        {#if bgOpen}
          <BackgroundEditor bind:canvas={settings.canvas} onclose={() => (bgOpen = false)} />
        {/if}
      </section>

      {#if mode !== 'screen'}
      <section aria-labelledby="cam-h">
        <h2 id="cam-h">Camera</h2>
        <div class="group">
          <div class="grow">
            <span class="glabel with-icon"><Icon name="camera" />Camera</span>
            <div class="gcontrol">
              <select id="camera" aria-label="Camera" bind:value={settings.camera}>
                {#each devices.cameras as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
              </select>
            </div>
          </div>
          <div class="grow">
            <span class="glabel">Quality</span>
            <div class="gcontrol">
              <select aria-label="Camera quality" value={settings.camera_quality ?? 'auto'} onchange={(e) => (settings!.camera_quality = e.currentTarget.value as 'auto')} title="The camera's sharpest format up to this; a camera that cannot reach the frame rate uses its fastest">
                <option value="auto">Auto (up to 1080p, 30 fps)</option>
                <option value="p720">720p, 30 fps</option>
                <option value="p1080x60">1080p, 60 fps</option>
                <option value="p2160">Up to 4K, 30 fps</option>
              </select>
            </div>
          </div>
          {#if mode === 'camera' && caps?.mic_modes}
            <div class="grow">
              <span class="glabel caption">The camera fills the frame.</span>
              <div class="gcontrol"><button onclick={() => run(() => api.showVideoEffects())}>Effects…</button></div>
            </div>
          {/if}
          {#if settings.camera && mode === 'both'}
            {@const k = settings.camera_key}
            <div class="grow stack">
              <span class="glabel">Shape</span>
              <div class="segmented shapes" role="radiogroup" aria-label="Camera shape">
                {#each CAMERA_SHAPES as sh (sh.value)}
                  <button role="radio" aria-checked={k.shape === sh.value} title={sh.title} onclick={() => setKey(shapeChange(k, sh.value))}>{sh.label}</button>
                {/each}
              </div>
            </div>
            {#if k.shape !== 'circle'}
              <div class="grow">
                <label class="glabel" for="radius">Corners</label>
                <div class="gcontrol"><input id="radius" class="slider" type="range" min="0" max="1" step="0.01" value={k.corner_radius} oninput={(e) => setKey({ corner_radius: Number(e.currentTarget.value) })} /></div>
              </div>
            {/if}
            <div class="grow">
              <span class="glabel">Size and place</span>
              <div class="gcontrol">
                <div class="segmented" role="radiogroup" aria-label="Camera size">
                  {#each ['S', 'M', 'L'] as label, i (label)}
                    <button role="radio" aria-checked={Math.abs(k.size_norm - CAMERA_SIZE_PRESETS[i]) < 0.005} onclick={() => setKey({ size_norm: CAMERA_SIZE_PRESETS[i] })}>{label}</button>
                  {/each}
                </div>
                <div class="corners" role="group" aria-label="Corner">
                  <button class="corner" title="Top left" aria-label="Top left" onclick={() => placeCorner('top_left')}><span class="tl"></span></button>
                  <button class="corner" title="Top right" aria-label="Top right" onclick={() => placeCorner('top_right')}><span class="tr"></span></button>
                  <button class="corner" title="Bottom left" aria-label="Bottom left" onclick={() => placeCorner('bottom_left')}><span class="bl"></span></button>
                  <button class="corner" title="Bottom right" aria-label="Bottom right" onclick={() => placeCorner('bottom_right')}><span class="br"></span></button>
                </div>
              </div>
            </div>
            <div class="grow stack">
              <div class="checks">
                <label><input type="checkbox" checked={k.visible} onchange={(e) => setKey({ visible: e.currentTarget.checked })} /> In the video</label>
                <label><input type="checkbox" checked={k.mirror} onchange={(e) => setKey({ mirror: e.currentTarget.checked })} /> Mirror</label>
                <label
                  ><input
                    type="checkbox"
                    checked={!!k.border}
                    onchange={(e) => setKey({ border: e.currentTarget.checked ? { width_px_1080: 6, color: [1, 1, 1, 1] } : null })}
                  /> Border</label
                >
                <label><input type="checkbox" checked={!!k.shadow} onchange={(e) => setKey({ shadow: e.currentTarget.checked ? { strength: 0.7 } : null })} /> Shadow</label>
              </div>
            </div>
            <div class="grow">
              <span class="glabel caption">Drag, resize and zoom the camera in the Preview window.</span>
              <div class="gcontrol">
                {#if k.zoom > 1 || k.pan_norm.x !== 0 || k.pan_norm.y !== 0}
                  <button onclick={() => setKey({ zoom: 1, pan_norm: { x: 0, y: 0 } })}>Reset Framing</button>
                {/if}
                {#if caps?.mic_modes}
                  <button onclick={() => run(() => api.showVideoEffects())}>Effects…</button>
                {/if}
              </div>
            </div>
          {/if}
        </div>
      </section>
      {/if}

      <section aria-labelledby="audio-h">
        <h2 id="audio-h">Audio</h2>
        <div class="group">
          <div class="grow">
            <span class="glabel with-icon"><Icon name="mic" />Microphone</span>
            <div class="gcontrol">
              <select id="mic" aria-label="Microphone" bind:value={settings.mic}>
                <option value={null}>None</option>
                {#each devices.mics as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
              </select>
              {#if caps?.mic_modes && settings.mic}
                <button class="icon" title="Mic Mode" aria-label="Mic Mode" onclick={() => run(() => api.showMicModes())}>⋯</button>
              {/if}
            </div>
          </div>
          {#if settings.mic && micDb !== null}
            <div class="meter" role="meter" aria-label="Microphone level" aria-valuemin={-60} aria-valuemax={0} aria-valuenow={Math.round(micDb)}>
              <span style:width={`${Math.max(0, Math.min(100, ((micDb + 60) / 60) * 100))}%`} class:hot={micDb > -3}></span>
            </div>
          {/if}
          <div class="grow">
            <span class="glabel with-icon"><Icon name="speaker" />System audio</span>
            <div class="gcontrol">
              <div class="segmented" role="radiogroup" aria-label="System audio">
                <button role="radio" aria-checked={settings.system_audio === 'off'} onclick={() => (settings!.system_audio = 'off')}>Off</button>
                <button role="radio" aria-checked={settings.system_audio === 'all'} onclick={() => (settings!.system_audio = 'all')}>All</button>
                <button
                  role="radio"
                  aria-checked={settings.system_audio === 'app'}
                  disabled={!!whyAppAudio}
                  title={whyAppAudio ?? 'Only the chosen app’s audio'}
                  onclick={() => (settings!.system_audio = 'app')}>App</button
                >
              </div>
            </div>
          </div>
        </div>
      </section>

      {#if mode !== 'camera'}
      <section aria-labelledby="vid-h">
        <h2 id="vid-h">Video</h2>
        <div class="group">
          <div class="grow">
            <span class="glabel">Frame rate</span>
            <div class="gcontrol">
              <div class="segmented" role="radiogroup" aria-label="Frame rate">
                <button role="radio" aria-checked={fpsChoice === '30'} onclick={() => setFps('30')}>30</button>
                <button role="radio" aria-checked={fpsChoice === '60'} disabled={!!why60} title={why60 ?? '60 fps'} onclick={() => setFps('60')}>60</button>
                <button role="radio" aria-checked={fpsChoice === '60-1x'} disabled={!!why60x1} title={why60x1 ?? '60 fps at 1×'} onclick={() => setFps('60-1x')}>60 at 1×</button>
              </div>
            </div>
          </div>
          <div class="grow">
            <span class="glabel">Resolution</span>
            <div class="gcontrol">
              <div class="segmented" role="radiogroup" aria-label="Resolution">
                <button
                  role="radio"
                  aria-checked={settings.resolution === 'native'}
                  disabled={settings.fps === 60 && !!why60}
                  onclick={() => (settings!.resolution = 'native')}>Retina</button
                >
                <button role="radio" aria-checked={settings.resolution === '1x'} onclick={() => (settings!.resolution = '1x')}>1×</button>
              </div>
            </div>
          </div>
          <label class="grow">
            <span class="glabel">Show the cursor</span>
            <input class="switch" type="checkbox" bind:checked={settings.cursor} />
          </label>
          {#if caps?.show_clicks || caps?.own_clicks}
            <label class="grow">
              <span class="glabel">Highlight clicks</span>
              <input class="switch" type="checkbox" bind:checked={settings.click_highlight} disabled={!settings.cursor} />
            </label>
          {/if}
        </div>
        {#if why60}<p class="caption note">60 fps: {why60}.</p>{/if}
        {#if why60x1 && why60x1 !== why60}<p class="caption note">60 fps at 1×: {why60x1}.</p>{/if}
      </section>
      {/if}

      <section aria-labelledby="after-h">
        <h2 id="after-h">When Recording Stops</h2>
        <div class="group">
          <div class="grow stack">
            <span class="glabel">Then</span>
            <div class="segmented shapes" role="radiogroup" aria-label="When recording stops">
                <button role="radio" aria-checked={(settings.after_recording ?? 'editor') === 'editor'} onclick={() => (settings!.after_recording = 'editor')}>Open Editor</button>
                <button role="radio" aria-checked={settings.after_recording === 'saved'} onclick={() => (settings!.after_recording = 'saved')}>Show Summary</button>
                <button role="radio" aria-checked={settings.after_recording === 'quick_export'} onclick={() => (settings!.after_recording = 'quick_export')} title="Export with the settings you used last">Export</button>
            </div>
          </div>
          <div class="grow">
            <label class="glabel" for="delete-raw">Move the recording to the Trash after a quick export</label>
            <div class="gcontrol"><input id="delete-raw" type="checkbox" class="switch" bind:checked={settings.delete_raw_after_export} /></div>
          </div>
        </div>
        <p class="caption note">The editor keeps its recording; only exports from the summary or right after recording move it.</p>
      </section>

      <section aria-labelledby="start-h">
        <h2 id="start-h">Starting and Shortcuts</h2>
        <div class="group">
          <div class="grow">
            <label class="glabel" for="countdown">Countdown</label>
            <div class="gcontrol">
              <select id="countdown" bind:value={settings.countdown_s}>
                <option value={0}>None</option>
                <option value={3}>3 seconds</option>
                <option value={5}>5 seconds</option>
                <option value={10}>10 seconds</option>
              </select>
            </div>
          </div>
          <div class="grow">
            <label class="glabel" for="debug-overlay">Show frame rate and drops while recording</label>
            <div class="gcontrol"><input id="debug-overlay" type="checkbox" class="switch" bind:checked={settings.debug_overlay} /></div>
          </div>
        </div>
        {#if shortcuts.length}
          <div class="group" role="group" aria-label="Shortcuts">
            {#each shortcuts as sc (sc.id)}
              <div class="grow">
                <span class="glabel">{sc.label}</span>
                <div class="gcontrol">
                  {#if sc.keys !== sc.default && capturing !== sc.id}
                    <button class="link small" onclick={() => shortcutsApi.set(sc.id, sc.default).then((r) => (shortcuts = r)).catch((e) => (error = errorText(e)))}>Default</button>
                  {/if}
                  <button
                    class="keycap"
                    class:capturing={capturing === sc.id}
                    class:bad={!!sc.error}
                    aria-label="{sc.label} shortcut"
                    title={sc.error ?? (capturing === sc.id ? 'Type the new shortcut. Delete clears it, Escape keeps it.' : 'Click to change')}
                    onclick={(e) => startCapture(e, sc.id)}
                    onkeydown={(e) => captureKey(e, sc.id)}
                    onblur={() => endCapture(sc.id, null)}
                  >{capturing === sc.id ? 'Type shortcut…' : keysLabel(sc.keys, mac)}</button>
                </div>
              </div>
              {#if sc.error}<p class="caption note bad-note">{sc.label}: {sc.error}</p>{/if}
            {/each}
          </div>
          <p class="caption note">Click a shortcut, then press the new keys. Delete clears it.</p>
        {/if}
      </section>
    {/if}

    {#if error}
      <p class="warn" role="alert"><Icon name="warning" size={14} /><span>{error}</span></p>
    {/if}
  </div>

  <footer>
    <div class="summary">
      {#if source}
        <span class="summary-name">{sourceLabel(source, all, devices.cameras)}{via === 'system_picker' ? ' (System Picker)' : ''}</span>
        <span class="caption tnum">
          {mode === 'camera' ? 'Camera only' : canvasPx ? `${canvasPx.w} × ${canvasPx.h}` : ''}{settings && mode !== 'camera' ? ` · ${settings.fps} fps` : ''}{mode === 'both' ? ' · Camera' : ''}{settings?.mic ? ' · Mic' : ''}{settings && settings.system_audio !== 'off' ? ' · System audio' : ''}
        </span>
      {:else}
        <span class="caption">Choose what to record.</span>
      {/if}
    </div>
    <button class="record" onclick={record} disabled={!settings?.source || recording || starting}>
      <span class="dot" aria-hidden="true"></span>
      {recording ? 'Recording' : starting ? 'Starting…' : 'Record'}
    </button>
  </footer>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .scroll {
    flex: 1 1 auto;
    overflow-y: auto;
    padding: 12px 14px 14px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .scroll > :global(*) {
    flex: none;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  section h2 {
    margin: 0 0 0 10px;
  }
  .modes {
    display: flex;
    width: 100%;
  }
  .modes button {
    min-height: 24px;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 9px;
    background: var(--warn-fill);
    box-shadow: none;
    text-align: left;
    justify-content: flex-start;
    color: var(--label);
    white-space: normal;
  }
  .banner-icon {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 7px;
    background: var(--orange);
    color: #fff;
  }
  .banner-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .banner-text strong {
    font-weight: 600;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 4px;
    max-height: 148px;
    overflow-y: auto;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    padding: 3px 7px;
    border-radius: 6px;
    background: transparent;
    box-shadow: none;
    text-align: left;
    justify-content: flex-start;
    color: var(--label);
    flex: none;
  }
  .item :global(.icon) {
    color: var(--accent-ink);
  }
  .item:hover:not([aria-pressed='true']) {
    background: var(--fill);
  }
  .item[aria-pressed='true'] {
    background: var(--accent);
    color: var(--on-accent);
  }
  .item[aria-pressed='true'] :global(.icon),
  .item[aria-pressed='true'] .dim,
  .item[aria-pressed='true'] .tag {
    color: inherit;
  }
  .item[aria-pressed='true'] .tag {
    background: rgba(255, 255, 255, 0.22);
  }
  .name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dim {
    flex: none;
    color: var(--label-2);
    font-size: 11px;
  }
  .tag {
    flex: none;
    font-size: 10px;
    font-weight: 600;
    padding: 0 5px;
    border-radius: 4px;
    color: var(--label-2);
    background: var(--fill);
  }
  .empty {
    padding: 8px 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .app-display {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 7px 5px;
  }
  .row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .tools {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(0, 1fr));
    gap: 6px;
  }
  .tools button {
    min-width: 0;
    font-size: 12px;
  }
  .tools :global(.icon) {
    color: var(--label-2);
  }
  .choice {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 9px 10px;
    border-radius: 9px;
    background: var(--group);
    box-shadow: 0 0 0 0.5px var(--group-border);
  }
  .stage-row {
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
    padding: 10px 10px 8px;
  }
  .stage-well {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 146px;
    border-radius: 7px;
    background: var(--fill);
  }
  .preview {
    flex: none;
    border-radius: 4px;
    overflow: hidden;
    box-shadow: 0 0 0 0.5px rgba(0, 0, 0, 0.2), 0 2px 8px rgba(0, 0, 0, 0.18);
  }
  .preview img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }
  .center {
    text-align: center;
  }
  .with-icon {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }
  .with-icon :global(.icon) {
    color: var(--label-2);
  }
  .gcontrol select {
    max-width: 190px;
  }
  .wide {
    max-width: 80%;
  }
  .bg-button {
    max-width: 200px;
  }
  .bg-name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chip {
    flex: none;
    width: 13px;
    height: 13px;
    border-radius: 3.5px;
    box-shadow: inset 0 0 0 0.5px rgba(0, 0, 0, 0.25);
  }
  .shapes {
    display: flex;
    width: 100%;
  }
  .shapes button {
    padding: 1px 4px;
  }
  .slider {
    width: 150px;
  }
  .corners {
    display: grid;
    grid-template-columns: repeat(2, 20px);
    grid-template-rows: repeat(2, 11px);
    gap: 2px;
  }
  .corner {
    min-height: 0;
    width: 20px;
    height: 11px;
    padding: 0;
    border-radius: 3px;
    position: relative;
  }
  .corner span {
    position: absolute;
    width: 5px;
    height: 4px;
    border-radius: 1px;
    background: var(--label-2);
  }
  .tl {
    top: 2px;
    left: 3px;
  }
  .tr {
    top: 2px;
    right: 3px;
  }
  .bl {
    bottom: 2px;
    left: 3px;
  }
  .br {
    bottom: 2px;
    right: 3px;
  }
  .checks {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px 12px;
    padding: 2px 0;
  }
  .checks label {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .note {
    margin: 0 10px;
  }
  footer {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px 12px;
    background: var(--window);
    box-shadow: 0 -0.5px 0 var(--separator);
  }
  .summary {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .summary-name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .record {
    flex: none;
    min-width: 104px;
    min-height: 30px;
    padding: 4px 16px;
    border-radius: 8px;
    font-weight: 600;
    gap: 8px;
    color: #fff;
    background: linear-gradient(rgba(255, 255, 255, 0.12), rgba(255, 255, 255, 0)), var(--red);
    box-shadow: 0 0 0 0.5px rgba(0, 0, 0, 0.15), 0 1px 2px rgba(0, 0, 0, 0.2);
  }
  .record:active:not(:disabled) {
    background: var(--red);
    filter: brightness(0.92);
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 0 0 2px rgba(255, 255, 255, 0.35);
  }
  .keycap {
    min-width: 72px;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.04em;
  }
  .keycap.capturing {
    box-shadow: 0 0 0 2px var(--accent);
    color: var(--label-2);
  }
  .keycap.bad {
    color: var(--orange);
  }
  .bad-note {
    color: var(--orange);
    margin: 0 10px;
  }
  .small {
    font-size: 11px;
  }
  .meter {
    height: 4px;
    margin: 0 10px 8px;
    border-radius: 2px;
    background: var(--fill-2);
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    background: var(--green);
    transition: width 60ms linear;
  }
  .meter span.hot {
    background: var(--orange);
  }
</style>
