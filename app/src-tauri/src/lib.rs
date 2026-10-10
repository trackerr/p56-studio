//! P56 Studio — Tauri backend. Thin command layer over `p56-core`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use p56_core::{transcode, Config, Event, FanType, Link, Placement};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

struct AppState {
    link: Mutex<Option<Link>>,
    fan_type: Mutex<u8>,
    cast_running: Arc<AtomicBool>,
    upload_seq: Mutex<u32>,
}

#[derive(Serialize, Clone)]
struct DeviceDto {
    id: u32,
    ip: String,
    name: String,
    mac: String,
    version: String,
    wifi: String,
    fan_type: u8,
    angles: u16,
    leds: u8,
    power: bool,
    playing: bool,
    play_index: u8,
    files: Vec<(u8, String)>,
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
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn png_bytes(img: &image::RgbImage) -> Vec<u8> {
    let mut v = Vec::new();
    image::DynamicImage::ImageRgb8(img.clone()).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).ok();
    v
}

fn link(state: &State<AppState>) -> Result<Link, String> {
    state.link.lock().unwrap().clone().ok_or_else(|| "link not started".into())
}

fn ft(state: &State<AppState>) -> FanType { FanType::by_id(*state.fan_type.lock().unwrap()) }

fn err<E: std::fmt::Display>(e: E) -> String { e.to_string() }

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn start_link(app: AppHandle, state: State<AppState>, ip: Option<String>) -> Result<String, String> {
    let mut g = state.link.lock().unwrap();
    if let Some(l) = g.as_ref() { return Ok(l.local_ip.to_string()); }
    let cfg = Config { local_ip: ip.and_then(|s| s.parse().ok()), ..Default::default() };
    let (l, rx) = Link::start(cfg).map_err(err)?;
    let ip = l.local_ip.to_string();
    *g = Some(l);
    thread::spawn(move || {
        for ev in rx {
            let (name, payload) = match ev {
                Event::Discovered { ip } => ("discovered", serde_json::json!({ "ip": ip.to_string() })),
                Event::Connected { id, ip } => ("connected", serde_json::json!({ "id": id, "ip": ip.to_string() })),
                Event::Disconnected { id } => ("disconnected", serde_json::json!({ "id": id })),
                Event::Packet { id, packet } => ("packet", serde_json::json!({ "id": id, "packet": format!("{packet:?}") })),
                Event::Log(s) => ("log", serde_json::json!({ "msg": s })),
            };
            let _ = app.emit("p56", serde_json::json!({ "type": name, "data": payload }));
        }
    });
    Ok(ip)
}

#[tauri::command]
fn devices(state: State<AppState>) -> Result<Vec<DeviceDto>, String> {
    let l = link(&state)?;
    Ok(l.devices().into_iter().map(|d| {
        let s = d.status.clone().unwrap_or_default();
        let i = d.info.clone().unwrap_or_default();
        DeviceDto {
            id: d.id,
            ip: d.ip.map(|i| i.to_string()).unwrap_or_default(),
            name: if i.name.is_empty() { format!("fan #{}", d.id) } else { i.name },
            mac: i.mac, version: i.version, wifi: i.wifi,
            fan_type: s.fan_type, angles: s.angles, leds: s.leds_per_arm,
            power: s.power, playing: s.playing, play_index: s.play_index,
            files: d.files,
        }
    }).collect())
}

#[tauri::command]
fn discovered(state: State<AppState>) -> Result<Vec<String>, String> {
    Ok(link(&state)?.discovered().into_iter().map(|i| i.to_string()).collect())
}

#[tauri::command]
fn set_fan_type(state: State<AppState>, fan_type: u8) { *state.fan_type.lock().unwrap() = fan_type; }

#[tauri::command]
fn fan_types() -> Vec<serde_json::Value> {
    FanType::all().iter().map(|&id| {
        let f = FanType::by_id(id);
        let (w, h) = f.jpeg_size();
        serde_json::json!({ "id": id, "angles": f.angles, "radius": f.radius, "arms": f.arms, "jpeg": format!("{w}×{h}") })
    }).collect()
}

