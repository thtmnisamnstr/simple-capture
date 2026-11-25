<script lang="ts">
  import { SPEEDS, formatTime, keyAt, sameSpeed, speedLabel, trackName, tlLen, type Doc, type EditOp, type EditorTrack, type ProjectCanvas } from '../../lib/editor.ts';
  import { CAMERA_SHAPES, shapeAspect, shapeChange } from '../../lib/camera.ts';
  import { CAMERA_SIZE_MAX, CAMERA_SIZE_MIN, CAMERA_SIZE_PRESETS, CAMERA_ZOOM_MAX, cornerCenter, type Corner } from '../../lib/geometry.ts';
  import { GRADIENT_PRESETS, SOLID_PALETTE, backgroundCss, hexToRgba, linear, rgbaToHex } from '../../lib/backgrounds.ts';
  import type { CameraKey } from '../../lib/types.ts';
  import { api, errorText } from '../../lib/api.ts';
  import { editor } from '../../lib/editor.ts';
  import TextInspector from './TextInspector.svelte';
  import type { FontInfo, TextItemFull } from '../../lib/text.ts';
  import type { Rgba } from '../../lib/types.ts';

  let {
    doc,
    tracks,
    selected,
    recNs,
    canvasPx,
    segmentId = null,
    segmentCount = 1,
    tab = $bindable('clip'),
    onedit,
    onsplit,
    onduplicate,
    ondelete,
    text = null,
    fonts = [],
    system = [],
    unresolved = 0,
    onaddtext,
    onselecttext,
    ontext,
    ontextduplicate,
    ontextorder,
    ontextdelete,
    onpick,
  }: {
    doc: Doc;
    tracks: EditorTrack[];
    selected: string[];
    recNs: number;
    canvasPx: { w: number; h: number };
    segmentId?: string | null;
    segmentCount?: number;
    tab?: 'clip' | 'text' | 'camera' | 'audio' | 'frame';
    onedit: (op: EditOp, coalesce?: string) => void;
    onsplit: () => void;
    onduplicate: () => void;
    ondelete: () => void;
    text?: TextItemFull | null;
    fonts?: FontInfo[];
    system?: string[];
    unresolved?: number;
    onaddtext: () => void;
    onselecttext: (id: string) => void;
    ontext: (item: TextItemFull, coalesce?: string) => void;
    ontextduplicate: () => void;
    ontextorder?: (dir: 1 | -1) => void;
    ontextdelete: () => void;
    onpick: () => Promise<Rgba | null>;
  } = $props();

  let error = $state<string | null>(null);

  const onCamera = (id: string) => !!doc.camera_edl?.clips.some((c) => c.id === id);
  const picked = $derived([...doc.edl.clips, ...(doc.camera_edl?.clips ?? [])].filter((c) => selected.includes(c.id)));
  const chosen = $derived(picked.filter((c) => !c.hidden));
  const gaps = $derived(picked.length - chosen.length);
  const overlay = $derived(doc.camera_layout.overlays[0] ?? null);
  const key = $derived(overlay ? keyAt(overlay.keyframes, recNs) : null);
  const keyHere = $derived(!!overlay && overlay.keyframes.some((k) => Math.abs(k.t_rec_ns - recNs) < 1_000_000));
  const audio = $derived(tracks.filter((t) => t.kind === 'mic' || t.kind === 'system_audio' || t.kind === 'device_audio'));
  const canvas = $derived(doc.output.canvas);

  function setKey(patch: Partial<CameraKey>, coalesce?: string) {
    if (!overlay || !key) return;
    onedit({ op: 'set_camera_key', overlay_id: overlay.id, key: { ...key, ...patch } }, coalesce);
  }

  function addKey() {
    if (!overlay || !key) return;
    onedit({ op: 'set_camera_key', overlay_id: overlay.id, key: { ...key, t_rec_ns: recNs, easing: 'linear' } });
  }

  function corner(c: Corner) {
    if (!key) return;
    setKey({ center_norm: cornerCenter(canvasPx, shapeAspect(key.shape), key.size_norm, c) });
  }

  const ownFraming = $derived(segmentId ? (doc.output.segment_framing?.[segmentId] ?? null) : null);

  function setFraming(fit: 'default' | 'contain' | 'cover', focus = ownFraming?.focus_norm ?? { x: 0.5, y: 0.5 }, coalesce?: string) {
    if (!segmentId) return;
    onedit(
      { op: 'set_segment_framing', segment_id: segmentId, framing: fit === 'default' ? null : { fit, focus_norm: focus } },
      coalesce,
    );
  }

  function setCanvas(patch: Partial<ProjectCanvas>) {
    onedit({ op: 'set_canvas', canvas: { ...canvas, ...patch } });
  }

  async function chooseImage() {
    error = null;
    try {
      const path = await api.chooseBackgroundImage();
      if (path) setCanvas({ background: await editor.backgroundImage(path) });
    } catch (e) {
      error = errorText(e);
    }
  }

  const ASPECTS: ProjectCanvas['aspect'][] = ['original', '16:9', '4:3', '1:1', '4:5', '9:16'];
  const mix = (id: string) => doc.audio_mix[id] ?? { gain_db: 0, muted: false };
