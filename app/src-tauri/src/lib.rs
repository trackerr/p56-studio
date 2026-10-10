//! P56 Studio — Tauri backend. Thin command layer over `p56-core`.
//!
//! Media never comes in as a file path: the frontend reads the file (which also works
//! for Android `content://` picks) or decodes video frames itself and sends the bytes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use p56_core::protocol::{FanError, Packet};
use p56_core::{transcode, ColorMode, Config, Event, FanType, Link, Placement};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

const MAX_SIDE: u32 = 720;

/// Media loaded from the frontend: source frames (encoded, to keep memory low) + delays in ms.
struct Media {
    name: String,
    frames: Vec<(Vec<u8>, u32)>,
}

struct AppState {
    link: Mutex<Option<Link>>,
    /// Fan type chosen by the user; 0 = follow what the fan reports.
    fan_type: Mutex<u8>,
    cast_running: Arc<AtomicBool>,
    upload_seq: AtomicU32,
    media: Mutex<HashMap<u32, Media>>,
    next_media: AtomicU32,
    live: Mutex<LiveStats>,
}

#[derive(Default)]
struct LiveStats { n: u64, since: Option<Instant> }

#[derive(Serialize, Clone)]
struct StatusDto {
    power: bool, playing: bool, repeat_all: bool, breath: bool, autostart: bool, master: bool,
    play_index: u8, brightness: u8, color_mode: u8, angle: i32, fan_type: u8,
}

#[derive(Serialize, Clone)]
struct DeviceDto {
    id: u32,
    ip: String,
    name: String,
    mac: String,
    hotspot: String,
    router: String,
    status: Option<StatusDto>,
    files: Vec<(u8, String)>,
    last_error: Option<String>,
    ble: Option<(u8, Vec<String>)>,
    support: String,
}

#[derive(Deserialize, Clone, Copy, Default)]
struct PlacementDto {
    #[serde(default)] x: f64,
    #[serde(default)] y: f64,
    #[serde(default = "one")] scale: f64,
    #[serde(default)] mirror: bool,
}
fn one() -> f64 { 1.0 }
impl From<PlacementDto> for Placement {
    fn from(p: PlacementDto) -> Self { Placement { x_offset: p.x, y_offset: p.y, scale: p.scale, mirror: p.mirror } }
}

#[derive(Serialize, Clone)]
struct PreviewDto {
    /// data: URL of the round preview (what the fan shows)
    round: String,
    /// data: URL of the raw fan-layout JPEG
    layout: String,
    jpeg_bytes: usize,
    quality: u8,
    width: u32,
    height: u32,
    frames: usize,
}

#[derive(Serialize)]
struct MediaDto { id: u32, name: String, frames: usize, width: u32, height: u32 }

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn png_bytes(img: &image::RgbImage) -> Vec<u8> {
    let mut v = Vec::new();
    image::DynamicImage::ImageRgb8(img.clone()).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).ok();
    v
}

fn jpeg_bytes(img: &image::RgbImage) -> Vec<u8> { transcode::jpeg(img, 92) }

fn b64(s: &str) -> Result<Vec<u8>, String> {
    let s = s.split_once("base64,").map(|(_, d)| d).unwrap_or(s);
    base64::engine::general_purpose::STANDARD.decode(s.trim()).map_err(|e| format!("ongeldige data: {e}"))
}

fn link(state: &State<AppState>) -> Result<Link, String> {
    state.link.lock().unwrap().clone().ok_or_else(|| "verbinding niet gestart".into())
}

fn err<E: std::fmt::Display>(e: E) -> String { e.to_string() }

/// Fan type for transcoding: user's choice, else what fan `id` (or the first fan) reports, else P56.
fn fan_type_for(state: &State<AppState>, id: Option<u32>) -> Result<FanType, String> {
    let chosen = *state.fan_type.lock().unwrap();
    let t = if chosen != 0 { chosen } else {
        state.link.lock().unwrap().as_ref().and_then(|l| {
            let devs = l.devices();
            id.and_then(|i| devs.iter().find(|d| d.id == i).cloned()).or_else(|| devs.into_iter().next())
        }).and_then(|d| d.status).map(|s| s.fan_type).filter(|t| *t != 0).unwrap_or(8)
    };
    if let Some(m) = FanType::check(t) { return Err(m); }
    Ok(FanType::by_id(t))
}

