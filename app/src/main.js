/* P56 Studio — frontend. Plain JS, talks to the Rust backend via Tauri commands. */
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const dialog = window.__TAURI__.dialog;

const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];
const log = (m) => { const el = $('#log'); el.textContent += `[${new Date().toLocaleTimeString()}] ${m}\n`; el.scrollTop = el.scrollHeight; };

let devices = [];
let fanTypes = [];
let autoType = true;
let previewTimer = null;

// ---------- tabs ----------
$$('nav button').forEach(b => b.onclick = () => {
  $$('nav button').forEach(x => x.classList.toggle('active', x === b));
  $$('.tab').forEach(t => t.classList.toggle('active', t.id === 'tab-' + b.dataset.tab));
});

// ---------- placement ----------
const placement = () => ({ x: +$('#pX').value, y: +$('#pY').value, scale: +$('#pS').value, mirror: $('#pM').checked });
const syncPlaceLabels = () => { $('#pXv').textContent = (+$('#pX').value).toFixed(2); $('#pYv').textContent = (+$('#pY').value).toFixed(2); $('#pSv').textContent = (+$('#pS').value).toFixed(2); };
['#pX', '#pY', '#pS', '#pM'].forEach(s => $(s).addEventListener('input', () => { syncPlaceLabels(); schedulePreview(); }));
$('#pReset').onclick = () => { $('#pX').value = 0; $('#pY').value = 0; $('#pS').value = 1; $('#pM').checked = false; syncPlaceLabels(); schedulePreview(); };

// ---------- devices ----------
function fanOptions() {
  const opts = devices.map(d => `<option value="${d.id}">${d.name} (${d.ip})</option>`).join('');
  ['#plFan', '#upFan', '#wifiFan', '#rnFan'].forEach(s => { const el = $(s); const v = el.value; el.innerHTML = opts; if ([...el.options].some(o => o.value === v)) el.value = v; });
}

function renderDevices() {
  const list = $('#deviceList');
  if (!devices.length) { list.innerHTML = `<div class="empty">Nog geen fan verbonden. Wachten op broadcast…</div>`; return; }
  list.innerHTML = devices.map(d => `
    <div class="card">
      <div class="name"><span>${d.name}</span><span class="badge ${d.power ? 'on' : 'off'}">${d.power ? (d.playing ? 'speelt' : 'pauze') : 'uit'}</span></div>
      <div class="kv">${d.ip} · type ${d.fan_type || '?'} · ${d.angles || '?'}×${d.leds || '?'} · slot ${d.play_index || '–'}</div>
      <div class="kv">${d.mac || ''} ${d.version ? '· fw ' + d.version : ''} ${d.wifi ? '· ' + d.wifi : ''}</div>
      <div class="kv">${d.files.length} bestand(en)</div>
      <div class="acts">
        <button class="small" data-id="${d.id}" data-act="on">Aan</button>
        <button class="small" data-id="${d.id}" data-act="off">Uit</button>
        <button class="small" data-id="${d.id}" data-act="play">Play</button>
        <button class="small" data-id="${d.id}" data-act="pause">Pauze</button>
        <button class="small ghost" data-id="${d.id}" data-act="refresh">↻</button>
      </div>
    </div>`).join('');
  list.querySelectorAll('button').forEach(b => b.onclick = () => invoke('control', { id: +b.dataset.id, action: b.dataset.act }).catch(e => log('fout: ' + e)));
}

function renderPlaylist() {
  const d = devices.find(x => x.id === +$('#plFan').value) || devices[0];
  const tb = $('#plTable tbody');
  if (!d) { tb.innerHTML = `<tr><td colspan="3" class="hint">Geen fan.</td></tr>`; return; }
  if (!d.files.length) { tb.innerHTML = `<tr><td colspan="3" class="hint">Lege afspeellijst.</td></tr>`; return; }
  tb.innerHTML = d.files.map(([n, name]) => `
    <tr class="${n === d.play_index ? 'cur' : ''}"><td>${n}</td><td>${name}</td>
    <td class="acts"><button class="small" data-slot="${n}" data-act="show">Tonen</button> <button class="small danger" data-slot="${n}" data-act="del">Wissen</button></td></tr>`).join('');
  tb.querySelectorAll('button').forEach(b => b.onclick = async () => {
    const slot = +b.dataset.slot;
    try {
      if (b.dataset.act === 'show') await invoke('select_slot', { id: d.id, slot });
      else if (confirm(`Slot ${slot} wissen van de fan?`)) await invoke('delete_slot', { id: d.id, slot });
    } catch (e) { log('fout: ' + e); }
  });
}

