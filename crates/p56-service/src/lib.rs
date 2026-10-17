//! Command layer shared by P56 Studio (Tauri, desktop + Android) and the Home Assistant
//! add-on server. Every UI action is `Service::call(cmd, json_args)`; events go out through
//! the `Emit` callback (`type`, `data`).
//!
//! Media never comes in as a file path from the UI: the frontend reads the file (which also
//! works for Android `content://` picks) or decodes video frames itself and sends the bytes.

pub mod text;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
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
    /// Sound from the first frame on: 16-bit mono PCM at 16 kHz (only for videos).
    audio: Option<Vec<u8>>,
}

impl Media {
    fn bytes(&self) -> usize {
        self.frames.iter().map(|(b, _)| b.len()).sum::<usize>() + self.audio.as_ref().map_or(0, Vec::len)
    }
}

/// Memory all loaded media may use together; the oldest items are dropped first.
const MAX_MEDIA_BYTES: usize = 512 * 1024 * 1024;

/// Drop the oldest media (never `keep`) until all of it fits in [`MAX_MEDIA_BYTES`] plus
/// `extra`. Returns whether it fits.
fn make_room(m: &mut HashMap<u32, Media>, keep: u32, extra: usize) -> bool {
    loop {
        let total: usize = m.values().map(Media::bytes).sum::<usize>() + extra;
        if total <= MAX_MEDIA_BYTES {
            return true;
        }
        let Some(oldest) = m.keys().copied().filter(|&k| k != keep).min() else { return false };
        m.remove(&oldest);
    }
}

#[derive(Default)]
struct LiveStats {
    n: u64,
    since: Option<Instant>,
}

/// Where the link listens and what it targets when the UI does not say otherwise.
#[derive(Debug, Clone, Default)]
pub struct Defaults {
    pub ip: Option<String>,
    pub targets: Vec<String>,
    pub port: Option<u16>,
    /// Folders `show_text` / `show_image` may read a `path` from. Empty = `path` is refused.
    pub media_dirs: Vec<std::path::PathBuf>,
    /// Only fans from these addresses may connect; empty = any.
    pub allow_fans: Vec<std::net::IpAddr>,
    /// Where previews of uploaded files are kept across restarts; `None` = in memory only.
    pub thumb_dir: Option<std::path::PathBuf>,
}

/// A temporary file (`show_seconds` > 0) waiting for its auto-delete timer, per (fan, file name).
/// Every upload of that name replaces or removes the entry, so an older timer sees that its
/// generation is no longer current and leaves the file alone.
#[derive(Default)]
struct NameState {
    /// From a service-wide counter, so a generation is never reused.
    generation: u64,
    /// File to switch back to when the temporary file is removed.
    restore: Option<String>,
    /// The fan's repeat mode before (true = the whole playlist): while a temporary file shows the
    /// fan repeats only that one, so it does not move on to the next file.
    repeat_all: Option<bool>,
}

/// Longest animation uploaded: 60 s at the fan's 20 fps.
const MAX_UPLOAD_FRAMES: usize = 1200;

/// The fan plays uploaded files at a fixed 20 fps: pick the source frame on screen at every
/// 50 ms (see [`transcode::fan_rate_indices`]), so animations keep their speed and their sound.
fn at_fan_rate<T: Clone>(frames: Vec<(T, u32)>, max: usize) -> Vec<T> {
    let delays: Vec<u32> = frames.iter().map(|(_, ms)| *ms).collect();
    transcode::fan_rate_indices(&delays, max).into_iter().map(|i| frames[i].0.clone()).collect()
}

/// " (not confirmed by the fan)" when the fan did not acknowledge or list an upload.
/// The newest picture of a cast source in the fan's layout, with its JPEG quality.
type Newest = Arc<Mutex<Option<(Vec<u8>, u8)>>>;

/// Whether fan `id` plays slot `slot` now (as it last reported).
fn plays(l: &Link, id: u32, slot: u8) -> bool {
    l.device(id).and_then(|d| d.status).is_some_and(|s| s.play_index == slot)
}

/// Show `slot` and wait until the fan reports that it plays it: right after an upload the fan
/// switches itself off and on and starts at the first file, and can miss a select sent then.
/// The select is sent again every 1.5 s; true once the fan plays the slot (at most 5 s).
fn select_and_confirm(l: &Link, id: u32, slot: u8) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut sent: Option<Instant> = None;
    loop {
        if sent.is_none_or(|t| t.elapsed() >= Duration::from_millis(1500)) {
            let _ = l.select(id, slot);
            sent = Some(Instant::now());
        }
        thread::sleep(Duration::from_millis(300));
        let _ = l.refresh(id);
        thread::sleep(Duration::from_millis(200));
        if plays(l, id, slot) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
    }
}

/// Delete the file called `name` (`<stem>.mp4`) from fan `id`, if there is one, and wait until the
/// fan no longer lists it, so a new file can take that name.
fn remove_named(l: &Link, id: u32, name: &str) {
    if let Some(old) = l.device(id).and_then(|d| d.files.iter().find(|(_, n)| *n == name).map(|(s, _)| *s)) {
        let _ = l.delete(id, old);
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline && l.device(id).is_some_and(|d| d.files.iter().any(|(_, n)| *n == name)) {
            let _ = l.refresh(id);
            thread::sleep(Duration::from_millis(250));
        }
    }
}

/// The windows that can be cast instead of the whole screen: visible, with a title, not tiny.
fn desktop_windows() -> Value {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let mut out = Vec::new();
        for w in xcap::Window::all().unwrap_or_default() {
            let (Ok(id), Ok(title)) = (w.id(), w.title()) else { continue };
            let big = w.width().unwrap_or(0) >= 120 && w.height().unwrap_or(0) >= 80;
            if title.trim().is_empty() || !big || w.is_minimized().unwrap_or(true) {
                continue;
            }
            out.push(json!({ "id": id, "title": title, "app": w.app_name().unwrap_or_default() }));
        }
        Value::Array(out)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        json!([])
    }
}

/// Turn on the fans in `ids` that are off, and resume those that are paused: a fan that is off
/// shows nothing that is cast to it, and a P56 with an empty playlist pauses itself.
fn wake(l: &Link, ids: &[u32]) {
    for &id in ids {
        let Some(s) = l.device(id).and_then(|d| d.status) else { continue };
        if !s.power {
            let _ = l.on(id);
        }
        if !s.playing {
            let _ = l.play(id);
        }
    }
}

fn unconfirmed(o: p56_core::net::UploadOutcome) -> &'static str {
    // a real P56 never acknowledges the data itself: the playlist is the proof
    if o.listed {
        ""
    } else {
        " (sent, but not confirmed by the fan)"
    }
}

pub struct Service {
    link: Mutex<Option<Link>>,
    /// Fan type chosen by the user; 0 = follow what the fan reports.
    fan_type: Mutex<u8>,
    /// The current cast session. Every cast (live, or a timed one from `show`) takes a new number;
    /// a loop only sends while its number is current, so starting or stopping one ends the other.
    cast_session: Arc<AtomicU64>,
    /// Counts on/off commands given by hand: switching a fan off again after a message gives
    /// up when one comes in between.
    by_hand: AtomicU64,
    media: Mutex<HashMap<u32, Media>>,
    next_media: AtomicU32,
    live: Mutex<LiveStats>,
    last_activity: Mutex<Option<std::time::SystemTime>>,
    /// What each fan (by MAC, and by id) was last sent by this server.
    shown: Mutex<HashMap<String, Shown>>,
    /// Per (fan MAC or id, file name). Also held while an auto-delete timer removes its file,
    /// so a new upload of the same name cannot interleave with that delete.
    names: Mutex<HashMap<(String, String), NameState>>,
    /// Temporary messages and their priorities.
    messages: Mutex<Messages>,
    /// Round preview (JPEG) of every file this app uploaded, per (fan MAC or id, file name): the
    /// fan cannot send its files back, so this is how the app shows what a file looks like.
    thumbs: Mutex<HashMap<(String, String), Vec<u8>>>,
    next_generation: AtomicU64,
    /// Canonicalized `Defaults::media_dirs`.
    media_dirs: Vec<std::path::PathBuf>,
    emit: Emit,
    defaults: Defaults,
}

#[derive(Serialize, Clone)]
pub struct StatusDto {
    pub power: bool,
    pub playing: bool,
    pub repeat_all: bool,
    pub breath: bool,
    pub autostart: bool,
    pub play_index: u8,
    pub brightness: u8,
    pub color_mode: u8,
    pub angle: i32,
    pub fan_type: u8,
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
    /// Speaker volume (0–10), once the fan reported it.
    pub volume: Option<u8>,
    /// Model name of the fan type (e.g. "P56").
    pub model: Option<String>,
    pub support: String,
    /// The picture this server last sent the fan, once it sent one.
    pub shown: Option<ShownDto>,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct PlacementDto {
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub mirror: bool,
    /// Clockwise, in degrees.
    #[serde(default)]
    pub rotate: u16,
}
fn one() -> f64 {
    1.0
}
impl Default for PlacementDto {
    fn default() -> Self {
        PlacementDto { x: 0.0, y: 0.0, scale: 1.0, mirror: false, rotate: 0 }
    }
}
impl From<PlacementDto> for Placement {
    fn from(p: PlacementDto) -> Self {
        Placement { x_offset: p.x, y_offset: p.y, scale: p.scale, mirror: p.mirror, rotation: p.rotate }
    }
}

/// What to show for `show_text` / `show_image`.
#[derive(Deserialize, Debug, Default, Clone)]
pub struct ShowRequest {
    /// Fan id; empty = all connected fans.
    pub id: Option<u32>,
    /// Fan MAC address (stable across reconnects), instead of id.
    #[serde(default)]
    pub mac: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    /// Image as base64 or data URL.
    #[serde(default)]
    pub data: Option<String>,
    /// Image file path on the server; only inside `Defaults::media_dirs` (add-on: /media, /share).
    #[serde(default, alias = "image")]
    pub path: Option<String>,
    #[serde(default, alias = "fg_color")]
    pub color: Option<Value>,
    #[serde(default, alias = "bg_color")]
    pub background: Option<Value>,
    #[serde(default)]
    pub font_size: Option<f32>,
    /// Remove again after this many seconds (0 = keep).
    #[serde(default)]
    pub show_seconds: Option<f64>,
    /// "upload" (store in the playlist, default) or "cast" (live, nothing stored).
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default, alias = "filename")]
    pub name: Option<String>,
    #[serde(default)]
    pub placement: Option<PlacementDto>,
    /// Icon above the text (or alone): a Material Design Icon such as "mdi:flash".
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub icon_color: Option<Value>,
    /// Ready-made look: alert, warning, info, success, price or clock.
    #[serde(default)]
    pub style: Option<String>,
    /// Text colour from a value: green at min, yellow halfway, red at max.
    #[serde(default)]
    pub color_scale: Option<ColorScaleDto>,
    /// Accent ring along the rim: true (text colour), false, or a colour.
    #[serde(default)]
    pub ring: Option<Value>,
    /// Several pictures (base64) shown as an animation.
    #[serde(default)]
    pub frames: Option<Vec<String>>,
    /// Time per frame when casting an animation (ms, default 100).
    #[serde(default)]
    pub frame_ms: Option<u64>,
    /// At most this many frames of a GIF or of `frames` (default 200).
    #[serde(default)]
    pub max_frames: Option<usize>,
    /// Moving text: "scroll" (a ticker across the middle) or "orbit" (round along the rim).
    #[serde(default)]
    pub motion: Option<String>,
    /// Speed of the motion, 1 = normal (a ticker: 14 px per frame; an orbit: one turn in 4 s).
    #[serde(default)]
    pub speed: Option<f32>,
    /// Move the other way: left to right / anticlockwise.
    #[serde(default)]
    pub reverse: Option<bool>,
    /// A picture (base64) behind `text` / `icon`, e.g. a background an AI made; the text is drawn
    /// over it with a dark edge so it stays readable.
    #[serde(default)]
    pub backdrop: Option<String>,
    /// For a temporary message (`show_seconds`): a lower one waits until the one showing is done,
    /// a higher one interrupts it (which comes back afterwards for the time it had left), an
    /// equal one replaces it. Default 0.
    #[serde(default)]
    pub priority: Option<i32>,
    /// A ring along the rim filled like a meter: the share of `color_scale`'s value between its
    /// min and max, clockwise from the top, in the text's colour.
    #[serde(default)]
    pub gauge: Option<bool>,
}