fn decode_source(bytes: &[u8]) -> Result<image::RgbImage, String> {
    Ok(transcode::fit(transcode::decode_rgb(bytes).map_err(err)?, MAX_SIDE))
}

// ---------------------------------------------------------------------------
// link & devices
// ---------------------------------------------------------------------------

#[tauri::command]
fn start_link(app: AppHandle, state: State<AppState>, ip: Option<String>, targets: Option<Vec<String>>, port: Option<u16>) -> Result<String, String> {
    let mut g = state.link.lock().unwrap();
    if let Some(l) = g.as_ref() { return Ok(l.local_ip.to_string()); }
    let cfg = Config {
        local_ip: ip.and_then(|s| s.trim().parse().ok()),
        targets: targets.unwrap_or_default().iter().filter_map(|t| {
            let t = t.trim();
            t.parse().ok().or_else(|| format!("{t}:8988").parse().ok())
        }).collect(),
        tcp_port: port.unwrap_or(p56_core::protocol::TCP_PORT),
        ..Default::default()
    };
    let (l, rx) = Link::start(cfg).map_err(err)?;
    let ip = l.local_ip.to_string();
    *g = Some(l);
    thread::spawn(move || {
        for ev in rx {
            let (name, payload) = match ev {
                Event::Discovered { ip } => ("discovered", serde_json::json!({ "ip": ip.to_string() })),
                Event::Connected { id, ip } => ("connected", serde_json::json!({ "id": id, "ip": ip.to_string() })),
                Event::Disconnected { id } => ("disconnected", serde_json::json!({ "id": id })),
                Event::Packet { id, packet } => {
                    let kind = match &packet {
                        Packet::Status(_) => "status", Packet::MachineInfo(_) => "info", Packet::FileList(_) => "files",
                        Packet::Error(_) => "error", Packet::List { .. } => "list", Packet::TransDone => "done", _ => continue,
                    };
                    let msg = if let Packet::Error(e) = &packet { e.to_string() } else { String::new() };
                    ("packet", serde_json::json!({ "id": id, "kind": kind, "msg": msg }))
                }
                Event::Log(s) => ("log", serde_json::json!({ "msg": s })),
            };
            let _ = app.emit("p56", serde_json::json!({ "type": name, "data": payload }));
        }
    });
    Ok(ip)
}

#[tauri::command]
fn stop_link(state: State<AppState>) {
    state.cast_running.store(false, Ordering::SeqCst);
    if let Some(l) = state.link.lock().unwrap().take() {
        l.shutdown();
        thread::sleep(Duration::from_millis(400));
    }
}

#[tauri::command]
fn link_info(state: State<AppState>) -> Result<serde_json::Value, String> {
    let l = link(&state)?;
    let ifs: Vec<String> = p56_core::net::interfaces().into_iter().map(|(ip, bc)| format!("{ip} (broadcast {bc})")).collect();
    Ok(serde_json::json!({ "ip": l.local_ip.to_string(), "port": l.tcp_port, "interfaces": ifs,
        "discovered": l.discovered().iter().map(|i| i.to_string()).collect::<Vec<_>>() }))
}

#[tauri::command]
fn devices(state: State<AppState>) -> Result<Vec<DeviceDto>, String> {
    let l = link(&state)?;
    Ok(l.devices().into_iter().map(|d| {
        let i = d.info.clone().unwrap_or_default();
        let status = d.status.as_ref().map(|s| StatusDto {
            power: s.power, playing: s.playing, repeat_all: s.repeat_all, breath: s.breath, autostart: s.autostart,
            master: s.master, play_index: s.play_index, brightness: s.brightness, color_mode: s.color_mode,
            angle: s.angle as i32 - 180, fan_type: s.fan_type,
        });
        let support = d.status.as_ref().map(|s| format!("{:?}", FanType::support(s.fan_type)).to_lowercase()).unwrap_or_default();
        DeviceDto {
            id: d.id,
            ip: d.ip.map(|i| i.to_string()).unwrap_or_default(),
            name: if i.name.is_empty() { format!("fan #{}", d.id) } else { i.name },
            mac: i.mac, hotspot: i.hotspot, router: i.router,
            status,
            files: d.files,
            last_error: d.last_error.map(|e: FanError| e.to_string()),
            ble: d.lists.iter().find(|(k, _)| **k & 0x80 != 0).map(|(_, v)| v.clone()),
            support,
        }
    }).collect())
}

