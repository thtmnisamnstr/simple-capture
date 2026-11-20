import { Channel, invoke } from '@tauri-apps/api/core';

export interface ModeCfg {
  name: string;
  transport: 'idle' | 'channel' | 'protocol' | 'native';
  renderer: 'gl' | '2d' | 'native' | 'native-above' | 'none';
  width: number;
  height: number;
  fps: number;
  duration_s: number;
  warmup_s: number;
  max_in_flight: number;
  fetch_per_raf: boolean;
  native_above: boolean;
  snapshot: boolean;
}

export interface LiveStats {
  phase: string;
  rafFps: number;
  presentedFps: number;
  latencyP50: number;
  drops: number;
}

const HEADER = 32;
const MAGIC = 0x56435053;
const BLOCK = 40;

const wallNow = () => performance.timeOrigin + performance.now();

interface Frame {
  id: number;
  w: number;
  h: number;
  prodTs: number;
  sendTs: number;
  recvTs: number;
  buf: ArrayBuffer;
}

function parseFrame(buf: ArrayBuffer, recvTs: number): Frame {
  const dv = new DataView(buf, 0, HEADER);
  if (dv.getUint32(0, true) !== MAGIC) throw new Error(`bad magic, len=${buf.byteLength}`);
  return {
    id: dv.getUint32(4, true),
    w: dv.getUint32(8, true),
    h: dv.getUint32(12, true),
    prodTs: dv.getFloat64(16, true),
    sendTs: dv.getFloat64(24, true),
    recvTs,
    buf,
  };
}

function decodeBits(row: Uint8Array, w: number, nbits: number): number {
  const block = (BLOCK * w) / 1920;
  let v = 0;
  for (let k = 0; k < nbits; k++) {
    const x = Math.floor((k + 0.5) * block);
    v = v * 2 + (row[x * 4] > 127 ? 1 : 0);
  }
  return v >>> 0;
}

interface Presenter {
  draw(f: Frame): void;
  readRow(rowFromTop: number): Uint8Array;
}

class GLPresenter implements Presenter {
  private gl: WebGL2RenderingContext;
  constructor(
    canvas: HTMLCanvasElement,
    private w: number,
    private h: number,
  ) {
    const gl = canvas.getContext('webgl2', {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      premultipliedAlpha: false,
      preserveDrawingBuffer: false,
      powerPreference: 'high-performance',
    });
    if (!gl) throw new Error('no webgl2');
    this.gl = gl;
    const vs = `#version 300 es
      out vec2 uv;
      void main() {
        vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
        uv = p;
        gl_Position = vec4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
      }`;
    const fs = `#version 300 es
      precision mediump float;
      uniform sampler2D tex;
      in vec2 uv;
      out vec4 color;
      void main() { color = texture(tex, uv); }`;
    const prog = gl.createProgram()!;
    for (const [type, src] of [
      [gl.VERTEX_SHADER, vs],
      [gl.FRAGMENT_SHADER, fs],
    ] as const) {
      const sh = gl.createShader(type)!;
      gl.shaderSource(sh, src);
      gl.compileShader(sh);
      if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(sh) ?? 'shader');
      gl.attachShader(prog, sh);
    }
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog) ?? 'link');
    gl.useProgram(prog);
    const tex = gl.createTexture();
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, w, h);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 4);
    gl.viewport(0, 0, w, h);
  }
  draw(f: Frame) {
    const gl = this.gl;
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, f.w, f.h, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array(f.buf, HEADER, f.w * f.h * 4));
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }
  readRow(r: number): Uint8Array {
    const out = new Uint8Array(this.w * 4);
    this.gl.readPixels(0, this.h - 1 - r, this.w, 1, this.gl.RGBA, this.gl.UNSIGNED_BYTE, out);
    return out;
  }
}

