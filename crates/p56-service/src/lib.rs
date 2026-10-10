//! Command layer shared by P56 Studio (Tauri, desktop + Android) and the Home Assistant
//! add-on server. Every UI action is `Service::call(cmd, json_args)`; events go out through
//! the `Emit` callback (`type`, `data`).
//!
//! Media never comes in as a file path from the UI: the frontend reads the file (which also
//! works for Android `content://` picks) or decodes video frames itself and sends the bytes.

pub mod text;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use p56_core::protocol::{FanError, Packet};
use p56_core::{transcode, ColorMode, Config, Event, FanType, Link, Placement};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MAX_SIDE: u32 = 720;

pub type Emit = Arc<dyn Fn(&str, Value) + Send + Sync>;

/// Media loaded by the frontend: source frames (encoded, to keep memory low) + delays in ms.
struct Media {
    name: String,
    frames: Vec<(Vec<u8>, u32)>,
}

#[derive(Default)]
struct LiveStats { n: u64, since: Option<Instant> }

/// Where the link listens and what it targets when the UI does not say otherwise.
#[derive(Debug, Clone, Default)]
pub struct Defaults {
    pub ip: Option<String>,
    pub targets: Vec<String>,
    pub port: Option<u16>,
}

pub struct Service {
    link: Mutex<Option<Link>>,
    /// Fan type chosen by the user; 0 = follow what the fan reports.
    fan_type: Mutex<u8>,
    cast_running: Arc<AtomicBool>,
    upload_seq: AtomicU32,
    media: Mutex<HashMap<u32, Media>>,
    next_media: AtomicU32,
    live: Mutex<LiveStats>,
    last_activity: Mutex<Option<std::time::SystemTime>>,
    /// Round preview (PNG) of what each fan (by MAC) was last sent.
    shown: Mutex<HashMap<String, Vec<u8>>>,
    emit: Emit,
    defaults: Defaults,
}

#[derive(Serialize, Clone)]
pub struct StatusDto {
    pub power: bool, pub playing: bool, pub repeat_all: bool, pub breath: bool, pub autostart: bool, pub master: bool,
    pub play_index: u8, pub brightness: u8, pub color_mode: u8, pub angle: i32, pub fan_type: u8,
}

