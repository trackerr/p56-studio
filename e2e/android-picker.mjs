// Android: real system file picker + content:// read, driven via adb taps.
//   node --experimental-websocket e2e/android-picker.mjs
// Checks use data-testids / data-* attributes, so they work whatever the UI language is.
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

const ADB = join(process.env.USERPROFILE, '.p56-android', 'sdk', 'platform-tools', 'adb.exe');
const adb = (...a) => spawnSync(ADB, ['-s', process.env.ANDROID_SERIAL || 'emulator-5554', ...a], { encoding: 'utf8' }).stdout || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const sock = (adb('shell', 'cat', '/proc/net/unix').match(/@(webview_devtools_remote_\d+)/) || [])[1];
if (!sock) throw new Error('the app is not running');
adb('forward', 'tcp:9336', `localabstract:${sock}`);
const page = (await (await fetch('http://127.0.0.1:9336/json')).json()).find((t) => t.type === 'page');
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0; const pend = new Map();
ws.onmessage = (m) => { const d = JSON.parse(m.data); pend.get(d.id)?.(d); };
const js = (e) => new Promise((r) => { const i = ++id; pend.set(i, (d) => r(d.result?.result?.value)); ws.send(JSON.stringify({ id: i, method: 'Runtime.evaluate', params: { expression: e, awaitPromise: true, returnByValue: true } })); });
const mediaInfo = () => js(`(() => { const e = document.querySelector('[data-testid="media-info"]'); return e ? { text: e.textContent, frames: Number(e.dataset.frames || 0), size: e.dataset.size || '' } : null; })()`);

await js(`(() => { const e = document.querySelector('[data-testid="tab-media"]'); e.dispatchEvent(new MouseEvent('mousedown', {bubbles: true, button: 0})); e.focus(); return true; })()`);
await sleep(500);
await js(`document.querySelector('[data-testid="pick"]').click(), true`);
console.log('picker opened');

// find the picture in the picker UI and tap it
let tapped = false;
for (let i = 0; i < 15 && !tapped; i++) {
  await sleep(1500);
  adb('shell', 'uiautomator', 'dump', '/sdcard/ui.xml');
  const xml = adb('shell', 'cat', '/sdcard/ui.xml');
  // a node mentioning our file, or the first image thumbnail in the photo picker / documents UI
  // (the system picker follows the phone's language, hence the Dutch alternatives)
  const m = xml.match(/<node[^>]*(?:text|content-desc)="[^"]*fanfoto[^"]*"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/)
    || xml.match(/<node[^>]*content-desc="(?:Photo|Foto|Image|Afbeelding)[^"]*"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/)
    || xml.match(/<node[^>]*resource-id="[^"]*(?:icon_thumb|thumbnail|preview_image)[^"]*"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/);
  if (m) {
    const [x, y] = [(+m[1] + +m[3]) >> 1, (+m[2] + +m[4]) >> 1];
    adb('shell', 'input', 'tap', String(x), String(y));
    console.log(`tapped at ${x},${y}`);
    tapped = true;
  } else if (i === 3) {
    console.log('nothing found yet; screen dump:', (xml.match(/(text|content-desc)="[^"]+"/g) || []).slice(0, 25).join(' | '));
  }
}
if (!tapped) throw new Error('photo not found in the picker');

// some pickers need a confirm ("Done"/"Gereed"/"Add"/"Select")
await sleep(1500);
adb('shell', 'uiautomator', 'dump', '/sdcard/ui.xml');
const xml2 = adb('shell', 'cat', '/sdcard/ui.xml');
const ok = xml2.match(/<node[^>]*text="(?:Done|Gereed|Add|Toevoegen|Select|Selecteren|Open|Openen)"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/);
if (ok) { adb('shell', 'input', 'tap', String((+ok[1] + +ok[3]) >> 1), String((+ok[2] + +ok[4]) >> 1)); console.log('confirmed'); }

for (let i = 0; i < 20; i++) {
  await sleep(500);
  const info = await mediaInfo();
  if (info && info.frames > 0 && /^\d+x\d+$/.test(info.size)) { console.log('✔ file read via content://:', info.text); process.exit(0); }
  // the "cannot open file" error toast
  const toast = await js(`[...document.querySelectorAll('[data-sonner-toast][data-testid="toast-open-failed"]')].map(t => t.textContent).join(' | ')`);
  if (toast) { console.log('✘', toast); process.exit(1); }
}
console.log('✘ no media info after picking:', (await mediaInfo())?.text);
process.exit(1);
