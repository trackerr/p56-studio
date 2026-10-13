//! `p56-server` — runs the P56 link on a server (Home Assistant add-on) and offers:
//! - `/` the P56 Studio web UI (Home Assistant ingress panel)
//! - `POST /api/call/<cmd>` every UI command, `GET /api/events` live events (SSE)
//! - `GET /api/state`, `POST /api/show` for the Home Assistant integration
//! - the add-on 1.x endpoints `/health`, `/status`, `/text`, `/upload`, `/delete`, `/on`, `/off`
//!
//! Access: by default only this machine and the Home Assistant Supervisor network are answered;
//! `--lan` opens the API to the network, and `--api-token` then makes every other client send
//! the token (except for `/` and `/api/ping`). Files on the server are only read from the
//! `--media-dir` folders. Bodies over 64 MB get 413; more than 64 requests or 32 event streams
//! at once get 503. `--fan-allow` limits which addresses may connect as a fan.

mod ai;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::Engine;
use clap::Parser;
use p56_service::{Defaults, Service, ShowRequest};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

static UI: &str = include_str!("../../../app/src/index.html");

#[derive(Parser, Clone)]
#[command(name = "p56-server", version, about = "P56 server: HTTP API, live events and web UI")]
struct Args {
    /// HTTP port (API + web UI; also the ingress port in Home Assistant). 0: any free port.
    #[arg(long, default_value_t = 8765)]
    http_port: u16,
    /// TCP port the fan connects to (the real fan always uses 6666). 0: any free port.
    #[arg(long, default_value_t = 6666)]
    tcp_port: u16,
    /// IP to advertise in the broadcast (default: every network card its own).
    #[arg(long)]
    ip: Option<String>,
    /// Fan IP(s) to send the discovery packet to directly.
    #[arg(long = "fan")]
    fans: Vec<String>,
    /// Start a simulated fan as well (for trying things without hardware).
    #[arg(long)]
    simulate: bool,
    /// Add-on options file (JSON). Read every 2 s, so "simulate" switches the virtual fan
    /// on and off without a restart.
    #[arg(long)]
    options_file: Option<std::path::PathBuf>,
    /// URL that returns the add-on's current options (Home Assistant: http://supervisor/addons/self/info,
    /// authenticated with SUPERVISOR_TOKEN). Checked every 2 s; the Supervisor only rewrites the
    /// options file when the add-on starts, so this is what makes changes apply at once.
    #[arg(long)]
    options_url: Option<String>,
    /// UDP port of the simulated fan. 0: any free port; the simulated fan then connects
    /// straight to this server instead of waiting for its broadcast.
    #[arg(long, default_value_t = 18988)]
    sim_udp_port: u16,
    /// Make AI pictures with ComfyUI at this address (e.g. http://192.168.1.20:8188, with Qwen
    /// Image 2.1 installed) instead of Home Assistant's AI; Home Assistant's is the fallback.
    #[arg(long)]
    comfyui: Option<String>,
    /// Size of ComfyUI's pictures in pixels (square; a P56 shows about 344 across).
    #[arg(long, default_value_t = 768)]
    comfyui_size: u32,
    /// Write the HTTP and TCP ports in use to this file ({"http": …, "tcp": …}) once both are
    /// open: how tests find the ports that port 0 picked.
    #[arg(long, hide = true)]
    ports_file: Option<std::path::PathBuf>,
    /// Announce this server to Home Assistant (Supervisor discovery) when running as an add-on.
    #[arg(long)]
    discovery: bool,
    /// Answer the whole network, not only this machine and the Home Assistant Supervisor
    /// network (172.30.32.0/23). Protect it with --api-token.
    #[arg(long)]
    lan: bool,
    /// Only this machine and the Home Assistant Supervisor network: the default, kept so
    /// older command lines keep working.
    #[arg(long, hide = true)]
    local_only: bool,
    /// Require this token from clients on the network (not from this machine or the Home
    /// Assistant Supervisor network): `Authorization: Bearer <token>` or `?token=<token>`.
    /// The web UI page itself and /api/ping stay open.
    #[arg(long, env = "P56_API_TOKEN", hide_env_values = true)]
    api_token: Option<String>,
    /// Folder that `path` in /api/show (and `image` in the 1.x /upload) may read from;
    /// repeat for more. Without any, reading files on the server is refused.
    #[arg(long = "media-dir")]
    media_dirs: Vec<std::path::PathBuf>,
    /// Keep previews of uploaded files in this folder (the add-on: /data/thumbs), so the app can
    /// show what a file looks like after a restart too. Without it they are kept in memory only.
    #[arg(long)]
    thumb_dir: Option<std::path::PathBuf>,
    /// Only accept fan connections (TCP port) from this address; repeat for more.
    /// Without any, every address may connect (as a real fan on the network needs).
    #[arg(long = "fan-allow")]
    fan_allow: Vec<std::net::IpAddr>,
}

