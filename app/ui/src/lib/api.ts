// Bridge to the Rust backend (Tauri). In a plain browser a small in-page demo fan
// answers the same commands, so the UI can be developed and tested without the app.

import { t } from '@/lib/i18n';

export type Status = {
  power: boolean; playing: boolean; repeat_all: boolean; breath: boolean; autostart: boolean;
  play_index: number; brightness: number; color_mode: number; angle: number; fan_type: number;
};
export type Device = {
  id: number; ip: string; name: string; mac: string; hotspot: string; router: string;
  status: Status | null; files: [number, string][]; last_error: string | null;
  /** Speaker volume (0–10), once the fan reported it. */
  volume: number | null; model: string | null; support: string;
};
export type FanTypeInfo = { id: number; model: string | null; angles: number; leds: number; jpeg: string | null; support: string };
export type Preview = { round: string; layout: string; jpeg_bytes: number; quality: number; width: number; height: number; frames: number };
export type MediaInfo = { id: number; name: string; frames: number; width: number; height: number; audioMs?: number };
/** Where the picture sits on the round display; `rotate` turns it clockwise, in degrees. */
export type Placement = { x: number; y: number; scale: number; mirror: boolean; rotate: number };
export const DEFAULT_PLACEMENT: Placement = { x: 0, y: 0, scale: 1, mirror: false, rotate: 0 };
export type P56Event = { type: string; data: any };

const T: any = (window as any).__TAURI__;
export const isTauri = !!T?.core?.invoke;
/** The app's own version (Tauri only). Through a p56-server, `platform` reports the server's. */
export const appVersion: Promise<string | null> = isTauri ? T.core.invoke('plugin:app|version').catch(() => null) : Promise.resolve(null);

// ---- API token for p56-server (LAN clients may have to send one) ----
// Taken from `?token=` in the page URL (then removed from the address bar) or from localStorage.
const TOKEN_KEY = 'p56.token';
let apiToken = '';
// Set when the server answers 401; the token prompt subscribes to it.
let authNeeded = false;
const authListeners = new Set<(needed: boolean) => void>();
try { apiToken = localStorage.getItem(TOKEN_KEY) || ''; } catch { /* private mode */ }
if (!isTauri) {
  try {
    const u = new URL(window.location.href);
    const q = u.searchParams.get('token');
    if (q) {
      apiToken = q;
      try { localStorage.setItem(TOKEN_KEY, q); } catch { /* private mode */ }
      u.searchParams.delete('token');
      history.replaceState(history.state, '', u.toString());
    }
  } catch { /* ignore */ }
}

/** Store a new API token (from the token prompt). */
export function setApiToken(token: string) {
  apiToken = token.trim();
  try { if (apiToken) localStorage.setItem(TOKEN_KEY, apiToken); else localStorage.removeItem(TOKEN_KEY); } catch { /* private mode */ }
  authNeeded = false;
  authListeners.forEach((l) => l(false));
}
export function onAuthNeeded(cb: (needed: boolean) => void): () => void {
  authListeners.add(cb);
  if (authNeeded) cb(true);
  return () => { authListeners.delete(cb); };
}
function requireAuth() {
  authNeeded = true;
  authListeners.forEach((l) => l(true));
}

function authHeaders(h: Record<string, string> = {}): Record<string, string> {
  return apiToken ? { ...h, Authorization: `Bearer ${apiToken}` } : h;
}

