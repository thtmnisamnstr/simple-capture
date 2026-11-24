<script lang="ts">
  import { hexToRgba, rgbaToHex, SOLID_PALETTE } from '../../lib/backgrounds.ts';
  import { formatTime } from '../../lib/editor.ts';
  import {
    EASINGS,
    EMPHASIS,
    IN_ANIMS,
    OUT_ANIMS,
    PRESETS,
    WEIGHTS,
    applyPreset,
    fillColor,
    plainCopy,
    recentStyles,
    rememberStyle,
    rgbaCss,
    type FontInfo,
    type TextItemFull,
    type TextStyle,
  } from '../../lib/text.ts';
  import type { Rgba } from '../../lib/types.ts';

  let {
    item,
    fonts,
    system,
    onchange,
    onduplicate,
    ondelete,
    onpick,
    unresolved = 0,
  }: {
    item: TextItemFull;
    fonts: FontInfo[];
    system: string[];
    onchange: (next: TextItemFull, coalesce?: string) => void;
    onduplicate: () => void;
    ondelete: () => void;
    onpick: () => Promise<Rgba | null>;
    unresolved?: number;
  } = $props();

  let recent = $state<TextStyle[]>(recentStyles());
  const s = $derived(item.style);
  const font = $derived(fonts.find((f) => f.name.toLowerCase() === s.font_family.toLowerCase()) ?? null);
  const weights = $derived(
    font
      ? WEIGHTS.filter(([w]) => (font.variable ? w >= font.weights[0] && w <= font.weights[1] : font.static_weights.includes(w)))
      : WEIGHTS,
  );
  const hex = (c: Rgba) => rgbaToHex(c);
  const gradient = $derived('linear' in s.fill);

  function setStyle(patch: Partial<TextStyle>, coalesce?: string) {
    onchange({ ...item, style: { ...s, ...patch } }, coalesce);
  }

  function setFamily(name: string) {
    const f = fonts.find((x) => x.name === name);
    const weight = f ? (f.variable ? Math.min(Math.max(s.weight, f.weights[0]), f.weights[1]) : f.static_weights.includes(s.weight) ? s.weight : f.default_weight) : s.weight;
    setStyle({ font_family: name, weight, italic: f && !f.italic ? false : s.italic });
  }

  function setColor(c: Rgba, coalesce?: string) {
    if ('linear' in s.fill) setStyle({ fill: { linear: { ...s.fill.linear, stops: [c, s.fill.linear.stops[1]] } } }, coalesce);
    else setStyle({ fill: { solid: c } }, coalesce);
  }

  function setGradient(on: boolean) {
    const c = fillColor(s.fill);
    setStyle({ fill: on ? { linear: { stops: [c, [0.04, 0.52, 1, 1]], angle_deg: 0 } } : { solid: c } });
  }

  async function pick(target: 'fill' | 'end') {
    const c = await onpick();
    if (!c) return;
    if (target === 'end' && 'linear' in s.fill) setStyle({ fill: { linear: { ...s.fill.linear, stops: [s.fill.linear.stops[0], c] } } });
    else setColor(c);
  }

  function preset(p: (typeof PRESETS)[number]) {
    const style = applyPreset(s, p);
    onchange({ ...item, style, anim: p.anim ? plainCopy(p.anim) : item.anim });
    rememberStyle(style);
    recent = recentStyles();
  }

  function useRecent(st: TextStyle) {
    onchange({ ...item, style: plainCopy(st) });
  }

  function setAnim(which: 'in' | 'out', kind: string) {
    const cur = item.anim[which];
    onchange({ ...item, anim: { ...item.anim, [which]: kind ? { kind, duration_s: cur?.duration_s ?? (kind === 'typewriter' ? 1.2 : 0.35), easing: cur?.easing ?? 'ease_out' } : null } });
  }

  function patchAnim(which: 'in' | 'out', patch: Record<string, unknown>, coalesce?: string) {
    const cur = item.anim[which];
    if (!cur) return;
    onchange({ ...item, anim: { ...item.anim, [which]: { ...cur, ...patch } } }, coalesce);
  }

  function setPos(x: number, y: number) {
    onchange({ ...item, transform: { ...item.transform, center_norm: { x, y } } });
  }
</script>