impl Args {
    fn token(&self) -> Option<&str> {
        self.api_token.as_deref().map(str::trim).filter(|t| !t.is_empty())
    }
}

/// Requests that need no token: the web UI page, the ping and CORS preflights.
fn public(method: &Method, url: &str) -> bool {
    let path = url.split('?').next().unwrap_or("/").trim_end_matches('/');
    *method == Method::Options || (*method == Method::Get && matches!(path, "" | "/index.html" | "/api/ping"))
}

/// Constant-time comparison (only the length can be learned from the timing).
fn same_secret(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Decode `%XX` escapes (a token in a URL is usually sent through encodeURIComponent).
fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// The request carries the API token in an `Authorization: Bearer` header or a `token` query parameter.
fn has_token(req: &Request, token: &str) -> bool {
    let header = req.headers().iter().filter(|h| h.field.equiv("Authorization")).any(|h| {
        h.value
            .as_str()
            .trim()
            .split_once(' ')
            .is_some_and(|(scheme, t)| scheme.eq_ignore_ascii_case("bearer") && same_secret(t.trim().as_bytes(), token.as_bytes()))
    });
    header
        || req
            .url()
            .split_once('?')
            .is_some_and(|(_, q)| q.split('&').any(|kv| kv.strip_prefix("token=").is_some_and(|v| same_secret(&percent_decode(v), token.as_bytes()))))
}

/// Origins of P56 Studio itself (Tauri on Windows / Linux / macOS / Android).
const APP_ORIGINS: [&str; 3] = ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"];

fn header<'a>(req: &'a Request, name: &'static str) -> Option<&'a str> {
    req.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str())
}

/// Where a browser request comes from, as far as it matters: `Ok(Some(origin))` for P56 Studio
/// (it gets CORS headers), `Ok(None)` for no Origin (curl, Home Assistant) or the page itself, and
/// `Err(())` for any other web page. Browsers send Origin on every cross-site request, so a page
/// on another site can never make this machine's browser call the API (no CSRF via localhost).
fn origin(req: &Request) -> Result<Option<String>, ()> {
    let Some(o) = header(req, "Origin").map(str::trim).filter(|o| !o.is_empty() && *o != "null") else { return Ok(None) };
    if APP_ORIGINS.contains(&o) {
        return Ok(Some(o.to_string()));
    }
    let host = header(req, "Host").unwrap_or("");
    let own = o.strip_prefix("http://").or_else(|| o.strip_prefix("https://")).is_some_and(|h| h.eq_ignore_ascii_case(host));
    if own {
        Ok(None)
    } else {
        Err(())
    }
}

/// CORS headers for P56 Studio's origin.
fn cors<R: Read>(resp: Response<R>, origin: &Option<String>) -> Response<R> {
    match origin {
        None => resp,
        Some(o) => resp
            .with_header(Header::from_bytes("Access-Control-Allow-Origin", o.as_bytes()).unwrap())
            .with_header(Header::from_bytes("Vary", "Origin").unwrap())
            .with_header(Header::from_bytes("Access-Control-Allow-Headers", "Authorization, Content-Type").unwrap())
            .with_header(Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap())
            .with_header(Header::from_bytes("Access-Control-Max-Age", "600").unwrap()),
    }
}

fn allowed(addr: Option<&std::net::SocketAddr>) -> bool {
    match addr.map(|a| a.ip()) {
        Some(std::net::IpAddr::V4(ip)) => ip.is_loopback() || (ip.octets()[0] == 172 && ip.octets()[1] == 30 && (ip.octets()[2] & 0xFE) == 32),
        Some(std::net::IpAddr::V6(ip)) => ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v4| allowed(Some(&std::net::SocketAddr::from((v4, 0))))),
        None => true,
    }
}

type Subscribers = Arc<Mutex<Vec<Sender<String>>>>;

