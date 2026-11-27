// Records a scene page in headless Chrome and encodes an App Store app preview:
// 1920x1080, 30 fps, H.264 High with a silent stereo AAC track, at most 30 s.
//   node record.mjs <url> <out.mp4> <ffmpeg>
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [, , url, out, ffmpeg = 'ffmpeg'] = process.argv;
const CH = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const port = 9300 + Math.floor(Math.random() * 500);
const prof = mkdtempSync(join(tmpdir(), 'cdp-'));
const work = mkdtempSync(join(tmpdir(), 'preview-'));
const ch = spawn(CH, ['--headless=new', '--hide-scrollbars', '--mute-audio', `--remote-debugging-port=${port}`, `--user-data-dir=${prof}`, '--window-size=1600,900', '--force-device-scale-factor=1.2', 'about:blank'], { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let targets;
for (let i = 0; i < 50; i++) { try { targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); break; } catch { await sleep(200); } }
const page = targets.find((t) => t.type === 'page');
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener('open', r));
let id = 0;
const pending = new Map();
const frames = [];
const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
ws.addEventListener('message', (e) => {
  const m = JSON.parse(e.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); return; }
  if (m.method === 'Page.screencastFrame') {
    frames.push({ t: Date.now() / 1000, data: m.params.data });
    send('Page.screencastFrameAck', { sessionId: m.params.sessionId });
  }
});
const title = async () => (await send('Runtime.evaluate', { expression: 'document.title', returnByValue: true })).result.result.value;

await send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 900, deviceScaleFactor: 1.2, mobile: false });
await send('Page.enable');
await send('Page.navigate', { url });
for (let i = 0; (await title()) !== 'READY'; i++) {
  if (i > 300) throw new Error('scene never got ready: ' + (await title()));
  await sleep(100);
}
await send('Page.startScreencast', { format: 'jpeg', quality: 95, maxWidth: 1920, maxHeight: 1080, everyNthFrame: 1 });
await sleep(400);
await send('Runtime.evaluate', { expression: '__go()' });
let t;
for (let i = 0; !(t = await title()).startsWith('DONE') && !t.startsWith('FAILED'); i++) {
  if (i > 600) throw new Error('scene did not finish');
  await sleep(100);
}
const doneAt = Date.now() / 1000;
await send('Page.stopScreencast');
ws.close();
ch.kill();
if (t.startsWith('FAILED')) { console.error(t); process.exit(1); }

const t0 = frames[0].t;
const name = (i) => `f${String(i).padStart(5, '0')}.jpg`;
frames.forEach((f, i) => writeFileSync(join(work, name(i)), Buffer.from(f.data, 'base64')));
let list = 'ffconcat version 1.0\n';
for (let k = 0, i = 0; t0 + k / 30 <= doneAt; k++) {
  while (i + 1 < frames.length && frames[i + 1].t <= t0 + k / 30) i++;
  list += `file ${name(i)}\nduration ${1 / 30}\n`;
}
writeFileSync(join(work, 'list.ffconcat'), list);
const secs = doneAt - t0;
console.log(`${frames.length} frames over ${secs.toFixed(1)} s`);
const r = spawnSync(ffmpeg, ['-hide_banner', '-loglevel', 'error', '-y',
  '-f', 'concat', '-safe', '0', '-i', join(work, 'list.ffconcat'),
  '-f', 'lavfi', '-i', 'anullsrc=r=48000:cl=stereo',
  '-r', '30', '-vf', 'scale=1920:1080:flags=lanczos,setsar=1,format=yuv420p',
  '-c:v', 'libx264', '-preset', 'slow', '-crf', '14', '-profile:v', 'high', '-level:v', '4.1',
  '-c:a', 'aac', '-b:a', '256k', '-ar', '48000', '-ac', '2',
  '-t', '29.8', '-shortest', '-movflags', '+faststart', out], { stdio: 'inherit' });
rmSync(work, { recursive: true, force: true });
rmSync(prof, { recursive: true, force: true });
process.exit(r.status ?? 1);