#[tauri::command]
fn control(state: State<AppState>, id: u32, action: String) -> Result<(), String> {
    let l = link(&state)?;
    match action.as_str() {
        "on" => l.on(id), "off" => l.off(id), "play" => l.play(id), "pause" => l.pause(id),
        "refresh" => l.refresh(id),
        _ => return Err(format!("unknown action {action}")),
    }.map_err(err)
}

#[tauri::command]
fn select_slot(state: State<AppState>, id: u32, slot: u8) -> Result<(), String> { link(&state)?.select(id, slot).map_err(err) }
#[tauri::command]
fn delete_slot(state: State<AppState>, id: u32, slot: u8) -> Result<(), String> { link(&state)?.delete(id, slot).map_err(err) }
#[tauri::command]
fn rename_file(state: State<AppState>, id: u32, old: String, new: String) -> Result<(), String> { link(&state)?.rename(id, &old, &new).map_err(err) }
#[tauri::command]
fn set_wifi(state: State<AppState>, id: u32, ssid: String, password: String) -> Result<(), String> { link(&state)?.set_wifi(id, &ssid, &password).map_err(err) }

/// Transcode a file (or "test") and return previews — no network.
#[tauri::command]
fn preview(state: State<AppState>, path: String, placement: PlacementDto) -> Result<PreviewDto, String> {
    let ft = ft(&state);
    let img = if path == "test" { transcode::test_card() } else { transcode::load_rgb(&PathBuf::from(&path)).map_err(err)? };
    let f = transcode::encode(&img, &ft, &placement.into());
    let round = transcode::preview_from_jpeg(&f.jpeg, &ft, 480).map_err(err)?;
    let (w, h) = f.layout.dimensions();
    Ok(PreviewDto {
        round: data_url("image/png", &png_bytes(&round)),
        layout: data_url("image/jpeg", &f.jpeg),
        jpeg_bytes: f.jpeg.len(), quality: f.quality, width: w, height: h,
    })
}

#[derive(Deserialize)]
struct UploadArgs { id: u32, path: String, name: Option<String>, slot: u8, placement: PlacementDto, #[serde(default)] max_frames: usize }

#[tauri::command]
async fn upload(app: AppHandle, state: State<'_, AppState>, args: UploadArgs) -> Result<String, String> {
    let l = link(&state)?;
    let ft = ft(&state);
    let seq = { let mut s = state.upload_seq.lock().unwrap(); *s += 1; *s };
    let pl: Placement = args.placement.into();
    let path = PathBuf::from(&args.path);
    let name = args.name.filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("upload".into()));
    let progress = app.clone();
    let res = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let frames: Vec<Vec<u8>> = if let Ok(img) = transcode::load_rgb(&path) {
            vec![transcode::encode(&img, &ft, &pl).jpeg]
        } else {
            let mut v = p56_core::video::open(&path, 720).map_err(err)?;
            let mut out = Vec::new();
            while let Some(fr) = v.next_frame() {
                out.push(transcode::encode(&fr, &ft, &pl).jpeg);
                if out.len() % 10 == 0 { let _ = progress.emit("p56", serde_json::json!({ "type": "progress", "data": { "stage": "transcode", "frames": out.len() } })); }
                if args.max_frames > 0 && out.len() >= args.max_frames { break; }
            }
            if out.is_empty() { return Err("no frames decoded".into()); }
            out
        };
        let _ = progress.emit("p56", serde_json::json!({ "type": "progress", "data": { "stage": "upload", "frames": frames.len() } }));
        l.upload(args.id, &frames, &name, Some(args.slot), seq).map_err(err)?;
        Ok(format!("{} ({} frame(s))", name, frames.len()))
    }).await.map_err(err)?;
    res
}

#[derive(Deserialize)]
struct CastArgs { source: String, fps: f64, placement: PlacementDto }