/// Largest request body (a picture or a few animation frames, base64).
const MAX_BODY: usize = 64 * 1024 * 1024;
/// Requests handled at once; more get 503.
const MAX_ACTIVE: usize = 64;
/// Live event streams open at once; more get 503.
const MAX_STREAMS: usize = 32;

/// Counts a running request or event stream for as long as it lives.
struct Slot(&'static AtomicUsize);
impl Slot {
    fn take(counter: &'static AtomicUsize, max: usize) -> Option<Slot> {
        if counter.fetch_add(1, Ordering::SeqCst) >= max {
            counter.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Slot(counter))
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static STREAMS: AtomicUsize = AtomicUsize::new(0);

fn busy(req: Request) {
    let _ = req
        .respond(json_response(&json!({ "error": "Too many requests at once, try again" }), 503).with_header(Header::from_bytes("Retry-After", "1").unwrap()));
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).format_timestamp_secs().init();
    let args = Args::parse();
    if let Some(url) = args.comfyui.as_deref().filter(|u| !u.trim().is_empty()) {
        match ai::use_comfyui(url, args.comfyui_size) {
            Ok(()) => log::info!("AI pictures from ComfyUI at {}", url.trim()),
            Err(e) => log::error!("{e}"),
        }
    }
    let subs: Subscribers = Arc::new(Mutex::new(Vec::new()));
    let svc = start_service(&args, subs.clone());
    let server = Server::http((Ipv4Addr::UNSPECIFIED, args.http_port)).unwrap_or_else(|e| {
        eprintln!("cannot open HTTP port {}: {e}", args.http_port);
        std::process::exit(1)
    });
    let http_port = server.server_addr().to_ip().map_or(args.http_port, |a| a.port());
    log::info!("HTTP API and web UI on :{http_port}");
    if let Some(path) = &args.ports_file {
        let tcp_port = svc.link().map_or(args.tcp_port, |l| l.tcp_port);
        if let Err(e) = std::fs::write(path, json!({ "http": http_port, "tcp": tcp_port }).to_string()) {
            log::error!("cannot write {}: {e}", path.display());
        }
    }
    match (args.lan, args.token()) {
        (false, _) => log::info!("API limited to this machine and Home Assistant (--lan opens it to the network)"),
        (true, Some(_)) => log::info!("API open to the network, with an API token"),
        (true, None) => log::warn!("API open to the whole network WITHOUT a token: anyone on it can control the fans; set an API token"),
    }
    if args.discovery {
        thread::spawn(move || announce(http_port));
    }
    let token = args.token().map(str::to_string);
    for req in server.incoming_requests() {
        let trusted = allowed(req.remote_addr());
        if !args.lan && !trusted {
            let _ = req.respond(json_response(&json!({ "error": "Forbidden" }), 403));
            continue;
        }
        // Home Assistant's ingress proxy (Supervisor network) forwards the browser's HA origin.
        let supervisor = trusted && req.remote_addr().is_some_and(|a| !a.ip().is_loopback());
        let origin = match origin(&req) {
            Ok(o) => o,
            Err(()) if supervisor => None,
            Err(()) => {
                log::debug!("403 for {} from a foreign web page ({:?})", req.url().split('?').next().unwrap_or(""), header(&req, "Origin"));
                let _ = req.respond(json_response(&json!({ "error": "Forbidden: requests from other web sites are not accepted" }), 403));
                continue;
            }
        };
        if let Some(t) = &token {
            if !trusted && !public(req.method(), req.url()) && !has_token(&req, t) {
                log::debug!("401 for {} {} from {:?}", req.method(), req.url().split('?').next().unwrap_or(""), req.remote_addr());
                let _ = req.respond(cors(
                    json_response(&json!({ "error": "Unauthorized" }), 401).with_header(Header::from_bytes("WWW-Authenticate", "Bearer").unwrap()),
                    &origin,
                ));
                continue;
            }
        }
        if req.body_length().is_some_and(|n| n > MAX_BODY) {
            let _ = req.respond(cors(json_response(&json!({ "error": format!("Payload too large (max {} MB)", MAX_BODY >> 20) }), 413), &origin));
            continue;
        }
        let stream = req.url().split('?').next().is_some_and(|p| p.trim_end_matches('/') == "/api/events");
        let Some(slot) = Slot::take(if stream { &STREAMS } else { &ACTIVE }, if stream { MAX_STREAMS } else { MAX_ACTIVE }) else {
            busy(req);
            continue;
        };
        let svc = svc.clone();
        let subs = subs.clone();
        thread::spawn(move || {
            let _slot = slot;
            handle(req, &svc, &subs, &origin)
        });
    }
}

fn start_service(args: &Args, subs: Subscribers) -> Arc<Service> {
    let emit: p56_service::Emit = Arc::new(move |t: &str, data: Value| {
        if t == "log" {
            if let Some(m) = data.get("msg").and_then(|m| m.as_str()) {
                log::info!("{m}");
            }
        }
        let line = format!("data: {}\n\n", json!({ "type": t, "data": data }));
        subs.lock().unwrap().retain(|s| s.send(line.clone()).is_ok());
    });
    let mut targets = args.fans.clone();
    if (args.simulate || args.options_file.is_some()) && args.sim_udp_port != 0 {
        targets.push(format!("127.0.0.1:{}", args.sim_udp_port));
    }
    let svc = Service::new(
        emit,
        Defaults {
            ip: args.ip.clone(),
            targets,
            port: Some(args.tcp_port),
            media_dirs: args.media_dirs.clone(),
            thumb_dir: args.thumb_dir.clone(),
            // the virtual fan connects from this machine
            allow_fans: if args.fan_allow.is_empty() {
                Vec::new()
            } else {
                args.fan_allow.iter().copied().chain([std::net::IpAddr::from(Ipv4Addr::LOCALHOST)]).collect()
            },
        },
    );
    let started = svc.start_link(None, None, None);
    let tcp_port = svc.link().map_or(args.tcp_port, |l| l.tcp_port);
    match started {
        Ok(ip) => log::info!("fan link started on TCP {tcp_port} (advertising {ip})"),
        Err(e) => log::error!("fan link not started: {e}"),
    }
    let virtual_fan = Arc::new(VirtualFan { udp_port: args.sim_udp_port, tcp_port, sim: Mutex::new(None) });
    if args.options_file.is_some() || args.options_url.is_some() {
        let vf = virtual_fan.clone();
        let (file, url) = (args.options_file.clone(), args.options_url.clone());
        let mut current = args.simulate;
        thread::spawn(move || loop {
            // the URL has the latest saved options; the file is what the add-on started with
            let from_url = url.as_deref().and_then(fetch_json).as_ref().and_then(simulate_option);
            let from_file = || {
                file.as_ref()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    .as_ref()
                    .and_then(simulate_option)
            };
            if let Some(wanted) = from_url.or_else(from_file) {
                current = wanted;
            }
            vf.set(current);
            thread::sleep(Duration::from_secs(2));
        });
    } else {
        virtual_fan.set(args.simulate);
    }
    std::mem::forget(virtual_fan);
    svc
}

/// "simulate" from an options object, also when wrapped as the Supervisor answers ({"data": {"options": …}}).
fn simulate_option(v: &Value) -> Option<bool> {
    [&v["data"]["options"]["simulate"], &v["options"]["simulate"], &v["simulate"]].into_iter().find_map(|b| b.as_bool())
}

/// Minimal HTTP GET returning JSON (plain http, as the Supervisor API is reached inside Home Assistant).
fn fetch_json(url: &str) -> Option<Value> {
    let token = std::env::var("SUPERVISOR_TOKEN").ok();
    serde_json::from_slice(&http_get(url, token.as_deref(), "application/json")?).ok()
}

/// Minimal HTTP/1.0 GET (plain http only) returning the body of a 200 answer.
fn http_get(url: &str, token: Option<&str>, accept: &str) -> Option<Vec<u8>> {
    match http_request("GET", url, token, accept, None, Duration::from_secs(5)) {
        Ok((200, body)) => Some(body),
        _ => None,
    }
}

/// Minimal HTTP/1.0 request (plain http only, a JSON body if any): the status and body of the answer.
fn http_request(method: &str, url: &str, token: Option<&str>, accept: &str, body: Option<&[u8]>, timeout: Duration) -> Result<(u16, Vec<u8>), String> {
    let rest = url.strip_prefix("http://").ok_or_else(|| format!("not a plain http address: {url}"))?;
    let (hostport, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let addr = if hostport.contains(':') { hostport.to_string() } else { format!("{hostport}:80") };
    let sock = std::net::ToSocketAddrs::to_socket_addrs(&addr).ok().and_then(|mut a| a.next()).ok_or_else(|| format!("{hostport}: unknown host"))?;
    let mut s = TcpStream::connect_timeout(&sock, Duration::from_secs(3)).map_err(|e| format!("{hostport}: {e}"))?;
    s.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    let content = body.map(|b| format!("Content-Type: application/json\r\nContent-Length: {}\r\n", b.len())).unwrap_or_default();
    write!(s, "{method} {path} HTTP/1.0\r\nHost: {hostport}\r\n{auth}Accept: {accept}\r\n{content}\r\n").map_err(|e| e.to_string())?;
    if let Some(b) = body {
        s.write_all(b).map_err(|e| e.to_string())?;
    }
    let mut resp = Vec::new();
    s.take(32 << 20).read_to_end(&mut resp).map_err(|e| format!("{hostport}: {e}"))?;
    let split = resp.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| format!("{hostport}: no HTTP answer"))?;
    let head = String::from_utf8_lossy(&resp[..split]);
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .filter(|_| head.starts_with("HTTP/1."))
        .ok_or_else(|| format!("{hostport}: no HTTP answer"))?;
    Ok((code, resp[split + 4..].to_vec()))
}

// ---- Home Assistant cameras (in the add-on) -------------------------------------------

/// Home Assistant's REST API as the add-on reaches it (`homeassistant_api: true` in its config),
/// or another one given by P56_HA_URL / P56_HA_TOKEN (tests). `None` outside Home Assistant.
fn ha_api() -> Option<(String, String)> {
    let token = std::env::var("P56_HA_TOKEN").or_else(|_| std::env::var("SUPERVISOR_TOKEN")).ok()?;
    let base = std::env::var("P56_HA_URL").unwrap_or_else(|_| "http://supervisor/core/api".into());
    Some((base.trim_end_matches('/').to_string(), token))
}

/// A camera entity id, nothing else (it becomes part of a URL).
fn camera_entity(id: &str) -> bool {
    id.strip_prefix("camera.").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
}

/// The calls the server answers itself: Home Assistant cameras and AI pictures. `None` for every other call.
fn ha_call(svc: &Arc<Service>, cmd: &str, args: &Value) -> Option<Result<Value, String>> {
    let source = args["args"]["source"].as_str().unwrap_or_default();
    if matches!(cmd, "ai_status" | "ai_image" | "show_ai") {
        let ha = ha_api();
        return Some(ai_call(svc, cmd, args, ha.as_ref().map(|(b, t)| (b.as_str(), t.as_str()))));
    }
    if cmd != "ha_cameras" && !(cmd == "cast_start" && source.starts_with("ha_camera:")) {
        return None;
    }
    let Some((base, token)) = ha_api() else {
        return Some(Err("Home Assistant cameras are only available in the Home Assistant add-on".into()));
    };
    if cmd == "ha_cameras" {
        let states = http_get(&format!("{base}/states"), Some(&token), "application/json").and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let Some(states) = states.as_ref().and_then(|v| v.as_array()) else {
            return Some(Err("Home Assistant did not list its entities (is homeassistant_api on?)".into()));
        };
        let mut cams: Vec<Value> = states
            .iter()
            .filter_map(|e| {
                let id = e["entity_id"].as_str().filter(|id| camera_entity(id))?;
                let name = e["attributes"]["friendly_name"].as_str().unwrap_or(id);
                Some(json!({ "entity_id": id, "name": name }))
            })
            .collect();
        cams.sort_by_key(|c| c["name"].as_str().unwrap_or_default().to_lowercase());
        return Some(Ok(Value::Array(cams)));
    }
    let entity = source.trim_start_matches("ha_camera:").to_string();
    if !camera_entity(&entity) {
        return Some(Err(format!("not a camera entity: {entity}")));
    }
    let fps = args["args"]["fps"].as_f64().unwrap_or(10.0);
    let pl: p56_core::Placement = serde_json::from_value::<p56_service::PlacementDto>(args["args"]["placement"].clone()).unwrap_or_default().into();
    let url = format!("{base}/camera_proxy/{entity}");
    let mut last = std::time::Instant::now() - Duration::from_secs(1);
    // a new snapshot at most 5 times a second; the cast repeats the newest one in between
    let next = move || {
        if let Some(wait) = Duration::from_millis(200).checked_sub(last.elapsed()) {
            thread::sleep(wait);
        }
        last = std::time::Instant::now();
        http_get(&url, Some(&token), "image/*")
    };
    log::info!("casting Home Assistant camera {entity}");
    Some(svc.cast_source(fps, pl, next).map(|_| Value::Null))
}

/// AI pictures through Home Assistant's AI Task:
/// - `ai_status`: the AI Task entities that can make pictures (empty: none set up);
/// - `ai_image` {prompt, kind, art_style?, entity_id?, name?}: a new media item (for the app's Media panel);
/// - `show_ai` {prompt, kind, entity_id?, …the fields of /api/show}: straight onto the fans; with
///   `text` (or `icon`) the AI makes a background and the text is written over it.
///
/// The same prompt again reuses the picture made before, unless `fresh` is true.
fn ai_call(svc: &Arc<Service>, cmd: &str, args: &Value, ha: Option<(&str, &str)>) -> Result<Value, String> {
    if cmd == "ai_status" {
        return ai::entities(ha).map(|e| json!({ "entities": e }));
    }
    let prompt = args["prompt"].as_str().unwrap_or_default();
    let text = cmd == "show_ai" && ["text", "icon"].iter().any(|k| args[k].as_str().is_some_and(|s| !s.trim().is_empty()));
    let kind = if text { ai::Kind::Backdrop } else { ai::Kind::parse(args["kind"].as_str().unwrap_or_default())? };
    let fresh = args["fresh"].as_bool().unwrap_or(false);
    let (images, ms) = ai::generate(ha, args["entity_id"].as_str(), prompt, kind, args["art_style"].as_str(), fresh)?;
    // a name for the fan from the prompt's first words
    let words: String = prompt.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' }).collect();
    let default_name = format!("ai {}", words.split_whitespace().collect::<Vec<_>>().join(" ")).chars().take(24).collect::<String>().trim().to_string();
    let name = args["name"].as_str().filter(|n| !n.trim().is_empty()).map(String::from).unwrap_or(default_name);
    if cmd == "ai_image" {
        return svc.media_images(name, images, ms);
    }
    let mut req = args.clone();
    let o = req.as_object_mut().ok_or("expected an object")?;
    for k in ["prompt", "kind", "art_style", "entity_id", "fresh", "data", "path", "image", "frames", "backdrop"] {
        o.remove(k);
    }
    o.entry("name").or_insert(json!(name));
    let jpeg = |img: &image::RgbImage| base64::engine::general_purpose::STANDARD.encode(p56_core::transcode::jpeg(img, 92));
    if text {
        o.insert("backdrop".into(), json!(jpeg(&images[0])));
        // over an AI picture only the text: an icon would be one picture too many
        if o.get("text").and_then(|t| t.as_str()).is_some_and(|t| !t.trim().is_empty()) {
            o.remove("icon");
        }
    } else {
        for k in ["text", "icon", "style"] {
            o.remove(k);
        }
        o.insert("frames".into(), json!(images.iter().map(jpeg).collect::<Vec<_>>()));
        if ms > 0 {
            o.insert("frame_ms".into(), json!(ms));
        }
    }
    let r: ShowRequest = serde_json::from_value(req).map_err(|e| e.to_string())?;
    svc.show(r)
}

/// The simulated fan, switched on and off while the server runs.
struct VirtualFan {
    udp_port: u16,
    tcp_port: u16,
    sim: Mutex<Option<p56_sim::Sim>>,
}

impl VirtualFan {
    fn set(&self, on: bool) {
        let mut sim = self.sim.lock().unwrap();
        if on == sim.is_some() {
            return;
        }
        if let Some(s) = sim.take() {
            s.stop();
            log::info!("virtual fan stopped");
            return;
        }
        // the UDP port of a just-stopped simulator can take a moment to be released
        for attempt in 0..10 {
            // without a fixed UDP port nothing broadcasts to it, so it connects straight away
            let pc = (self.udp_port == 0).then(|| std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, self.tcp_port)));
            let cfg = p56_sim::SimConfig { name: "VIRTUAL-P56".into(), udp_port: self.udp_port, pc, pc_port: self.tcp_port, ..Default::default() };
            match p56_sim::Sim::start(cfg) {
                Ok(s) => {
                    log::info!("virtual fan started");
                    *sim = Some(s);
                    return;
                }
                Err(e) if attempt == 9 => log::warn!("virtual fan not started: {e}"),
                Err(_) => thread::sleep(Duration::from_millis(300)),
            }
        }
    }
}

