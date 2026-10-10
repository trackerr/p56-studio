// End-to-end test: the real P56 Studio app (WebView2) against the fan simulator.
// Drives the UI through the Chrome DevTools Protocol and checks the simulated fan.
//   node --experimental-websocket e2e/app-e2e.mjs [--release] [--keep]
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const profile = process.argv.includes('--release') ? 'release' : 'debug';
const keep = process.argv.includes('--keep');
const target = process.env.CARGO_TARGET_DIR || join(root, 'target');
const exe = (n) => join(target, profile, `${n}.exe`);
const out = join(root, 'e2e', 'out');
mkdirSync(out, { recursive: true });
const CDP_PORT = 9333, SIM_HTTP = 8099;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const procs = [];
let passed = 0, failed = 0;
const results = [];

function start(cmd, args, env = {}) {
  const p = spawn(cmd, args, { env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'] });
  p.stdout.on('data', (d) => process.env.E2E_VERBOSE && process.stdout.write(`[${cmd.split(/[\\/]/).pop()}] ${d}`));
  p.stderr.on('data', (d) => process.env.E2E_VERBOSE && process.stdout.write(`[${cmd.split(/[\\/]/).pop()}] ${d}`));
  procs.push(p);
  return p;
}
function cleanup() { if (!keep) for (const p of procs) { try { p.kill(); } catch {} } }
process.on('exit', cleanup);

async function sim() { return (await (await fetch(`http://127.0.0.1:${SIM_HTTP}/state.json`)).json())[0]; }
async function waitSim(desc, pred, ms = 8000) {
  const end = Date.now() + ms;
  let s;
  while (Date.now() < end) { s = await sim(); if (pred(s)) return s; await sleep(100); }
  throw new Error(`simulator: ${desc} — staat: ${JSON.stringify({ ...s, commands: s?.commands?.slice(-8) })}`);
}

// ---- CDP ----
let ws, msgId = 0;
const pending = new Map();
async function cdpConnect() {
  const end = Date.now() + 60000;
  while (Date.now() < end) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${CDP_PORT}/json`)).json();
      const page = list.find((t) => t.type === 'page' && /P56 Studio|index\.html|tauri\.localhost/.test(t.title + t.url));
      if (page) {
        ws = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
        ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } };
        return;
      }
    } catch {}
    await sleep(500);
  }
  throw new Error('geen webview met debugpoort gevonden');
}
function cdp(method, params = {}) {
  const id = ++msgId;
  ws.send(JSON.stringify({ id, method, params }));
  return new Promise((res) => pending.set(id, res));
}
async function js(expr) {
  const r = await cdp('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error(`JS: ${r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text}`);
  return r.result?.result?.value;
}
async function shot(name) {
  const r = await cdp('Page.captureScreenshot', { format: 'png' });
  if (r.result?.data) writeFileSync(join(out, `${name}.png`), Buffer.from(r.result.data, 'base64'));
}

// page helpers (installed into the page)
const HELPERS = `
window.$t = (id) => document.querySelector('[data-testid="' + id + '"]');
window.$click = (id) => { const e = $t(id); if (!e) throw new Error('niet gevonden: ' + id); e.click(); return true; };
window.$tab = (v) => { const e = $t('tab-' + v); e.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 })); e.focus(); e.click(); return true; };
window.$type = (id, v) => { const e = $t(id); const s = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set; s.call(e, v); e.dispatchEvent(new Event('input', { bubbles: true })); return true; };
window.$key = (id, key) => { const th = $t(id).querySelector('[role=slider]'); th.focus(); th.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true })); th.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true })); return true; };
window.$dialogBtn = (text) => { const b = [...document.querySelectorAll('[role=dialog] button')].find((b) => b.textContent.trim() === text); if (!b) throw new Error('dialoogknop niet gevonden: ' + text); b.click(); return true; };
window.$text = (id) => ($t(id) || {}).textContent || '';
`;

async function test(name, f) {
  const t0 = Date.now();
  try {
    await f();
    passed++; results.push(`✔ ${name} (${Date.now() - t0} ms)`);
    console.log(`  ✔ ${name}`);
  } catch (e) {
    failed++; results.push(`✘ ${name}: ${e.message}`);
    console.log(`  ✘ ${name}\n      ${e.message}`);
    await shot(`FAIL-${name.replace(/\W+/g, '_')}`).catch(() => {});
  }
}

// a small PNG and GIF made in the page itself
const MAKE_IMAGE = `(async () => {
  const c = document.createElement('canvas'); c.width = 640; c.height = 480; const g = c.getContext('2d');
  g.fillStyle = '#102030'; g.fillRect(0, 0, 640, 480); g.fillStyle = '#ffcc00'; g.beginPath(); g.arc(320, 240, 150, 0, 7); g.fill();
  g.fillStyle = '#000'; g.font = 'bold 80px sans-serif'; g.fillText('P56', 250, 270);
  return c.toDataURL('image/png').split(',')[1];
})()`;
const MAKE_VIDEO = `(async () => {
  const c = document.createElement('canvas'); c.width = 320; c.height = 240; const g = c.getContext('2d');
  const rec = new MediaRecorder(c.captureStream(20), { mimeType: 'video/webm' });
  const chunks = []; rec.ondataavailable = (e) => chunks.push(e.data);
  rec.start(100);
  for (let i = 0; i < 30; i++) { g.fillStyle = 'hsl(' + i * 12 + ',90%,50%)'; g.fillRect(0, 0, 320, 240); g.fillStyle = '#fff'; g.fillRect(i * 10, 100, 40, 40); await new Promise((r) => setTimeout(r, 50)); }
  rec.stop(); await new Promise((r) => (rec.onstop = r));
  const b = new Blob(chunks, { type: 'video/webm' });
  const buf = new Uint8Array(await b.arrayBuffer()); let s = ''; for (const x of buf) s += String.fromCharCode(x); return btoa(s);
})()`;

async function main() {
  for (const n of ['p56-sim', 'p56-studio']) if (!existsSync(exe(n))) throw new Error(`${exe(n)} ontbreekt; eerst bouwen`);
  console.log(`P56 end-to-end (${profile})`);
  start(exe('p56-sim'), ['--name', 'E2E-FAN', '--http', String(SIM_HTTP), '--max-files', '8']);
  await sleep(500);
  start(exe('p56-studio'), [], { WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`, RUST_LOG: 'info' });
  await cdpConnect();
  await cdp('Runtime.enable');
  await cdp('Page.enable');
  await sleep(1500);
  await js(HELPERS);
  // point the app at the simulator on this pc, then reconnect
  await js(`localStorage.setItem('p56.targets', '127.0.0.1'); localStorage.removeItem('p56.ip'); true`);
  await js(`$tab('app')`); await sleep(300);
  await js(`$click('apply-net')`);

  await test('fan verbindt en verschijnt in de app', async () => {
    await waitSim('verbonden', (s) => s.connected, 15000);
    const end = Date.now() + 10000;
    while (!(await js(`$text('link-status')`)).includes('E2E-FAN')) { if (Date.now() > end) throw new Error('status: ' + await js(`$text('link-status')`)); await sleep(200); }
    await js(`$tab('fan')`); await sleep(400);
    if (!(await js(`$text('fan-name')`)).includes('E2E-FAN')) throw new Error('fannaam niet getoond');
    await shot('01-fan');
  });

  await test('aan/uit en pauze/afspelen', async () => {
    await js(`$click('btn-power')`); await waitSim('uit', (s) => !s.power);
    await sleep(400); await js(`$click('btn-power')`); await waitSim('aan', (s) => s.power);
    await sleep(400); await js(`$click('btn-play')`); await waitSim('pauze', (s) => !s.playing);
    await sleep(400); await js(`$click('btn-play')`); await waitSim('speelt', (s) => s.playing);
  });

  await test('helderheid en hoek via schuifregelaars', async () => {
    await js(`$key('sl-brightness', 'Home')`); await waitSim('helderheid 1', (s) => s.brightness === 1);
    await js(`$key('sl-brightness', 'End')`); await waitSim('helderheid 16', (s) => s.brightness === 16);
    await js(`$key('sl-angle', 'Home')`); await waitSim('hoek -180 → 0', (s) => s.angle === 0);
    await js(`$key('sl-angle', 'End')`); await waitSim('hoek 180 → 360', (s) => s.angle === 360);
    await sleep(1600);
    const shown = await js(`$text('sl-angle')`);
    if (!shown.includes('180°')) throw new Error('UI toont hoek niet terug: ' + shown);
  });

  await test('kleur, herhalen, ademlicht, autostart, master', async () => {
    await js(`$click('color-true')`); await waitSim('true color', (s) => s.color_mode === 1);
    await sleep(300); await js(`$click('color-bright')`); await waitSim('hoge helderheid', (s) => s.color_mode === 2);
    await js(`$click('repeat-one')`); await waitSim('herhaal huidig', (s) => !s.repeat_all);
    await sleep(300); await js(`$click('repeat-all')`); await waitSim('herhaal alles', (s) => s.repeat_all);
    await js(`$click('sw-breath')`); await waitSim('ademlicht aan', (s) => s.breath);
    await sleep(300); await js(`$click('sw-autostart')`); await waitSim('autostart uit', (s) => !s.autostart);
    await sleep(300); await js(`$click('sw-master')`); await waitSim('master aan', (s) => s.master);
    await js(`$key('sl-volume', 'Home')`); await waitSim('volume 0', (s) => s.volume === 0);
    await shot('02-instellingen');
  });

  await test('foto uploaden via Media', async () => {
    await js(`$tab('media')`); await sleep(400);
    const png = await js(MAKE_IMAGE);
    await js(`window.__p56pick('zon.png', ${JSON.stringify(png)}, 'image/png')`);
    await sleep(800);
    if (!(await js(`$text('media-info')`)).includes('zon.png')) throw new Error('media-info: ' + await js(`$text('media-info')`));
    await js(`$click('upload')`);
    await waitSim('bestand zon.mp4', (s) => s.files.some((f) => f.name === 'zon.mp4' && f.width === 688 && f.height === 240), 15000);
    await shot('03-media');
  });

  await test('video omzetten naar frames en uploaden', async () => {
    const webm = await js(MAKE_VIDEO);
    await js(`window.__p56pick('clip.webm', ${JSON.stringify(webm)}, 'video/webm')`);
    await sleep(500);
    await js(`$click('make-frames')`);
    const end = Date.now() + 30000;
    while (!/frame\(s\)/.test(await js(`$text('media-info')`))) { if (Date.now() > end) throw new Error('geen frames: ' + await js(`$text('media-info')`)); await sleep(300); }
    await js(`$type('up-name', 'clip')`);
    await sleep(200);
    await js(`$click('upload')`);
    const s = await waitSim('video clip.mp4', (s) => s.files.some((f) => f.name === 'clip.mp4' && f.frames > 5), 30000);
    results.push(`    video: ${s.files.find((f) => f.name === 'clip.mp4').frames} frames`);
  });

  await test('afspeellijst: tonen, hernoemen, wissen', async () => {
    await js(`$tab('playlist')`); await sleep(600);
    await js(`$click('show-1')`); await waitSim('slot 1', (s) => s.current === 1);
    await js(`$click('rename-1')`); await sleep(300);
    await js(`$type('rename-input', 'zonnetje')`); await sleep(100);
    await js(`$click('rename-ok')`); await waitSim('hernoemd', (s) => s.files[0]?.name === 'zonnetje.mp4');
    await sleep(800);
    await js(`$click('delete-2')`); await sleep(300); await js(`$dialogBtn('Wissen')`);
    await waitSim('gewist', (s) => s.files.length === 1);
    await sleep(800);
    await shot('04-afspeellijst');
  });

  await test('netwerk: naam, router, bluetooth', async () => {
    await js(`$tab('network')`); await sleep(400);
    await js(`$type('dev-name', 'Woonkamer')`); await js(`$click('dev-name-save')`);
    await waitSim('naam', (s) => s.device_name === 'Woonkamer');
    await js(`$type('rt-ssid', 'ThuisNet')`); await js(`$type('rt-pw', 'geheim')`); await sleep(100); await js(`$click('rt-go')`);
    await waitSim('router', (s) => s.router[0] === 'ThuisNet' && s.router[1] === 'geheim');
    await js(`$click('ble-on')`); await waitSim('bt aan', (s) => s.ble_on);
    await sleep(300); await js(`$click('ble-scan')`);
    const end = Date.now() + 6000;
    while (!(await js(`$text('ble-list')`)).includes('BT-Speaker')) { if (Date.now() > end) throw new Error('bt-lijst: ' + await js(`$text('ble-list')`)); await sleep(200); }
    await shot('05-netwerk');
  });

  await test('live casten: testkaart en media', async () => {
    await js(`$tab('cast')`); await sleep(400);
    const before = (await sim()).cast_ok;
    await js(`$click('src-test')`); await js(`$click('cast-start')`);
    await waitSim('cast-frames', (s) => s.cast_ok >= before + 20, 15000);
    await sleep(800);
    if (!/fps/.test(await js(`$text('cast-stats')`))) throw new Error('geen statistiek: ' + await js(`$text('cast-stats')`));
    await shot('06-casten');
    await js(`$click('cast-stop')`); await sleep(800);
    const b2 = (await sim()).cast_ok;
    await js(`$click('src-media')`); await js(`$click('cast-start')`);
    await waitSim('media-cast', (s) => s.cast_ok >= b2 + 10, 15000);
    await js(`$click('cast-stop')`); await sleep(500);
  });

  await test('live casten: video (live)', async () => {
    const b = (await sim()).cast_ok;
    await js(`$click('src-video')`); await js(`$click('cast-start')`);
    await waitSim('video-cast', (s) => s.cast_ok >= b + 10, 20000);
    await js(`$click('cast-stop')`); await sleep(500);
  });

  await test('formatteren en fabrieksinstellingen (met bevestiging)', async () => {
    await js(`$tab('fan')`); await sleep(400);
    await js(`$click('btn-reset')`); await sleep(300); await js(`$dialogBtn('Herstellen')`);
    await waitSim('reset', (s) => s.resets === 1 && s.device_name === 'E2E-FAN');
    await js(`$click('btn-format')`); await sleep(300); await js(`$dialogBtn('Formatteren')`);
    await waitSim('format', (s) => s.formats === 1 && s.files.length === 0);
  });

  await test('geen protocolfouten in de simulator', async () => {
    const s = await sim();
    if (s.protocol_errors.length) throw new Error(s.protocol_errors.join(' | '));
    results.push(`    commando's ontvangen: ${s.commands.length}, cast-frames: ${s.cast_ok}, uploads: ${s.uploads}`);
  });

  console.log(`\n${passed} geslaagd, ${failed} mislukt`);
  writeFileSync(join(out, 'result.txt'), results.join('\n') + `\n\n${passed} geslaagd, ${failed} mislukt\n`);
  cleanup();
  process.exit(failed ? 1 : 0);
}

main().catch((e) => { console.error('E2E afgebroken:', e); cleanup(); process.exit(2); });