/// Temporary messages per fan: which one shows, and which wait for it.
#[derive(Default)]
struct Messages {
    next: u64,
    /// By fan key: the message showing there.
    showing: HashMap<String, Showing>,
    waiting: Vec<Waiting>,
    /// Fans (by key) that were off when a message came: they go off again after the last one.
    woke: Vec<String>,
}

struct Showing {
    message: u64,
    priority: i32,
    req: ShowRequest,
    /// When it ends (generous until it really shows: an upload takes a while).
    ends: Instant,
    /// It really shows (so `ends` is exact).
    shown: bool,
}

/// The picture a fan was last sent, and what it was sent as.
struct Shown {
    /// The fan it was sent to.
    id: u32,
    /// Round preview (PNG), and the same faded for when it is no longer on the fan.
    png: Vec<u8>,
    ghost: Vec<u8>,
    /// Unix time it was sent.
    at: u64,
    what: ShownAs,
}

enum ShownAs {
    /// A stored file: on the fan while the fan plays it.
    File(String),
    /// A timed cast: on the fan while that cast session runs, until `until`.
    Cast { session: u64, until: Instant },
}

/// Whether the picture a fan was last sent is on it now.
#[derive(Serialize, Clone, Copy, Debug)]
pub struct ShownDto {
    /// Unix time it was sent.
    pub at: u64,
    pub live: bool,
}

struct Waiting {
    priority: i32,
    req: ShowRequest,
    since: Instant,
}

/// A waiting message that could not be shown within this time is dropped.
const MESSAGE_WAIT: Duration = Duration::from_secs(600);
/// An interrupted message comes back only for at least this long.
const MESSAGE_RESUME_MIN: f64 = 2.0;
/// A fan that was off first spins up this long on black before something is shown: the first
/// moments after switching on the picture is not steady yet.
const WARM_UP: Duration = Duration::from_millis(2500);
/// Keep a live cast comfortably inside the real P56's roughly 130 ms fallback window. Slow
/// animations repeat their current picture at this rate; their visual frame time stays intact.
const CAST_KEEPALIVE: Duration = Duration::from_millis(100);

fn timed_cast_frame_duration(frame_ms: u64) -> Duration {
    Duration::from_millis(frame_ms.clamp(20, 2000))
}

fn timed_cast_send_period(frames: usize, frame_ms: u64) -> Duration {
    if frames > 1 {
        timed_cast_frame_duration(frame_ms).min(CAST_KEEPALIVE)
    } else {
        CAST_KEEPALIVE
    }
}

fn timed_cast_frame(elapsed: Duration, frames: usize, frame_ms: u64) -> usize {
    if frames <= 1 {
        return 0;
    }
    ((elapsed.as_millis() / timed_cast_frame_duration(frame_ms).as_millis()) % frames as u128) as usize
}

#[derive(Deserialize, Debug, Clone)]
pub struct ColorScaleDto {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    /// Colours from min to max (default green, yellow, red).
    #[serde(default)]
    pub colors: Option<Vec<Value>>,
    /// Red at min and green at max instead.
    #[serde(default)]
    pub invert: bool,
}

/// Defaults of a ready-made style; explicit request fields win.
struct Style {
    fg: [u8; 3],
    bg: [u8; 3],
    icon: Option<&'static str>,
    ring: Option<[u8; 3]>,
    ticks: bool,
    ring_follows_text: bool,
}

fn style(name: &str) -> Result<Style, String> {
    let s = |fg, bg, icon, ring| Style { fg, bg, icon, ring, ticks: false, ring_follows_text: false };
    Ok(match name.trim().to_ascii_lowercase().as_str() {
        "alert" => s([255, 255, 255], [40, 0, 0], Some("alert"), Some([255, 59, 48])),
        "warning" => s([255, 214, 10], [30, 18, 0], Some("alert-outline"), Some([255, 149, 0])),
        "info" => s([255, 255, 255], [0, 12, 30], Some("information-outline"), Some([10, 132, 255])),
        "success" => s([255, 255, 255], [0, 24, 8], Some("check-circle-outline"), Some([48, 209, 88])),
        "price" => Style { ring_follows_text: true, ..s([255, 214, 10], [0, 0, 0], Some("flash"), None) },
        "clock" => Style { ticks: true, ..s([255, 255, 255], [0, 0, 0], None, Some([0, 200, 255])) },
        other => return Err(format!("unknown style {other}: use alert, warning, info, success, price or clock")),
    })
}

/// The card for a text request (text and/or icon), with style, colour scale and ring applied.
fn card_for(r: &ShowRequest) -> Result<text::Card, String> {
    let st = r.style.as_deref().filter(|s| !s.trim().is_empty()).map(style).transpose()?;
    let scaled = r.color_scale.as_ref().map(|cs| {
        let mut stops: Vec<[u8; 3]> = cs
            .colors
            .as_ref()
            .map(|v| v.iter().map(|c| color(&Some(c.clone()), [255, 255, 255])).collect())
            .filter(|v: &Vec<[u8; 3]>| !v.is_empty())
            .unwrap_or_else(|| text::SCALE_GREEN_RED.to_vec());
        if cs.invert {
            stops.reverse();
        }
        text::color_scale(cs.value, cs.min, cs.max, &stops)
    });
    let fg = scaled.unwrap_or_else(|| color(&r.color, st.as_ref().map(|s| s.fg).unwrap_or([255, 255, 255])));
    // a meter ring: how far the value is between min and max
    let gauge = match (r.gauge, &r.color_scale) {
        (Some(true), Some(cs)) if cs.max != cs.min => Some(((cs.value - cs.min) / (cs.max - cs.min)).clamp(0.0, 1.0) as f32),
        (Some(true), _) => return Err("a gauge needs value, value_min and value_max (different)".into()),
        _ => None,
    };
    let bg = color(&r.background, st.as_ref().map(|s| s.bg).unwrap_or([0, 0, 0]));
    let icon_name = match r.icon.as_deref().map(str::trim) {
        Some("") | Some("none") => None,
        Some(n) => Some(n.to_string()),
        None => st.as_ref().and_then(|s| s.icon).map(String::from),
    };
    let icon = match icon_name {
        Some(n) => Some(text::icon_char(&n).ok_or_else(|| format!("unknown icon {n}: use a Material Design Icon such as mdi:flash"))?),
        None => None,
    };
    let ring = match &r.ring {
        Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(fg),
        Some(v @ Value::String(_)) | Some(v @ Value::Array(_)) => Some(color(&Some(v.clone()), fg)),
        _ => match &st {
            Some(s) if s.ring_follows_text => Some(fg),
            Some(s) => s.ring.map(|c| if scaled.is_some() { fg } else { c }),
            None => None,
        },
    };
    Ok(text::Card {
        text: r.text.clone().unwrap_or_default(),
        fg,
        bg,
        max_size: r.font_size.unwrap_or(0.0),
        icon,
        icon_color: color(&r.icon_color, if scaled.is_some() { fg } else { st.as_ref().and_then(|s| s.ring).unwrap_or(fg) }),
        ring: if gauge.is_some() { ring.or(Some(fg)) } else { ring },
        ticks: st.as_ref().is_some_and(|s| s.ticks),
        gauge,
    })
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn png_bytes(img: &image::RgbImage) -> Vec<u8> {
    let mut v = Vec::new();
    image::DynamicImage::ImageRgb8(img.clone()).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).ok();
    v
}

/// A picture that is no longer on the fan: mostly grey and see-through, so it is clearly not live.
fn ghost_png(img: &image::RgbImage) -> Vec<u8> {
    let ghost = image::RgbaImage::from_fn(img.width(), img.height(), |x, y| {
        let [r, g, b] = img.get_pixel(x, y).0;
        let grey = (0.3 * r as f32 + 0.59 * g as f32 + 0.11 * b as f32) as u16;
        let mix = |c: u8| ((c as u16 + 3 * grey) / 4) as u8;
        image::Rgba([mix(r), mix(g), mix(b), 90])
    });
    let mut v = Vec::new();
    image::DynamicImage::ImageRgba8(ghost).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).ok();
    v
}