fn json_response(v: &Value, status: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(serde_json::to_vec(v).unwrap_or_default())
        .with_status_code(status)
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
}

fn body_json(req: &mut Request) -> Value {
    let mut s = String::new();
    let _ = req.as_reader().take(MAX_BODY as u64).read_to_string(&mut s);
    serde_json::from_str(&s).unwrap_or(Value::Null)
}

fn handle(mut req: Request, svc: &Arc<Service>, subs: &Subscribers, origin: &Option<String>) {
    let url = req.url().split('?').next().unwrap_or("/").trim_end_matches('/').to_string();
    let method = req.method().clone();
    let path = if url.is_empty() { "/".to_string() } else { url };
    let resp = match (&method, path.as_str()) {
        (Method::Get, "/" | "/index.html") => {
            let r = Response::from_string(UI)
                .with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap())
                .with_header(Header::from_bytes("Cache-Control", "no-cache").unwrap());
            let _ = req.respond(r);
            return;
        }
        (Method::Get, "/api/ping") => json_response(&json!({ "ok": true, "server": "p56-server", "version": env!("CARGO_PKG_VERSION") }), 200),
        (Method::Get, "/api/events") => {
            sse(req, subs, origin);
            return;
        }
        (Method::Get, "/api/state") => json_response(&state(svc), 200),
        (Method::Post, p) if p.starts_with("/api/call/") => {
            let cmd = p.trim_start_matches("/api/call/").to_string();
            let args = body_json(&mut req);
            match ha_call(svc, &cmd, &args).unwrap_or_else(|| svc.call(&cmd, &args)) {
                Ok(v) => json_response(&json!({ "ok": true, "result": v }), 200),
                Err(e) => json_response(&json!({ "ok": false, "error": e }), 200),
            }
        }
        (Method::Post, "/api/show") => show(svc, body_json(&mut req)),
        (Method::Get, p) if p.starts_with("/api/shown/") => {
            let mac = p.trim_start_matches("/api/shown/").replace("%3A", ":").replace("%3a", ":");
            match svc.shown(&mac) {
                Some((png, live)) => {
                    let r = Response::from_data(png)
                        .with_header(Header::from_bytes("Content-Type", "image/png").unwrap())
                        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                        .with_header(Header::from_bytes("X-P56-Live", if live { "1" } else { "0" }).unwrap());
                    let _ = req.respond(cors(r, origin));
                    return;
                }
                None => json_response(&json!({ "error": "Nothing shown yet" }), 404),
            }
        }
        // ---- add-on 1.x API --------------------------------------------------
        (Method::Get, "/health") => {
            let st = state(svc);
            let connected = st["connected"].as_bool().unwrap_or(false);
            json_response(&json!({ "status": if connected { "ok" } else { "waiting" }, "connected": connected, "fan_ip": st["fan_ip"] }), 200)
        }
        (Method::Get, "/status") => {
            let st = state(svc);
            // 1.x answered 503 without a fan; 2.x always answers so the integration can load.
            json_response(&st, 200)
        }
        (Method::Post, "/text") => {
            let mut b = body_json(&mut req);
            if b.get("text").and_then(|t| t.as_str()).unwrap_or("").is_empty() {
                json_response(&json!({ "error": "Missing 'text'" }), 400)
            } else {
                if let Some(o) = b.as_object_mut() {
                    o.entry("mode").or_insert(json!("upload"));
                }
                show(svc, b)
            }
        }
        (Method::Post, "/upload") => {
            let b = body_json(&mut req);
            if b.get("image").is_none() && b.get("path").is_none() && b.get("data").is_none() {
                json_response(&json!({ "error": "Missing 'image' path" }), 400)
            } else {
                show(svc, b)
            }
        }
        (Method::Post, "/delete") => {
            let b = body_json(&mut req);
            match (b.get("slot").and_then(|s| s.as_i64()), first_fan(svc)) {
                (Some(slot), Some(id)) if slot >= 1 => {
                    let ok = svc.set_value(id, "delete", slot as i32).is_ok();
                    json_response(&json!({ "success": ok, "action": "delete", "slot": slot }), 200)
                }
                (_, None) => json_response(&json!({ "error": "No fan connected" }), 503),
                _ => json_response(&json!({ "error": "Missing or invalid 'slot'" }), 400),
            }
        }
        (Method::Post, a @ ("/on" | "/off")) => match first_fan(svc) {
            Some(_) => {
                let action = &a[1..];
                let ok = all_fans(svc).iter().all(|id| svc.control(*id, action).is_ok());
                json_response(&json!({ "success": ok, "action": format!("display_{action}") }), 200)
            }
            None => json_response(&json!({ "error": "No fan connected" }), 503),
        },
        (Method::Options, _) => json_response(&Value::Null, 204),
        _ => json_response(&json!({ "error": "Not found" }), 404),
    };
    let _ = req.respond(cors(resp, origin));
}