#[tauri::command]
fn set_fan_type(state: State<AppState>, fan_type: u8) { *state.fan_type.lock().unwrap() = fan_type; }

#[tauri::command]
fn fan_types() -> Vec<serde_json::Value> {
    (1..=40u8).filter_map(|id| {
        let known = FanType::known(id);
        let (angles, leds, _) = FanType::apk_geometry(id).or(known.map(|f| (f.angles, f.radius, 20)))?;
        let jpeg = known.map(|f| { let (w, h) = f.jpeg_size(); format!("{w}×{h}") });
        Some(serde_json::json!({ "id": id, "angles": angles, "leds": leds, "jpeg": jpeg,
            "support": format!("{:?}", FanType::support(id)).to_lowercase() }))
    }).collect()
}

/// Simple commands without a value.
#[tauri::command]
async fn control(state: State<'_, AppState>, id: u32, action: String) -> Result<(), String> {
    let l = link(&state)?;
    tauri::async_runtime::spawn_blocking(move || match action.as_str() {
        "on" => l.on(id), "off" => l.off(id), "play" => l.play(id), "pause" => l.pause(id),
        "refresh" => l.refresh(id),
        "repeat_all" => l.repeat(id, true), "repeat_one" => l.repeat(id, false),
        "breath_on" => l.breath(id, true), "breath_off" => l.breath(id, false),
        "autostart_on" => l.autostart(id, true), "autostart_off" => l.autostart(id, false),
        "master_on" => l.master(id, true), "master_off" => l.master(id, false),
        "color_true" => l.color_mode(id, ColorMode::True), "color_bright" => l.color_mode(id, ColorMode::Bright),
        "ble_on" => l.ble(id, true), "ble_off" => l.ble(id, false), "ble_scan" => l.ble_scan(id), "ble_disconnect" => l.ble_disconnect(id),
        "format" => l.format_card(id), "reset" => l.factory_reset(id),
        other => Err(p56_core::Error::Invalid(format!("onbekende actie {other}"))),
    }).await.map_err(err)?.map_err(err)
}

/// Commands with a number.
#[tauri::command]
async fn set_value(state: State<'_, AppState>, id: u32, what: String, value: i32) -> Result<(), String> {
    let l = link(&state)?;
    tauri::async_runtime::spawn_blocking(move || match what.as_str() {
        "brightness" => l.brightness(id, value.clamp(1, 16) as u8),
        "angle" => l.angle(id, (value.clamp(-180, 180) + 180) as u16),
        "volume" => l.volume(id, value.clamp(0, 10) as u8),
        "select" => l.select(id, value.clamp(1, 255) as u8),
        "delete" => l.delete(id, value.clamp(1, 255) as u8),
        "ble_connect" => l.ble_connect(id, value.clamp(0, 255) as u8),
        other => Err(p56_core::Error::Invalid(format!("onbekende instelling {other}"))),
    }).await.map_err(err)?.map_err(err)
}

/// Commands with text: rename (slot + name), name, hotspot, router.
#[tauri::command]
async fn set_text(state: State<'_, AppState>, id: u32, what: String, a: String, b: Option<String>, slot: Option<u8>) -> Result<(), String> {
    let l = link(&state)?;
    let b = b.unwrap_or_default();
    match what.as_str() {
        "hotspot" if b.len() < 8 => return Err("wachtwoord moet minstens 8 tekens zijn".into()),
        "hotspot" | "router" | "name" if a.trim().is_empty() => return Err("naam mag niet leeg zijn".into()),
        _ => {}
    }
    tauri::async_runtime::spawn_blocking(move || match what.as_str() {
        "rename" => l.rename(id, slot.unwrap_or(1), &a),
        "name" => l.device_name(id, &a),
        "hotspot" => l.set_hotspot(id, &a, &b),
        "router" => l.set_router(id, &a, &b),
        other => Err(p56_core::Error::Invalid(format!("onbekend veld {other}"))),
    }).await.map_err(err)?.map_err(err)
}