class Canvas2DPresenter implements Presenter {
  private ctx: CanvasRenderingContext2D;
  constructor(
    canvas: HTMLCanvasElement,
    private w: number,
  ) {
    const ctx = canvas.getContext('2d', { alpha: false });
    if (!ctx) throw new Error('no 2d context');
    this.ctx = ctx;
  }
  draw(f: Frame) {
    const data = new Uint8ClampedArray(f.buf, HEADER, f.w * f.h * 4);
    this.ctx.putImageData(new ImageData(data, f.w, f.h), 0, 0);
  }
  readRow(r: number): Uint8Array {
    return new Uint8Array(this.ctx.getImageData(0, r, this.w, 1).data.buffer);
  }
}

function summarize(v: number[]) {
  if (!v.length) return null;
  const s = [...v].sort((a, b) => a - b);
  const pct = (p: number) => s[Math.round((s.length - 1) * p)];
  return { n: s.length, mean: s.reduce((a, b) => a + b, 0) / s.length, p50: pct(0.5), p95: pct(0.95), p99: pct(0.99), max: s[s.length - 1] };
}

async function calibrateClock() {
  let best = { rtt: Infinity, offset: 0 };
  for (let i = 0; i < 25; i++) {
    const t0 = wallNow();
    const r = await invoke<number>('clock_ping');
    const t1 = wallNow();
    if (t1 - t0 < best.rtt) best = { rtt: t1 - t0, offset: r - (t0 + t1) / 2 };
  }
  return best;
}

export function log(msg: string) {
  console.log(msg);
  invoke('log', { msg }).catch(() => {});
}