async function refreshDevices() {
  try {
    devices = await invoke('devices');
    const seen = await invoke('discovered');
    $('#discoveredHint').textContent = seen.length ? `gezien via UDP: ${seen.join(', ')}` : '';
    fanOptions(); renderDevices(); renderPlaylist();
    const t = devices.find(d => d.fan_type)?.fan_type;
    if (autoType && t && +$('#setType').value !== t) { $('#setType').value = t; await invoke('set_fan_type', { fanType: t }); log(`fantype ${t} overgenomen van de fan`); schedulePreview(); }
    $('#linkStatus').textContent = devices.length ? `${devices.length} fan(s) verbonden` : 'wacht op fan…';
    $('#linkStatus').classList.toggle('ok', devices.length > 0);
  } catch (e) { /* link not started yet */ }
}

$('#plFan').onchange = renderPlaylist;
$('#plRefresh').onclick = () => { const id = +$('#plFan').value; if (id) invoke('control', { id, action: 'refresh' }); };
$$('#tab-playlist [data-ctl]').forEach(b => b.onclick = () => { const id = +$('#plFan').value; if (id) invoke('control', { id, action: b.dataset.ctl }).catch(e => log('fout: ' + e)); });

// ---------- upload ----------
async function pickFile(target) {
  const p = await dialog.open({ multiple: false, filters: [{ name: 'Beeld of video', extensions: ['png', 'jpg', 'jpeg', 'bmp', 'gif', 'webp', 'mp4', 'mov', 'avi', 'mkv', 'webm'] }] });
  if (p) { $(target).value = p; schedulePreview(); }
}
$('#upPick').onclick = () => pickFile('#upPath');
$('#upTest').onclick = () => { $('#upPath').value = 'test'; schedulePreview(); };

function schedulePreview() { clearTimeout(previewTimer); previewTimer = setTimeout(doPreview, 150); }
async function doPreview() {
  const path = $('#upPath').value;
  if (!path) return;
  try {
    const p = await invoke('preview', { path, placement: placement() });
    $('#pvRound').src = p.round; $('#pvLayout').src = p.layout;
    $('#pvMeta').textContent = `${p.width}×${p.height} · JPEG ${(p.jpeg_bytes / 1024).toFixed(1)} kB · kwaliteit ${p.quality}`;
  } catch (e) { $('#pvMeta').textContent = 'voorbeeld: ' + e; $('#pvRound').removeAttribute('src'); }
}

$('#upGo').onclick = async () => {
  const id = +$('#upFan').value; const path = $('#upPath').value;
  if (!id) return alert('Geen fan verbonden.');
  if (!path) return alert('Kies eerst een bestand.');
  $('#upGo').disabled = true; $('#upMsg').textContent = 'bezig…';
  try {
    const r = await invoke('upload', { args: { id, path, name: $('#upName').value || null, slot: +$('#upSlot').value, placement: placement(), max_frames: +$('#upMax').value } });
    $('#upMsg').textContent = 'klaar: ' + r; log('upload ' + r);
  } catch (e) { $('#upMsg').textContent = 'fout: ' + e; log('upload fout: ' + e); }
  $('#upGo').disabled = false;
};

// ---------- cast ----------
$$('input[name=castSrc]').forEach(r => r.onchange = () => { $('#castFileRow').hidden = r.value !== 'file' || !r.checked; });
$('#castPick').onclick = () => pickFile('#castPath');
$('#castFps').oninput = () => $('#castFpsV').textContent = $('#castFps').value;
$('#castStart').onclick = async () => {
  const src = $$('input[name=castSrc]').find(r => r.checked).value;
  const source = src === 'file' ? $('#castPath').value : src;
  if (src === 'file' && !source) return alert('Kies een bestand.');
  try {
    await invoke('cast_start', { args: { source, fps: +$('#castFps').value, placement: placement() } });
    $('#castStart').disabled = true; $('#castStop').disabled = false; $('#castMsg').textContent = 'casten…';
  } catch (e) { $('#castMsg').textContent = 'fout: ' + e; }
};
$('#castStop').onclick = () => invoke('cast_stop');