// ---------------------------------------------------------------------------
// media
// ---------------------------------------------------------------------------

/// Load an image or GIF file (bytes as base64). Returns a media id.
#[tauri::command]
async fn media_load(state: State<'_, AppState>, name: String, data: String) -> Result<MediaDto, String> {
    let bytes = b64(&data)?;
    let (frames, w, h) = tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
        let fr = transcode::decode_frames(&bytes, 600).map_err(|e| format!("kan beeld niet lezen: {e}"))?;
        let (w, h) = fr[0].0.dimensions();
        let frames = if fr.len() == 1 { vec![(bytes, 0)] } else {
            fr.into_iter().map(|(f, ms)| (jpeg_bytes(&transcode::fit(f, MAX_SIDE)), ms)).collect()
        };
        Ok((frames, w, h))
    }).await.map_err(err)??;
    let id = state.next_media.fetch_add(1, Ordering::SeqCst) + 1;
    let n = frames.len();
    let mut m = state.media.lock().unwrap();
    if m.len() > 8 { let oldest = *m.keys().min().unwrap(); m.remove(&oldest); }
    m.insert(id, Media { name, frames });
    Ok(MediaDto { id, name: m[&id].name.clone(), frames: n, width: w, height: h })
}

/// Start media built from frames the frontend decodes (video, camera recording).
#[tauri::command]
fn media_begin(state: State<AppState>, name: String) -> u32 {
    let id = state.next_media.fetch_add(1, Ordering::SeqCst) + 1;
    let mut m = state.media.lock().unwrap();
    if m.len() > 8 { let oldest = *m.keys().min().unwrap(); m.remove(&oldest); }
    m.insert(id, Media { name, frames: Vec::new() });
    id
}

#[tauri::command]
fn media_push(state: State<AppState>, media: u32, data: String, delay: u32) -> Result<usize, String> {
    let bytes = b64(&data)?;
    let mut m = state.media.lock().unwrap();
    let md = m.get_mut(&media).ok_or("media bestaat niet meer")?;
    if md.frames.len() >= 3000 { return Err("maximaal 3000 frames".into()); }
    md.frames.push((bytes, delay));
    Ok(md.frames.len())
}

#[tauri::command]
fn media_drop(state: State<AppState>, media: u32) { state.media.lock().unwrap().remove(&media); }

/// Preview of one frame of a media item ("test" when media is 0) — no network.
#[tauri::command]
async fn preview(state: State<'_, AppState>, media: u32, frame: Option<usize>, placement: PlacementDto, id: Option<u32>) -> Result<PreviewDto, String> {
    let ft = fan_type_for(&state, id)?;
    let (src, n) = if media == 0 { (None, 1) } else {
        let m = state.media.lock().unwrap();
        let md = m.get(&media).ok_or("media bestaat niet meer")?;
        if md.frames.is_empty() { return Err("nog geen frames".into()); }
        (Some(md.frames[frame.unwrap_or(0).min(md.frames.len() - 1)].0.clone()), md.frames.len())
    };
    tauri::async_runtime::spawn_blocking(move || -> Result<PreviewDto, String> {
        let img = match src { None => transcode::test_card(), Some(b) => decode_source(&b)? };
        let f = transcode::encode(&img, &ft, &placement.into());
        let round = transcode::preview_from_jpeg(&f.jpeg, &ft, 480).map_err(err)?;
        let (w, h) = f.layout.dimensions();
        Ok(PreviewDto { round: data_url("image/png", &png_bytes(&round)), layout: data_url("image/jpeg", &f.jpeg),
            jpeg_bytes: f.jpeg.len(), quality: f.quality, width: w, height: h, frames: n })
    }).await.map_err(err)?
}