</script>

<div class="inspector">
  <div class="segmented tabs" role="tablist" aria-label="Inspector">
    {#each [['clip', 'Clip'], ['text', 'Text'], ['camera', 'Camera'], ['audio', 'Audio'], ['frame', 'Frame']] as [id, label] (id)}
      <button role="tab" aria-selected={tab === id} onclick={() => (tab = id as typeof tab)}>{label}</button>
    {/each}
  </div>

  <div class="body">
    {#if tab === 'clip'}
      {#if chosen.length === 0 && gaps > 0}
        <p class="empty">{gaps === 1 ? 'A gap' : `${gaps} gaps`}: nothing shows or plays here. Delete closes it up.</p>
        <div class="actions">
          <button class="destructive" onclick={ondelete}>Close Gap</button>
        </div>
      {:else if chosen.length === 0}
        <p class="empty">Select a clip on the timeline to change its speed or mute it. Split at the playhead with S.</p>
        <div class="group">
          <div class="grow"><span class="glabel">Clips</span><span class="gcontrol muted tnum">{doc.edl.clips.length}</span></div>
          <div class="grow"><span class="glabel">Length</span><span class="gcontrol muted tnum">{formatTime(doc.edl.clips.reduce((n, c) => n + tlLen(c), 0))}</span></div>
        </div>
      {:else}
        <h2>{chosen.length === 1 ? (onCamera(chosen[0].id) ? 'Camera clip' : doc.camera_edl ? 'Screen clip' : 'Clip') : `${chosen.length} clips`}</h2>
        <div class="group">
          <div class="grow stack">
            <span class="glabel">Speed</span>
            <div class="segmented" role="radiogroup" aria-label="Speed">
              {#each SPEEDS as s (s.label)}
                <button
                  role="radio"
                  aria-checked={chosen.every((c) => sameSpeed(c.speed, s.speed))}
                  onclick={() => onedit({ op: 'set_speed', ids: chosen.map((c) => c.id), speed: s.speed })}>{s.label}</button
                >
              {/each}
            </div>
            <span class="caption">Pitch stays the same at every speed.</span>
          </div>
          <div class="grow">
            <span class="glabel">Mute clip</span>
            <span class="gcontrol"
              ><input
                type="checkbox"
                class="switch"
                checked={chosen.every((c) => c.muted)}
                onchange={(e) => onedit({ op: 'set_muted', ids: chosen.map((c) => c.id), muted: e.currentTarget.checked })}
              /></span
            >
          </div>
          {#if chosen.length === 1}
            <div class="grow">
              <span class="glabel">Length</span>
              <span class="gcontrol muted tnum">{formatTime(tlLen(chosen[0]))}{chosen[0].speed.num !== chosen[0].speed.den ? ` at ${speedLabel(chosen[0].speed)}` : ''}</span>
            </div>
            <div class="grow">
              <span class="glabel">From the recording</span>
              <span class="gcontrol muted tnum">{formatTime(chosen[0].src_in_rec_ns)} – {formatTime(chosen[0].src_out_rec_ns)}</span>
            </div>
          {/if}
        </div>
        <div class="actions">
          <button onclick={onsplit}>Split at Playhead</button>
          <button onclick={onduplicate}>Duplicate</button>
          <button class="destructive" onclick={ondelete}>Delete</button>
        </div>
      {/if}
    {:else if tab === 'text'}
      {#if text}
        <TextInspector item={text} {fonts} {system} {unresolved} onchange={ontext} onduplicate={ontextduplicate} onorder={ontextorder} ondelete={ontextdelete} {onpick} />
      {:else}
        <button class="primary add" onclick={onaddtext}>Add Text at Playhead</button>
        <p class="empty">Or press T. Double-click text in the preview to type on it.</p>
        {#if doc.text.length}
          <div class="group">
            {#each [...doc.text].sort((a, b) => a.start_tl_ns - b.start_tl_ns) as t (t.id)}
              <button class="grow listrow" onclick={() => onselecttext(t.id)}>
                <span class="glabel ell">{t.content.split('\n')[0] || 'Text'}</span>
                <span class="gcontrol muted tnum">{formatTime(t.start_tl_ns)}</span>
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    {:else if tab === 'camera'}
      {#if !overlay || !key}
        <p class="empty">This recording has no camera.</p>
      {:else}
        <div class="group">
          <div class="grow">
            <span class="glabel">Show camera</span>
            <span class="gcontrol"><input type="checkbox" class="switch" checked={key.visible} onchange={(e) => setKey({ visible: e.currentTarget.checked })} /></span>
          </div>
          <div class="grow stack">
            <span class="glabel">Shape</span>
            <div class="segmented wide" role="radiogroup" aria-label="Shape">
              {#each CAMERA_SHAPES as s (s.value)}
                <button role="radio" title={s.title} aria-checked={key.shape === s.value} onclick={() => setKey(shapeChange(key, s.value))}>{s.label}</button>
              {/each}
            </div>
          </div>
          {#if key.shape === 'rounded'}
            <div class="grow">
              <span class="glabel">Corners</span>
              <span class="gcontrol"
                ><input type="range" min="0.05" max="1" step="0.01" value={key.corner_radius} oninput={(e) => setKey({ corner_radius: Number(e.currentTarget.value) }, 'cam-radius')} /></span
              >
            </div>
          {/if}
          <div class="grow">
            <span class="glabel">Size</span>
            <span class="gcontrol">
              <span class="segmented">
                {#each CAMERA_SIZE_PRESETS as p, i (p)}
                  <button aria-pressed={Math.abs(key.size_norm - p) < 0.005} onclick={() => setKey({ size_norm: p })}>{['S', 'M', 'L'][i]}</button>
                {/each}
              </span>
              <input type="range" min={CAMERA_SIZE_MIN} max={CAMERA_SIZE_MAX} step="0.01" value={key.size_norm} oninput={(e) => setKey({ size_norm: Number(e.currentTarget.value) }, 'cam-size')} />
            </span>
          </div>
          <div class="grow">
            <span class="glabel">Position</span>
            <span class="gcontrol corners">
              {#each [['top_left', '↖'], ['top_right', '↗'], ['bottom_left', '↙'], ['bottom_right', '↘']] as [c, label] (c)}
                <button class="icon" onclick={() => corner(c as Corner)} aria-label={c.replace('_', ' ')}>{label}</button>
              {/each}
            </span>
          </div>
          <div class="grow">
            <span class="glabel">Zoom</span>
            <span class="gcontrol"
              ><input type="range" min="1" max={CAMERA_ZOOM_MAX} step="0.01" value={key.zoom} oninput={(e) => setKey({ zoom: Number(e.currentTarget.value) }, 'cam-zoom')} />
              <button class="plain" onclick={() => setKey({ zoom: 1, pan_norm: { x: 0, y: 0 } })}>Reset</button></span
            >
          </div>
          <div class="grow">
            <span class="glabel">Mirror</span>
            <span class="gcontrol"><input type="checkbox" class="switch" checked={key.mirror} onchange={(e) => setKey({ mirror: e.currentTarget.checked })} /></span>
          </div>
          <div class="grow">
            <span class="glabel">Border</span>
            <span class="gcontrol"
              ><input
                type="checkbox"
                class="switch"
                checked={!!key.border}
                onchange={(e) => setKey({ border: e.currentTarget.checked ? { width_px_1080: 6, color: [1, 1, 1, 1] } : null })}
              /></span
            >
          </div>
          <div class="grow">
            <span class="glabel">Shadow</span>
            <span class="gcontrol"
              ><input
                type="range"
                min="0"
                max="1"
                step="0.05"
                value={key.shadow?.strength ?? 0}
                oninput={(e) => {
                  const v = Number(e.currentTarget.value);
                  setKey({ shadow: v > 0 ? { strength: v } : null }, 'cam-shadow');
                }}
              /></span
            >
          </div>
        </div>
        <h2>Keyframes</h2>
        <div class="group">
          <div class="grow">
            <span class="glabel">{overlay.keyframes.length} {overlay.keyframes.length === 1 ? 'keyframe' : 'keyframes'}</span>
            <span class="gcontrol muted caption">Changes apply to the one in effect at the playhead</span>
          </div>
        </div>
        <div class="actions">
          <button onclick={addKey} disabled={keyHere}>Add at Playhead</button>
          <button
            disabled={!keyHere || overlay.keyframes.length < 2}
            onclick={() => onedit({ op: 'delete_camera_key', overlay_id: overlay.id, t_rec_ns: key.t_rec_ns })}>Delete</button
          >
          <button disabled={overlay.keyframes.length < 2} onclick={() => onedit({ op: 'set_camera_static', overlay_id: overlay.id, key })}>Use Everywhere</button>
        </div>
      {/if}
    {:else if tab === 'audio'}
      {#if audio.length === 0}
        <p class="empty">This recording has no audio.</p>
      {:else}
        <div class="group">
          {#each audio as tr (tr.id)}
            {@const m = mix(tr.id)}
            <div class="grow stack">
              <span class="row">
                <span class="glabel">{trackName(tr.kind)}</span>
                <span class="muted tnum">{m.muted ? 'Muted' : `${m.gain_db > 0 ? '+' : ''}${m.gain_db.toFixed(0)} dB`}</span>
                <input
                  type="checkbox"
                  class="switch"
                  aria-label="{trackName(tr.kind)} on"
                  checked={!m.muted}
                  onchange={(e) => onedit({ op: 'set_track_mix', track_id: tr.id, gain_db: m.gain_db, muted: !e.currentTarget.checked })}
                />
              </span>
              <input
                type="range"
                min="-30"
                max="12"
                step="1"
                value={m.gain_db}
                disabled={m.muted}
                aria-label="{trackName(tr.kind)} volume"
                oninput={(e) => onedit({ op: 'set_track_mix', track_id: tr.id, gain_db: Number(e.currentTarget.value), muted: false }, `gain-${tr.id}`)}
                ondblclick={() => onedit({ op: 'set_track_mix', track_id: tr.id, gain_db: 0, muted: false })}
              />
            </div>
          {/each}
        </div>
        <p class="caption note">Cuts get short fades so they never click.</p>
      {/if}
    {:else}
      <h2>Frame shape</h2>
      <div class="group">
        <div class="grow stack">
          <div class="segmented" role="radiogroup" aria-label="Frame shape">
            {#each ASPECTS as a (a)}
              <button role="radio" aria-checked={canvas.aspect === a} onclick={() => setCanvas({ aspect: a })}>{a === 'original' ? 'Original' : a}</button>
            {/each}
          </div>
          <span class="caption">{canvas.fit === 'cover' ? 'The recording fills the frame; its edges are cropped.' : 'The recording fits inside; the rest shows the background.'}</span>
        </div>
        <div class="grow">
          <span class="glabel">Picture</span>
          <span class="gcontrol">
            <span class="segmented" role="radiogroup" aria-label="How the picture fits the frame">
              <button role="radio" aria-checked={canvas.fit !== 'cover'} onclick={() => setCanvas({ fit: 'contain' })}>Fit</button>
              <button role="radio" aria-checked={canvas.fit === 'cover'} onclick={() => setCanvas({ fit: 'cover' })}>Fill</button>
            </span>
          </span>
        </div>
      </div>
      {#if segmentId}
        <h2>{segmentCount > 1 ? 'This section' : 'Picture'}</h2>
        <div class="group">
          <div class="grow stack">
            <div class="segmented" role="radiogroup" aria-label="How this section fits the frame">
              <button role="radio" aria-checked={!ownFraming} onclick={() => setFraming('default')}>Default</button>
              <button role="radio" aria-checked={ownFraming?.fit === 'contain'} onclick={() => setFraming('contain')}>Fit</button>
              <button role="radio" aria-checked={ownFraming?.fit === 'cover'} onclick={() => setFraming('cover')}>Fill</button>
            </div>
            <span class="caption">
              {segmentCount > 1
                ? 'The part of the recording under the playhead, from one source to the next switch. Default follows the frame setting above.'
                : 'Default follows the frame setting above.'}
            </span>
          </div>
          {#if ownFraming?.fit === 'cover'}
            {@const f = ownFraming.focus_norm}
            <div class="grow">
              <span class="glabel">Keep</span>
              <span class="gcontrol">
                <span class="caption">Left</span>
                <input type="range" min="0" max="1" step="0.01" value={f.x} aria-label="Horizontal crop position" oninput={(e) => setFraming('cover', { x: Number(e.currentTarget.value), y: f.y }, 'focus-x')} />
                <span class="caption">Right</span>
              </span>
            </div>
            <div class="grow">
              <span class="glabel"></span>
              <span class="gcontrol">
                <span class="caption">Top</span>
                <input type="range" min="0" max="1" step="0.01" value={f.y} aria-label="Vertical crop position" oninput={(e) => setFraming('cover', { x: f.x, y: Number(e.currentTarget.value) }, 'focus-y')} />
                <span class="caption">Bottom</span>
              </span>
            </div>
          {/if}
        </div>
      {/if}
      <h2>Background</h2>
      <div class="group">
        <div class="grow stack">
          <div class="swatches">
            {#each SOLID_PALETTE as hex (hex)}
              <button
                class="swatch"
                aria-label={hex}
                aria-pressed={canvas.background.kind === 'solid' && rgbaToHex(canvas.background.color) === hex}
                style:background={hex}
                onclick={() => setCanvas({ background: { kind: 'solid', color: hexToRgba(hex) } })}
              ></button>
            {/each}
          </div>
          <div class="swatches">
            {#each GRADIENT_PRESETS as p (p.name)}
              <button
                class="swatch"
                title={p.name}
                aria-label={p.name}
                style:background={backgroundCss(linear(p.from, p.to, 135))}
                onclick={() => setCanvas({ background: linear(p.from, p.to, 135) })}
              ></button>
            {/each}
          </div>
          <div class="row">
            <button onclick={chooseImage}>Image…</button>
            <span class="caption">PNG or JPEG, fills the frame</span>
          </div>
        </div>
      </div>
      {#if error}<p class="warn" role="alert">{error}</p>{/if}
    {/if}
  </div>
</div>

<style>
  .inspector {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .tabs {
    margin: 10px 12px 6px;
    display: flex;
  }
  .body {
    flex: 1;
    overflow-y: auto;
    padding: 6px 12px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  h2 {
    margin: 6px 0 -4px 10px;
  }
  .empty {
    color: var(--label-2);
    padding: 6px 4px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .grow.stack .row .glabel {
    flex: 1;
  }
  .grow.stack input[type='range'] {
    width: 100%;
  }
  .gcontrol input[type='range'] {
    width: 110px;
  }
  .segmented.wide {
    display: flex;
  }
  .segmented.wide button {
    padding: 1px 4px;
  }
  .corners {
    gap: 4px;
  }
  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .swatch {
    width: 22px;
    height: 22px;
    min-height: 22px;
    padding: 0;
    border-radius: 6px;
    box-shadow: inset 0 0 0 0.5px rgba(0, 0, 0, 0.25);
  }
  .swatch[aria-pressed='true'] {
    box-shadow: 0 0 0 2px var(--window), 0 0 0 4px var(--accent);
  }
  .note {
    color: var(--label-2);
    padding: 0 4px;
  }
  .add {
    align-self: flex-start;
  }
  .listrow {
    width: 100%;
    border-radius: 0;
    box-shadow: none;
    background: transparent;
    justify-content: space-between;
  }
  .ell {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
</style>
