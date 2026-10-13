// End-to-end test: the real P56 Studio app (WebView2) against the fan simulator.
// Drives the UI through the Chrome DevTools Protocol and checks the simulated fan.
//   node --experimental-websocket e2e/app-e2e.mjs [--release] [--keep]
// The UI is switched to English for the run (the previous language is restored at the end);
// checks use data-testids and data-* attributes, so they do not depend on the visible text.
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
// --android: the app runs in an emulator/phone; adb forwards its WebView DevTools and TCP 6666.
const android = process.argv.includes('--android');
const CDP_PORT = android ? 9335 : 9333, SIM_HTTP = android ? 8098 : 8099;
const ADB = join(process.env.ANDROID_HOME || join(process.env.USERPROFILE, '.p56-android', 'sdk'), 'platform-tools', 'adb.exe');
async function adb(...args) {
  return new Promise((res) => {
    const p = spawn(ADB, args, { stdio: ['ignore', 'pipe', 'pipe'] });
    let o = ''; p.stdout.on('data', (d) => (o += d)); p.stderr.on('data', (d) => (o += d));
    p.on('close', () => res(o.trim()));
  });
}
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
  throw new Error(`simulator: ${desc} — state: ${JSON.stringify({ ...s, commands: s?.commands?.slice(-8) })}`);
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
  throw new Error('no webview with a debug port found');
}
function cdp(method, params = {}) {
  const id = ++msgId;
  ws.send(JSON.stringify({ id, method, params }));
  return new Promise((res) => pending.set(id, res));
}
async function js(expr, userGesture = false) {
  const r = await cdp('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true, userGesture });
  if (r.result?.exceptionDetails) throw new Error(`JS: ${r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text}`);
  return r.result?.result?.value;
}
async function shot(name) {
  const r = await cdp('Page.captureScreenshot', { format: 'png' });
  if (r.result?.data) writeFileSync(join(out, `${name}.png`), Buffer.from(r.result.data, 'base64'));
}

// The app shares its settings (localStorage) with the owner's installed P56 Studio: every p56.*
// key is saved before the run and put back exactly afterwards (language, server mode, fan IP …).
let prevStore;
const P56_KEYS = `Object.keys(localStorage).filter((k) => k.startsWith('p56.'))`;
async function restoreStore() {
  if (prevStore === undefined || !ws || ws.readyState !== 1) return;
  const expr = `${P56_KEYS}.forEach((k) => localStorage.removeItem(k));`
    + ` Object.entries(${JSON.stringify(prevStore)}).forEach(([k, v]) => localStorage.setItem(k, v)); true`;
  await Promise.race([js(expr).catch(() => {}), sleep(2000)]);
}

// page helpers (installed into the page)
const HELPERS = `
window.$t = (id) => document.querySelector('[data-testid="' + id + '"]');
window.$click = (id) => { const e = $t(id); if (!e) throw new Error('not found: ' + id); e.click(); return true; };
window.$tab = (v) => { const e = $t('tab-' + v); e.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 })); e.focus(); e.click(); return true; };
window.$type = (id, v) => { const e = $t(id); const s = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set; s.call(e, v); e.dispatchEvent(new Event('input', { bubbles: true })); return true; };
window.$key = (id, key) => { const th = $t(id).querySelector('[role=slider]'); th.focus(); th.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true })); th.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true })); return true; };
window.$confirm = () => { const b = document.querySelector('[role=dialog] [data-testid="confirm-ok"]'); if (!b) throw new Error('confirm button not found in dialog'); b.click(); return true; };
window.$text = (id) => ($t(id) || {}).textContent || '';
window.$data = (id, k) => (($t(id) || {}).dataset || {})[k] || '';
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

// a small PNG and a WebM video made in the page itself
const MAKE_IMAGE = `(async () => {
  const c = document.createElement('canvas'); c.width = 640; c.height = 480; const g = c.getContext('2d');
  g.fillStyle = '#102030'; g.fillRect(0, 0, 640, 480); g.fillStyle = '#ffcc00'; g.beginPath(); g.arc(320, 240, 150, 0, 7); g.fill();
  g.fillStyle = '#000'; g.font = 'bold 80px sans-serif'; g.fillText('P56', 250, 270);
  return c.toDataURL('image/png').split(',')[1];
})()`;
// a moving square with a 440 Hz tone, so the sound path (WebView decode → fan records) is tested too
const MAKE_VIDEO = `(async () => {
  const c = document.createElement('canvas'); c.width = 320; c.height = 240; const g = c.getContext('2d');
  const ac = new AudioContext(); await ac.resume();
  const osc = ac.createOscillator(); osc.frequency.value = 440; const dest = ac.createMediaStreamDestination(); osc.connect(dest); osc.start();
  const stream = new MediaStream([...c.captureStream(20).getVideoTracks(), ...dest.stream.getAudioTracks()]);
  const rec = new MediaRecorder(stream, { mimeType: 'video/webm' });
  const chunks = []; rec.ondataavailable = (e) => chunks.push(e.data);
  rec.start(100);
  for (let i = 0; i < 30; i++) { g.fillStyle = 'hsl(' + i * 12 + ',90%,50%)'; g.fillRect(0, 0, 320, 240); g.fillStyle = '#fff'; g.fillRect(i * 10, 100, 40, 40); await new Promise((r) => setTimeout(r, 50)); }
  rec.stop(); await new Promise((r) => (rec.onstop = r)); osc.stop(); ac.close();
  const b = new Blob(chunks, { type: 'video/webm' });
  const buf = new Uint8Array(await b.arrayBuffer()); let s = ''; for (const x of buf) s += String.fromCharCode(x); return btoa(s);
})()`;

async function main() {
  for (const n of android ? ['p56-sim'] : ['p56-sim', 'p56-studio']) if (!existsSync(exe(n))) throw new Error(`${exe(n)} is missing; build it first`);
  console.log(`P56 end-to-end (${android ? 'Android' : profile})`);
  if (android) {
    // fresh app start, then forward its WebView DevTools socket and the fan port
    await adb('shell', 'am', 'force-stop', 'nl.trackerr.p56studio');
    await adb('shell', 'monkey', '-p', 'nl.trackerr.p56studio', '-c', 'android.intent.category.LAUNCHER', '1');
    let sock = '';
    for (let i = 0; i < 60 && !sock; i++) {
      await sleep(1000);
      const unix = await adb('shell', 'cat', '/proc/net/unix');
      sock = (unix.match(/@(webview_devtools_remote_\d+)/) || [])[1] || '';
    }
    if (!sock) throw new Error('no WebView debug socket (is this a debug build?)');
    await adb('forward', `tcp:${CDP_PORT}`, `localabstract:${sock}`);
    await adb('forward', 'tcp:16666', 'tcp:6666');
    await sleep(2000);
    start(exe('p56-sim'), ['--name', 'E2E-FAN', '--http', String(SIM_HTTP), '--max-files', '8', '--udp-port', '18988', '--pc', '127.0.0.1:16666']);
  } else {
    // own UDP port and a direct connection: another P56 app on the network (a phone) never gets this fan
    start(exe('p56-sim'), ['--name', 'E2E-FAN', '--http', String(SIM_HTTP), '--max-files', '8', '--udp-port', '18987', '--pc', '127.0.0.1:6666']);
    await sleep(500);
    // only the simulator on this pc may connect: a real fan on the network must never get the
    // test's actions (a run once renamed and filled the owner's fan)
    start(exe('p56-studio'), [], { WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`, RUST_LOG: 'info', P56_FAN_ALLOW: '127.0.0.1' });
  }
  await cdpConnect();
  await cdp('Runtime.enable');
  await cdp('Page.enable');
  await sleep(1500);
  await js(HELPERS);
  prevStore = JSON.parse(await js(`JSON.stringify(Object.fromEntries(${P56_KEYS}.map((k) => [k, localStorage.getItem(k)])))`));
  // the owner's app may go through a P56 server: the test runs the app's own engine
  if (prevStore['p56.server']) {
    await js(`localStorage.removeItem('p56.server'); location.reload(); true`);
    await sleep(3000);
    await js(HELPERS);
  }
  // run the UI in English; the switch applies at once (no reload) and stores 'p56.lang'
  await js(`localStorage.setItem('p56.lang', 'en'); $click('lang-en')`);
  await sleep(200);
  if (!android) {
    // point the app at the simulator on this pc, then reconnect
    await js(`localStorage.setItem('p56.targets', '127.0.0.1'); localStorage.removeItem('p56.ip'); true`);
    await js(`$tab('app')`); await sleep(300);
    await js(`$click('apply-net')`);
  }

  await test('fan connects and appears in the app', async () => {
    await waitSim('connected', (s) => s.connected, 15000);
    const end = Date.now() + 10000;
    while (!(await js(`$text('link-status')`)).includes('E2E-FAN')) { if (Date.now() > end) throw new Error('status: ' + await js(`$text('link-status')`)); await sleep(200); }
    // never act on a real fan: stop the whole run when anything but the simulator connected
    await sleep(3000);
    const status = await js(`$text('link-status')`);
    if (/\d+ fans/.test(status) || !status.includes('E2E-FAN')) {
      console.error(`ABORTED: another fan connected (${status}); nothing was sent to it by the tests`);
      await restoreStore(); cleanup(); process.exit(3);
    }
    await js(`$tab('fan')`); await sleep(400);
    if (!(await js(`$text('fan-name')`)).includes('E2E-FAN')) throw new Error('fan name not shown');
    await shot('01-fan');
  });

  await test('language switch NL / EN', async () => {
    await js(`$click('lang-nl')`); await sleep(200);
    if ((await js(`document.documentElement.lang`)) !== 'nl' || !(await js(`$text('tab-playlist')`)).includes('Afspeellijst')) throw new Error('Dutch not applied: ' + await js(`$text('tab-playlist')`));
    await js(`$click('lang-en')`); await sleep(200);
    if ((await js(`document.documentElement.lang`)) !== 'en' || !(await js(`$text('tab-playlist')`)).includes('Playlist')) throw new Error('English not applied: ' + await js(`$text('tab-playlist')`));
    if ((await js(`localStorage.getItem('p56.lang')`)) !== 'en') throw new Error('language not stored');
  });

  await test('power on/off and pause/play', async () => {
    await js(`$click('btn-power')`); await waitSim('off', (s) => !s.power);
    await sleep(400); await js(`$click('btn-power')`); await waitSim('on', (s) => s.power);
    await sleep(400); await js(`$click('btn-play')`); await waitSim('paused', (s) => !s.playing);
    await sleep(400); await js(`$click('btn-play')`); await waitSim('playing', (s) => s.playing);
  });

  await test('brightness and angle via sliders', async () => {
    await js(`$key('sl-brightness', 'Home')`); await waitSim('brightness 1', (s) => s.brightness === 1);
    await js(`$key('sl-brightness', 'End')`); await waitSim('brightness 16', (s) => s.brightness === 16);
    await js(`$key('sl-angle', 'Home')`); await waitSim('angle -180 → 0', (s) => s.angle === 0);
    await js(`$key('sl-angle', 'End')`); await waitSim('angle 180 → 360', (s) => s.angle === 360);
    await sleep(1600);
    const shown = await js(`$text('sl-angle')`);
    if (!shown.includes('180°')) throw new Error('UI does not show the angle back: ' + shown);
  });

  await test('repeat, breathing light, autostart', async () => {
    await js(`$click('repeat-one')`); await waitSim('repeat current', (s) => !s.repeat_all);
    await sleep(300); await js(`$click('repeat-all')`); await waitSim('repeat all', (s) => s.repeat_all);
    await js(`$click('sw-breath')`); await waitSim('breathing light on', (s) => s.breath);
    await sleep(300); await js(`$click('sw-autostart')`); await waitSim('autostart off', (s) => !s.autostart);
    await js(`$key('sl-volume', 'Home')`); await waitSim('volume 0', (s) => s.volume === 0);
    await shot('02-settings');
  });

  await test('upload a photo via Media', async () => {
    await js(`$tab('media')`); await sleep(400);
    const png = await js(MAKE_IMAGE);
    await js(`window.__p56pick('zon.png', ${JSON.stringify(png)}, 'image/png')`);
    await sleep(800);
    if (!(await js(`$text('media-info')`)).includes('zon.png')) throw new Error('media-info: ' + await js(`$text('media-info')`));
    await js(`$click('upload')`);
    await waitSim('file zon.mp4', (s) => s.files.some((f) => f.name === 'zon.mp4' && f.width === 688 && f.height === 240), 15000);
    await shot('03-media');
  });

  await test('the same name again: asks to overwrite, then replaces the file', async () => {
    // the previous upload finishes first (the button is disabled meanwhile)
    const ready = Date.now() + 15000;
    while (await js(`$t('upload').disabled`)) { if (Date.now() > ready) throw new Error('upload stays disabled'); await sleep(200); }
    const before = (await sim()).uploads;
    await js(`$click('upload')`);
    const end = Date.now() + 15000;
    while (!(await js(`!!$t('overwrite-ok')`))) { if (Date.now() > end) throw new Error('no overwrite question'); await sleep(200); }
    await js(`$click('overwrite-ok')`);
    await waitSim('zon.mp4 replaced', (s) => s.uploads === before + 1 && s.files.filter((f) => f.name === 'zon.mp4').length === 1, 20000);
  });

  await test('moving text: a ticker uploaded as an animation', async () => {
    // an upload from the test before may still be finishing: the app is busy until then
    const idle = Date.now() + 20000;
    while (await js(`!!$t('busy') || $t('upload').disabled`)) { if (Date.now() > idle) throw new Error('the app stays busy'); await sleep(200); }
    await js(`$type('text-input', 'Lichtkrant')`); await sleep(100);
    await js(`$click('motion-scroll')`); await sleep(100);
    await js(`$click('text-make')`);
    // while the ticker renders, upload must wait (it would send the previous picture): the
    // button is disabled until the frames are there
    let waited = false;
    for (let i = 0; i < 80 && !waited; i++) {
      await sleep(25);
      const e = JSON.parse(await js(`JSON.stringify({ off: document.querySelector('[data-testid=upload]').disabled, frames: Number(document.querySelector('[data-testid=media-info]')?.dataset.frames || 0) })`));
      if (e.off) waited = true;
      else if (e.frames > 20) break;
      else if (i === 79) throw new Error('upload is possible while the ticker is still being made: ' + JSON.stringify(e) + ' text-make disabled: ' + await js(`$t('text-make').disabled`) + ' busy: ' + await js(`$text('busy')`));
    }
    const end = Date.now() + 15000;
    while (!(Number(await js(`$data('media-info', 'frames')`)) > 20)) { if (Date.now() > end) throw new Error('no ticker frames: ' + await js(`$text('media-info')`)); await sleep(200); }
    await js(`$type('up-name', 'ticker')`); await sleep(100);
    await js(`$click('upload')`);
    await waitSim('ticker.mp4', (s) => s.files.some((f) => f.name === 'ticker.mp4' && f.frames > 20), 30000);
    await js(`$click('motion-none')`);
  });

  await test('upload a typed text without pressing Text first', async () => {
    await js(`$click('testcard')`); await sleep(200);
    const before = (await sim()).files.length;
    await js(`$type('text-input', 'Direct')`); await sleep(100);
    await js(`$click('upload')`);
    const s = await waitSim('Direct.mp4', (s) => s.files.some((f) => f.name === 'Direct.mp4'), 30000);
    await sleep(1500);
    if ((await sim()).files.length !== before + 1) throw new Error('expected only the text to be added: ' + JSON.stringify(s.files.map((f) => f.name)));
  });

  await test('convert a video to frames and upload it', async () => {
    const webm = await js(MAKE_VIDEO, true);
    await js(`window.__p56pick('clip.webm', ${JSON.stringify(webm)}, 'video/webm')`);
    await sleep(500);
    await js(`$click('make-frames')`);
    const end = Date.now() + 30000;
    while (!(Number(await js(`$data('media-info', 'frames')`)) > 0)) { if (Date.now() > end) throw new Error('no frames: ' + await js(`$text('media-info')`) + ' | toasts: ' + await js(`[...document.querySelectorAll('[data-sonner-toast]')].map((t) => t.textContent).join(' / ')`)); await sleep(300); }
    await js(`$type('up-name', 'clip')`);
    await sleep(200);
    await js(`$click('upload')`);
    const s = await waitSim('video clip.mp4 with sound', (s) => s.files.some((f) => f.name === 'clip.mp4' && f.frames > 5 && f.sound_frames > 0), 30000);
    const f = s.files.find((f) => f.name === 'clip.mp4');
    results.push(`    video: ${f.frames} frames, ${f.sound_frames} with sound`);
  });

  await test('upload a video without pressing Make frames first', async () => {
    const webm = await js(MAKE_VIDEO, true);
    await js(`window.__p56pick('direct.webm', ${JSON.stringify(webm)}, 'video/webm')`);
    await sleep(500);
    await js(`$type('up-name', 'direct-video')`); await sleep(200);
    await js(`$click('upload')`);
    await waitSim('direct-video.mp4', (s) => s.files.some((f) => f.name === 'direct-video.mp4' && f.frames > 5), 60000);
  });

  await test('playlist: show, rename, delete', async () => {
    await js(`$tab('playlist')`); await sleep(600);
    await js(`$click('show-1')`); await waitSim('slot 1', (s) => s.current === 1);
    await js(`$click('rename-1')`); await sleep(300);
    await js(`$type('rename-input', 'sunshine')`); await sleep(100);
    await js(`$click('rename-ok')`); await waitSim('renamed', (s) => s.files[0]?.name === 'sunshine.mp4');
    await sleep(800);
    const files = (await sim()).files.length;
    await js(`$click('delete-2')`); await sleep(300); await js(`$confirm()`);
    await waitSim('deleted', (s) => s.files.length === files - 1);
    await sleep(800);
    await shot('04-playlist');
  });

  await test('network: name, router', async () => {
    await js(`$tab('network')`); await sleep(400);
    await js(`$type('dev-name', 'LivingRoom')`); await js(`$click('dev-name-save')`);
    await waitSim('name', (s) => s.device_name === 'LivingRoom');
    await js(`$type('rt-ssid', 'HomeNet')`); await js(`$type('rt-pw', 'secret')`); await sleep(100); await js(`$click('rt-go')`);
    await waitSim('router', (s) => s.router[0] === 'HomeNet' && s.router[1] === 'secret');
    // P56 Studio does not use the fan's Bluetooth: no section for it
    if (/bluetooth/i.test(await js(`document.body.textContent`))) throw new Error('the network tab still shows Bluetooth');
    await shot('05-network');
  });

  await test('live cast: test card and media', async () => {
    await js(`$tab('cast')`); await sleep(400);
    const before = (await sim()).cast_ok;
    await js(`$click('src-test')`); await js(`$click('cast-start')`);
    await waitSim('cast frames', (s) => s.cast_ok >= before + 20, 15000);
    await sleep(800);
    if (!(await js(`$data('cast-stats', 'fps')`))) throw new Error('no statistics: ' + await js(`$text('cast-stats')`));
    await shot('06-cast');
    await js(`$click('cast-stop')`); await sleep(800);
    const b2 = (await sim()).cast_ok;
    await js(`$click('src-media')`); await js(`$click('cast-start')`);
    await waitSim('media cast', (s) => s.cast_ok >= b2 + 10, 15000);
    await js(`$click('cast-stop')`); await sleep(500);
  });

  await test('cast sources: no live video, Home Assistant cameras only in Home Assistant', async () => {
    // a video is cast as media (frames made on the Media tab); outside the add-on there are no HA cameras
    if (await js(`!!$t('src-video')`)) throw new Error('the video source is still there');
    if (await js(`!!$t('src-ha')`)) throw new Error('Home Assistant cameras offered outside Home Assistant');
    if (!(await js(`!!$t('src-test') && !!$t('src-media')`))) throw new Error('test card or media source missing');
  });

  await test('format and factory reset (with confirmation)', async () => {
    await js(`$tab('fan')`); await sleep(400);
    await js(`$click('btn-reset')`); await sleep(300); await js(`$confirm()`);
    await waitSim('reset', (s) => s.resets === 1 && s.device_name === 'E2E-FAN');
    await js(`$click('btn-format')`); await sleep(300); await js(`$confirm()`);
    await waitSim('format', (s) => s.formats === 1 && s.files.length === 0);
  });

  if (!android) await test('control through a p56-server (the app as its client)', async () => {
    const http = 8771;
    const srv = start(join(target, profile, 'p56-server.exe'), ['--http-port', String(http), '--tcp-port', '6666', '--fan', '127.0.0.1', '--ip', '127.0.0.1', '--fan-allow', '127.0.0.1']);
    await js(`$tab('app')`); await sleep(400);
    await js(`$click('mode-server')`); await sleep(200);
    await js(`$type('server-url', '127.0.0.1:${http}')`); await sleep(100);
    // "use" lets the app's own link go (so the fan can reach the server) and reloads
    await js(`$click('server-use')`);
    await sleep(3000);
    await js(HELPERS);
    const end = Date.now() + 20000;
    while (!(await js(`$text('link-status')`)).includes('E2E-FAN')) { if (Date.now() > end) throw new Error('through the server: ' + await js(`$text('link-status')`)); await sleep(300); }
    if (!(await js(`$text('link-status')`)).includes('via server')) throw new Error('header does not say "via server"');
    await js(`$tab('fan')`); await sleep(400);
    const before = (await sim()).power;
    await js(`$click('btn-power')`); await waitSim('power toggled through the server', (s) => s.power !== before);
    // let the app see the new state first (it refreshes every 1.5 s), or it sends the same action again
    await sleep(2000);
    await js(`$click('btn-power')`); await waitSim('power back', (s) => s.power === before);
    await shot('07-via-server');
    // back to the app's own engine (the server lets go of the fan first)
    srv.kill(); await sleep(500);
    await js(`$tab('app')`); await sleep(300);
    await js(`$click('mode-local')`);
    await sleep(2500);
    await js(HELPERS);
    // the address is still filled in when choosing a server again
    await js(`$tab('app')`); await sleep(300);
    await js(`$click('mode-server')`); await sleep(200);
    const kept = await js(`$t('server-url').value`);
    await js(`$click('mode-local')`);
    if (!String(kept).includes(`127.0.0.1:${http}`)) throw new Error(`server address not kept: "${kept}"`);
  });

  await test('no protocol errors in the simulator', async () => {
    const s = await sim();
    if (s.protocol_errors.length) throw new Error(s.protocol_errors.join(' | '));
    results.push(`    commands received: ${s.commands.length}, cast frames: ${s.cast_ok}, uploads: ${s.uploads}`);
  });

  await restoreStore();
  console.log(`\n${passed} passed, ${failed} failed`);
  writeFileSync(join(out, 'result.txt'), results.join('\n') + `\n\n${passed} passed, ${failed} failed\n`);
  cleanup();
  process.exit(failed ? 1 : 0);
}

main().catch(async (e) => { console.error('E2E aborted:', e); await restoreStore(); cleanup(); process.exit(2); });
