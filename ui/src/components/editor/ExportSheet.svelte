<script lang="ts">
  import { editor } from '../../lib/editor.ts';
  import { formatBytes } from '../../lib/format.ts';
  import {
    AUDIO_LABELS,
    FORMAT_ALLOWED,
    GIF_FPS,
    VIDEO_FPS,
    VIDEO_LABELS,
    forFormat,
    type ExportEstimate,
    type ExportOptions,
    type ExportSettings,
    type Format,
  } from '../../lib/exporting.ts';

  let { onclose, onstarted }: { onclose: () => void; onstarted: (job: string) => void } = $props();

  let opts = $state<ExportOptions | null>(null);
  let st = $state<ExportSettings>(forFormat('mp4'));
  let est = $state<ExportEstimate | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let presetName = $state('');
  let customW = $state(0);
  let customH = $state(0);

  const ratio = $derived(opts ? opts.ratio[0] / opts.ratio[1] : 16 / 9);
  const isGif = $derived(st.format === 'gif');
  const allowed = $derived(FORMAT_ALLOWED[st.format]);
  const fpsList = $derived(isGif ? GIF_FPS : VIDEO_FPS);
  const sizeKey = $derived(st.size.kind === 'class' ? `class:${st.size.value}` : st.size.kind === 'original' ? 'original' : st.size.kind === 'long' ? `long:${st.size.value}` : 'custom');

  function avail(v: string, enc: 'best' | 'fast'): boolean {
    if (!opts) return true;
    if (v === 'gif') return true;
    return opts.available.video[v]?.[enc] ?? false;
  }

  async function load() {
    try {
      opts = (await editor.exportOptions()) as ExportOptions;
      const f = opts.store.last_format ?? 'mp4';
      st = opts.store.per_format[f] ? { ...forFormat(f), ...opts.store.per_format[f] } : forFormat(f);
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    void load();
  });

  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const snap = JSON.stringify(st);
    clearTimeout(timer);
    timer = setTimeout(async () => {
      try {
        est = (await editor.exportEstimate(JSON.parse(snap))) as ExportEstimate;
        error = null;
        if (st.size.kind !== 'width' && st.size.kind !== 'height') {
          customW = est.canvas.w;
          customH = est.canvas.h;
        }
      } catch (e) {
        est = null;
        error = String(e);
      }
    }, 120);
  });

  function setFormat(f: Format) {
    const saved = opts?.store.per_format[f];
    st = saved ? { ...forFormat(f), ...saved } : forFormat(f);
  }

  function setSize(key: string) {
    if (key === 'original') st.size = { kind: 'original' };
    else if (key.startsWith('class:')) st.size = { kind: 'class', value: Number(key.slice(6)) };
    else if (key.startsWith('long:')) st.size = { kind: 'long', value: Number(key.slice(5)) };
    else st.size = { kind: 'width', value: customW || est?.canvas.w || 1280 };
  }

  function typedW(w: number) {
    customW = w;
    customH = Math.round(w / ratio);
    st.size = { kind: 'width', value: w };
  }

  function typedH(h: number) {
    customH = h;
    customW = Math.round(h * ratio);
    st.size = { kind: 'height', value: h };
  }

  async function chooseFolder() {
    const p = await editor.chooseFolder();
    if (p) st.out_dir = p;
  }

  async function savePreset() {
    if (!presetName.trim()) return;
    opts = { ...opts!, store: (await editor.exportPresetSave(presetName.trim(), $state.snapshot(st))) as ExportOptions['store'] };
    presetName = '';
  }

  async function usePreset(name: string) {
    const p = opts?.store.presets.find((x) => x.name === name);
    if (p) st = { ...forFormat(p.settings.format), ...structuredClone(p.settings) };
  }

  async function deletePreset(name: string) {
    opts = { ...opts!, store: (await editor.exportPresetDelete(name)) as ExportOptions['store'] };
  }

  async function start() {
    busy = true;
    try {
      const job = await editor.exportStart($state.snapshot(st));
      onstarted(job);
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function seconds(s: number): string {
    if (s < 60) return `${Math.max(1, Math.round(s))} s`;
    return `${Math.round(s / 60)} min`;
  }

  const sizes = $derived.by(() => {
    const list: [string, string][] = [['original', opts ? `Original (${opts.reference.w} × ${opts.reference.h})` : 'Original']];
    if (isGif) list.push(['long:960', 'Long side 960 (GIF default)']);
    for (const c of isGif ? [1080, 720, 480, 360, 240] : [2160, 1440, 1080, 720, 480]) list.push([`class:${c}`, `${c}p`]);
    list.push(['custom', 'Custom…']);
    return list;
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose();
    }
  }
</script>

<div class="scrim" role="presentation" onpointerdown={onclose}></div>
<div class="sheet" role="dialog" aria-label="Export" tabindex="-1" {onkeydown}>
  <header>
    <h1>Export</h1>
    {#if opts?.store.presets.length}
      <select aria-label="Presets" value="" onchange={(e) => usePreset(e.currentTarget.value)}>
        <option value="" disabled>Presets</option>
        {#each opts.store.presets as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
      </select>
    {/if}
  </header>

  <div class="body">
    <div class="segmented formats" role="radiogroup" aria-label="Format">
      {#each [['mp4', 'MP4'], ['webm', 'WebM'], ['gif', 'GIF'], ['mov', 'MOV']] as [f, label] (f)}
        <button role="radio" aria-checked={st.format === f} onclick={() => setFormat(f as Format)}>{label}</button>
      {/each}
    </div>

    <div class="group">
      {#if !isGif}
        <div class="grow">
          <span class="glabel">Video</span>
          <span class="gcontrol">
            <select value={st.video} onchange={(e) => (st.video = e.currentTarget.value as ExportSettings['video'])} aria-label="Video codec">
              {#each allowed.video as v (v)}<option value={v} disabled={!avail(v, 'best') && !avail(v, 'fast')}>{VIDEO_LABELS[v]}</option>{/each}
            </select>
          </span>
        </div>
        <div class="grow">
          <span class="glabel">Audio</span>
          <span class="gcontrol">
            <select value={st.audio} onchange={(e) => (st.audio = e.currentTarget.value as ExportSettings['audio'])} aria-label="Audio codec">
              {#each allowed.audio as a (a)}<option value={a}>{AUDIO_LABELS[a]}</option>{/each}
            </select>
          </span>
        </div>
        <div class="grow">
          <span class="glabel">Encoder</span>
          <span class="gcontrol">
            <span class="segmented" role="radiogroup" aria-label="Encoder">
              <button role="radio" aria-checked={st.encoder === 'best'} disabled={!avail(st.video, 'best')} onclick={() => (st.encoder = 'best')}>Best Quality</button>
              <button role="radio" aria-checked={st.encoder === 'fast'} disabled={!avail(st.video, 'fast')} title={avail(st.video, 'fast') ? 'Hardware encoder' : 'No hardware encoder for this codec here'} onclick={() => (st.encoder = 'fast')}>Fast</button>
            </span>
          </span>
        </div>
        <div class="grow">
          <span class="glabel">Quality</span>
          <span class="gcontrol">
            <span class="segmented" role="radiogroup" aria-label="Quality">
              {#each [['small', 'Small'], ['balanced', 'Balanced'], ['high', 'High'], ['custom', 'Custom']] as [q, label] (q)}
                <button role="radio" aria-checked={st.quality.tier === q} onclick={() => (st.quality = q === 'custom' ? { tier: 'custom', crf: 23, kbps: null } : { tier: q as 'small' })}>{label}</button>
              {/each}
            </span>
          </span>
        </div>
        {#if st.quality.tier === 'custom'}
          {@const q = st.quality}
          <div class="grow">
            <span class="glabel">{q.kbps != null ? 'Bitrate' : 'Constant quality'}</span>
            <span class="gcontrol">
              {#if q.kbps != null}
                <input type="number" min="200" max="200000" step="100" value={q.kbps} oninput={(e) => (st.quality = { tier: 'custom', crf: null, kbps: Number(e.currentTarget.value) })} aria-label="Kilobits per second" /> kbps
              {:else}
                <input type="range" min="10" max="50" step="1" value={q.crf ?? 23} oninput={(e) => (st.quality = { tier: 'custom', crf: Number(e.currentTarget.value), kbps: null })} aria-label="CRF" />
                <span class="num tnum">CRF {q.crf}</span>
              {/if}
              <button class="plain" onclick={() => (st.quality = q.kbps != null ? { tier: 'custom', crf: 23, kbps: null } : { tier: 'custom', crf: null, kbps: 8000 })}>{q.kbps != null ? 'Use CRF' : 'Use Bitrate'}</button>
            </span>
          </div>
        {/if}
      {:else}
        <div class="grow">
          <span class="glabel">Encoder</span>
          <span class="gcontrol">
            <span class="segmented" role="radiogroup" aria-label="GIF encoder">
              <button role="radio" aria-checked={st.gif.mode === 'gifski'} disabled={opts ? !opts.available.gifski : false} onclick={() => (st.gif.mode = 'gifski')}>Best Quality (gifski)</button>
              <button role="radio" aria-checked={st.gif.mode === 'palette'} onclick={() => (st.gif.mode = 'palette')}>Palette</button>
            </span>
          </span>
        </div>
        {#if st.gif.mode === 'gifski'}
          {#each [['quality', 'Quality'], ['motion_quality', 'Motion quality'], ['lossy', 'Lossy quality']] as [k, label] (k)}
            <div class="grow">
              <span class="glabel">{label}</span>
              <span class="gcontrol">
                <input type="range" min="1" max="100" step="1" value={st.gif[k as 'quality']} oninput={(e) => (st.gif[k as 'quality'] = Number(e.currentTarget.value))} aria-label={label} />
                <span class="num tnum">{st.gif[k as 'quality']}</span>
              </span>
            </div>
          {/each}
        {:else}
          <div class="grow">
            <span class="glabel">Colours</span>
            <span class="gcontrol">
              <select value={st.gif.colors} onchange={(e) => (st.gif.colors = Number(e.currentTarget.value))} aria-label="Colours">
                {#each [32, 64, 128, 256] as c (c)}<option value={c}>{c}</option>{/each}
              </select>
              <label class="inline"><input type="checkbox" class="switch" bind:checked={st.gif.dither} /> Dither</label>
            </span>
          </div>
        {/if}
        <div class="grow">
          <span class="glabel">Loop</span>
          <span class="gcontrol">
            <select
              value={st.gif.repeat.kind}
              onchange={(e) => {
                const k = e.currentTarget.value;
                st.gif.repeat = k === 'times' ? { kind: 'times', times: 3 } : { kind: k as 'forever' };
              }}
              aria-label="Loop"
            >
              <option value="forever">Forever</option>
              <option value="once">Play once</option>
              <option value="times">Several times</option>
            </select>
            {#if st.gif.repeat.kind === 'times'}
              <input type="number" min="2" max="100" value={st.gif.repeat.times} oninput={(e) => (st.gif.repeat = { kind: 'times', times: Number(e.currentTarget.value) })} aria-label="Times" />
            {/if}
          </span>
        </div>
      {/if}
      <div class="grow">
        <span class="glabel">Frame rate</span>
        <span class="gcontrol">
          <select value={st.fps} onchange={(e) => (st.fps = Number(e.currentTarget.value))} aria-label="Frame rate">
            {#each fpsList as f (f)}<option value={f} disabled={!!opts && f > opts.recording_fps + 0.1}>{f} fps{isGif && f === 30 ? ' (about)' : ''}</option>{/each}
          </select>
        </span>
      </div>
      <div class="grow">
        <span class="glabel">Size</span>
        <span class="gcontrol">
          <select value={sizeKey} onchange={(e) => setSize(e.currentTarget.value)} aria-label="Size">
            {#each sizes as [k, label] (k)}<option value={k}>{label}</option>{/each}
          </select>
        </span>
      </div>
      {#if sizeKey === 'custom'}
        <div class="grow">
          <span class="glabel">Width × height</span>
          <span class="gcontrol">
            <input type="number" min="16" max="8192" value={customW} oninput={(e) => typedW(Number(e.currentTarget.value))} aria-label="Width" />
            <span>×</span>
            <input type="number" min="16" max="8192" value={customH} oninput={(e) => typedH(Number(e.currentTarget.value))} aria-label="Height" />
          </span>
        </div>
        <label class="grow">
          <span class="glabel">Allow upscaling</span>
          <input type="checkbox" class="switch" bind:checked={st.upscale} />
        </label>
      {/if}
    </div>

    <div class="group">
      <div class="grow">
        <span class="glabel">Save to</span>
        <span class="gcontrol path">
          <span class="ell muted">{st.out_dir ?? opts?.default_dir ?? 'Next to the recording'}</span>
          <button onclick={chooseFolder}>Choose…</button>
        </span>
      </div>
      <div class="grow">
        <span class="glabel">Name</span>
        <span class="gcontrol"><input type="text" bind:value={st.name} aria-label="File name" spellcheck="false" /></span>
      </div>
      <div class="grow">
        <span class="caption muted">Name tokens: {'{title} {date} {time} {size} {fps} {format}'}</span>
      </div>
    </div>

    <div class="estimate" aria-live="polite">
      {#if est}
        <span class="tnum">{est.canvas.w} × {est.canvas.h} · {est.fps} fps · about {formatBytes(est.bytes_low)}–{formatBytes(est.bytes_high)} · about {seconds(est.seconds)}</span>
        <span class="caption ell">{est.file}</span>
        {#if est.less_compatible}<span class="warn small">{est.less_compatible}</span>{/if}
        {#if est.too_large_for_fast}<span class="warn small">The hardware H.264 encoder stops at 4096 pixels; choose a smaller size, HEVC, or Best Quality.</span>{/if}
      {/if}
      {#if error}<span class="warn small">{error}</span>{/if}
    </div>
  </div>

  <footer>
    <span class="preset">
      <input type="text" placeholder="Preset name" bind:value={presetName} aria-label="Preset name" />
      <button onclick={savePreset} disabled={!presetName.trim()}>Save Preset</button>
      {#if opts?.store.presets.length}
        <select aria-label="Delete a preset" value="" onchange={(e) => deletePreset(e.currentTarget.value)}>
          <option value="" disabled>Delete…</option>
          {#each opts.store.presets as p (p.name)}<option value={p.name}>{p.name}</option>{/each}
        </select>
      {/if}
    </span>
    <span class="spacer"></span>
    <button onclick={onclose}>Cancel</button>
    <button class="primary" onclick={start} disabled={busy || !est || est.too_large_for_fast}>Export</button>
  </footer>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.25);
    z-index: 20;
  }
  .sheet {
    position: fixed;
    top: 52px;
    left: 50%;
    transform: translateX(-50%);
    width: 520px;
    max-height: calc(100vh - 70px);
    display: flex;
    flex-direction: column;
    background: var(--window);
    border-radius: 0 0 12px 12px;
    box-shadow: var(--shadow-pop);
    z-index: 21;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 16px 6px;
  }
  .body {
    overflow-y: auto;
    padding: 6px 16px 10px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .formats {
    display: flex;
  }
  .gcontrol input[type='number'] {
    width: 72px;
  }
  .gcontrol input[type='text'] {
    width: 220px;
  }
  .gcontrol input[type='range'] {
    width: 130px;
  }
  .num {
    min-width: 50px;
    color: var(--label-2);
    font-size: 11px;
  }
  .inline {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .path {
    max-width: 70%;
  }
  .ell {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .estimate {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 0 4px;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px 14px;
    border-top: 1px solid var(--separator);
  }
  .preset {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .preset input {
    width: 110px;
  }
  .spacer {
    flex: 1;
  }
</style>
