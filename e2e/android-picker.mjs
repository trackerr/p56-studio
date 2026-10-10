// Android: real system file picker + content:// read, driven via adb taps.
//   node --experimental-websocket e2e/android-picker.mjs
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

const ADB = join(process.env.USERPROFILE, '.p56-android', 'sdk', 'platform-tools', 'adb.exe');
const adb = (...a) => spawnSync(ADB, ['-s', process.env.ANDROID_SERIAL || 'emulator-5554', ...a], { encoding: 'utf8' }).stdout || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const sock = (adb('shell', 'cat', '/proc/net/unix').match(/@(webview_devtools_remote_\d+)/) || [])[1];
if (!sock) throw new Error('app draait niet');
adb('forward', 'tcp:9336', `localabstract:${sock}`);
const page = (await (await fetch('http://127.0.0.1:9336/json')).json()).find((t) => t.type === 'page');
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0; const pend = new Map();
ws.onmessage = (m) => { const d = JSON.parse(m.data); pend.get(d.id)?.(d); };
const js = (e) => new Promise((r) => { const i = ++id; pend.set(i, (d) => r(d.result?.result?.value)); ws.send(JSON.stringify({ id: i, method: 'Runtime.evaluate', params: { expression: e, awaitPromise: true, returnByValue: true } })); });

await js(`(() => { const e = document.querySelector('[data-testid="tab-media"]'); e.dispatchEvent(new MouseEvent('mousedown', {bubbles: true, button: 0})); e.focus(); return true; })()`);
await sleep(500);
await js(`document.querySelector('[data-testid="pick"]').click(), true`);
console.log('kiesdialoog geopend');

// find the picture in the picker UI and tap it
let tapped = false;
for (let i = 0; i < 15 && !tapped; i++) {
  await sleep(1500);
  adb('shell', 'uiautomator', 'dump', '/sdcard/ui.xml');
  const xml = adb('shell', 'cat', '/sdcard/ui.xml');
  // a node mentioning our file, or the first image thumbnail in the photo picker / documents UI
  const m = xml.match(/<node[^>]*(?:text|content-desc)="[^"]*fanfoto[^"]*"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/)
    || xml.match(/<node[^>]*content-desc="(?:Photo|Foto|Image|Afbeelding)[^"]*"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/)
    || xml.match(/<node[^>]*resource-id="[^"]*(?:icon_thumb|thumbnail|preview_image)[^"]*"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/);
  if (m) {
    const [x, y] = [(+m[1] + +m[3]) >> 1, (+m[2] + +m[4]) >> 1];
    adb('shell', 'input', 'tap', String(x), String(y));
    console.log(`getikt op ${x},${y}`);
    tapped = true;
  } else if (i === 3) {
    console.log('nog niets gevonden; schermdump:', (xml.match(/(text|content-desc)="[^"]+"/g) || []).slice(0, 25).join(' | '));
  }
}
if (!tapped) throw new Error('foto niet gevonden in kiesdialoog');

// some pickers need a confirm ("Done"/"Gereed"/"Add"/"Select")
await sleep(1500);
adb('shell', 'uiautomator', 'dump', '/sdcard/ui.xml');
const xml2 = adb('shell', 'cat', '/sdcard/ui.xml');
const ok = xml2.match(/<node[^>]*text="(?:Done|Gereed|Add|Toevoegen|Select|Selecteren|Open|Openen)"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/);
if (ok) { adb('shell', 'input', 'tap', String((+ok[1] + +ok[3]) >> 1), String((+ok[2] + +ok[4]) >> 1)); console.log('bevestigd'); }

for (let i = 0; i < 20; i++) {
  await sleep(500);
  const info = await js(`(document.querySelector('[data-testid="media-info"]')||{}).textContent`);
  if (/frame/.test(info) && /\d+×\d+/.test(info)) { console.log('✔ bestand gelezen via content://:', info); process.exit(0); }
  const toast = await js(`[...document.querySelectorAll('[data-sonner-toast]')].map(t => t.textContent).join(' | ')`);
  if (toast && /Kan bestand/.test(toast)) { console.log('✘', toast); process.exit(1); }
}
console.log('✘ geen media-info na kiezen:', await js(`(document.querySelector('[data-testid="media-info"]')||{}).textContent`));
process.exit(1);