pub fn b64(s: &str) -> Result<Vec<u8>, String> {
    let s = s.split_once("base64,").map(|(_, d)| d).unwrap_or(s);
    base64::engine::general_purpose::STANDARD.decode(s.trim()).map_err(|e| format!("invalid data: {e}"))
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn decode_source(bytes: &[u8]) -> Result<image::RgbImage, String> {
    Ok(transcode::fit(transcode::decode_rgb(bytes).map_err(err)?, MAX_SIDE))
}

/// Read an argument by snake_case or camelCase name.
fn arg<T: serde::de::DeserializeOwned>(a: &Value, key: &str) -> Option<T> {
    let camel: String = key.split('_').enumerate().map(|(i, p)| if i == 0 { p.to_string() } else { p[..1].to_uppercase() + &p[1..] }).collect();
    a.get(key).or_else(|| a.get(&camel)).filter(|v| !v.is_null()).and_then(|v| serde_json::from_value(v.clone()).ok())
}
fn need<T: serde::de::DeserializeOwned>(a: &Value, key: &str) -> Result<T, String> {
    arg(a, key).ok_or_else(|| format!("argument '{key}' is missing or invalid"))
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
        let media_dirs = defaults
            .media_dirs
            .iter()
            .filter_map(|d| match std::fs::canonicalize(d) {
                Ok(c) if c.is_dir() => Some(c),
                Ok(_) => {
                    log::warn!("media folder {} is not a folder; ignored", d.display());
                    None
                }
                Err(e) => {
                    log::warn!("media folder {}: {e}; ignored", d.display());
                    None
                }
            })
            .collect();
        Arc::new(Service {
            link: Mutex::new(None),
            fan_type: Mutex::new(0),
            cast_session: Arc::new(AtomicU64::new(0)),
            by_hand: AtomicU64::new(0),
            media: Mutex::new(HashMap::new()),
            next_media: AtomicU32::new(0),
            live: Mutex::new(LiveStats::default()),
            last_activity: Mutex::new(None),
            shown: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
            messages: Mutex::new(Messages::default()),
            thumbs: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(0),
            media_dirs,
            emit,
            defaults,
        })
    }

    /// Check that `path` (a file the request wants read from the server) lies inside one of the
    /// media folders, after resolving `..` and symlinks. Returns the resolved path to read.
    pub fn media_path(&self, path: &str) -> Result<std::path::PathBuf, String> {
        if self.media_dirs.is_empty() {
            return Err("reading server files is disabled; use data or frames".into());
        }
        let allowed = || self.media_dirs.iter().map(|d| d.display().to_string().trim_start_matches(r"\\?\").to_string()).collect::<Vec<_>>().join(", ");
        match std::fs::canonicalize(path) {
            Ok(c) if self.media_dirs.iter().any(|d| c.starts_with(d)) && c.is_file() => Ok(c),
            _ => Err(format!("path {path} is not allowed: it must be an existing file inside {}", allowed())),
        }
    }

    /// Stable key for a fan in per-fan bookkeeping: its MAC, else its connection id.
    fn fan_key(l: &Link, id: u32) -> String {
        l.device(id).and_then(|d| d.info).map(|i| i.mac.to_ascii_lowercase()).filter(|m| !m.is_empty()).unwrap_or_else(|| format!("#{id}"))
    }

    /// A new upload of `name` to fan `id`: makes every pending auto-delete timer for it stale.
    /// For a temporary upload, `restore` is the file its own timer switches back to; when that
    /// is the file itself (shown again before its timer ran), the earlier target is kept.
    /// Returns the key, the generation for the new timer and the restore target.
    fn new_upload(&self, l: &Link, id: u32, name: &str, temporary: bool) -> ((String, String), u64, Option<String>) {
        let key = (Self::fan_key(l, id), name.to_string());
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let mut m = self.names.lock().unwrap();
        if !temporary {
            m.remove(&key);
            return (key, generation, None);
        }
        // what plays now, by name (a slot number shifts when files are deleted), and the repeat
        // mode: read under the lock, as an expiring message may still be restoring them
        let restore = l.device(id).and_then(|d| {
            let s = d.status?.play_index;
            d.files.iter().find(|(slot, _)| *slot == s).map(|(_, n)| n.clone())
        });
        let repeat_all = l.device(id).and_then(|d| d.status).map(|s| s.repeat_all);
        // what plays is itself a temporary file: go back to what that one would go back to, and
        // to the repeat mode from before it
        let (restore, repeat_all) = match restore {
            Some(r) => match m.get(&(key.0.clone(), r.clone())) {
                Some(other) => (other.restore.clone(), other.repeat_all),
                None => (Some(r), repeat_all),
            },
            None => (None, repeat_all),
        };
        let st = m.entry(key.clone()).or_default();
        st.generation = generation;
        st.restore = if restore.as_deref() == Some(name) { st.restore.take() } else { restore };
        st.repeat_all = repeat_all;
        let restore = st.restore.clone();
        (key, generation, restore)
    }

    /// Auto-delete timer of a temporary file: remove the file with exactly that name, but only
    /// if no newer upload of that name happened, and switch back to the file that played
    /// before (by name) if the temporary file was still playing.
    fn expire(&self, l: &Link, key: &(String, String), generation: u64) {
        let mut names = self.names.lock().unwrap();
        let (restore, repeat_all) = match names.get(key) {
            Some(st) if st.generation == generation => (st.restore.clone(), st.repeat_all),
            _ => return, // replaced by a newer upload of this name
        };
        names.remove(key);
        // the fan may have reconnected with a new id
        let Some(id) = l.ids().into_iter().find(|i| Self::fan_key(l, *i) == key.0 || format!("#{i}") == key.0) else { return };
        let slot_of = |n: &str| l.device(id).and_then(|d| d.files.iter().find(|(_, f)| f == n).map(|(s, _)| *s));
        // removed by hand meanwhile: nothing to do (never fall back to the old slot number)
        let Some(slot) = slot_of(&key.1) else { return };
        let was_playing = l.device(id).and_then(|d| d.status).is_some_and(|s| s.play_index == slot);
        if let Err(e) = l.delete(id, slot) {
            log::warn!("removing {} from fan #{id}: {e}", key.1);
            return;
        }
        // keep the lock until the playlist shows the file gone and the fan is back on what it
        // played, so a new upload cannot pick up the stale slot or the message's repeat mode
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline && slot_of(&key.1).is_some() {
            let _ = l.refresh(id);
            thread::sleep(Duration::from_millis(250));
        }
        if was_playing {
            if let Some(back) = restore.as_deref().and_then(slot_of) {
                let _ = l.select(id, back);
            }
            // the repeat mode from before the message (only while it still showed: a newer
            // message keeps its own)
            if repeat_all == Some(true) && l.repeat(id, true).is_ok() {
                // until the fan reports it, so a message right after this one sees the real mode
                let deadline = Instant::now() + Duration::from_secs(3);
                while Instant::now() < deadline && !l.device(id).and_then(|d| d.status).is_some_and(|s| s.repeat_all) {
                    let _ = l.refresh(id);
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
        // only now may a new message for this fan start: it reads the restored mode
        drop(names);
    }

    pub fn link(&self) -> Result<Link, String> {
        self.link.lock().unwrap().clone().ok_or_else(|| "link not started".into())
    }

    fn emit(&self, t: &str, data: Value) {
        (self.emit)(t, data);
    }

    /// Remember what fan id shows now (as a round preview), for dashboards.
    fn remember(&self, ids: &[u32], jpeg: &[u8], ft: &FanType, what: impl Fn() -> ShownAs) {
        let Ok(l) = self.link() else { return };
        let Ok(round) = transcode::preview_from_jpeg(jpeg, ft, 360) else { return };
        let (png, ghost) = (png_bytes(&round), ghost_png(&round));
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let mut m = self.shown.lock().unwrap();
        for &id in ids {
            let shown = || Shown { id, png: png.clone(), ghost: ghost.clone(), at, what: what() };
            if let Some(mac) = l.device(id).and_then(|d| d.info).map(|i| i.mac).filter(|m| !m.is_empty()) {
                m.insert(mac, shown());
            }
            m.insert(id.to_string(), shown());
        }
        drop(m);
        self.emit("shown", json!({ "ids": ids }));
    }

    /// The file a preview of (fan key, file name) is kept in, when previews are kept on disk.
    fn thumb_path(&self, key: &str, name: &str) -> Option<std::path::PathBuf> {
        let dir: String = key.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let file: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
        Some(self.defaults.thumb_dir.as_ref()?.join(dir).join(format!("{file}.jpg")))
    }

    /// Keep a preview of file `name` on fan `id`, made from its first frame (fan layout).
    fn keep_thumb(&self, l: &Link, id: u32, name: &str, jpeg: &[u8], ft: &FanType) {
        let Ok(round) = transcode::preview_from_jpeg(jpeg, ft, 240) else { return };
        let thumb = transcode::jpeg(&round, 80);
        let key = Self::fan_key(l, id);
        if let Some(path) = self.thumb_path(&key, name) {
            let saved = path.parent().map(std::fs::create_dir_all).transpose().and_then(|_| std::fs::write(&path, &thumb));
            if let Err(e) = saved {
                log::warn!("preview {}: {e}", path.display());
            }
        }
        self.thumbs.lock().unwrap().insert((key, name.to_string()), thumb);
    }

    /// Give the preview of `old` on fan `id` to its new name.
    fn rename_thumb(&self, l: &Link, id: u32, old: &str, new: &str) {
        let key = Self::fan_key(l, id);
        let thumb = self.thumbs.lock().unwrap().remove(&(key.clone(), old.to_string()));
        if let (Some(from), Some(to)) = (self.thumb_path(&key, old), self.thumb_path(&key, new)) {
            let _ = std::fs::rename(from, to);
        }
        if let Some(t) = thumb {
            self.thumbs.lock().unwrap().insert((key, new.to_string()), t);
        }
    }

    /// Previews (data URLs) of the files on fan `id` that this app uploaded, by file name.
    fn thumbs(&self, id: u32) -> Result<Value, String> {
        let l = self.link()?;
        let d = l.device(id).ok_or("no such fan")?;
        let key = Self::fan_key(&l, id);
        let mut out = serde_json::Map::new();
        let mut m = self.thumbs.lock().unwrap();
        for (_, name) in &d.files {
            let k = (key.clone(), name.clone());
            if !m.contains_key(&k) {
                if let Some(b) = self.thumb_path(&key, name).and_then(|p| std::fs::read(p).ok()) {
                    m.insert(k.clone(), b);
                }
            }
            if let Some(b) = m.get(&k) {
                out.insert(name.clone(), Value::from(data_url("image/jpeg", b)));
            }
        }
        Ok(Value::Object(out))
    }

    /// Round preview (PNG) of what the fan with this MAC (or id) was last sent by this server, and
    /// whether it is on the fan now; faded when it is not.
    pub fn shown(&self, mac: &str) -> Option<(Vec<u8>, bool)> {
        let l = self.link().ok();
        let m = self.shown.lock().unwrap();
        let s = m.get(mac).or_else(|| m.get(&mac.to_ascii_uppercase())).or_else(|| m.get(&mac.to_ascii_lowercase()))?;
        let live = l.is_some_and(|l| self.is_live(&l, s));
        Some((if live { s.png.clone() } else { s.ghost.clone() }, live))
    }

    /// Whether a picture this server sent is on the fan now: the fan is on and plays that file,
    /// or the cast that sent it still runs.
    fn is_live(&self, l: &Link, s: &Shown) -> bool {
        let Some(d) = l.device(s.id) else { return false };
        let Some(st) = d.status.filter(|st| st.power) else { return false };
        match &s.what {
            ShownAs::File(name) => d.files.iter().any(|(slot, n)| *slot == st.play_index && n == name),
            ShownAs::Cast { session, until } => self.cast_session.load(Ordering::SeqCst) == *session && Instant::now() < *until,
        }
    }

    /// Dispatch one command from the UI or the HTTP API.
    pub fn call(self: &Arc<Self>, cmd: &str, a: &Value) -> Result<Value, String> {
        match cmd {
            "start_link" => self.start_link(arg(a, "ip"), arg(a, "targets"), arg(a, "port")).map(Value::from),
            "stop_link" => {
                self.stop_link();
                Ok(Value::Null)
            }
            "link_info" => self.link_info(),
            "devices" => Ok(serde_json::to_value(self.devices()?).unwrap()),
            "set_fan_type" => {
                *self.fan_type.lock().unwrap() = need::<u8>(a, "fan_type")?;
                Ok(Value::Null)
            }
            "fan_types" => Ok(fan_types()),
            "thumbs" => self.thumbs(need(a, "id")?),
            "control" => self.control(need(a, "id")?, &need::<String>(a, "action")?).map(|_| Value::Null),
            "set_value" => self.set_value(need(a, "id")?, &need::<String>(a, "what")?, need(a, "value")?).map(|_| Value::Null),
            "set_text" => self.set_text(need(a, "id")?, &need::<String>(a, "what")?, need(a, "a")?, arg(a, "b"), arg(a, "slot")).map(|_| Value::Null),
            "media_load" => self.media_load(need(a, "name")?, &need::<String>(a, "data")?),
            "media_text" => self.media_text(a),
            "media_begin" => Ok(self.media_begin(need(a, "name")?).into()),
            "media_push" => self.media_push(need(a, "media")?, &need::<String>(a, "data")?, arg(a, "delay").unwrap_or(0)).map(Value::from),
            "media_audio" => self.media_audio(need(a, "media")?, &need::<String>(a, "data")?).map(Value::from),
            "media_drop" => {
                self.media.lock().unwrap().remove(&need::<u32>(a, "media")?);
                Ok(Value::Null)
            }
            "preview" => self.preview(arg(a, "media").unwrap_or(0), arg(a, "frame"), arg(a, "placement").unwrap_or_default(), arg(a, "id")),
            "upload" => self.upload(need(a, "args")?),
            "cast_start" => self.cast_start(need(a, "args")?).map(|_| Value::Null),
            "cast_frame" => self.cast_frame(&need::<String>(a, "data")?, arg(a, "placement").unwrap_or_default()).map(Value::from),
            "cast_stop" => {
                self.cast_stop();
                Ok(Value::Null)
            }
            "show_text" | "show_image" => self.show(serde_json::from_value(a.clone()).map_err(err)?),
            "windows" => Ok(desktop_windows()),
            "platform" => Ok(json!({ "os": std::env::consts::OS, "screen": cfg!(any(target_os = "windows", target_os = "macos")),
                "version": env!("CARGO_PKG_VERSION") })),
            other => Err(format!("unknown command {other}")),
        }
    }

    // ---- link & devices ----------------------------------------------------

    pub fn start_link(self: &Arc<Self>, ip: Option<String>, targets: Option<Vec<String>>, port: Option<u16>) -> Result<String, String> {
        let mut g = self.link.lock().unwrap();
        if let Some(l) = g.as_ref() {
            return Ok(l.local_ip.to_string());
        }
        let ip = ip.filter(|s| !s.trim().is_empty()).or(self.defaults.ip.clone());
        let mut tg = targets.unwrap_or_default();
        tg.extend(self.defaults.targets.iter().cloned());
        let cfg = Config {
            local_ip: ip.and_then(|s| s.trim().parse().ok()),
            targets: tg
                .iter()
                .filter_map(|t| {
                    let t = t.trim();
                    t.parse().ok().or_else(|| format!("{t}:8988").parse().ok())
                })
                .collect(),
            tcp_port: port.or(self.defaults.port).unwrap_or(p56_core::protocol::TCP_PORT),
            allow: self.defaults.allow_fans.clone(),
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
                            Packet::Status(_) => "status",
                            Packet::MachineInfo(_) => "info",
                            Packet::FileList(_) => "files",
                            Packet::Error(_) => "error",
                            Packet::Bluetooth(_) => "list", // the 8F report carries the volume; Home Assistant refreshes on it
                            Packet::TransDone => "done",
                            _ => continue,
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
        self.cast_session.fetch_add(1, Ordering::SeqCst);
        if let Some(l) = self.link.lock().unwrap().take() {
            l.shutdown();
            thread::sleep(Duration::from_millis(400));
        }
    }

    fn link_info(&self) -> Result<Value, String> {
        let l = self.link()?;
        let ifs: Vec<String> = p56_core::net::interfaces().into_iter().map(|(ip, bc)| format!("{ip} (broadcast {bc})")).collect();
        Ok(json!({ "ip": l.advertised().to_string(), "port": l.tcp_port, "interfaces": ifs,
            "discovered": l.discovered().iter().map(|i| i.to_string()).collect::<Vec<_>>() }))
    }

    pub fn devices(&self) -> Result<Vec<DeviceDto>, String> {
        let l = self.link()?;
        Ok(l.devices()
            .into_iter()
            .map(|d| {
                let i = d.info.clone().unwrap_or_default();
                let status = d.status.as_ref().map(|s| StatusDto {
                    power: s.power,
                    playing: s.playing,
                    repeat_all: s.repeat_all,
                    breath: s.breath,
                    autostart: s.autostart,
                    play_index: s.play_index,
                    brightness: s.brightness,
                    color_mode: s.color_mode,
                    angle: s.angle as i32 - 180,
                    fan_type: s.fan_type,
                });
                let support = d.status.as_ref().map(|s| format!("{:?}", FanType::support(s.fan_type)).to_lowercase()).unwrap_or_default();
                let shown = {
                    let m = self.shown.lock().unwrap();
                    m.get(&i.mac).filter(|_| !i.mac.is_empty()).or_else(|| m.get(&d.id.to_string())).map(|s| ShownDto { at: s.at, live: self.is_live(&l, s) })
                };
                DeviceDto {
                    id: d.id,
                    ip: d.ip.map(|i| i.to_string()).unwrap_or_default(),
                    name: if i.name.is_empty() { format!("fan #{}", d.id) } else { i.name },
                    mac: i.mac,
                    hotspot: i.hotspot,
                    router: i.router,
                    status,
                    files: d.files,
                    last_error: d.last_error.map(|e: FanError| e.to_string()),
                    model: d.status.as_ref().and_then(|s| FanType::model(s.fan_type)).map(str::to_string),
                    volume: d.volume,
                    support,
                    shown,
                }
            })
            .collect())
    }

    /// Unix time of the last packet from any fan.
    pub fn last_activity(&self) -> Option<f64> {
        self.last_activity.lock().unwrap().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64())
    }

    /// Fan type for transcoding: the user's choice, else what fan `id` (or the first fan) reports, else P56.
    fn fan_type_for(&self, id: Option<u32>) -> Result<FanType, String> {
        let chosen = *self.fan_type.lock().unwrap();
        let t = if chosen != 0 {
            chosen
        } else {
            self.link
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|l| {
                    let devs = l.devices();
                    id.and_then(|i| devs.iter().find(|d| d.id == i).cloned()).or_else(|| devs.into_iter().next())
                })
                .and_then(|d| d.status)
                .map(|s| s.fan_type)
                .filter(|t| *t != 0)
                .unwrap_or(8)
        };
        FanType::usable(t)
    }

    pub fn control(self: &Arc<Self>, id: u32, action: &str) -> Result<(), String> {
        let l = self.link()?;
        if action == "on" || action == "off" {
            self.by_hand.fetch_add(1, Ordering::SeqCst);
        }
        match action {
            "on" => l.on(id),
            "off" => {
                // switched off by hand: a running cast (e.g. a message) stops too, or its
                // next session would switch the fan on again
                self.cast_session.fetch_add(1, Ordering::SeqCst);
                let sent = l.off(id);
                if sent.is_ok() {
                    // right after a cast the fan may still be in live mode and ignore it
                    let (me, l2, key, hand) = (self.clone(), l.clone(), Self::fan_key(&l, id), self.by_hand.load(Ordering::SeqCst));
                    thread::spawn(move || me.confirm_off(&l2, id, &key, hand));
                }
                sent
            }
            "play" => l.play(id),
            "pause" => l.pause(id),
            "refresh" => l.refresh(id),
            "repeat_all" => l.repeat(id, true),
            "repeat_one" => l.repeat(id, false),
            "breath_on" => l.breath(id, true),
            "breath_off" => l.breath(id, false),
            "autostart_on" => l.autostart(id, true),
            "autostart_off" => l.autostart(id, false),
            "color_true" => l.color_mode(id, ColorMode::True),
            "color_bright" => l.color_mode(id, ColorMode::Bright),
            "format" => l.format_card(id),
            "reset" => l.factory_reset(id),
            other => Err(p56_core::Error::Invalid(format!("unknown action {other}"))),
        }
        .map_err(err)
    }

    pub fn set_value(&self, id: u32, what: &str, value: i32) -> Result<(), String> {
        let l = self.link()?;
        match what {
            "brightness" => l.brightness(id, value.clamp(1, 16) as u8),
            "angle" => l.angle(id, (value.clamp(-180, 180) + 180) as u16),
            "volume" => l.volume(id, value.clamp(0, 10) as u8),
            "select" => l.select(id, value.clamp(1, 255) as u8),
            "delete" => l.delete(id, value.clamp(1, 255) as u8),
            other => Err(p56_core::Error::Invalid(format!("unknown setting {other}"))),
        }
        .map_err(err)
    }

    pub fn set_text(&self, id: u32, what: &str, a: String, b: Option<String>, slot: Option<u8>) -> Result<(), String> {
        let l = self.link()?;
        let b = b.unwrap_or_default();
        match what {
            "hotspot" if b.len() < 8 => return Err("password must be at least 8 characters".into()),
            "hotspot" | "router" | "name" if a.trim().is_empty() => return Err("name must not be empty".into()),
            _ => {}
        }
        match what {
            "rename" => {
                let slot = slot.unwrap_or(1);
                let old = l.device(id).and_then(|d| d.files.iter().find(|(s, _)| *s == slot).map(|(_, n)| n.clone()));
                let r = l.rename(id, slot, &a);
                if let (Ok(()), Some(old)) = (&r, old) {
                    self.rename_thumb(&l, id, &old, &p56_core::protocol::mp4_name(&a));
                }
                r
            }
            "name" => l.device_name(id, &a),
            "hotspot" => l.set_hotspot(id, &a, &b),
            "router" => l.set_router(id, &a, &b),
            other => Err(p56_core::Error::Invalid(format!("unknown field {other}"))),
        }
        .map_err(err)
    }

    // ---- media ---------------------------------------------------------------

    fn store_media(&self, name: String, frames: Vec<(Vec<u8>, u32)>) -> u32 {
        let id = self.next_media.fetch_add(1, Ordering::SeqCst) + 1;
        let mut m = self.media.lock().unwrap();
        if m.len() > 8 {
            let oldest = *m.keys().min().unwrap();
            m.remove(&oldest);
        }
        m.insert(id, Media { name, frames, audio: None });
        make_room(&mut m, id, 0);
        id
    }

    /// Load an image or GIF file (bytes as base64).
    pub fn media_load(&self, name: String, data: &str) -> Result<Value, String> {
        let bytes = b64(data)?;
        let fr = transcode::decode_frames(&bytes, 600).map_err(|e| format!("cannot read image: {e}"))?;
        let (w, h) = fr[0].0.dimensions();
        let frames =
            if fr.len() == 1 { vec![(bytes, 0)] } else { fr.into_iter().map(|(f, ms)| (transcode::jpeg(&transcode::fit(f, MAX_SIDE), 92), ms)).collect() };
        let n = frames.len();
        let id = self.store_media(name.clone(), frames);
        Ok(json!({ "id": id, "name": name, "frames": n, "width": w, "height": h }))
    }

    /// Pictures made on the server (e.g. by an AI) as a media item: one picture is a still,
    /// more are an animation with `ms` per frame.
    pub fn media_images(&self, name: String, images: Vec<image::RgbImage>, ms: u32) -> Result<Value, String> {
        let first = images.first().ok_or("no picture")?;
        let (w, h) = first.dimensions();
        let frames: Vec<(Vec<u8>, u32)> = if images.len() == 1 {
            vec![(png_bytes(first), 0)]
        } else {
            images.into_iter().map(|f| (transcode::jpeg(&transcode::fit(f, MAX_SIDE), 92), ms)).collect()
        };
        let n = frames.len();
        let id = self.store_media(name.clone(), frames);
        Ok(json!({ "id": id, "name": name, "frames": n, "width": w, "height": h }))
    }

    /// Render text as a media item.
    fn media_text(&self, a: &Value) -> Result<Value, String> {
        let t: String = need(a, "text")?;
        let (fg, bg) = (color(&arg(a, "color"), [255, 255, 255]), color(&arg(a, "background"), [0, 0, 0]));
        let name = arg::<String>(a, "name").unwrap_or_else(|| "text".into());
        let frames = match text::Motion::parse(&arg::<String>(a, "motion").unwrap_or_default())? {
            // moving text: frames of 50 ms (the fan's 20 fps), stored as JPEG like GIF frames
            Some(m) => text::render_motion(&t, m, fg, bg, arg(a, "speed").unwrap_or(1.0), arg(a, "reverse").unwrap_or(false))
                .iter()
                .map(|f| (transcode::jpeg(f, 92), p56_core::protocol::FRAME_MS))
                .collect(),
            None => vec![(png_bytes(&text::render(&t, fg, bg, arg(a, "font_size").unwrap_or(0.0))), 0)],
        };
        let n = frames.len();
        let id = self.store_media(name.clone(), frames);
        Ok(json!({ "id": id, "name": name, "frames": n, "width": text::SIDE, "height": text::SIDE }))
    }

    pub fn media_begin(&self, name: String) -> u32 {
        self.store_media(name, Vec::new())
    }

    fn media_push(&self, media: u32, data: &str, delay: u32) -> Result<usize, String> {
        let bytes = b64(data)?;
        let mut m = self.media.lock().unwrap();
        let md = m.get_mut(&media).ok_or("media no longer exists")?;
        if md.frames.len() >= 3000 {
            return Err("at most 3000 frames".into());
        }
        if !make_room(&mut m, media, bytes.len()) {
            return Err(format!("this media is too large (more than {} MB)", MAX_MEDIA_BYTES >> 20));
        }
        let md = m.get_mut(&media).ok_or("media no longer exists")?;
        md.frames.push((bytes, delay));
        Ok(md.frames.len())
    }

    /// Attach the sound of a video to a media item: base64 of 16-bit little-endian mono PCM at
    /// 16 kHz, starting where the first frame starts. Returns its length in milliseconds.
    fn media_audio(&self, media: u32, data: &str) -> Result<u64, String> {
        let mut pcm = b64(data)?;
        pcm.truncate(pcm.len() & !1);
        let mut m = self.media.lock().unwrap();
        if !make_room(&mut m, media, pcm.len()) {
            return Err(format!("this media is too large (more than {} MB)", MAX_MEDIA_BYTES >> 20));
        }
        let md = m.get_mut(&media).ok_or("media no longer exists")?;
        let ms = pcm.len() as u64 * 1000 / (2 * p56_core::video::AUDIO_RATE as u64);
        md.audio = Some(pcm);
        Ok(ms)
    }

    /// Preview of one frame of a media item (0 = test card) — no network.
    fn preview(&self, media: u32, frame: Option<usize>, placement: PlacementDto, id: Option<u32>) -> Result<Value, String> {
        let ft = self.fan_type_for(id)?;
        let (src, n) = if media == 0 {
            (None, 1)
        } else {
            let m = self.media.lock().unwrap();
            let md = m.get(&media).ok_or("media no longer exists")?;
            if md.frames.is_empty() {
                return Err("no frames yet".into());
            }
            (Some(md.frames[frame.unwrap_or(0).min(md.frames.len() - 1)].0.clone()), md.frames.len())
        };
        let img = match src {
            None => transcode::test_card(),
            Some(b) => decode_source(&b)?,
        };
        // what an upload would send (the cast preview comes from the cast loop itself)
        let f = transcode::encode_upload(&img, &ft, &placement.into());
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
        let with_sound: bool = arg(&a, "sound").unwrap_or(true);
        let mut audio: Option<Vec<u8>> = None;
        let (src, default_name): (Vec<(Vec<u8>, u32)>, String) = if media == 0 {
            (vec![], "testcard".into())
        } else {
            let m = self.media.lock().unwrap();
            let md = m.get(&media).ok_or("media no longer exists")?;
            audio = md.audio.clone().filter(|_| with_sound);
            let mut v = md.frames.clone();
            if max_frames > 0 {
                v.truncate(max_frames);
            }
            (v, md.name.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or(md.name.clone()))
        };
        let name = arg::<String>(&a, "name").filter(|n| !n.trim().is_empty()).unwrap_or(default_name);
        // "replace": a file with this name on the fan is deleted first (the user chose to overwrite it)
        if arg::<bool>(&a, "replace").unwrap_or(false) {
            remove_named(&l, id, &p56_core::protocol::mp4_name(&name));
        }
        let progress = |stage: &str, done: usize, total: usize| self.emit("progress", json!({ "stage": stage, "done": done, "total": total }));
        let frames: Vec<Vec<u8>> = if src.is_empty() {
            vec![transcode::encode_upload(&transcode::test_card(), &ft, &pl).jpeg]
        } else {
            let total = src.len();
            let encoded = src
                .iter()
                .enumerate()
                .map(|(i, (b, delay))| {
                    if i % 5 == 0 {
                        progress("transcoding", i, total);
                    }
                    decode_source(b).map(|img| (transcode::encode_upload(&img, &ft, &pl).jpeg, *delay))
                })
                .collect::<Result<Vec<_>, _>>()?;
            at_fan_rate(encoded, MAX_UPLOAD_FRAMES)
        };
        progress("sending", 0, frames.len());
        // a pending auto-delete of an earlier show_text/show_image with this name must not remove this file
        self.new_upload(&l, id, &p56_core::protocol::mp4_name(&name), false);
        let audio = audio.filter(|_| frames.len() > 1);
        let outcome = l.upload_with_audio(id, &frames, audio.as_deref(), &name, Some(slot)).map_err(err)?;
        self.remember(&[id], &frames[0], &ft, || ShownAs::File(p56_core::protocol::mp4_name(&name)));
        self.keep_thumb(&l, id, &p56_core::protocol::mp4_name(&name), &frames[0], &ft);
        progress("done", frames.len(), frames.len());
        Ok(Value::from(format!("{} ({} frame(s)){}", p56_core::protocol::mp4_name(&name), frames.len(), unconfirmed(outcome))))
    }

    // ---- live cast -------------------------------------------------------------

    fn emit_cast(&self, ft: &FanType, fps: f64, fans: usize, jpeg: &[u8], q: u8) {
        let preview = transcode::preview_from_jpeg(jpeg, ft, 320).map(|r| data_url("image/jpeg", &transcode::jpeg(&r, 85))).unwrap_or_default();
        self.emit("cast", json!({ "fps": (fps * 10.0).round() / 10.0, "fans": fans, "jpeg": jpeg.len(), "q": q, "preview": preview }));
    }

    /// Backend-driven cast: "test", "screen" (desktop) or "media:<id>".
    fn cast_start(self: &Arc<Self>, a: Value) -> Result<(), String> {
        let source: String = need(&a, "source")?;
        let fps: f64 = arg(&a, "fps").unwrap_or(25.0);
        let pl: Placement = arg::<PlacementDto>(&a, "placement").unwrap_or_default().into();
        let l = self.link()?;
        let ft = self.fan_type_for(None)?;
        let media: Option<Vec<(Vec<u8>, u32)>> = match source.strip_prefix("media:") {
            Some(id) => {
                let id: u32 = id.parse().map_err(err)?;
                Some(self.media.lock().unwrap().get(&id).ok_or("media no longer exists")?.frames.clone())
            }
            None => None,
        };
        // "window:<id>": one window instead of the whole screen (ids from the `windows` command)
        let window_id: Option<u32> = source.strip_prefix("window:").and_then(|w| w.parse().ok());
        let desktop = source == "screen" || window_id.is_some();
        if desktop && !cfg!(any(target_os = "windows", target_os = "macos")) {
            return Err("screen casting is not available on this platform; use the camera or a file".into());
        }
        let session = self.cast_session.fetch_add(1, Ordering::SeqCst) + 1;
        let me = self.clone();
        thread::spawn(move || {
            wake(&l, &l.ids());
            let period = Duration::from_secs_f64(1.0 / fps.clamp(0.5, 60.0));
            let (mut n, mut last_report, mut i) = (0u64, Instant::now(), 0usize);
            let mut cache: Vec<Option<Vec<u8>>> = media.as_ref().map(|m| vec![None; m.len()]).unwrap_or_default();
            let test = transcode::encode(&transcode::test_card(), &ft, &pl).jpeg;
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            let screen =
                if source == "screen" { xcap::Monitor::all().ok().and_then(|m| m.into_iter().find(|m| m.is_primary().unwrap_or(false))) } else { None };
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            let window = window_id.and_then(|id| xcap::Window::all().ok()?.into_iter().find(|w| w.id().ok() == Some(id)));
            // a window that was closed or a screen that cannot be captured ends the cast after 3 s
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            let mut misses = 0;
            while me.cast_session.load(Ordering::SeqCst) == session {
                let t = Instant::now();
                let (jpeg, q, delay) = if let Some(m) = &media {
                    if m.is_empty() {
                        break;
                    }
                    let k = i % m.len();
                    i += 1;
                    if cache[k].is_none() {
                        match decode_source(&m[k].0) {
                            Ok(img) => cache[k] = Some(transcode::encode(&img, &ft, &pl).jpeg),
                            Err(_) => continue,
                        }
                    }
                    (cache[k].clone().unwrap(), 0, m[k].1)
                } else if desktop {
                    #[cfg(any(target_os = "windows", target_os = "macos"))]
                    {
                        let shot = match (&window, &screen) {
                            (Some(w), _) => w.capture_image(),
                            (None, Some(m)) => m.capture_image(),
                            (None, None) => break,
                        };
                        match shot {
                            Ok(rgba) => {
                                misses = 0;
                                let img = transcode::fit(image::DynamicImage::ImageRgba8(rgba).to_rgb8(), MAX_SIDE);
                                let f = transcode::encode(&img, &ft, &pl);
                                (f.jpeg, f.quality, 0)
                            }
                            Err(_) => {
                                misses += 1;
                                if misses > 30 {
                                    break;
                                }
                                thread::sleep(Duration::from_millis(100));
                                continue;
                            }
                        }
                    }
                    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                    {
                        break;
                    }
                } else {
                    (test.clone(), 80, 0)
                };
                let fans = l.cast(&jpeg);
                if fans > 0 {
                    n += 1;
                }
                if last_report.elapsed() > Duration::from_millis(500) {
                    me.emit_cast(&ft, n as f64 / last_report.elapsed().as_secs_f64(), fans, &jpeg, q);
                    n = 0;
                    last_report = Instant::now();
                }
                let wait = if delay > 0 { Duration::from_millis(delay as u64) } else { period };
                if let Some(rest) = wait.checked_sub(t.elapsed()) {
                    thread::sleep(rest);
                }
            }
            // Ended by itself (no source left): report it. A cast that was stopped or replaced
            // stays quiet, so a late "stopped" never cancels the cast that replaced it.
            if me.cast_session.compare_exchange(session, session + 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                me.emit("cast", json!({ "stopped": true }));
            }
        });
        Ok(())
    }

    /// Cast pictures from `next` (an image file in memory per call, `None` while there is no new
    /// one) as a session like `cast_start`'s: `cast_stop` or the next cast ends it. A slow source,
    /// such as a camera snapshot, is fetched on its own thread while the newest picture is sent
    /// at `fps`: the fan needs a frame about every 130 ms or it leaves live mode.
    pub fn cast_source(self: &Arc<Self>, fps: f64, pl: Placement, mut next: impl FnMut() -> Option<Vec<u8>> + Send + 'static) -> Result<(), String> {
        let l = self.link()?;
        let ft = self.fan_type_for(None)?;
        let session = self.cast_session.fetch_add(1, Ordering::SeqCst) + 1;
        let latest: Newest = Arc::default();
        let (me, slot) = (self.clone(), latest.clone());
        thread::spawn(move || {
            while me.cast_session.load(Ordering::SeqCst) == session {
                match next().and_then(|b| decode_source(&b).ok()) {
                    Some(img) => {
                        let f = transcode::encode(&img, &ft, &pl);
                        *slot.lock().unwrap() = Some((f.jpeg, f.quality));
                    }
                    None => thread::sleep(Duration::from_millis(250)),
                }
            }
        });
        let me = self.clone();
        thread::spawn(move || {
            wake(&l, &l.ids());
            let period = Duration::from_secs_f64(1.0 / fps.clamp(1.0, 25.0));
            let (mut n, mut last_report) = (0u64, Instant::now());
            while me.cast_session.load(Ordering::SeqCst) == session {
                let t = Instant::now();
                let frame = latest.lock().unwrap().clone();
                if let Some((jpeg, q)) = frame {
                    let fans = l.cast(&jpeg);
                    if fans > 0 {
                        n += 1;
                    }
                    if last_report.elapsed() > Duration::from_millis(500) {
                        me.emit_cast(&ft, n as f64 / last_report.elapsed().as_secs_f64(), fans, &jpeg, q);
                        n = 0;
                        last_report = Instant::now();
                    }
                }
                if let Some(rest) = period.checked_sub(t.elapsed()) {
                    thread::sleep(rest);
                }
            }
        });
        Ok(())
    }

    /// Frontend-driven cast: one frame (camera, video) per call. Returns the number of fans reached.
    fn cast_frame(&self, data: &str, placement: PlacementDto) -> Result<usize, String> {
        let l = self.link()?;
        let ft = self.fan_type_for(None)?;
        let img = decode_source(&b64(data)?)?;
        let f = transcode::encode(&img, &ft, &placement.into());
        if self.live.lock().unwrap().since.is_none() {
            // first frame of a live cast
            wake(&l, &l.ids());
        }
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
        self.cast_session.fetch_add(1, Ordering::SeqCst);
        *self.live.lock().unwrap() = LiveStats::default();
    }

    // ---- show text / image (Home Assistant) --------------------------------------

    /// Show a text or picture on one or all fans. "upload" stores it in the playlist and
    /// switches to it (and removes it again after `show_seconds`, returning to the file that
    /// played before); "cast" streams it live to the selected fans only for `show_seconds`
    /// (default 10) without storing anything.
    pub fn show(self: &Arc<Self>, r: ShowRequest) -> Result<Value, String> {
        let l = self.link()?;
        let ids = Self::targets(&l, r.id, r.mac.as_deref());
        if ids.is_empty() {
            return Err("no fan connected".into());
        }
        // a temporary message waits for a more important one, or interrupts a less important one
        let message = match r.show_seconds.filter(|s| *s > 0.0) {
            Some(secs) => match self.message_gate(&l, &ids, &r, secs) {
                Some(m) => Some(m),
                None => return Ok(json!({ "success": true, "queued": true })),
            },
            None => None,
        };
        // a fan that is off spins up on black first, so what comes is steady at once
        let off: Vec<u32> = ids.iter().copied().filter(|&i| l.device(i).and_then(|d| d.status).is_some_and(|s| !s.power)).collect();
        if !off.is_empty() {
            self.warm_up(&l, &off);
        }
        // a message that could not be shown must not keep the others waiting
        let result = self.show_now(&l, ids, r, message);
        if result.as_ref().map_or(true, |v| v["success"] != true) {
            if let Some(m) = message {
                self.message_done(m);
            }
        }
        result
    }

    /// Switch fans that are off on and let them spin up for [`WARM_UP`] on black: on a black file in
    /// their playlist when there is one (see [`Self::black_slot`]), otherwise on black sent live.
    fn warm_up(&self, l: &Link, ids: &[u32]) {
        let until = Instant::now() + WARM_UP;
        wake(l, ids);
        let mut live = Vec::new();
        for &id in ids {
            match self.black_slot(l, id) {
                Some(slot) => {
                    log::info!("fan #{id} spins up on its black file");
                    select_and_confirm(l, id, slot);
                }
                None => live.push(id),
            }
        }
        if live.is_empty() {
            thread::sleep(until.saturating_duration_since(Instant::now()));
            return;
        }
        log::info!("fans {live:?} spin up on black, sent live");
        let black = image::RgbImage::new(64, 64);
        let mut frames: Vec<(Vec<u32>, Vec<u8>)> = Vec::new();
        for &id in &live {
            let Ok(ft) = self.fan_type_for(Some(id)) else { continue };
            frames.push((vec![id], transcode::encode(&black, &ft, &Placement::default()).jpeg));
        }
        while Instant::now() < until {
            for (ids, jpeg) in &frames {
                l.cast_to(ids, jpeg);
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// The slot of a black file on fan `id`: one whose kept preview is all but black, or else one
    /// named black, zwart, blank or leeg.
    fn black_slot(&self, l: &Link, id: u32) -> Option<u8> {
        let files = l.device(id)?.files;
        let key = Self::fan_key(l, id);
        let dark = |name: &str| {
            let k = (key.clone(), name.to_string());
            let mut m = self.thumbs.lock().unwrap();
            if !m.contains_key(&k) {
                if let Some(b) = self.thumb_path(&key, name).and_then(|p| std::fs::read(p).ok()) {
                    m.insert(k.clone(), b);
                }
            }
            m.get(&k).and_then(|b| image::load_from_memory(b).ok()).map(|img| {
                let img = img.to_rgb8();
                let lit = img.pixels().filter(|p| p.0.iter().any(|&c| c > 40)).count();
                lit * 200 < (img.width() * img.height()) as usize
            })
        };
        let named = |name: &str| {
            let stem = name.trim_end_matches(".mp4").to_ascii_lowercase();
            ["black", "zwart", "blank", "leeg"].contains(&stem.as_str())
        };
        files.iter().find(|(_, n)| dark(n) == Some(true)).or_else(|| files.iter().find(|(_, n)| named(n))).map(|(s, _)| *s)
    }

    /// Whether a temporary message for these fans may show now: `Some(its id)`, or `None` when a
    /// more important message shows there (then it waits). A less important message showing
    /// there is interrupted and waits to come back for the time it had left.
    fn message_gate(&self, l: &Link, ids: &[u32], r: &ShowRequest, secs: f64) -> Option<u64> {
        let keys: Vec<String> = ids.iter().map(|&i| Self::fan_key(l, i)).collect();
        let priority = r.priority.unwrap_or(0);
        let now = Instant::now();
        let mut m = self.messages.lock().unwrap();
        m.showing.retain(|_, s| s.ends > now);
        if keys.iter().filter_map(|k| m.showing.get(k)).any(|s| s.priority > priority) {
            log::info!("message waits for a more important one (priority {priority})");
            m.waiting.push(Waiting { priority, req: r.clone(), since: now });
            return None;
        }
        // interrupted by a more important message: comes back afterwards (not when merely
        // replaced by an equal one, e.g. a price that updates)
        let mut interrupted: Vec<u64> = Vec::new();
        for k in &keys {
            if let Some(s) = m.showing.remove(k) {
                // a cast message would otherwise keep casting over what comes now
                if s.req.mode.as_deref() == Some("cast") && !interrupted.contains(&s.message) {
                    self.cast_session.fetch_add(1, Ordering::SeqCst);
                }
                let left = if s.shown { s.ends.saturating_duration_since(now).as_secs_f64() } else { s.req.show_seconds.unwrap_or(0.0) };
                let same = s.req.name.is_some() && s.req.name == r.name;
                if s.priority < priority && !same && left >= MESSAGE_RESUME_MIN && !interrupted.contains(&s.message) {
                    interrupted.push(s.message);
                    let mut back = s.req.clone();
                    back.show_seconds = Some(left);
                    m.waiting.push(Waiting { priority: s.priority, req: back, since: now });
                }
            }
        }
        // a fan that is off now goes off again once the messages are over
        for (&i, k) in ids.iter().zip(&keys) {
            let off = l.device(i).and_then(|d| d.status).is_some_and(|s| !s.power);
            if off && !m.woke.contains(k) {
                m.woke.push(k.clone());
            }
        }
        m.next += 1;
        let message = m.next;
        // until it really shows, an upload may take a minute: `ends` is set again then
        let ends = now + Duration::from_secs_f64(secs + 60.0);
        for k in keys {
            m.showing.insert(k, Showing { message, priority, req: r.clone(), ends, shown: false });
        }
        Some(message)
    }

    /// A temporary message really shows now: it ends `secs` from now.
    fn message_shows(&self, message: u64, secs: f64) {
        let ends = Instant::now() + Duration::from_secs_f64(secs);
        for s in self.messages.lock().unwrap().showing.values_mut().filter(|s| s.message == message) {
            s.ends = ends;
            s.shown = true;
        }
    }

    /// A temporary message is over: show the most important waiting one whose fans are free.
    fn message_done(self: &Arc<Self>, message: u64) {
        let Ok(l) = self.link() else { return };
        let now = Instant::now();
        let next = {
            let mut m = self.messages.lock().unwrap();
            m.showing.retain(|_, s| s.message != message && s.ends > now);
            m.waiting.retain(|w| now.duration_since(w.since) < MESSAGE_WAIT);
            let free = |w: &Waiting| {
                let ids = Self::targets(&l, w.req.id, w.req.mac.as_deref());
                !ids.is_empty() && ids.iter().all(|&i| !m.showing.contains_key(&Self::fan_key(&l, i)))
            };
            // the most important first, then the one that waits longest
            let best = m.waiting.iter().enumerate().filter(|(_, w)| free(w)).max_by_key(|(i, w)| (w.priority, std::cmp::Reverse(*i))).map(|(i, _)| i);
            let next = best.map(|i| m.waiting.remove(i));
            // fans a message switched on, with nothing more to show: off again
            let coming: Vec<String> = next.iter().flat_map(|w| Self::targets(&l, w.req.id, w.req.mac.as_deref())).map(|i| Self::fan_key(&l, i)).collect();
            let showing: Vec<String> = m.showing.keys().cloned().collect();
            let (done, still): (Vec<String>, Vec<String>) = m.woke.drain(..).partition(|k| !showing.contains(k) && !coming.contains(k));
            m.woke = still;
            for k in done {
                if let Some(id) = l.ids().into_iter().find(|&i| Self::fan_key(&l, i) == k) {
                    log::info!("fan #{id} was off before the message: off again");
                    let me = self.clone();
                    let l2 = l.clone();
                    let hand = self.by_hand.load(Ordering::SeqCst);
                    thread::spawn(move || me.off_and_confirm(&l2, id, k, hand));
                }
            }
            next
        };
        if let Some(w) = next {
            let me = self.clone();
            thread::spawn(move || {
                if let Err(e) = me.show(w.req) {
                    log::warn!("waiting message not shown: {e}");
                }
            });
        }
    }

    /// Switch fan `id` (key `key`) off and check that it went off (see [`Self::confirm_off`]).
    fn off_and_confirm(&self, l: &Link, id: u32, key: String, hand: u64) {
        if !self.fan_taken(&key, hand) {
            let _ = l.off(id);
            self.confirm_off(l, id, &key, hand);
        }
    }

    /// Check that fan `id` reports off after "off" was sent; send it once more if not. Gives up as
    /// soon as a new message takes the fan or it is switched on or off by hand (`hand` is
    /// [`Self::by_hand`] when "off" was sent).
    fn confirm_off(&self, l: &Link, id: u32, key: &str, hand: u64) {
        for attempt in 0..2 {
            if attempt == 1 {
                if self.fan_taken(key, hand) {
                    return;
                }
                let _ = l.off(id);
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                thread::sleep(Duration::from_millis(300));
                let _ = l.refresh(id);
                thread::sleep(Duration::from_millis(200));
                if self.fan_taken(key, hand) || l.device(id).and_then(|d| d.status).is_some_and(|s| !s.power) {
                    return;
                }
            }
        }
        log::warn!("fan #{id} stayed on after \"off\"");
    }

    /// Whether a message or a hand on/off has taken fan `key` since `hand`.
    fn fan_taken(&self, key: &str, hand: u64) -> bool {
        let m = self.messages.lock().unwrap();
        self.by_hand.load(Ordering::SeqCst) != hand || m.showing.contains_key(key) || m.woke.iter().any(|k| k == key)
    }

    /// The fans a request is for: one id, the fan with that MAC, or all.
    fn targets(l: &Link, id: Option<u32>, mac: Option<&str>) -> Vec<u32> {
        match (id, mac.filter(|m| !m.is_empty())) {
            (Some(i), _) => vec![i],
            (None, Some(mac)) => {
                l.ids().into_iter().filter(|i| l.device(*i).and_then(|d| d.info).is_some_and(|inf| inf.mac.eq_ignore_ascii_case(mac))).collect()
            }
            (None, None) => l.ids(),
        }
    }

    fn show_now(self: &Arc<Self>, l: &Link, ids: Vec<u32>, r: ShowRequest, message: Option<u64>) -> Result<Value, String> {
        let l = l.clone();
        let max_frames = r.max_frames.unwrap_or(200).clamp(1, 600);
        let has_text = r.text.as_deref().is_some_and(|t| !t.trim().is_empty());
        let has_icon = r.icon.as_deref().is_some_and(|i| !i.trim().is_empty() && i.trim() != "none");
        // a picture or animation sent along: (frames, ms per frame)
        let picture = || -> Result<Option<(Vec<image::RgbImage>, u64)>, String> {
            if let Some(fr) = r.frames.as_ref().filter(|f| !f.is_empty()) {
                let imgs = fr.iter().take(max_frames).map(|f| decode_source(&b64(f)?)).collect::<Result<Vec<_>, String>>()?;
                return Ok(Some((imgs, r.frame_ms.unwrap_or(100))));
            }
            let bytes = match (&r.data, &r.path) {
                (Some(d), _) => b64(d)?,
                (None, Some(p)) => std::fs::read(self.media_path(p)?).map_err(|e| format!("{p}: {e}"))?,
                _ => return Ok(None),
            };
            let fr = transcode::decode_frames(&bytes, max_frames).map_err(|e| format!("cannot read image: {e}"))?;
            let ms = r.frame_ms.unwrap_or_else(|| fr.first().map(|(_, d)| *d as u64).filter(|d| *d > 0).unwrap_or(100));
            Ok(Some((fr.into_iter().map(|(img, _)| transcode::fit(img, MAX_SIDE)).collect(), ms)))
        };
        // (pictures, ms per frame when casting)
        let (images, frame_ms): (Vec<image::RgbImage>, u64) = if let Some(m) = text::Motion::parse(r.motion.as_deref().unwrap_or(""))?.filter(|_| has_text) {
            // moving text: one frame per 50 ms, the fan's own rate
            let card = card_for(&r)?;
            (text::render_motion(&card.text, m, card.fg, card.bg, r.speed.unwrap_or(1.0), r.reverse.unwrap_or(false)), 50)
        } else if has_text || has_icon {
            let card = card_for(&r)?;
            let rendered = text::render_card(&card);
            match picture()? {
                // text over every frame of a picture or animation sent along (e.g. a rain radar)
                Some((frames, ms)) => (frames.iter().map(|p| text::over_backdrop(&rendered, p, card.bg)).collect(), ms),
                None => (vec![rendered], 100),
            }
        } else if let Some(p) = picture()? {
            p
        } else if r.style.is_some() {
            (vec![text::render_card(&card_for(&r)?)], 100)
        } else {
            return Err("provide 'text', 'icon', 'data', 'path' or 'frames'".into());
        };
        // text over a picture
        let images = match r.backdrop.as_deref().filter(|b| !b.is_empty() && (has_text || has_icon)) {
            Some(b) => {
                let (picture, bg) = (decode_source(&b64(b)?)?, card_for(&r)?.bg);
                images.iter().map(|img| text::over_backdrop(img, &picture, bg)).collect()
            }
            None => images,
        };
        let pl: Placement = r.placement.unwrap_or_default().into();
        let secs = r.show_seconds.unwrap_or(0.0).max(0.0);
        let cast = r.mode.as_deref() == Some("cast");
        let mut results = Vec::new();
        if cast {
            // only the selected fans, each with frames encoded for its own fan type
            let mut groups: Vec<(FanType, Vec<u32>, Vec<Vec<u8>>)> = Vec::new();
            for &id in &ids {
                let ft = match self.fan_type_for(Some(id)) {
                    Ok(f) => f,
                    Err(e) => {
                        results.push(json!({ "id": id, "success": false, "error": e }));
                        continue;
                    }
                };
                match groups.iter_mut().find(|(f, _, _)| *f == ft) {
                    Some((_, gids, _)) => gids.push(id),
                    None => groups.push((ft, vec![id], images.iter().map(|img| transcode::encode(img, &ft, &pl).jpeg).collect())),
                }
            }
            if groups.is_empty() {
                return Err(results.first().and_then(|r| r["error"].as_str()).unwrap_or("no fan connected").to_string());
            }
            let dur = if secs > 0.0 { secs } else { 10.0 };
            let n = images.len();
            let period = timed_cast_send_period(n, frame_ms);
            let fans: usize = groups.iter().map(|(_, g, _)| g.len()).sum();
            // a timed cast is a session like any other: it ends the running one and is ended by the next
            let session = self.cast_session.fetch_add(1, Ordering::SeqCst) + 1;
            let until = Instant::now() + Duration::from_secs_f64(dur);
            for (ft, gids, jpegs) in &groups {
                self.remember(gids, &jpegs[0], ft, || ShownAs::Cast { session, until });
            }
            if let Some(m) = message {
                self.message_shows(m, dur);
            }
            let me = self.clone();
            thread::spawn(move || {
                if let Ok(l) = me.link() {
                    wake(&l, &groups.iter().flat_map(|(_, g, _)| g.clone()).collect::<Vec<_>>());
                }
                let end = Instant::now() + Duration::from_secs_f64(dur);
                let started = Instant::now();
                while Instant::now() < end && me.cast_session.load(Ordering::SeqCst) == session {
                    let tick = Instant::now();
                    let i = timed_cast_frame(started.elapsed(), n, frame_ms);
                    if let Ok(l) = me.link() {
                        for (_, gids, jpegs) in &groups {
                            l.cast_to(gids, &jpegs[i]);
                        }
                    }
                    thread::sleep(period.saturating_sub(tick.elapsed()));
                }
                // what Home Assistant shows as on the fan is not any more
                me.emit("shown", json!({ "ids": groups.iter().flat_map(|(_, g, _)| g.clone()).collect::<Vec<_>>() }));
                if let Some(m) = message {
                    me.message_done(m);
                }
            });
            return Ok(json!({ "success": true, "mode": "cast", "seconds": dur, "fans": fans, "frames": n, "results": results }));
        }
        // a running cast (e.g. a cast message) ends now: a fan in live mode does not store a file
        // (the upload itself waits until the fan has left live mode)
        self.cast_session.fetch_add(1, Ordering::SeqCst);
        for id in ids {
            let ft = match self.fan_type_for(Some(id)) {
                Ok(f) => f,
                Err(e) => {
                    results.push(json!({ "id": id, "success": false, "error": e }));
                    continue;
                }
            };
            // every picture once, then repeated to the fan's fixed 20 fps so the animation keeps its speed
            let encoded: Vec<(Vec<u8>, u32)> = images.iter().map(|img| (transcode::encode_upload(img, &ft, &pl).jpeg, frame_ms as u32)).collect();
            // one picture is a still (one frame: the fan shows it 5 s, like Holoscope does), not a
            // 0.1 s animation that a playlist would flash past
            let jpegs = if encoded.len() == 1 { vec![encoded[0].0.clone()] } else { at_fan_rate(encoded, MAX_UPLOAD_FRAMES) };
            let name =
                p56_core::protocol::mp4_name(&r.name.clone().unwrap_or_else(|| {
                    format!("ha_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0))
                }));
            // makes the timer of an earlier upload with this name stale before the file is touched
            let (key, generation, _) = self.new_upload(&l, id, &name, secs > 0.0);
            // same name again (e.g. a text entity that keeps updating): replace instead of adding
            remove_named(&l, id, &name);
            let outcome = match l.upload(id, &jpegs, &name, None) {
                Ok(o) => o,
                Err(e) => {
                    results.push(json!({ "id": id, "success": false, "error": e.to_string() }));
                    continue;
                }
            };
            // find the slot the fan gave the new file
            let deadline = Instant::now() + Duration::from_secs(6);
            let mut slot = 0u8;
            while Instant::now() < deadline {
                if let Some(s) = l.device(id).and_then(|d| d.files.iter().find(|(_, n)| *n == name).map(|(s, _)| *s)) {
                    slot = s;
                    break;
                }
                let _ = l.refresh(id);
                thread::sleep(Duration::from_millis(300));
            }
            if slot == 0 {
                results.push(json!({ "id": id, "success": false, "error": "upload sent but not found in the playlist" }));
                continue;
            }
            if !select_and_confirm(&l, id, slot) {
                log::warn!("fan #{id} did not switch to {name}");
            }
            // a temporary file stays on until its time is up: repeat only this one meanwhile. Only
            // now: a select sets a real P56 back to repeating the whole playlist.
            if secs > 0.0 {
                let _ = l.repeat(id, false);
            }
            self.remember(&[id], &jpegs[0], &ft, || ShownAs::File(name.clone()));
            self.keep_thumb(&l, id, &name, &jpegs[0], &ft);
            if secs > 0.0 {
                if let Some(m) = message {
                    self.message_shows(m, secs);
                }
                let me = self.clone();
                let l2 = l.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs_f64(secs));
                    me.expire(&l2, &key, generation);
                    if let Some(m) = message {
                        me.message_done(m);
                    }
                });
            }
            results.push(json!({ "id": id, "success": true, "slot": slot, "name": name, "frames": images.len(), "fan_frames": jpegs.len(), "acknowledged": outcome.acked }));
        }
        let ok = results.iter().any(|r| r["success"] == true);
        Ok(json!({ "success": ok, "mode": "upload", "results": results }))
    }
}

pub fn fan_types() -> Value {
    (1..=40u8)
        .filter_map(|id| {
            let known = FanType::known(id);
            let (angles, leds, _) = FanType::apk_geometry(id).or(known.map(|f| (f.angles, f.radius, 20)))?;
            let jpeg = known.map(|f| {
                let (w, h) = f.jpeg_size();
                format!("{w}×{h}")
            });
            Some(json!({ "id": id, "model": FanType::model(id), "angles": angles, "leds": leds, "jpeg": jpeg,
            "support": format!("{:?}", FanType::support(id)).to_lowercase() }))
        })
        .collect::<Vec<_>>()
        .into()
}

#[cfg(test)]
mod media_cache_tests {
    use super::*;

    fn media(mb: usize) -> Media {
        Media { name: String::new(), frames: vec![(vec![0u8; mb << 20], 0)], audio: None }
    }

    #[test]
    fn oldest_media_goes_first_and_the_current_one_stays() {
        let mut m = HashMap::new();
        m.insert(1, media(300));
        m.insert(2, media(200));
        m.insert(3, media(100));
        assert!(make_room(&mut m, 3, 0));
        assert_eq!(m.keys().copied().collect::<std::collections::BTreeSet<_>>(), [2, 3].into(), "600 MB → the oldest (1) dropped");
        assert!(!make_room(&mut m, 3, MAX_MEDIA_BYTES), "the item being filled alone does not fit");
        assert!(m.contains_key(&3), "the current item is never dropped");
    }
}

#[cfg(test)]
mod fan_rate_tests {
    use super::at_fan_rate;

    #[test]
    fn frames_are_repeated_to_20_fps() {
        // on the 50 ms timeline: a 0–100, b 100–150, c 150–170, d 170–345
        assert_eq!(at_fan_rate(vec![('a', 100), ('b', 50), ('c', 20), ('d', 175)], 100), vec!['a', 'a', 'b', 'c', 'd', 'd', 'd']);
        assert_eq!(at_fan_rate(vec![('a', 0)], 100), vec!['a'], "a still picture stays one frame");
        assert_eq!(at_fan_rate(vec![('a', 1000)], 5).len(), 5, "capped");
    }
}

#[cfg(test)]
mod timed_cast_tests {
    use super::*;

    #[test]
    fn still_picture_is_kept_alive_ten_times_per_second() {
        assert_eq!(timed_cast_send_period(1, 5000), Duration::from_millis(100));
        assert_eq!(timed_cast_frame(Duration::from_secs(10), 1, 5000), 0);
    }

    #[test]
    fn slow_animation_repeats_frames_without_speeding_up() {
        assert_eq!(timed_cast_send_period(3, 500), Duration::from_millis(100));
        assert_eq!(timed_cast_frame(Duration::from_millis(0), 3, 500), 0);
        assert_eq!(timed_cast_frame(Duration::from_millis(400), 3, 500), 0);
        assert_eq!(timed_cast_frame(Duration::from_millis(500), 3, 500), 1);
        assert_eq!(timed_cast_frame(Duration::from_millis(1000), 3, 500), 2);
        assert_eq!(timed_cast_frame(Duration::from_millis(1500), 3, 500), 0);
    }

    #[test]
    fn fast_animation_keeps_its_own_frame_rate() {
        assert_eq!(timed_cast_send_period(4, 50), Duration::from_millis(50));
        assert_eq!(timed_cast_frame(Duration::from_millis(150), 4, 50), 3);
        assert_eq!(timed_cast_frame(Duration::from_millis(200), 4, 50), 0);
    }
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
        for t in ["HELLO", "0,23 kWh", "0,23 €/kWh"] {
            let (src, out) = lit(t);
            let frac = out as f64 / (360.0 * 360.0);
            assert!(src > 2000 && (0.02..0.5).contains(&frac), "{t}: {src} lit pixels in, {:.1}% of the preview lit", frac * 100.0);
        }
    }
}