// ---- where the commands go ----
// In the app: to the built-in engine, or (when set in the App tab) to a p56-server on the network,
// so several devices can control one fan through it. In a browser: to the p56-server that served
// the page (relative URLs keep working behind Home Assistant's ingress), else to the demo fan.
const SERVER_KEY = 'p56.server';
// the last server address entered, kept when switching back to this app's own engine
const LAST_SERVER_KEY = 'p56.serverLast';
/** The p56-server address last used or tested here, for the App tab's field. */
export const lastServer: string = (() => {
  try { return (localStorage.getItem(LAST_SERVER_KEY) || '').trim(); } catch { return ''; }
})();
/** Remember `url` as the last server address (also when it is not used yet). */
export function rememberServer(url: string) {
  const u = serverUrl(url);
  try { if (u) localStorage.setItem(LAST_SERVER_KEY, u); } catch { /* private mode */ }
}
/** The p56-server this app uses instead of its own engine ('' = none). */
export const remoteServer: string = (() => {
  if (!isTauri) return '';
  try { return (localStorage.getItem(SERVER_KEY) || '').trim(); } catch { return ''; }
})();
const withSlash = (u: string) => u.replace(/\/*$/, '/');
const base = remoteServer ? withSlash(remoteServer) : '';
/** "192.168.1.10" → "http://192.168.1.10:8765": add the scheme and p56-server's port when missing. */
export function serverUrl(input: string): string {
  let u = input.trim().replace(/\/+$/, '');
  if (!u) return '';
  if (!/^https?:\/\//i.test(u)) u = `http://${u}`;
  const host = u.replace(/^https?:\/\//i, '').split('/')[0];
  if (!/:\d+$/.test(host)) u = u.replace(host, `${host}:8765`);
  return u;
}
/** Use a p56-server from now on ('' = the app's own engine again); reloads the app. */
export async function setRemoteServer(url: string) {
  const u = serverUrl(url);
  // let go of the fan, so it can connect to the server instead (the engine keeps running otherwise)
  if (isTauri && u && !remoteServer) await T.core.invoke('p56', { cmd: 'stop_link', args: {} }).catch(() => {});
  try { if (u) localStorage.setItem(SERVER_KEY, u); else localStorage.removeItem(SERVER_KEY); } catch { /* private mode */ }
  rememberServer(u);
  window.location.reload();
}
/** Is a p56-server answering at `url`? (for the App tab's "test" button) */
export async function pingServer(url: string): Promise<boolean> {
  try {
    const r = await fetch(`${withSlash(serverUrl(url))}api/ping`, { cache: 'no-store', headers: authHeaders() });
    return r.ok && !!(await r.json())?.ok;
  } catch { return false; }
}

const serverProbe: Promise<boolean> = remoteServer ? Promise.resolve(true) : isTauri ? Promise.resolve(false)
  : fetch('api/ping', { cache: 'no-store', headers: authHeaders() }).then((r) => r.ok && r.json()).then((j) => !!j?.ok).catch(() => false);
export const transport = serverProbe.then((s) => (s ? 'server' : isTauri ? 'tauri' : 'demo') as 'tauri' | 'server' | 'demo');

export async function invoke<R = unknown>(cmd: string, args?: Record<string, unknown>): Promise<R> {
  if (isTauri && !remoteServer) return T.core.invoke('p56', { cmd, args: args ?? {} });
  if (await serverProbe) {
    const r = await fetch(`${base}api/call/${cmd}`, { method: 'POST', headers: authHeaders({ 'Content-Type': 'application/json' }), body: JSON.stringify(args ?? {}) });
    if (r.status === 401) { requireAuth(); throw t('api.unauthorized'); }
    const j = await r.json();
    if (!j.ok) throw j.error;
    return j.result as R;
  }
  return demo(cmd, args ?? {}) as Promise<R>;
}

const listeners = new Set<(e: P56Event) => void>();
export function onEvent(cb: (e: P56Event) => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}
function emit(e: P56Event) { listeners.forEach((l) => l(e)); }
if (isTauri && !remoteServer) T.event.listen('p56', (e: { payload: P56Event }) => emit(e.payload));
serverProbe.then((s) => {
  if (!s) return;
  const connect = () => {
    const es = new EventSource(`${base}api/events${apiToken ? `?token=${encodeURIComponent(apiToken)}` : ''}`);
    es.onmessage = (m) => { try { emit(JSON.parse(m.data)); } catch { /* ignore */ } };
    es.onerror = () => { es.close(); setTimeout(connect, 3000); };
  };
  connect();
});

export const VIDEO_EXT = ['mp4', 'm4v', 'mov', 'webm', 'mkv', 'avi', '3gp'];
export const IMAGE_EXT = ['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp'];

export type Picked = { name: string; bytes: Uint8Array; video: boolean; mime: string };

function extOf(name: string) { return (name.split('.').pop() || '').toLowerCase(); }
function mimeOf(ext: string) {
  return ({ mp4: 'video/mp4', m4v: 'video/mp4', mov: 'video/quicktime', webm: 'video/webm', mkv: 'video/x-matroska', avi: 'video/x-msvideo', '3gp': 'video/3gpp',
    png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', bmp: 'image/bmp', webp: 'image/webp' } as Record<string, string>)[ext] || 'application/octet-stream';
}
function sniffVideo(b: Uint8Array) {
  const s = String.fromCharCode(...b.slice(4, 12));
  return s.startsWith('ftyp') || (b[0] === 0x1a && b[1] === 0x45 && b[2] === 0xdf && b[3] === 0xa3) || String.fromCharCode(...b.slice(0, 4)) === 'RIFF' && String.fromCharCode(...b.slice(8, 11)) === 'AVI';
}

/** Let the user pick an image or video. Works for Android `content://` picks too. */
export async function pickMedia(): Promise<Picked | null> {
  if (isTauri && T.dialog && T.fs) {
    const p = await T.dialog.open({ multiple: false, filters: [{ name: t('api.pickFilter'), extensions: [...IMAGE_EXT, ...VIDEO_EXT] }] });
    if (!p) return null;
    const path: string = typeof p === 'string' ? p : (p.path ?? String(p));
    const bytes: Uint8Array = await T.fs.readFile(path);
    let name = decodeURIComponent(path.split(/[\\/]/).pop() || 'media');
    if (name.includes(':')) name = name.split(':').pop() || name;
    // Android media URIs end in a number, not a file name.
    if (/^\d+$/.test(name)) name = `${t('api.photoPrefix')}-${name}`;
    let ext = extOf(name);
    const video = VIDEO_EXT.includes(ext) || (!IMAGE_EXT.includes(ext) && sniffVideo(bytes));
    if (!IMAGE_EXT.includes(ext) && !VIDEO_EXT.includes(ext)) { ext = video ? 'mp4' : 'jpg'; name = `${name}.${ext}`; }
    return { name, bytes, video, mime: mimeOf(ext) };
  }
  return new Promise((resolve) => {
    const inp = document.createElement('input');
    inp.type = 'file';
    inp.accept = 'image/*,video/*';
    inp.onchange = async () => {
      const f = inp.files?.[0];
      if (!f) return resolve(null);
      const bytes = new Uint8Array(await f.arrayBuffer());
      const ext = extOf(f.name);
      resolve({ name: f.name, bytes, video: f.type.startsWith('video') || VIDEO_EXT.includes(ext), mime: f.type || mimeOf(ext) });
    };
    inp.click();
  });
}

export function toBase64(data: Uint8Array | Blob): Promise<string> {
  const blob = data instanceof Blob ? data : new Blob([data as BlobPart]);
  return new Promise((res, rej) => {
    const r = new FileReader();
    r.onload = () => res(String(r.result).split(',')[1] ?? '');
    r.onerror = () => rej(r.error);
    r.readAsDataURL(blob);
  });
}

/** Sample rate of the sound the fan plays with a file (16-bit mono, 1600 bytes per 20 fps frame). */
const AUDIO_RATE = 16000;

/**
 * The sound of a picked video, from `start` for `seconds`, as 16-bit little-endian mono PCM at
 * 16 kHz (base64), for the fan's speaker. `null` when the video has no (audible) sound or the
 * WebView cannot decode it.
 */
export async function decodeAudio(picked: Picked, start: number, seconds: number): Promise<string | null> {
  try {
    // decodeAudioData resamples to the context's rate
    const ctx = new OfflineAudioContext(1, 1, AUDIO_RATE);
    const buf = await ctx.decodeAudioData(picked.bytes.slice().buffer as ArrayBuffer);
    const from = Math.max(0, Math.floor(start * AUDIO_RATE));
    const to = Math.min(buf.length, from + Math.ceil(seconds * AUDIO_RATE));
    if (to <= from) return null;
    const channels = Array.from({ length: buf.numberOfChannels }, (_, i) => buf.getChannelData(i));
    const pcm = new Int16Array(to - from);
    let loud = false;
    for (let i = from; i < to; i++) {
      let v = 0;
      for (const c of channels) v += c[i];
      const s = Math.round(Math.max(-1, Math.min(1, v / channels.length)) * 0x7fff);
      pcm[i - from] = s;
      if (s !== 0) loud = true;
    }
    return loud ? toBase64(new Uint8Array(pcm.buffer)) : null;
  } catch {
    return null; // no sound track, or a codec the WebView cannot decode
  }
}

/** Decode a video in the WebView: seek frame by frame and hand out JPEG data URLs. */
export async function decodeVideo(
  picked: Picked, opts: { fps: number; start: number; end: number; max: number; side?: number },
  onFrame: (jpegB64: string, i: number, total: number) => Promise<void>,
  cancelled: () => boolean,
): Promise<number> {
  const url = URL.createObjectURL(new Blob([picked.bytes as BlobPart], { type: picked.mime }));
  const v = document.createElement('video');
  v.muted = true; v.playsInline = true; v.preload = 'auto'; v.src = url;
  try {
    // The webview's media pipeline can be slow to wake up (or silently stall) the first time:
    // never wait forever for a media event.
    await new Promise<void>((res, rej) => {
      const timer = setTimeout(() => rej(new Error(t('api.videoUnreadable'))), 10000);
      v.onloadedmetadata = () => { clearTimeout(timer); res(); };
      v.onerror = () => { clearTimeout(timer); rej(new Error(t('api.videoUnreadable'))); };
      v.load();
    });
    let dur = v.duration;
    if (!isFinite(dur)) {
      // Recorder-made WebM has no duration in its header; seeking far ahead makes the browser compute it.
      await new Promise<void>((res) => { v.ondurationchange = () => isFinite(v.duration) && res(); v.currentTime = 1e9; setTimeout(res, 3000); });
      dur = isFinite(v.duration) ? v.duration : v.currentTime;
      v.ondurationchange = null;
    }
    if (!isFinite(dur) || dur <= 0) throw new Error(t('api.videoNoDuration'));
    const side = opts.side ?? 720;
    const s = Math.min(1, side / Math.min(v.videoWidth, v.videoHeight));
    const c = document.createElement('canvas');
    c.width = Math.round(v.videoWidth * s); c.height = Math.round(v.videoHeight * s);
    const ctx = c.getContext('2d')!;
    const start = Math.max(0, Math.min(opts.start, dur));
    const end = opts.end > start ? Math.min(opts.end, dur) : dur;
    let total = Math.max(1, Math.floor((end - start) * opts.fps));
    if (opts.max > 0) total = Math.min(total, opts.max);
    for (let i = 0; i < total; i++) {
      if (cancelled()) break;
      const t = Math.min(end - 0.001, start + i / opts.fps);
      await new Promise<void>((res) => { const timer = setTimeout(res, 5000); v.onseeked = () => { clearTimeout(timer); res(); }; v.currentTime = t; });
      ctx.drawImage(v, 0, 0, c.width, c.height);
      const b64 = c.toDataURL('image/jpeg', 0.88).split(',')[1];
      await onFrame(b64, i, total);
    }
    return total;
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** Grab the current frame of a <video> (camera or live video) as JPEG base64. */
export function grab(v: HTMLVideoElement, c: HTMLCanvasElement, side = 720): string | null {
  if (!v.videoWidth) return null;
  const s = Math.min(1, side / Math.min(v.videoWidth, v.videoHeight));
  c.width = Math.round(v.videoWidth * s); c.height = Math.round(v.videoHeight * s);
  c.getContext('2d')!.drawImage(v, 0, 0, c.width, c.height);
  return c.toDataURL('image/jpeg', 0.85).split(',')[1];
}

// ---------------------------------------------------------------------------
// Demo fan for running the UI in a plain browser
// ---------------------------------------------------------------------------
const demoFan: Device = {
  id: 1, ip: '192.168.4.1', name: 'DEMO-P56', mac: '02:e0:e0:56:00:01', hotspot: 'DEMO-P56-AP', router: '',
  status: { power: true, playing: true, repeat_all: true, breath: false, autostart: true, play_index: 1, brightness: 16, color_mode: 2, angle: 0, fan_type: 8 },
  files: [[1, t('demo.file1')], [2, 'logo.mp4']], last_error: null, volume: null, model: 'P56', support: 'jpeg',
};
let demoMedia = 0;
const demoFrames = new Map<number, number>();
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const demoTexts = new Map<number, { text: string; color: string; bg: string }>();
/** Demo preview: the test card or a text card, plus that picture unwrapped into angle × radius like the fan's own layout. */
function demoPreview(media = 0): Preview {
  const N = 360, c = document.createElement('canvas'); c.width = c.height = N;
  const g = c.getContext('2d')!;
  const txt = demoTexts.get(media);
  g.fillStyle = txt?.bg ?? '#000'; g.fillRect(0, 0, N, N);
  if (txt) {
    const words = txt.text.split(/\s+/).filter(Boolean);
    let size = 84, lines: string[] = [];
    for (; size > 18; size -= 4) {
      g.font = `bold ${size}px sans-serif`;
      lines = [];
      for (const w of words) {
        const l = lines.length ? `${lines[lines.length - 1]} ${w}` : w;
        if (lines.length && g.measureText(l).width <= N * 0.72) lines[lines.length - 1] = l; else lines.push(w);
      }
      if (lines.every((l) => g.measureText(l).width <= N * 0.72) && lines.length * size * 1.1 <= N * 0.6) break;
    }
    g.fillStyle = txt.color; g.textAlign = 'center'; g.textBaseline = 'middle';
    lines.forEach((l, i) => g.fillText(l, N / 2, N / 2 + (i - (lines.length - 1) / 2) * size * 1.1));
  } else {
    [[165, '#ff3b3b'], [120, '#3bff7a'], [75, '#3b7aff']].forEach(([r, col]) => { g.strokeStyle = col as string; g.lineWidth = 12; g.beginPath(); g.arc(N / 2, N / 2, r as number, 0, Math.PI * 2); g.stroke(); });
  }
  const W = 688, H = 240, px = g.getImageData(0, 0, N, N).data;
  const s = document.createElement('canvas'); s.width = W; s.height = H;
  const sg = s.getContext('2d')!, out = sg.createImageData(W, H);
  for (let x = 0; x < W; x++) {
    const th = (x / W) * Math.PI * 2 - Math.PI / 2;
    for (let y = 0; y < H; y++) {
      const r = (y / H) * (N / 2);
      const sx = Math.min(N - 1, Math.round(N / 2 + r * Math.cos(th))), sy = Math.min(N - 1, Math.round(N / 2 + r * Math.sin(th)));
      out.data.set(px.subarray((sy * N + sx) * 4, (sy * N + sx) * 4 + 4), (y * W + x) * 4);
    }
  }
  sg.putImageData(out, 0, 0);
  return { round: c.toDataURL('image/png'), layout: s.toDataURL('image/png'), jpeg_bytes: 18873, quality: 80, width: W, height: H, frames: 1 };
}
async function demo(cmd: string, a: any): Promise<unknown> {
  await sleep(60);
  const s = demoFan.status!;
  const st = () => emit({ type: 'packet', data: { id: 1, kind: 'status', msg: '' } });
  switch (cmd) {
    case 'start_link': setTimeout(() => emit({ type: 'connected', data: { id: 1, ip: demoFan.ip } }), 400); return '192.168.4.2';
    case 'stop_link': return null;
    case 'link_info': return { ip: '192.168.4.2', port: 6666, interfaces: ['192.168.4.2 (broadcast 192.168.4.255)'], discovered: ['192.168.4.1'] };
    case 'devices': return [structuredClone(demoFan)];
    case 'platform': return { os: 'browser', screen: false, version: 'demo' };
    case 'fan_types': return [{ id: 8, model: 'P56', angles: 960, leds: 172, jpeg: '688×240', support: 'jpeg' }, { id: 1, model: 'HS65', angles: 960, leds: 192, jpeg: '768×240', support: 'jpeg' }, { id: 32, model: null, angles: 960, leds: 122, jpeg: null, support: 'unsupported' }];
    case 'set_fan_type': return null;
    case 'control': {
      const m: Record<string, () => void> = {
        on: () => (s.power = true), off: () => (s.power = false), play: () => { s.playing = true; s.power = true; }, pause: () => (s.playing = false),
        repeat_all: () => (s.repeat_all = true), repeat_one: () => (s.repeat_all = false), breath_on: () => (s.breath = true), breath_off: () => (s.breath = false),
        autostart_on: () => (s.autostart = true), autostart_off: () => (s.autostart = false),
        format: () => (demoFan.files = []), reset: () => { s.brightness = 16; s.angle = 0; },
        refresh: () => {},
      };
      m[a.action]?.(); st(); return null;
    }
    case 'set_value':
      if (a.what === 'brightness') s.brightness = a.value;
      if (a.what === 'angle') s.angle = a.value;
      if (a.what === 'select') s.play_index = a.value;
      if (a.what === 'delete') demoFan.files = demoFan.files.filter(([n]) => n !== a.value).map(([, f], i) => [i + 1, f]);
      if (a.what === 'volume') demoFan.volume = a.value;
      st(); return null;
    case 'set_text':
      if (a.what === 'name') demoFan.name = a.a;
      if (a.what === 'hotspot') demoFan.hotspot = a.a;
      if (a.what === 'router') demoFan.router = a.a;
      if (a.what === 'rename') demoFan.files = demoFan.files.map(([n, f]) => [n, n === a.slot ? (a.a.endsWith('.mp4') ? a.a : a.a + '.mp4') : f]);
      return null;
    case 'media_load': demoMedia++; demoFrames.set(demoMedia, 1); return { id: demoMedia, name: a.name, frames: 1, width: 1024, height: 768 };
    case 'media_text': demoMedia++; demoFrames.set(demoMedia, a.motion ? 40 : 1); demoTexts.set(demoMedia, { text: a.text, color: a.color, bg: a.background }); return { id: demoMedia, name: a.name || t('demo.textName'), frames: a.motion ? 40 : 1, width: 720, height: 720 };
    case 'media_begin': demoMedia++; demoFrames.set(demoMedia, 0); return demoMedia;
    case 'media_audio': return Math.round((atob(a.data).length / 2 / 16000) * 1000);
    case 'media_push': { const n = (demoFrames.get(a.media) ?? 0) + 1; demoFrames.set(a.media, n); return n; }
    case 'media_drop': return null;
    case 'preview': return { ...demoPreview(a.media), frames: demoFrames.get(a.media) ?? 1 };
    case 'upload': {
      for (const [stage, d] of [['transcoding', 0], ['sending', 1]] as const) { emit({ type: 'progress', data: { stage, done: d, total: 2 } }); await sleep(300); }
      const name = (a.args.name || 'media') + '.mp4';
      demoFan.files.push([demoFan.files.length + 1, name]);
      emit({ type: 'progress', data: { stage: 'done', done: 2, total: 2 } });
      return t('api.uploadResult', { name, count: 1 });
    }
    case 'cast_start': setTimeout(() => emit({ type: 'cast', data: { fps: 15, fans: 1, jpeg: 18000, q: 80, preview: demoPreview().round } }), 300); return null;
    case 'cast_frame': return 1;
    case 'cast_stop': setTimeout(() => emit({ type: 'cast', data: { stopped: true } }), 50); return null;
  }
  throw new Error(t('demo.unknownCommand', { cmd }));
}