export async function runBench(
  cfg: ModeCfg,
  canvas: HTMLCanvasElement | null,
  previewBox: HTMLElement,
  onLive: (s: LiveStats) => void,
) {
  const errors: string[] = [];
  window.addEventListener('error', (e) => errors.push(String(e.message)));
  window.addEventListener('unhandledrejection', (e) => errors.push(String(e.reason)));
  const origError = console.error.bind(console);
  console.error = (...args: unknown[]) => {
    const msg = args.map((a) => (a instanceof Error ? `${a.name}: ${a.message}` : String(a))).join(' ');
    errors.push(`console.error: ${msg}`);
    log(`console.error: ${msg}`);
    origError(...args);
  };

  const clock = await calibrateClock();
  const dpr = window.devicePixelRatio;

  let presenter: Presenter | null = null;
  if (canvas && (cfg.renderer === 'gl' || cfg.renderer === '2d')) {
    canvas.width = cfg.width;
    canvas.height = cfg.height;
    presenter = cfg.renderer === 'gl' ? new GLPresenter(canvas, cfg.width, cfg.height) : new Canvas2DPresenter(canvas, cfg.width);
  }

  let measuring = false;
  let latest: Frame | null = null;
  let received = 0;
  let superseded = 0;
  let drawn = 0;
  let drops = 0;
  let lastDrawnId = 0;
  let rafCount = 0;
  let lastRaf = 0;
  let verifyOk = 0;
  let verifyFail = 0;
  const rafIntervals: number[] = [];
  const drawLatency: number[] = [];
  const recvLatency: number[] = [];
  const sendToRecv: number[] = [];
  const cbMs: number[] = [];
  const fetchMs: number[] = [];
  let firstDrawTs = 0;
  let lastDrawTs = 0;
  let live = { draws: 0, rafs: 0, t: performance.now() };

  const chFetch = { started: 0, resolved: 0, rejected: 0, pending: new Map<number, number>() };
  const origFetch = window.fetch.bind(window);
  let fetchSeq = 0;
  window.fetch = (input: RequestInfo | URL, init?: RequestInit) => {
    const p = origFetch(input, init);
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    if (url.includes('__TAURI_CHANNEL__')) {
      const id = ++fetchSeq;
      chFetch.started++;
      chFetch.pending.set(id, performance.now());
      p.then(
        () => {
          chFetch.resolved++;
          chFetch.pending.delete(id);
        },
        () => {
          chFetch.rejected++;
          chFetch.pending.delete(id);
        },
      );
    }
    return p;
  };
  const chFetchSnapshot = () => ({
    started: chFetch.started,
    resolved: chFetch.resolved,
    rejected: chFetch.rejected,
    pending_ages_ms: [...chFetch.pending.values()].map((t) => Math.round(performance.now() - t)),
  });
  const ackStats = { sent: 0, ok: 0, failed: 0, maxMs: 0, lastError: '' };
  let lastRecvPerf = 0;
  let lastRecvId = 0;
  const stalls: { after_id: number; at_s: number; gap_ms: number }[] = [];
  const stallTimer = setInterval(() => {
    if (lastRecvPerf && performance.now() - lastRecvPerf > 500 && !stalls.some((s) => s.after_id === lastRecvId)) {
      stalls.push({ after_id: lastRecvId, at_s: performance.now() / 1000, gap_ms: performance.now() - lastRecvPerf });
      log(`stall: no frame for >500 ms after id ${lastRecvId} (measuring=${measuring}) channel fetch ${JSON.stringify(chFetchSnapshot())} acks ${JSON.stringify(ackStats)}`);
    }
  }, 100);
  const receive = (buf: ArrayBuffer) => {
    lastRecvPerf = performance.now();
    const f = parseFrame(buf, wallNow());
    lastRecvId = f.id;
    if (latest) {
      if (measuring) superseded++;
    }
    latest = f;
    if (measuring) {
      received++;
      recvLatency.push(f.recvTs + clock.offset - f.prodTs);
      sendToRecv.push(f.recvTs + clock.offset - f.sendTs);
    }
    return f;
  };

  let running = true;
  let startFetchFromRaf: (() => void) | null = null;
  if (cfg.transport === 'channel') {
    const ch = new Channel<ArrayBuffer>();
    ch.onmessage = (msg) => {
      if (!(msg instanceof ArrayBuffer)) {
        errors.push(`channel message is ${Object.prototype.toString.call(msg)}, not ArrayBuffer`);
        return;
      }
      const f = receive(msg);
      ackStats.sent++;
      const t0 = performance.now();
      invoke('ack', { id: f.id }).then(
        () => {
          ackStats.ok++;
          ackStats.maxMs = Math.max(ackStats.maxMs, performance.now() - t0);
        },
        (e) => {
          ackStats.failed++;
          if (ackStats.lastError === '') log(`ack ${f.id} failed: ${e}`);
          ackStats.lastError = String(e);
        },
      );
    };
    await invoke('start_channel', { channel: ch });
  } else if (cfg.transport === 'protocol') {
    const base = /Windows|Android/.test(navigator.userAgent) ? 'http://frames.localhost' : 'frames://localhost';
    await invoke('start_stream');
    let after = 0;
    const fetchOnce = async () => {
      const t0 = performance.now();
      const r = await fetch(`${base}/next?after=${after}`, { cache: 'no-store' });
      if (r.status !== 200) return;
      const buf = await r.arrayBuffer();
      if (measuring) fetchMs.push(performance.now() - t0);
      after = receive(buf).id;
    };
    if (cfg.fetch_per_raf) {
      let inflight = false;
      startFetchFromRaf = () => {
        if (inflight || !running) return;
        inflight = true;
        fetchOnce()
          .catch((e) => errors.push(`fetch: ${e}`))
          .finally(() => (inflight = false));
      };
    } else {
      (async () => {
        while (running) {
          try {
            await fetchOnce();
          } catch (e) {
            errors.push(`fetch: ${e}`);
            await new Promise((res) => setTimeout(res, 50));
          }
        }
      })();
    }
  } else if (cfg.transport === 'native') {
    const r = previewBox.getBoundingClientRect();
    const desc = await invoke<string>('set_preview_rect', { x: r.left, y: r.top, w: r.width, h: r.height, innerH: window.innerHeight, dpr });
    log(`native view: ${desc}`);
  }

  const frame = () => {
    const now = performance.now();
    if (measuring) {
      rafCount++;
      if (lastRaf) rafIntervals.push(now - lastRaf);
    }
    lastRaf = now;
    live.rafs++;
    startFetchFromRaf?.();
    const f = latest;
    if (f && presenter) {
      latest = null;
      const t0 = performance.now();
      presenter.draw(f);
      const ts = wallNow();
      live.draws++;
      if (measuring) {
        drawn++;
        if (lastDrawnId && f.id > lastDrawnId + 1) drops += f.id - lastDrawnId - 1;
        drawLatency.push(ts + clock.offset - f.prodTs);
        cbMs.push(performance.now() - t0);
        if (!firstDrawTs) firstDrawTs = ts;
        lastDrawTs = ts;
        if (drawn % 30 === 0) {
          const block = (BLOCK * f.h) / 1080;
          const id = decodeBits(presenter.readRow(Math.floor(block * 0.5)), f.w, 24);
          const tsLo = decodeBits(presenter.readRow(Math.floor(block * 1.5)), f.w, 32);
          const expectTs = Number(BigInt(Math.floor(f.prodTs)) & 0xffffffffn);
          if (id === (f.id & 0xffffff) && tsLo === expectTs) verifyOk++;
          else {
            verifyFail++;
            if (verifyFail < 4) errors.push(`verify mismatch: id ${id} vs ${f.id}, ts ${tsLo} vs ${expectTs}`);
          }
        }
      }
      lastDrawnId = f.id;
    }
    if (now - live.t >= 500) {
      const dt = (now - live.t) / 1000;
      const lat = drawLatency.length ? drawLatency[drawLatency.length - 1] : NaN;
      onLive({ phase: measuring ? 'measuring' : 'warmup', rafFps: live.rafs / dt, presentedFps: live.draws / dt, latencyP50: lat, drops });
      live = { draws: 0, rafs: 0, t: now };
    }
    if (running) requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);

  await new Promise((r) => setTimeout(r, cfg.warmup_s * 1000));
  await invoke('measure_begin');
  latest = null;
  measuring = true;
  const tBegin = performance.now();
  await new Promise((r) => setTimeout(r, cfg.duration_s * 1000));
  measuring = false;
  const windowS = (performance.now() - tBegin) / 1000;

  let snapshot: unknown = null;
  if (cfg.snapshot) {
    const rect = (el: Element) => {
      const r = el.getBoundingClientRect();
      return { x: r.left, y: r.top, w: r.width, h: r.height };
    };
    const btn = previewBox.querySelector('button');
    try {
      snapshot = await invoke('snapshot', {
        preview: rect(previewBox),
        button: btn ? rect(btn) : { x: 0, y: 0, w: 0, h: 0 },
        innerW: window.innerWidth,
        innerH: window.innerHeight,
      });
      log(`snapshot: ${JSON.stringify(snapshot)}`);
    } catch (e) {
      snapshot = { error: String(e) };
    }
  }

  const js = {
    dpr,
    user_agent: navigator.userAgent,
    canvas_css: canvas ? { w: canvas.clientWidth, h: canvas.clientHeight } : null,
    clock_offset_ms: clock.offset,
    clock_rtt_ms: clock.rtt,
    window_s: windowS,
    raf_count: rafCount,
    raf_fps: rafCount / windowS,
    raf_interval_ms: summarize(rafIntervals),
    received,
    superseded_before_draw: superseded,
    drawn_unique: drawn,
    presented_fps: drawn / windowS,
    presented_fps_span: drawn > 1 ? (drawn - 1) / ((lastDrawTs - firstDrawTs) / 1000) : 0,
    id_gaps: drops,
    latency_draw_ms: summarize(drawLatency),
    latency_recv_ms: summarize(recvLatency),
    send_to_recv_ms: summarize(sendToRecv),
    draw_call_ms: summarize(cbMs),
    fetch_ms: summarize(fetchMs),
    verify_ok: verifyOk,
    verify_fail: verifyFail,
    errors,
    stalls,
    channel_fetch: chFetchSnapshot(),
    acks: ackStats,
    snapshot,
  };
  clearInterval(stallTimer);
  running = false;
  const path = await invoke<string>('report', { js });
  log(`report written: ${path}`);
}