#[derive(Serialize, Clone)]
pub struct DeviceDto {
    pub id: u32,
    pub ip: String,
    pub name: String,
    pub mac: String,
    pub hotspot: String,
    pub router: String,
    pub status: Option<StatusDto>,
    pub files: Vec<(u8, String)>,
    pub last_error: Option<String>,
    pub ble: Option<(u8, Vec<String>)>,
    pub support: String,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct PlacementDto {
    #[serde(default)] pub x: f64,
    #[serde(default)] pub y: f64,
    #[serde(default = "one")] pub scale: f64,
    #[serde(default)] pub mirror: bool,
}
fn one() -> f64 { 1.0 }
impl Default for PlacementDto {
    fn default() -> Self { PlacementDto { x: 0.0, y: 0.0, scale: 1.0, mirror: false } }
}
impl From<PlacementDto> for Placement {
    fn from(p: PlacementDto) -> Self { Placement { x_offset: p.x, y_offset: p.y, scale: p.scale, mirror: p.mirror } }
}

/// What to show for `show_text` / `show_image`.
#[derive(Deserialize, Debug, Default)]
pub struct ShowRequest {
    /// Fan id; empty = all connected fans.
    pub id: Option<u32>,
    /// Fan MAC address (stable across reconnects), instead of id.
    #[serde(default)] pub mac: Option<String>,
    #[serde(default)] pub text: Option<String>,
    /// Image as base64 or data URL.
    #[serde(default)] pub data: Option<String>,
    /// Image file path (inside the add-on: /media, /share, /config).
    #[serde(default, alias = "image")] pub path: Option<String>,
    #[serde(default, alias = "fg_color")] pub color: Option<Value>,
    #[serde(default, alias = "bg_color")] pub background: Option<Value>,
    #[serde(default)] pub font_size: Option<f32>,
    /// Remove again after this many seconds (0 = keep).
    #[serde(default)] pub show_seconds: Option<f64>,
    /// "upload" (store in the playlist, default) or "cast" (live, nothing stored).
    #[serde(default)] pub mode: Option<String>,
    #[serde(default, alias = "filename")] pub name: Option<String>,
    #[serde(default)] pub placement: Option<PlacementDto>,
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn png_bytes(img: &image::RgbImage) -> Vec<u8> {
    let mut v = Vec::new();
    image::DynamicImage::ImageRgb8(img.clone()).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).ok();
    v
}

pub fn b64(s: &str) -> Result<Vec<u8>, String> {
    let s = s.split_once("base64,").map(|(_, d)| d).unwrap_or(s);
    base64::engine::general_purpose::STANDARD.decode(s.trim()).map_err(|e| format!("ongeldige data: {e}"))
}

fn err<E: std::fmt::Display>(e: E) -> String { e.to_string() }

fn decode_source(bytes: &[u8]) -> Result<image::RgbImage, String> {
    Ok(transcode::fit(transcode::decode_rgb(bytes).map_err(err)?, MAX_SIDE))
}

/// Read an argument by snake_case or camelCase name.
fn arg<T: serde::de::DeserializeOwned>(a: &Value, key: &str) -> Option<T> {
    let camel: String = key.split('_').enumerate().map(|(i, p)| if i == 0 { p.to_string() } else { p[..1].to_uppercase() + &p[1..] }).collect();
    a.get(key).or_else(|| a.get(&camel)).filter(|v| !v.is_null()).and_then(|v| serde_json::from_value(v.clone()).ok())
}
fn need<T: serde::de::DeserializeOwned>(a: &Value, key: &str) -> Result<T, String> {
    arg(a, key).ok_or_else(|| format!("argument '{key}' ontbreekt of is ongeldig"))
}

fn color(v: &Option<Value>, default: [u8; 3]) -> [u8; 3] {
    match v {
        Some(Value::String(s)) => text::parse_color(s).unwrap_or(default),
        Some(Value::Array(a)) if a.len() == 3 => {
            let c: Vec<u8> = a.iter().map(|x| x.as_u64().unwrap_or(0).min(255) as u8).collect();
            [c[0], c[1], c[2]]
        }
        _ => default,
    }
}

impl Service {
    pub fn new(emit: Emit, defaults: Defaults) -> Arc<Service> {
        Arc::new(Service {
            link: Mutex::new(None), fan_type: Mutex::new(0), cast_running: Arc::new(AtomicBool::new(false)),
            upload_seq: AtomicU32::new(0), media: Mutex::new(HashMap::new()), next_media: AtomicU32::new(0),
            live: Mutex::new(LiveStats::default()), last_activity: Mutex::new(None), shown: Mutex::new(HashMap::new()), emit, defaults,
        })
    }

    pub fn link(&self) -> Result<Link, String> {
        self.link.lock().unwrap().clone().ok_or_else(|| "verbinding niet gestart".into())
    }

    fn emit(&self, t: &str, data: Value) { (self.emit)(t, data); }

    /// Remember what fan id shows now (as a round preview), for dashboards.
    fn remember(&self, ids: &[u32], jpeg: &[u8], ft: &FanType) {
        let Ok(l) = self.link() else { return };
        let Ok(round) = transcode::preview_from_jpeg(jpeg, ft, 360) else { return };
        let png = png_bytes(&round);
        let mut m = self.shown.lock().unwrap();
        for id in ids {
            let mac = l.device(*id).and_then(|d| d.info).map(|i| i.mac).filter(|m| !m.is_empty());
            m.insert(id.to_string(), png.clone());
            if let Some(mac) = mac { m.insert(mac, png.clone()); }
        }
        drop(m);
        self.emit("shown", json!({ "ids": ids }));
    }

    /// Round preview (PNG) of what the fan with this MAC (or id) was last sent by this server.
    pub fn shown(&self, mac: &str) -> Option<Vec<u8>> { self.shown.lock().unwrap().get(mac).cloned() }