// ---------- settings ----------
$('#setType').onchange = async () => { autoType = false; await invoke('set_fan_type', { fanType: +$('#setType').value }); schedulePreview(); };
$('#wifiGo').onclick = async () => {
  const id = +$('#wifiFan').value; const ssid = $('#wifiSsid').value; const password = $('#wifiPw').value;
  if (!id || !ssid) return;
  if (!confirm(`Wifi van de fan zetten op "${ssid}"? Daarna is hij alleen via dat netwerk bereikbaar.`)) return;
  try { await invoke('set_wifi', { id, ssid, password }); log('wifi ingesteld'); } catch (e) { log('wifi fout: ' + e); }
};
$('#rnGo').onclick = async () => {
  const id = +$('#rnFan').value;
  try { await invoke('rename_file', { id, old: $('#rnOld').value, new: $('#rnNew').value }); log('hernoemd'); } catch (e) { log('hernoemen fout: ' + e); }
};
$('#setIpApply').onclick = () => { localStorage.setItem('p56.ip', $('#setIp').value.trim()); alert('Opgeslagen. Herstart de app om het nieuwe IP te gebruiken.'); };

// ---------- events from backend ----------
listen('p56', ({ payload }) => {
  const { type, data } = payload;
  if (type === 'log') log(data.msg);
  else if (type === 'connected') { log(`fan #${data.id} verbonden (${data.ip})`); refreshDevices(); }
  else if (type === 'disconnected') { log(`fan #${data.id} weg`); refreshDevices(); }
  else if (type === 'discovered') { log(`fan gezien: ${data.ip}`); refreshDevices(); }
  else if (type === 'packet') { if (/Status|FileList|MachineInfo/.test(data.packet)) refreshDevices(); }
  else if (type === 'progress') $('#upMsg').textContent = `${data.stage}: ${data.frames} frame(s)…`;
  else if (type === 'cast') {
    if (data.stopped) { $('#castStart').disabled = false; $('#castStop').disabled = true; $('#castMsg').textContent = 'gestopt'; return; }
    if (data.error) { $('#castMsg').textContent = data.error; $('#castStart').disabled = false; $('#castStop').disabled = true; return; }
    $('#castMeta').textContent = `${data.fps} fps · ${data.fans} fan(s) · JPEG ${(data.jpeg / 1024).toFixed(1)} kB q${data.q}`;
    if (data.preview) $('#castPv').src = data.preview;
  }
});

// ---------- boot ----------
(async () => {
  fanTypes = await invoke('fan_types');
  $('#setType').innerHTML = fanTypes.map(t => `<option value="${t.id}">type ${t.id} — ${t.angles}×${t.radius}, ${t.arms || 'geen'} armen (${t.jpeg})</option>`).join('');
  $('#setType').value = 8;
  syncPlaceLabels();
  const ff = await invoke('has_ffmpeg');
  $('#ffmpegHint').textContent = ff ? 'ffmpeg gevonden: video-upload en video-cast beschikbaar.' : 'ffmpeg niet gevonden op PATH: alleen foto\'s. Installeer ffmpeg voor video.';
  const ip = localStorage.getItem('p56.ip') || '';
  $('#setIp').value = ip;
  try {
    const used = await invoke('start_link', { ip: ip || null });
    $('#linkStatus').textContent = `wacht op fan… (uitzenden als ${used})`;
    log(`link gestart, uitzenden als ${used}`);
  } catch (e) { $('#linkStatus').textContent = 'fout: ' + e; log('kan niet starten: ' + e + ' (is Holoscope of een ander programma nog open op poort 6666?)'); }
  refreshDevices();
  setInterval(refreshDevices, 3000);
})();