#[tauri::command]
fn cast_start(app: AppHandle, state: State<AppState>, args: CastArgs) -> Result<(), String> {
    let l = link(&state)?;
    let ft = ft(&state);
    let pl: Placement = args.placement.into();
    let running = state.cast_running.clone();
    if running.swap(true, Ordering::SeqCst) { return Err("cast already running".into()); }
    thread::spawn(move || {
        let period = Duration::from_secs_f64(1.0 / args.fps.clamp(0.5, 60.0));
        let mut n = 0u64;
        let mut last_report = Instant::now();
        let mut still: Option<image::RgbImage> = None;
        let mut video: Option<p56_core::video::VideoSource> = None;
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let mut screen: Option<xcap::Monitor> = None;
        let src = args.source.clone();
        if src == "test" { still = Some(transcode::test_card()); }
        else if src == "screen" {
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            { screen = xcap::Monitor::all().ok().and_then(|m| m.into_iter().find(|m| m.is_primary().unwrap_or(false)).or_else(|| None)); }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            { let _ = app.emit("p56", serde_json::json!({ "type": "cast", "data": { "error": "screen capture not available on this platform" } })); running.store(false, Ordering::SeqCst); return; }
        } else if let Ok(img) = transcode::load_rgb(&PathBuf::from(&src)) { still = Some(img); }
        else { video = p56_core::video::open(&PathBuf::from(&src), 720).ok(); }

        while running.load(Ordering::SeqCst) {
            let t = Instant::now();
            let img = if let Some(s) = &still { s.clone() }
                else if let Some(v) = &mut video {
                    match v.next_frame() { Some(f) => f, None => { video = p56_core::video::open(&PathBuf::from(&src), 720).ok(); continue; } }
                } else {
                    #[cfg(any(target_os = "windows", target_os = "macos"))]
                    {
                        let Some(m) = &screen else { break };
                        match m.capture_image() {
                            Ok(rgba) => {
                                let (w, h) = (rgba.width(), rgba.height());
                                // downscale to ~720 px on the short side to keep the warp cheap
                                let dyn_img = image::DynamicImage::ImageRgba8(rgba);
                                let s = 720.0 / w.min(h) as f64;
                                if s < 1.0 { dyn_img.resize((w as f64 * s) as u32, (h as f64 * s) as u32, image::imageops::FilterType::Triangle).to_rgb8() } else { dyn_img.to_rgb8() }
                            }
                            Err(_) => { thread::sleep(Duration::from_millis(100)); continue; }
                        }
                    }
                    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                    { break }
                };
            let f = transcode::encode(&img, &ft, &pl);
            let ok = l.cast(&f.jpeg);
            if ok > 0 { n += 1; }
            if last_report.elapsed() > Duration::from_millis(500) {
                let fps = n as f64 / last_report.elapsed().as_secs_f64();
                let _ = app.emit("p56", serde_json::json!({ "type": "cast", "data": { "fps": (fps * 10.0).round() / 10.0, "fans": ok, "jpeg": f.jpeg.len(), "q": f.quality,
                    "preview": data_url("image/jpeg", &f.jpeg) } }));
                n = 0; last_report = Instant::now();
            }
            if let Some(rest) = period.checked_sub(t.elapsed()) { thread::sleep(rest); }
        }
        running.store(false, Ordering::SeqCst);
        let _ = app.emit("p56", serde_json::json!({ "type": "cast", "data": { "stopped": true } }));
    });
    Ok(())
}

#[tauri::command]
fn cast_stop(state: State<AppState>) { state.cast_running.store(false, Ordering::SeqCst); }

#[tauri::command]
fn has_ffmpeg() -> bool {
    std::process::Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState { link: Mutex::new(None), fan_type: Mutex::new(8), cast_running: Arc::new(AtomicBool::new(false)), upload_seq: Mutex::new(0) })
        .invoke_handler(tauri::generate_handler![
            start_link, devices, discovered, set_fan_type, fan_types, control,
            select_slot, delete_slot, rename_file, set_wifi, preview, upload,
            cast_start, cast_stop, has_ffmpeg
        ])
        .setup(|app| {
            let _ = app.handle(); Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running P56 Studio");
}