    /// Dispatch one command from the UI or the HTTP API.
    pub fn call(self: &Arc<Self>, cmd: &str, a: &Value) -> Result<Value, String> {
        match cmd {
            "start_link" => self.start_link(arg(a, "ip"), arg(a, "targets"), arg(a, "port")).map(Value::from),
            "stop_link" => { self.stop_link(); Ok(Value::Null) }
            "link_info" => self.link_info(),
            "devices" => Ok(serde_json::to_value(self.devices()?).unwrap()),
            "set_fan_type" => { *self.fan_type.lock().unwrap() = need::<u8>(a, "fan_type")?; Ok(Value::Null) }
            "fan_types" => Ok(fan_types()),
            "control" => self.control(need(a, "id")?, &need::<String>(a, "action")?).map(|_| Value::Null),
            "set_value" => self.set_value(need(a, "id")?, &need::<String>(a, "what")?, need(a, "value")?).map(|_| Value::Null),
            "set_text" => self.set_text(need(a, "id")?, &need::<String>(a, "what")?, need(a, "a")?, arg(a, "b"), arg(a, "slot")).map(|_| Value::Null),
            "media_load" => self.media_load(need(a, "name")?, &need::<String>(a, "data")?),
            "media_text" => self.media_text(a),
            "media_begin" => Ok(self.media_begin(need(a, "name")?).into()),
            "media_push" => self.media_push(need(a, "media")?, &need::<String>(a, "data")?, arg(a, "delay").unwrap_or(0)).map(Value::from),
            "media_drop" => { self.media.lock().unwrap().remove(&need::<u32>(a, "media")?); Ok(Value::Null) }
            "preview" => self.preview(arg(a, "media").unwrap_or(0), arg(a, "frame"), arg(a, "placement").unwrap_or_default(), arg(a, "id")),
            "upload" => self.upload(need(a, "args")?),
            "cast_start" => self.cast_start(need(a, "args")?).map(|_| Value::Null),
            "cast_frame" => self.cast_frame(&need::<String>(a, "data")?, arg(a, "placement").unwrap_or_default()).map(Value::from),
            "cast_stop" => { self.cast_stop(); Ok(Value::Null) }
            "show_text" | "show_image" => self.show(serde_json::from_value(a.clone()).map_err(err)?),
            "platform" => Ok(json!({ "os": std::env::consts::OS, "screen": cfg!(any(target_os = "windows", target_os = "macos")),
                "version": env!("CARGO_PKG_VERSION") })),
            other => Err(format!("onbekend commando {other}")),
        }
    }

    // ---- link & devices ----------------------------------------------------

    pub fn start_link(self: &Arc<Self>, ip: Option<String>, targets: Option<Vec<String>>, port: Option<u16>) -> Result<String, String> {
        let mut g = self.link.lock().unwrap();
        if let Some(l) = g.as_ref() { return Ok(l.local_ip.to_string()); }
        let ip = ip.filter(|s| !s.trim().is_empty()).or(self.defaults.ip.clone());
        let mut tg = targets.unwrap_or_default();
        tg.extend(self.defaults.targets.iter().cloned());
        let cfg = Config {
            local_ip: ip.and_then(|s| s.trim().parse().ok()),
            targets: tg.iter().filter_map(|t| { let t = t.trim(); t.parse().ok().or_else(|| format!("{t}:8988").parse().ok()) }).collect(),
            tcp_port: port.or(self.defaults.port).unwrap_or(p56_core::protocol::TCP_PORT),
            ..Default::default()
        };
        let (l, rx) = Link::start(cfg).map_err(err)?;
        let ip = l.local_ip.to_string();
        *g = Some(l);
        let me = Arc::downgrade(self);
        thread::spawn(move || {
            for ev in rx {
                let Some(s) = me.upgrade() else { break };
                *s.last_activity.lock().unwrap() = Some(std::time::SystemTime::now());
                let (name, payload) = match ev {
                    Event::Discovered { ip } => ("discovered", json!({ "ip": ip.to_string() })),
                    Event::Connected { id, ip } => ("connected", json!({ "id": id, "ip": ip.to_string() })),
                    Event::Disconnected { id } => ("disconnected", json!({ "id": id })),
                    Event::Packet { id, packet } => {
                        let kind = match &packet {
                            Packet::Status(_) => "status", Packet::MachineInfo(_) => "info", Packet::FileList(_) => "files",
                            Packet::Error(_) => "error", Packet::List { .. } => "list", Packet::TransDone => "done", _ => continue,
                        };
                        let msg = if let Packet::Error(e) = &packet { e.to_string() } else { String::new() };
                        ("packet", json!({ "id": id, "kind": kind, "msg": msg }))
                    }
                    Event::Log(s) => ("log", json!({ "msg": s })),
                };
                s.emit(name, payload);
            }
        });
        Ok(ip)
    }

