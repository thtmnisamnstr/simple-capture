import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const dir = process.argv[2] ?? 'results/default';
const order = ['idle', 'a-gl-540', 'a-2d-540', 'a-gl-720', 'a-2d-720', 'a-gl-1080', 'a-2d-1080', 'b-gl-540', 'b-2d-540', 'b-gl-720', 'b-gl-1080', 'b-gl-1080-raf', 'c-native-540', 'c-native-1080', 'c-above-1080'];
const rank = (m) => (order.includes(m) ? order.indexOf(m) : 999);
const files = readdirSync(dir).filter((f) => f.endsWith('.json'));
const rows = files
  .map((f) => JSON.parse(readFileSync(join(dir, f), 'utf8')))
  .sort((a, b) => rank(a.mode.name) - rank(b.mode.name));

const n = (v, d = 1) => (v === null || v === undefined || Number.isNaN(v) ? '–' : Number(v).toFixed(d));
const role = (r, name) => r.processes?.filter((p) => p.role === name).reduce((a, p) => a + p.cpu_pct, 0);
const mem = (r, name) => r.processes?.filter((p) => p.role === name).reduce((a, p) => a + (p.footprint_mb ?? p.rss_mb), 0);

console.log(`Results from ${dir} (${rows[0]?.os ?? ''}, tauri ${rows[0]?.tauri ?? ''}, dpr ${rows[0]?.js?.dpr ?? '?'})\n`);
console.log('| mode | frame px | produced fps | presented fps | drops (id gaps) | latency p50 / p95 / p99 ms | rAF fps | CPU% app | CPU% WebContent | CPU% WebKit GPU | CPU% total | mem MB (app / WC / GPU) | verify ok/fail | snapshot | errors |');
console.log('|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|');
for (const r of rows) {
  const m = r.mode;
  const native = m.transport === 'native';
  const p = r.producer;
  const lat = native ? p.present_latency_ms : r.js?.latency_draw_ms;
  const presented = native ? p.presented_fps : r.js?.presented_fps;
  const drops = native ? p.produced - p.presented : r.js?.id_gaps;
  const errs = [r.error, ...(r.js?.errors ?? [])].filter(Boolean);
  const sn = r.js?.snapshot;
  const snap = !sn ? '–' : sn.skipped ? 'skipped' : sn.error ? 'error' : `video ${sn.preview_visible ? 'visible' : 'NOT visible'}, HTML ${sn.html_over_preview ? 'over it' : 'not over it'}`;
  console.log(
    `| ${m.name} | ${m.transport === 'idle' ? '–' : `${m.width}×${m.height}`} | ${n(p.produced_fps)} | ${m.transport === 'idle' ? '–' : n(presented)} | ${m.transport === 'idle' ? '–' : drops} | ${lat ? `${n(lat.p50)} / ${n(lat.p95)} / ${n(lat.p99)}` : '–'} | ${n(r.js?.raf_fps)} | ${n(role(r, 'app'))} | ${n(role(r, 'webkit-webcontent'))} | ${n(role(r, 'webkit-gpu'))} | ${n(r.app_plus_webkit_cpu_pct)} | ${n(mem(r, 'app'), 0)} / ${n(mem(r, 'webkit-webcontent'), 0)} / ${n(mem(r, 'webkit-gpu'), 0)} | ${r.js?.verify_ok ?? '–'}/${r.js?.verify_fail ?? '–'} | ${snap} | ${errs.length ? errs.join('; ').slice(0, 80) : ''} |`,
  );
}
console.log('\nProducer timings (ms, p50/p95): encode | GPU wait (render+readback) | copy into Vec | send | recv latency (producer→JS onmessage) | fetch');
for (const r of rows) {
  const p = r.producer;
  const s = (x) => (x ? `${n(x.p50, 2)}/${n(x.p95, 2)}` : '–');
  console.log(`- ${r.mode.name}: ${s(p.encode_ms)} | ${s(p.gpu_wait_ms)} | ${s(p.copy_ms)} | ${s(p.send_ms)} | ${s(r.js?.latency_recv_ms)} | ${s(r.js?.fetch_ms)}; backpressure drops ${p.dropped_backpressure} (timeout overrides ${p.backpressure_overrides ?? '–'}), unfetched ${p.dropped_unfetched}, late ticks ${p.late_ticks}; GPU util ${n(r.gpu_util_pct?.mean)}%`);
}