fn all_fans(svc: &Service) -> Vec<u32> {
    svc.link().map(|l| l.ids()).unwrap_or_default()
}
fn first_fan(svc: &Service) -> Option<u32> {
    all_fans(svc).first().copied()
}

fn show(svc: &Arc<Service>, b: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let req: ShowRequest = match serde_json::from_value(b) {
        Ok(r) => r,
        Err(e) => return json_response(&json!({ "success": false, "error": e.to_string() }), 400),
    };
    // a file on the server may only be read from the media folders
    if let Some(p) = req.path.as_deref().filter(|p| !p.trim().is_empty()) {
        if let Err(e) = svc.media_path(p) {
            return json_response(&json!({ "success": false, "error": e }), 403);
        }
    }
    if first_fan(svc).is_none() {
        return json_response(&json!({ "success": false, "error": "No fan connected" }), 503);
    }
    match svc.show(req) {
        Ok(v) => json_response(&v, 200),
        Err(e) => json_response(&json!({ "success": false, "error": e }), 500),
    }
}

/// Snapshot for the integration (and the 1.x `/status` shape: connected, fan_ip, playlist, last_activity).
fn state(svc: &Service) -> Value {
    let devices = svc.devices().unwrap_or_default();
    let first = devices.first();
    json!({
        "connected": !devices.is_empty(),
        "fan_ip": first.map(|d| d.ip.clone()),
        "playlist": first.map(|d| d.files.iter().map(|(s, n)| json!({ "slot": s, "name": n })).collect::<Vec<_>>()).unwrap_or_default(),
        "last_activity": svc.last_activity(),
        "devices": devices,
        "version": env!("CARGO_PKG_VERSION"),
    })
}