    pub fn stop_link(&self) {
        self.cast_running.store(false, Ordering::SeqCst);
        if let Some(l) = self.link.lock().unwrap().take() {
            l.shutdown();
            thread::sleep(Duration::from_millis(400));
        }
    }

    fn link_info(&self) -> Result<Value, String> {
        let l = self.link()?;
        let ifs: Vec<String> = p56_core::net::interfaces().into_iter().map(|(ip, bc)| format!("{ip} (broadcast {bc})")).collect();
        Ok(json!({ "ip": l.local_ip.to_string(), "port": l.tcp_port, "interfaces": ifs,
            "discovered": l.discovered().iter().map(|i| i.to_string()).collect::<Vec<_>>() }))
    }

    pub fn devices(&self) -> Result<Vec<DeviceDto>, String> {
        let l = self.link()?;
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

    /// Unix time of the last packet from any fan.
    pub fn last_activity(&self) -> Option<f64> {
        self.last_activity.lock().unwrap().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64())
    }

    /// Fan type for transcoding: the user's choice, else what fan `id` (or the first fan) reports, else P56.
    fn fan_type_for(&self, id: Option<u32>) -> Result<FanType, String> {
        let chosen = *self.fan_type.lock().unwrap();
        let t = if chosen != 0 { chosen } else {
            self.link.lock().unwrap().as_ref().and_then(|l| {
                let devs = l.devices();
                id.and_then(|i| devs.iter().find(|d| d.id == i).cloned()).or_else(|| devs.into_iter().next())
            }).and_then(|d| d.status).map(|s| s.fan_type).filter(|t| *t != 0).unwrap_or(8)
        };
        if let Some(m) = FanType::check(t) { return Err(m); }
        Ok(FanType::by_id(t))
    }

    pub fn control(&self, id: u32, action: &str) -> Result<(), String> {
        let l = self.link()?;
        match action {
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
        }.map_err(err)
    }

    pub fn set_value(&self, id: u32, what: &str, value: i32) -> Result<(), String> {
        let l = self.link()?;
        match what {
            "brightness" => l.brightness(id, value.clamp(1, 16) as u8),
            "angle" => l.angle(id, (value.clamp(-180, 180) + 180) as u16),
            "volume" => l.volume(id, value.clamp(0, 10) as u8),
            "select" => l.select(id, value.clamp(1, 255) as u8),
            "delete" => l.delete(id, value.clamp(1, 255) as u8),
            "ble_connect" => l.ble_connect(id, value.clamp(0, 255) as u8),
            other => Err(p56_core::Error::Invalid(format!("onbekende instelling {other}"))),
        }.map_err(err)
    }

    pub fn set_text(&self, id: u32, what: &str, a: String, b: Option<String>, slot: Option<u8>) -> Result<(), String> {
        let l = self.link()?;
        let b = b.unwrap_or_default();
        match what {
            "hotspot" if b.len() < 8 => return Err("wachtwoord moet minstens 8 tekens zijn".into()),
            "hotspot" | "router" | "name" if a.trim().is_empty() => return Err("naam mag niet leeg zijn".into()),
            _ => {}
        }
        match what {
            "rename" => l.rename(id, slot.unwrap_or(1), &a),
            "name" => l.device_name(id, &a),
            "hotspot" => l.set_hotspot(id, &a, &b),
            "router" => l.set_router(id, &a, &b),
            other => Err(p56_core::Error::Invalid(format!("onbekend veld {other}"))),
        }.map_err(err)
    }

    // ---- media ---------------------------------------------------------------