#[derive(Deserialize)]
struct UploadArgs { id: u32, media: u32, name: Option<String>, slot: u8, placement: PlacementDto, #[serde(default)] max_frames: usize }

#[tauri::command]
async fn upload(app: AppHandle, state: State<'_, AppState>, args: UploadArgs) -> Result<String, String> {
    let l = link(&state)?;
    let ft = fan_type_for(&state, Some(args.id))?;
    let seq = state.upload_seq.fetch_add(1, Ordering::SeqCst) + 1;
    let pl: Placement = args.placement.into();
    let (src, default_name): (Vec<Vec<u8>>, String) = if args.media == 0 { (vec![], "testkaart".into()) } else {
        let m = state.media.lock().unwrap();
        let md = m.get(&args.media).ok_or("media bestaat niet meer")?;
        let mut v: Vec<Vec<u8>> = md.frames.iter().map(|(b, _)| b.clone()).collect();
        if args.max_frames > 0 { v.truncate(args.max_frames); }
        (v, md.name.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or(md.name.clone()))
    };
    let name = args.name.filter(|n| !n.trim().is_empty()).unwrap_or(default_name);
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let emit = |stage: &str, done: usize, total: usize| {
            let _ = app.emit("p56", serde_json::json!({ "type": "progress", "data": { "stage": stage, "done": done, "total": total } }));
        };
        let frames: Vec<Vec<u8>> = if src.is_empty() {
            vec![transcode::encode(&transcode::test_card(), &ft, &pl).jpeg]
        } else {
            let total = src.len();
            src.iter().enumerate().map(|(i, b)| {
                if i % 5 == 0 { emit("transcoderen", i, total); }
                decode_source(b).map(|img| transcode::encode(&img, &ft, &pl).jpeg)
            }).collect::<Result<_, _>>()?
        };
        emit("versturen", 0, frames.len());
        l.upload(args.id, &frames, &name, Some(args.slot), seq).map_err(err)?;
        emit("klaar", frames.len(), frames.len());
        Ok(format!("{} ({} frame(s))", p56_core::protocol::mp4_name(&name), frames.len()))
    }).await.map_err(err)?
}

// ---------------------------------------------------------------------------
// live cast
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CastArgs { source: String, fps: f64, placement: PlacementDto }

/// Cast statistics plus the round picture the fan shows (decoded from the fan-layout JPEG).
fn emit_cast(app: &AppHandle, ft: &FanType, fps: f64, fans: usize, jpeg: &[u8], q: u8) {
    let preview = transcode::preview_from_jpeg(jpeg, ft, 320).map(|r| data_url("image/jpeg", &transcode::jpeg(&r, 85))).unwrap_or_default();
    let _ = app.emit("p56", serde_json::json!({ "type": "cast", "data": { "fps": (fps * 10.0).round() / 10.0, "fans": fans,
        "jpeg": jpeg.len(), "q": q, "preview": preview } }));
}