/// Server-sent events: one `data:` line per event, a comment every 15 s to keep proxies open.
fn sse(req: Request, subs: &Subscribers, origin: &Option<String>) {
    let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    subs.lock().unwrap().push(tx);
    let mut w = req.into_writer();
    let cors = origin.as_ref().map(|o| format!("Access-Control-Allow-Origin: {o}\r\nVary: Origin\r\n")).unwrap_or_default();
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\nX-Accel-Buffering: no\r\n{cors}\r\n"
    );
    if w.write_all(head.as_bytes()).and_then(|_| w.write_all(b": p56\n\n")).and_then(|_| w.flush()).is_err() {
        return;
    }
    loop {
        let msg = match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => ": keepalive\n\n".to_string(),
            Err(_) => return,
        };
        if w.write_all(msg.as_bytes()).and_then(|_| w.flush()).is_err() {
            return;
        }
    }
}

/// Tell Home Assistant about this server so the integration is offered automatically.
fn announce(port: u16) {
    let Ok(token) = std::env::var("SUPERVISOR_TOKEN") else {
        log::info!("no SUPERVISOR_TOKEN: discovery skipped");
        return;
    };
    thread::sleep(Duration::from_secs(3));
    let host = std::env::var("HOSTNAME").ok().filter(|h| !h.is_empty()).unwrap_or_else(|| "127.0.0.1".into());
    let body = json!({ "service": "p56_pov", "config": { "host": "127.0.0.1", "port": port, "addon_host": host } }).to_string();
    let req = format!("POST /discovery HTTP/1.1\r\nHost: supervisor\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    match TcpStream::connect(("supervisor", 80)).and_then(|mut s| {
        s.write_all(req.as_bytes())?;
        let mut r = String::new();
        s.read_to_string(&mut r)?;
        Ok(r)
    }) {
        Ok(r) => log::info!("discovery to Home Assistant: {}", r.lines().next().unwrap_or("")),
        Err(e) => log::warn!("discovery failed: {e}"),
    }
}