    fn store_media(&self, name: String, frames: Vec<(Vec<u8>, u32)>) -> u32 {
        let id = self.next_media.fetch_add(1, Ordering::SeqCst) + 1;
        let mut m = self.media.lock().unwrap();
        if m.len() > 8 { let oldest = *m.keys().min().unwrap(); m.remove(&oldest); }
        m.insert(id, Media { name, frames });
        id
    }

    /// Load an image or GIF file (bytes as base64).
    pub fn media_load(&self, name: String, data: &str) -> Result<Value, String> {
        let bytes = b64(data)?;
        let fr = transcode::decode_frames(&bytes, 600).map_err(|e| format!("kan beeld niet lezen: {e}"))?;
        let (w, h) = fr[0].0.dimensions();
        let frames = if fr.len() == 1 { vec![(bytes, 0)] } else {
            fr.into_iter().map(|(f, ms)| (transcode::jpeg(&transcode::fit(f, MAX_SIDE), 92), ms)).collect()
        };
        let n = frames.len();
        let id = self.store_media(name.clone(), frames);
        Ok(json!({ "id": id, "name": name, "frames": n, "width": w, "height": h }))
    }

    /// Render text as a media item.
    fn media_text(&self, a: &Value) -> Result<Value, String> {
        let t: String = need(a, "text")?;
        let img = text::render(&t, color(&arg(a, "color"), [255, 255, 255]), color(&arg(a, "background"), [0, 0, 0]), arg(a, "font_size").unwrap_or(0.0));
        let name = arg::<String>(a, "name").unwrap_or_else(|| "tekst".into());
        let id = self.store_media(name.clone(), vec![(png_bytes(&img), 0)]);
        Ok(json!({ "id": id, "name": name, "frames": 1, "width": text::SIDE, "height": text::SIDE }))
    }

    pub fn media_begin(&self, name: String) -> u32 { self.store_media(name, Vec::new()) }

    fn media_push(&self, media: u32, data: &str, delay: u32) -> Result<usize, String> {
        let bytes = b64(data)?;
        let mut m = self.media.lock().unwrap();
        let md = m.get_mut(&media).ok_or("media bestaat niet meer")?;
        if md.frames.len() >= 3000 { return Err("maximaal 3000 frames".into()); }
        md.frames.push((bytes, delay));
        Ok(md.frames.len())
    }

    /// Preview of one frame of a media item (0 = test card) — no network.
    fn preview(&self, media: u32, frame: Option<usize>, placement: PlacementDto, id: Option<u32>) -> Result<Value, String> {
        let ft = self.fan_type_for(id)?;
        let (src, n) = if media == 0 { (None, 1) } else {
            let m = self.media.lock().unwrap();
            let md = m.get(&media).ok_or("media bestaat niet meer")?;
            if md.frames.is_empty() { return Err("nog geen frames".into()); }
            (Some(md.frames[frame.unwrap_or(0).min(md.frames.len() - 1)].0.clone()), md.frames.len())
        };
        let img = match src { None => transcode::test_card(), Some(b) => decode_source(&b)? };
        let f = transcode::encode(&img, &ft, &placement.into());
        let round = transcode::preview_from_jpeg(&f.jpeg, &ft, 480).map_err(err)?;
        let (w, h) = f.layout.dimensions();
        Ok(json!({ "round": data_url("image/png", &png_bytes(&round)), "layout": data_url("image/jpeg", &f.jpeg),
            "jpeg_bytes": f.jpeg.len(), "quality": f.quality, "width": w, "height": h, "frames": n }))
    }