<div class="text-insp">
  <label class="content">
    <span class="sr-only">Text</span>
    <textarea
      rows="3"
      value={item.content}
      oninput={(e) => onchange({ ...item, content: e.currentTarget.value }, `text-${item.id}`)}
      placeholder="Type something"
    ></textarea>
  </label>
  {#if unresolved > 0}<p class="warn small">{unresolved} character{unresolved === 1 ? '' : 's'} not in {s.font_family}; they don't show. Try another font.</p>{/if}

  <div class="presets" role="group" aria-label="Styles">
    {#each PRESETS as p (p.name)}
      <button class="preset" title={p.name} onclick={() => preset(p)}><span style={p.css}>{p.name}</span></button>
    {/each}
  </div>
  {#if recent.length}
    <div class="recent" role="group" aria-label="Recent styles">
      <span class="caption">Recent</span>
      {#each recent as r, i (i)}
        <button class="swatch" title="{r.font_family} {r.weight}" style:background={rgbaCss(fillColor(r.fill))} onclick={() => useRecent(r)}>
          <span style:font-family={r.font_family}>A</span>
        </button>
      {/each}
    </div>
  {/if}

  <h2>Font</h2>
  <div class="group">
    <div class="grow">
      <span class="glabel">Family</span>
      <span class="gcontrol">
        <select value={s.font_family} onchange={(e) => setFamily(e.currentTarget.value)} aria-label="Font">
          <optgroup label="Included">
            {#each fonts as f (f.name)}<option value={f.name}>{f.name} — {f.kind}</option>{/each}
          </optgroup>
          {#if system.length}
            <optgroup label="On this Mac">
              {#each system as name (name)}<option value={name}>{name}</option>{/each}
            </optgroup>
          {/if}
        </select>
      </span>
    </div>
    <div class="grow">
      <span class="glabel">Weight</span>
      <span class="gcontrol">
        <select value={s.weight} onchange={(e) => setStyle({ weight: Number(e.currentTarget.value) })} aria-label="Weight">
          {#each weights as [w, label] (w)}<option value={w}>{label}</option>{/each}
        </select>
        <span class="segmented">
          <button aria-pressed={s.italic} disabled={!!font && !font.italic} title="Italic" onclick={() => setStyle({ italic: !s.italic })}><i>I</i></button>
          <button aria-pressed={s.uppercase} title="All caps" onclick={() => setStyle({ uppercase: !s.uppercase })}>AA</button>
        </span>
      </span>
    </div>
    <div class="grow">
      <span class="glabel">Size</span>
      <span class="gcontrol"><input type="range" min="20" max="320" step="1" value={s.size_1080} oninput={(e) => setStyle({ size_1080: Number(e.currentTarget.value) }, 'size')} /><span class="num tnum">{Math.round(s.size_1080)}</span></span>
    </div>
    <div class="grow">
      <span class="glabel">Align</span>
      <span class="gcontrol">
        <span class="segmented" role="radiogroup" aria-label="Alignment">
          {#each [['left', 'Left'], ['center', 'Centre'], ['right', 'Right']] as [a, label] (a)}
            <button role="radio" aria-checked={s.align === a} onclick={() => setStyle({ align: a as TextStyle['align'] })}>{label}</button>
          {/each}
        </span>
      </span>
    </div>
    <div class="grow">
      <span class="glabel">Line height</span>
      <span class="gcontrol"><input type="range" min="0.8" max="2" step="0.05" value={s.line_height} oninput={(e) => setStyle({ line_height: Number(e.currentTarget.value) }, 'lh')} /><span class="num tnum">{s.line_height.toFixed(2)}</span></span>
    </div>
    <div class="grow">
      <span class="glabel">Letter spacing</span>
      <span class="gcontrol"><input type="range" min="-10" max="40" step="0.5" value={s.letter_spacing} oninput={(e) => setStyle({ letter_spacing: Number(e.currentTarget.value) }, 'ls')} /><span class="num tnum">{s.letter_spacing}</span></span>
    </div>
    <div class="grow">
      <span class="glabel">Opacity</span>
      <span class="gcontrol"><input type="range" min="0.05" max="1" step="0.01" value={s.opacity} oninput={(e) => setStyle({ opacity: Number(e.currentTarget.value) }, 'op')} /><span class="num tnum">{Math.round(s.opacity * 100)}%</span></span>
    </div>
  </div>

  <h2>Text colour</h2>
  <div class="group">
    <div class="grow stack">
      <div class="swatches">
        {#each [...SOLID_PALETTE, '#ffd60a', '#ff2d55', '#32d74b'] as c (c)}
          <button class="swatch" aria-label={c} aria-pressed={hex(fillColor(s.fill)) === c} style:background={c} onclick={() => setColor(hexToRgba(c))}></button>
        {/each}
        <input type="color" aria-label="Custom text colour" value={hex(fillColor(s.fill))} oninput={(e) => setColor(hexToRgba(e.currentTarget.value), 'fill')} />
        <button class="plain icon" title="Pick a colour from the screen" aria-label="Eyedropper" onclick={() => pick('fill')}>⌖</button>
      </div>
    </div>
    <div class="grow">
      <span class="glabel">Gradient</span>
      <span class="gcontrol">
        {#if 'linear' in s.fill}
          {@const lin = s.fill.linear}
          <input type="color" aria-label="Gradient end" value={hex(lin.stops[1])} oninput={(e) => setStyle({ fill: { linear: { ...lin, stops: [lin.stops[0], hexToRgba(e.currentTarget.value)] } } }, 'g2')} />
          <button class="plain icon" title="Pick the end colour" aria-label="Eyedropper for the gradient end" onclick={() => pick('end')}>⌖</button>
          <input type="range" min="0" max="360" step="15" value={lin.angle_deg} aria-label="Angle" oninput={(e) => setStyle({ fill: { linear: { ...lin, angle_deg: Number(e.currentTarget.value) } } }, 'ga')} />
        {/if}
        <input type="checkbox" class="switch" checked={gradient} onchange={(e) => setGradient(e.currentTarget.checked)} aria-label="Gradient" />
      </span>
    </div>
  </div>

  <h2>Effects</h2>
  <div class="group">
    <div class="grow">
      <span class="glabel">Outline</span>
      <span class="gcontrol">
        {#if s.outline}
          {@const o = s.outline}
          <input type="range" min="1" max="20" step="0.5" value={o.width_1080} aria-label="Outline width" oninput={(e) => setStyle({ outline: { ...o, width_1080: Number(e.currentTarget.value) } }, 'ow')} />
          <input type="color" aria-label="Outline colour" value={hex(o.color)} oninput={(e) => setStyle({ outline: { ...o, color: hexToRgba(e.currentTarget.value) } }, 'oc')} />
        {/if}
        <input type="checkbox" class="switch" checked={!!s.outline} onchange={(e) => setStyle({ outline: e.currentTarget.checked ? { width_1080: 5, color: [0, 0, 0, 1] } : null })} aria-label="Outline" />
      </span>
    </div>
    <div class="grow">
      <span class="glabel">Shadow</span>
      <span class="gcontrol">
        {#if s.shadow}
          {@const sh = s.shadow}
          <input type="range" min="0" max="30" step="0.5" value={sh.blur_1080} aria-label="Shadow blur" oninput={(e) => setStyle({ shadow: { ...sh, blur_1080: Number(e.currentTarget.value) } }, 'sb')} />
          <input type="range" min="-20" max="20" step="1" value={sh.offset_1080.y} aria-label="Shadow offset" oninput={(e) => setStyle({ shadow: { ...sh, offset_1080: { x: sh.offset_1080.x, y: Number(e.currentTarget.value) } } }, 'so')} />
          <input type="color" aria-label="Shadow colour" value={hex(sh.color)} oninput={(e) => setStyle({ shadow: { ...sh, color: [...hexToRgba(e.currentTarget.value).slice(0, 3), sh.color[3]] as Rgba } }, 'sc')} />
        {/if}
        <input type="checkbox" class="switch" checked={!!s.shadow} onchange={(e) => setStyle({ shadow: e.currentTarget.checked ? { blur_1080: 6, offset_1080: { x: 0, y: 3 }, color: [0, 0, 0, 0.5] } : null })} aria-label="Shadow" />
      </span>
    </div>
    <div class="grow">
      <span class="glabel">Glow</span>
      <span class="gcontrol">
        {#if s.glow}
          {@const g = s.glow}
          <input type="range" min="2" max="40" step="1" value={g.radius_1080} aria-label="Glow size" oninput={(e) => setStyle({ glow: { ...g, radius_1080: Number(e.currentTarget.value) } }, 'gr')} />
          <input type="color" aria-label="Glow colour" value={hex(g.color)} oninput={(e) => setStyle({ glow: { ...g, color: hexToRgba(e.currentTarget.value) } }, 'gc')} />
        {/if}
        <input type="checkbox" class="switch" checked={!!s.glow} onchange={(e) => setStyle({ glow: e.currentTarget.checked ? { radius_1080: 14, color: fillColor(s.fill) } : null })} aria-label="Glow" />
      </span>
    </div>
  </div>

  <h2>Background</h2>
  <div class="group">
    <div class="grow">
      <span class="glabel">Box</span>
      <span class="gcontrol">
        <span class="segmented" role="radiogroup" aria-label="Background box">
          {#each [['', 'None'], ['per_line', 'Lines'], ['merged', 'Merged'], ['inverted', 'Inverted']] as [k, label] (k)}
            <button
              role="radio"
              aria-checked={(s.background?.kind ?? '') === k}
              onclick={() =>
                setStyle({
                  background: k
                    ? { kind: k as 'per_line', color: s.background?.color ?? [0, 0, 0, 0.75], radius_1080: s.background?.radius_1080 ?? 14, padding_1080: s.background?.padding_1080 ?? { x: 20, y: 6 } }
                    : null,
                })}>{label}</button
            >
          {/each}
        </span>
      </span>
    </div>
    {#if s.background}
      {@const b = s.background}
      <div class="grow stack">
        <span class="glabel">Background colour</span>
        <div class="swatches">
          {#each ['#000000', '#ffffff', ...SOLID_PALETTE.filter((c) => c !== '#000000' && c !== '#ffffff'), '#ffd60a'] as c (c)}
            <button class="swatch" aria-label="Background {c}" aria-pressed={hex(b.color) === c} style:background={c} onclick={() => setStyle({ background: { ...b, color: [...hexToRgba(c).slice(0, 3), b.color[3]] as Rgba } })}></button>
          {/each}
          <input type="color" aria-label="Custom background colour" value={hex(b.color)} oninput={(e) => setStyle({ background: { ...b, color: [...hexToRgba(e.currentTarget.value).slice(0, 3), b.color[3]] as Rgba } }, 'bc')} />
        </div>
      </div>
      <div class="grow">
        <span class="glabel">Opacity</span>
        <span class="gcontrol"><input type="range" min="0.1" max="1" step="0.05" value={b.color[3]} aria-label="Background opacity" oninput={(e) => setStyle({ background: { ...b, color: [b.color[0], b.color[1], b.color[2], Number(e.currentTarget.value)] } }, 'ba')} /><span class="num tnum">{Math.round(b.color[3] * 100)}%</span></span>
      </div>
      <div class="grow">
        <span class="glabel">Corners</span>
        <span class="gcontrol"><input type="range" min="0" max="40" step="1" value={b.radius_1080} aria-label="Background corners" oninput={(e) => setStyle({ background: { ...b, radius_1080: Number(e.currentTarget.value) } }, 'br')} /><span class="num tnum">{b.radius_1080}</span></span>
      </div>
    {/if}
  </div>

  <h2>Position</h2>
  <div class="group">
    <div class="grow">
      <span class="glabel">Place</span>
      <span class="gcontrol">
        <span class="segmented">
          <button onclick={() => setPos(0.5, 0.18)}>Top</button>
          <button onclick={() => setPos(0.5, 0.5)}>Middle</button>
          <button onclick={() => setPos(0.5, 0.8)}>Bottom</button>
        </span>
      </span>
    </div>
    <div class="grow">
      <span class="glabel">Scale</span>
      <span class="gcontrol"><input type="range" min="0.2" max="4" step="0.01" value={item.transform.scale} oninput={(e) => onchange({ ...item, transform: { ...item.transform, scale: Number(e.currentTarget.value) } }, 'scale')} /><span class="num tnum">{item.transform.scale.toFixed(2)}×</span></span>
    </div>
    <div class="grow">
      <span class="glabel">Rotation</span>
      <span class="gcontrol"><input type="range" min="-180" max="180" step="1" value={item.transform.rotation_deg} oninput={(e) => onchange({ ...item, transform: { ...item.transform, rotation_deg: Number(e.currentTarget.value) } }, 'rot')} /><span class="num tnum">{Math.round(item.transform.rotation_deg)}°</span></span>
    </div>
  </div>

  <h2>Animation</h2>
  <div class="group">
    {#each [['in', 'In', IN_ANIMS], ['out', 'Out', OUT_ANIMS]] as [which, label, list] (which)}
      {@const a = item.anim[which as 'in' | 'out']}
      <div class="grow stack">
        <span class="row">
          <span class="glabel">{label}</span>
          <select value={a?.kind ?? ''} onchange={(e) => setAnim(which as 'in' | 'out', e.currentTarget.value)} aria-label="{label} animation">
            <option value="">None</option>
            {#each list as [k, name] (k)}<option value={k}>{name}</option>{/each}
          </select>
        </span>
        {#if a}
          <span class="row">
            <span class="caption">Duration</span>
            <input type="range" min="0.1" max="3" step="0.05" value={a.duration_s} oninput={(e) => patchAnim(which as 'in' | 'out', { duration_s: Number(e.currentTarget.value) }, `${which}-d`)} />
            <span class="num tnum">{a.duration_s.toFixed(2)} s</span>
            <select value={a.easing} onchange={(e) => patchAnim(which as 'in' | 'out', { easing: e.currentTarget.value })} aria-label="{label} easing">
              {#each EASINGS as [k, name] (k)}<option value={k}>{name}</option>{/each}
            </select>
          </span>
        {/if}
      </div>
    {/each}
    <div class="grow stack">
      <span class="row">
        <span class="glabel">Loop</span>
        <select
          value={item.anim.emphasis?.kind ?? ''}
          aria-label="Loop animation"
          onchange={(e) => {
            const k = e.currentTarget.value;
            onchange({ ...item, anim: { ...item.anim, emphasis: k ? { kind: k as 'pulse', period_s: item.anim.emphasis?.period_s ?? 1.2 } : null } });
          }}
        >
          <option value="">None</option>
          {#each EMPHASIS as [k, name] (k)}<option value={k}>{name}</option>{/each}
        </select>
      </span>
      {#if item.anim.emphasis}
        {@const em = item.anim.emphasis}
        <span class="row">
          <span class="caption">Every</span>
          <input type="range" min="0.3" max="4" step="0.1" value={em.period_s} oninput={(e) => onchange({ ...item, anim: { ...item.anim, emphasis: { ...em, period_s: Number(e.currentTarget.value) } } }, 'em')} />
          <span class="num tnum">{em.period_s.toFixed(1)} s</span>
        </span>
      {/if}
    </div>
  </div>

  <div class="group">
    <div class="grow">
      <span class="glabel">Shows</span>
      <span class="gcontrol muted tnum">{formatTime(item.start_tl_ns)} – {formatTime(item.end_tl_ns)}</span>
    </div>
  </div>
  <div class="actions">
    <button onclick={onduplicate}>Duplicate</button>
    <button class="destructive" onclick={ondelete}>Delete</button>
  </div>
</div>

<style>
  .text-insp {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  textarea {
    width: 100%;
    resize: vertical;
    min-height: 54px;
    padding: 6px 8px;
    border: none;
    border-radius: 6px;
    background: var(--field);
    box-shadow: var(--field-edge);
    font: inherit;
    -webkit-user-select: text;
    user-select: text;
  }
  textarea:focus {
    outline: none;
    box-shadow: var(--field-edge), var(--focus-ring);
  }
  h2 {
    margin: 4px 0 -4px 10px;
  }
  .presets {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 6px;
  }
  .preset {
    height: 36px;
    background: #2b2b2e;
    box-shadow: inset 0 0 0 0.5px rgba(255, 255, 255, 0.12);
    overflow: hidden;
  }
  .preset span {
    padding: 1px 5px;
    white-space: nowrap;
  }
  .recent {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .swatch {
    width: 22px;
    height: 22px;
    min-height: 22px;
    padding: 0;
    border-radius: 6px;
    box-shadow: inset 0 0 0 0.5px rgba(0, 0, 0, 0.25);
    color: #000;
    font-size: 11px;
  }
  .swatch[aria-pressed='true'] {
    box-shadow: 0 0 0 2px var(--window), 0 0 0 4px var(--accent);
  }
  input[type='color'] {
    appearance: none;
    width: 30px;
    height: 22px;
    padding: 2px;
    border: none;
    border-radius: 6px;
    background: var(--control);
    box-shadow: var(--control-edge);
  }
  input[type='color']::-webkit-color-swatch-wrapper {
    padding: 0;
  }
  input[type='color']::-webkit-color-swatch {
    border: none;
    border-radius: 4px;
  }
  .gcontrol input[type='range'] {
    width: 96px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .row .glabel {
    flex: 1;
  }
  .row input[type='range'] {
    flex: 1;
    min-width: 60px;
  }
  .num {
    min-width: 34px;
    text-align: right;
    color: var(--label-2);
    font-size: 11px;
  }
  select {
    max-width: 150px;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .gcontrol .segmented button {
    padding: 1px 7px;
  }
</style>
