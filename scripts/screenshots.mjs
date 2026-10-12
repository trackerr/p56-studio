// Makes the README screenshots from the UI in its browser demo mode (no fan needed).
//   pnpm --dir app/ui dev            (serves the UI on http://localhost:1420)
//   node --experimental-websocket scripts/screenshots.mjs [url] [--out dir]
// Uses headless Microsoft Edge (or Chrome, via EDGE=path) through the DevTools protocol.
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const url = args.find((a) => a.startsWith('http')) || 'http://localhost:1420/';
const outIdx = args.indexOf('--out');
const out = outIdx >= 0 ? args[outIdx + 1] : join(root, 'docs', 'screenshots');
mkdirSync(out, { recursive: true });

const browsers = [
  process.env.EDGE,
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  '/usr/bin/microsoft-edge', '/usr/bin/google-chrome', '/usr/bin/chromium',
].filter(Boolean);
const exe = browsers.find((b) => existsSync(b));
if (!exe) throw new Error('no Edge or Chrome found (set EDGE=path)');

const PORT = 9340;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const proc = spawn(exe, ['--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${mkdtempSync(join(tmpdir(), 'p56shots-'))}`,
  '--hide-scrollbars', '--no-first-run', '--autoplay-policy=no-user-gesture-required', 'about:blank'], { stdio: 'ignore' });
process.on('exit', () => { try { proc.kill(); } catch { /* gone */ } });

let ws, msgId = 0;
const pending = new Map();
async function connect() {
  for (let i = 0; i < 60; i++) {
    try {
      const page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).find((t) => t.type === 'page');
      if (page) {
        ws = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
        ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } };
        return;
      }
    } catch { /* not up yet */ }
    await sleep(250);
  }
  throw new Error('browser did not start');
}
const cdp = (method, params = {}) => { const id = ++msgId; ws.send(JSON.stringify({ id, method, params })); return new Promise((r) => pending.set(id, r)); };
async function js(expr) {
  const r = await cdp('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error(`JS: ${r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text}`);
  return r.result?.result?.value;
}

const HELPERS = `
window.$t = (id) => document.querySelector('[data-testid="' + id + '"]');
window.$click = (id) => { const e = $t(id); if (!e) throw new Error('not found: ' + id); e.click(); return true; };
window.$tab = (v) => { const e = $t('tab-' + v); e.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 })); e.click(); return true; };
window.$type = (id, v) => { const e = $t(id); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(e, v); e.dispatchEvent(new Event('input', { bubbles: true })); return true; };
true`;

/** Load the UI at a size, run `steps` (page JS), then save a screenshot. */
async function shot(name, { width, height, scale = 1, mobile = false, lang = 'en', steps = [], wait = 900, jpeg = true, file }) {
  await cdp('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: scale, mobile });
  await cdp('Page.navigate', { url });
  await sleep(1500);
  await js(`localStorage.setItem('p56.lang', '${lang}'); location.reload(); true`);
  await sleep(1800);
  await js(HELPERS);
  // no notifications or keyboard focus rings in the pictures
  await js(`const st = document.createElement('style'); st.textContent = '[data-sonner-toaster]{display:none!important}'; document.head.append(st); true`);
  for (const s of steps) { if (typeof s === 'number') await sleep(s); else await js(s); }
  await js(`document.activeElement?.blur(); true`);
  await sleep(wait);
  const r = await cdp('Page.captureScreenshot', jpeg ? { format: 'jpeg', quality: 88 } : { format: 'png' });
  file ??= join(out, `${name}.${jpeg ? 'jpg' : 'png'}`);
  writeFileSync(file, Buffer.from(r.result.data, 'base64'));
  console.log('  ✔', file);
}

await connect();
await cdp('Page.enable');
await cdp('Runtime.enable');
console.log(`screenshots of ${url}`);
const desktop = { width: 1440, height: 900 };
await shot('fan', { ...desktop });
await shot('media', { ...desktop, steps: [`$tab('media')`, 300, `$type('text-input', 'Welcome home!')`, 100, `$click('text-make')`] });
await shot('cast', { ...desktop, steps: [`$tab('cast')`, 300, `$click('src-test')`, `$click('cast-start')`] });
await shot('phone', { width: 390, height: 844, scale: 2, mobile: true, jpeg: false });
// the Home Assistant README (ha/): the app as the panel shows it
if (outIdx < 0) await shot('panel', { ...desktop, file: join(root, 'ha', 'docs', 'panel.jpg') });
proc.kill();
process.exit(0);