    fn upload(&self, a: Value) -> Result<Value, String> {
        let id: u32 = need(&a, "id")?;
        let media: u32 = arg(&a, "media").unwrap_or(0);
        let slot: u8 = arg(&a, "slot").unwrap_or(1);
        let max_frames: usize = arg(&a, "max_frames").unwrap_or(0);
        let pl: Placement = arg::<PlacementDto>(&a, "placement").unwrap_or_default().into();
        let l = self.link()?;
        let ft = self.fan_type_for(Some(id))?;
        let seq = self.upload_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let (src, default_name): (Vec<Vec<u8>>, String) = if media == 0 { (vec![], "testkaart".into()) } else {
            let m = self.media.lock().unwrap();
            let md = m.get(&media).ok_or("media bestaat niet meer")?;
            let mut v: Vec<Vec<u8>> = md.frames.iter().map(|(b, _)| b.clone()).collect();
            if max_frames > 0 { v.truncate(max_frames); }
            (v, md.name.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or(md.name.clone()))
        };
        let name = arg::<String>(&a, "name").filter(|n| !n.trim().is_empty()).unwrap_or(default_name);
        let progress = |stage: &str, done: usize, total: usize| self.emit("progress", json!({ "stage": stage, "done": done, "total": total }));
        let frames: Vec<Vec<u8>> = if src.is_empty() {
            vec![transcode::encode(&transcode::test_card(), &ft, &pl).jpeg]
        } else {
            let total = src.len();
            src.iter().enumerate().map(|(i, b)| {
                if i % 5 == 0 { progress("transcoderen", i, total); }
                decode_source(b).map(|img| transcode::encode(&img, &ft, &pl).jpeg)
            }).collect::<Result<_, _>>()?
        };
        progress("versturen", 0, frames.len());
        l.upload(id, &frames, &name, Some(slot), seq).map_err(err)?;
        self.remember(&[id], &frames[0], &ft);
        progress("klaar", frames.len(), frames.len());
        Ok(Value::from(format!("{} ({} frame(s))", p56_core::protocol::mp4_name(&name), frames.len())))
    }

    // ---- live cast -------------------------------------------------------------

    fn emit_cast(&self, ft: &FanType, fps: f64, fans: usize, jpeg: &[u8], q: u8) {
        let preview = transcode::preview_from_jpeg(jpeg, ft, 320).map(|r| data_url("image/jpeg", &transcode::jpeg(&r, 85))).unwrap_or_default();
        self.emit("cast", json!({ "fps": (fps * 10.0).round() / 10.0, "fans": fans, "jpeg": jpeg.len(), "q": q, "preview": preview }));
    }

    /// Backend-driven cast: "test", "screen" (desktop) or "media:<id>".
    fn cast_start(self: &Arc<Self>, a: Value) -> Result<(), String> {
        let source: String = need(&a, "source")?;
        let fps: f64 = arg(&a, "fps").unwrap_or(15.0);
        let pl: Placement = arg::<PlacementDto>(&a, "placement").unwrap_or_default().into();
        let l = self.link()?;
        let ft = self.fan_type_for(None)?;
        let media: Option<Vec<(Vec<u8>, u32)>> = match source.strip_prefix("media:") {
            Some(id) => {
                let id: u32 = id.parse().map_err(err)?;
                Some(self.media.lock().unwrap().get(&id).ok_or("media bestaat niet meer")?.frames.clone())
            }
            None => None,
        };
        if source == "screen" && !cfg!(any(target_os = "windows", target_os = "macos")) {
            return Err("scherm casten kan op dit platform niet; gebruik camera of een bestand".into());
        }
        if self.cast_running.swap(true, Ordering::SeqCst) { return Err("er loopt al een cast".into()); }
        let me = self.clone();
        thread::spawn(move || {
            let period = Duration::from_secs_f64(1.0 / fps.clamp(0.5, 60.0));
            let (mut n, mut last_report, mut i) = (0u64, Instant::now(), 0usize);
            let mut cache: Vec<Option<Vec<u8>>> = media.as_ref().map(|m| vec![None; m.len()]).unwrap_or_default();
            let test = transcode::encode(&transcode::test_card(), &ft, &pl).jpeg;
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            let screen = if source == "screen" { xcap::Monitor::all().ok().and_then(|m| m.into_iter().find(|m| m.is_primary().unwrap_or(false))) } else { None };
            while me.cast_running.load(Ordering::SeqCst) {
                let t = Instant::now();
                let (jpeg, q, delay) = if let Some(m) = &media {
                    if m.is_empty() { break; }
                    let k = i % m.len();
                    i += 1;
                    if cache[k].is_none() {
                        match decode_source(&m[k].0) { Ok(img) => cache[k] = Some(transcode::encode(&img, &ft, &pl).jpeg), Err(_) => continue }
                    }
                    (cache[k].clone().unwrap(), 0, m[k].1)
                } else if source == "screen" {
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
                    me.emit_cast(&ft, n as f64 / last_report.elapsed().as_secs_f64(), fans, &jpeg, q);
                    n = 0;
                    last_report = Instant::now();
                }
                let wait = if delay > 0 { Duration::from_millis(delay as u64) } else { period };
                if let Some(rest) = wait.checked_sub(t.elapsed()) { thread::sleep(rest); }
            }
            me.cast_running.store(false, Ordering::SeqCst);
            me.emit("cast", json!({ "stopped": true }));
        });
        Ok(())
    }