/// Backend-driven cast: "test", "screen" (desktop) or "media:<id>".
#[tauri::command]
fn cast_start(app: AppHandle, state: State<AppState>, args: CastArgs) -> Result<(), String> {
    let l = link(&state)?;
    let ft = fan_type_for(&state, None)?;
    let pl: Placement = args.placement.into();
    let running = state.cast_running.clone();
    let media: Option<Vec<(Vec<u8>, u32)>> = match args.source.strip_prefix("media:") {
        Some(id) => {
            let id: u32 = id.parse().map_err(err)?;
            Some(state.media.lock().unwrap().get(&id).ok_or("media bestaat niet meer")?.frames.clone())
        }
        None => None,
    };
    if args.source == "screen" && !cfg!(any(target_os = "windows", target_os = "macos")) {
        return Err("scherm casten kan op dit platform niet; gebruik camera of een bestand".into());
    }
    if running.swap(true, Ordering::SeqCst) { return Err("er loopt al een cast".into()); }
    let src = args.source.clone();
    thread::spawn(move || {
        let period = Duration::from_secs_f64(1.0 / args.fps.clamp(0.5, 60.0));
        let mut n = 0u64;
        let mut last_report = Instant::now();
        let mut i = 0usize;
        // stills/GIFs: encode once
        let mut cache: Vec<Option<Vec<u8>>> = media.as_ref().map(|m| vec![None; m.len()]).unwrap_or_default();
        let test = transcode::encode(&transcode::test_card(), &ft, &pl).jpeg;
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let screen = if src == "screen" { xcap::Monitor::all().ok().and_then(|m| m.into_iter().find(|m| m.is_primary().unwrap_or(false))) } else { None };
        while running.load(Ordering::SeqCst) {
            let t = Instant::now();
            let (jpeg, q, delay) = if let Some(m) = &media {
                let k = i % m.len();
                i += 1;
                if cache[k].is_none() {
                    match decode_source(&m[k].0) { Ok(img) => cache[k] = Some(transcode::encode(&img, &ft, &pl).jpeg), Err(_) => continue }
                }
                (cache[k].clone().unwrap(), 0, m[k].1)
            } else if src == "screen" {
                #[cfg(any(target_os = "windows", target_os = "macos"))]
                {
                    let Some(mon) = &screen else { break };
                    match mon.capture_image() {
                        Ok(rgba) => {
                            let img = transcode::fit(image::DynamicImage::ImageRgba8(rgba).to_rgb8(), MAX_SIDE);
                            let f = transcode::encode(&img, &ft, &pl);
                            (f.jpeg, f.quality, 0)
                        }
                        Err(_) => { thread::sleep(Duration::from_millis(100)); continue; }
                    }
                }
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                { break }
            } else {
                (test.clone(), 80, 0)
            };
            let fans = l.cast(&jpeg);
            if fans > 0 { n += 1; }
            if last_report.elapsed() > Duration::from_millis(500) {
                emit_cast(&app, &ft, n as f64 / last_report.elapsed().as_secs_f64(), fans, &jpeg, q);
                n = 0;
                last_report = Instant::now();
            }
            let wait = if delay > 0 { Duration::from_millis(delay as u64) } else { period };
            if let Some(rest) = wait.checked_sub(t.elapsed()) { thread::sleep(rest); }
        }
        running.store(false, Ordering::SeqCst);
        let _ = app.emit("p56", serde_json::json!({ "type": "cast", "data": { "stopped": true } }));
    });
    Ok(())
}

/// Frontend-driven cast: one frame (camera, video) per call. Returns the number of fans reached.
#[tauri::command]
async fn cast_frame(app: AppHandle, state: State<'_, AppState>, data: String, placement: PlacementDto) -> Result<usize, String> {
    let l = link(&state)?;
    let ft = fan_type_for(&state, None)?;
    let bytes = b64(&data)?;
    let fans = tauri::async_runtime::spawn_blocking(move || -> Result<(usize, Vec<u8>, u8), String> {
        let img = decode_source(&bytes)?;
        let f = transcode::encode(&img, &ft, &placement.into());
        Ok((l.cast(&f.jpeg), f.jpeg, f.quality))
    }).await.map_err(err)??;
    let mut s = state.live.lock().unwrap();
    s.n += 1;
    let since = *s.since.get_or_insert_with(Instant::now);
    if since.elapsed() > Duration::from_millis(500) {
        emit_cast(&app, &ft, s.n as f64 / since.elapsed().as_secs_f64(), fans.0, &fans.1, fans.2);
        s.n = 0;
        s.since = Some(Instant::now());
    }
    Ok(fans.0)
}

#[tauri::command]
fn cast_stop(state: State<AppState>) {
    state.cast_running.store(false, Ordering::SeqCst);
    *state.live.lock().unwrap() = LiveStats::default();
}

#[tauri::command]
fn platform() -> serde_json::Value {
    serde_json::json!({ "os": std::env::consts::OS, "screen": cfg!(any(target_os = "windows", target_os = "macos")),
        "version": env!("CARGO_PKG_VERSION") })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            link: Mutex::new(None), fan_type: Mutex::new(0), cast_running: Arc::new(AtomicBool::new(false)),
            upload_seq: AtomicU32::new(0), media: Mutex::new(HashMap::new()), next_media: AtomicU32::new(0),
            live: Mutex::new(LiveStats::default()),
        })
        .invoke_handler(tauri::generate_handler![
            start_link, stop_link, link_info, devices, set_fan_type, fan_types, control, set_value, set_text,
            media_load, media_begin, media_push, media_drop, preview, upload,
            cast_start, cast_frame, cast_stop, platform
        ])
        .run(tauri::generate_context!())
        .expect("error while running P56 Studio");
}