    /// Frontend-driven cast: one frame (camera, video) per call. Returns the number of fans reached.
    fn cast_frame(&self, data: &str, placement: PlacementDto) -> Result<usize, String> {
        let l = self.link()?;
        let ft = self.fan_type_for(None)?;
        let img = decode_source(&b64(data)?)?;
        let f = transcode::encode(&img, &ft, &placement.into());
        let fans = l.cast(&f.jpeg);
        let mut s = self.live.lock().unwrap();
        s.n += 1;
        let since = *s.since.get_or_insert_with(Instant::now);
        if since.elapsed() > Duration::from_millis(500) {
            self.emit_cast(&ft, s.n as f64 / since.elapsed().as_secs_f64(), fans, &f.jpeg, f.quality);
            s.n = 0;
            s.since = Some(Instant::now());
        }
        Ok(fans)
    }

    pub fn cast_stop(&self) {
        self.cast_running.store(false, Ordering::SeqCst);
        *self.live.lock().unwrap() = LiveStats::default();
    }

    // ---- show text / image (Home Assistant) --------------------------------------

    /// Show a text or picture on one or all fans. "upload" stores it in the playlist and
    /// switches to it (and removes it again after `show_seconds`, returning to the previous
    /// slot); "cast" streams it live for `show_seconds` (default 10) without storing anything.
    pub fn show(self: &Arc<Self>, r: ShowRequest) -> Result<Value, String> {
        let l = self.link()?;
        let ids: Vec<u32> = match (r.id, r.mac.as_deref().filter(|m| !m.is_empty())) {
            (Some(i), _) => vec![i],
            (None, Some(mac)) => l.ids().into_iter().filter(|i| l.device(*i).and_then(|d| d.info).is_some_and(|inf| inf.mac.eq_ignore_ascii_case(mac))).collect(),
            (None, None) => l.ids(),
        };
        if ids.is_empty() { return Err("geen fan verbonden".into()); }
        let img = if let Some(t) = r.text.as_deref().filter(|t| !t.trim().is_empty()) {
            text::render(t, color(&r.color, [255, 255, 255]), color(&r.background, [0, 0, 0]), r.font_size.unwrap_or(0.0))
        } else if let Some(d) = &r.data {
            decode_source(&b64(d)?)?
        } else if let Some(p) = &r.path {
            decode_source(&std::fs::read(p).map_err(|e| format!("{p}: {e}"))?)?
        } else {
            return Err("geef 'text', 'data' of 'path' mee".into());
        };
        let pl: Placement = r.placement.unwrap_or_default().into();
        let secs = r.show_seconds.unwrap_or(0.0).max(0.0);
        let cast = r.mode.as_deref() == Some("cast");
        let mut results = Vec::new();
        if cast {
            let ft = self.fan_type_for(ids.first().copied())?;
            let jpeg = transcode::encode(&img, &ft, &pl).jpeg;
            self.remember(&ids, &jpeg, &ft);
            let dur = if secs > 0.0 { secs } else { 10.0 };
            let me = self.clone();
            thread::spawn(move || {
                let end = Instant::now() + Duration::from_secs_f64(dur);
                while Instant::now() < end && !me.cast_running.load(Ordering::SeqCst) {
                    if let Ok(l) = me.link() { l.cast(&jpeg); }
                    thread::sleep(Duration::from_millis(200));
                }
            });
            return Ok(json!({ "success": true, "mode": "cast", "seconds": dur, "fans": ids.len() }));
        }
        for id in ids {
            let ft = match self.fan_type_for(Some(id)) { Ok(f) => f, Err(e) => { results.push(json!({ "id": id, "success": false, "error": e })); continue } };
            let jpeg = transcode::encode(&img, &ft, &pl).jpeg;
            let name = p56_core::protocol::mp4_name(&r.name.clone().unwrap_or_else(|| format!("ha_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0))));
            // same name again (e.g. a text entity that keeps updating): replace instead of adding
            if let Some(old) = l.device(id).and_then(|d| d.files.iter().find(|(_, n)| *n == name).map(|(s, _)| *s)) {
                let _ = l.delete(id, old);
                let deadline = Instant::now() + Duration::from_secs(4);
                while Instant::now() < deadline && l.device(id).is_some_and(|d| d.files.iter().any(|(_, n)| *n == name)) {
                    let _ = l.refresh(id);
                    thread::sleep(Duration::from_millis(250));
                }
            }
            let prev = l.device(id).and_then(|d| d.status).map(|s| s.play_index).unwrap_or(0);
            let seq = self.upload_seq.fetch_add(1, Ordering::SeqCst) + 1;
            if let Err(e) = l.upload(id, std::slice::from_ref(&jpeg), &name, None, seq) {
                results.push(json!({ "id": id, "success": false, "error": e.to_string() }));
                continue;
            }
            // find the slot the fan gave the new file
            let deadline = Instant::now() + Duration::from_secs(6);
            let mut slot = 0u8;
            while Instant::now() < deadline {
                if let Some(s) = l.device(id).and_then(|d| d.files.iter().find(|(_, n)| *n == name).map(|(s, _)| *s)) { slot = s; break; }
                let _ = l.refresh(id);
                thread::sleep(Duration::from_millis(300));
            }
            if slot == 0 {
                results.push(json!({ "id": id, "success": false, "error": "upload verstuurd maar niet in de afspeellijst gevonden" }));
                continue;
            }
            let _ = l.select(id, slot);
            self.remember(&[id], &jpeg, &ft);
            if secs > 0.0 {
                let l2 = l.clone();
                let name2 = name.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs_f64(secs));
                    let cur = l2.device(id).and_then(|d| d.files.iter().find(|(_, n)| *n == name2).map(|(s, _)| *s)).unwrap_or(slot);
                    let _ = l2.delete(id, cur);
                    if prev > 0 {
                        let back = if prev > cur { prev - 1 } else { prev };
                        let _ = l2.select(id, back);
                    }
                });
            }
            results.push(json!({ "id": id, "success": true, "slot": slot, "name": name }));
        }
        let ok = results.iter().any(|r| r["success"] == true);
        Ok(json!({ "success": ok, "mode": "upload", "results": results }))
    }
}

pub fn fan_types() -> Value {
    (1..=40u8).filter_map(|id| {
        let known = FanType::known(id);
        let (angles, leds, _) = FanType::apk_geometry(id).or(known.map(|f| (f.angles, f.radius, 20)))?;
        let jpeg = known.map(|f| { let (w, h) = f.jpeg_size(); format!("{w}×{h}") });
        Some(json!({ "id": id, "angles": angles, "leds": leds, "jpeg": jpeg,
            "support": format!("{:?}", FanType::support(id)).to_lowercase() }))
    }).collect::<Vec<_>>().into()
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    fn lit(text: &str) -> (usize, usize) {
        let img = text::render(text, [255, 255, 255], [0, 0, 0], 0.0);
        let src = img.pixels().filter(|p| p[0] > 128).count();
        let jpeg = transcode::encode(&img, &FanType::P56, &Placement::default()).jpeg;
        let round = transcode::preview_from_jpeg(&jpeg, &FanType::P56, 360).unwrap();
        (src, round.pixels().filter(|p| p[0] > 128).count())
    }

    #[test]
    fn text_survives_the_round_trip() {
        for t in ["HALLO", "0,23 kWh", "0,23 €/kWh"] {
            let (src, out) = lit(t);
            let frac = out as f64 / (360.0 * 360.0);
            assert!(src > 2000 && (0.02..0.5).contains(&frac), "{t}: {src} lit pixels in, {:.1}% of the preview lit", frac * 100.0);
        }
    }
}
