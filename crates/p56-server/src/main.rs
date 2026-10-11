//! `p56-server` — runs the P56 link on a server (Home Assistant add-on) and offers:
//! - `/` the P56 Studio web UI (Home Assistant ingress panel)
//! - `POST /api/call/<cmd>` every UI command, `GET /api/events` live events (SSE)
//! - `GET /api/state`, `POST /api/show` for the Home Assistant integration
//! - the add-on 1.x endpoints `/health`, `/status`, `/text`, `/upload`, `/delete`, `/on`, `/off`
//!
//! Access: `--local-only` answers only this machine and the Home Assistant Supervisor network;
//! otherwise `--api-token` makes every other client send the token (except for `/` and
//! `/api/ping`). Files on the server are only read from the `--media-dir` folders.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clap::Parser;
use p56_service::{Defaults, Service, ShowRequest};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

static UI: &str = include_str!("../../../app/src/index.html");

#[derive(Parser, Clone)]
#[command(name = "p56-server", version, about = "P56 server: HTTP API, live events and web UI")]
struct Args {
    /// HTTP port (API + web UI; also the ingress port in Home Assistant).
    #[arg(long, default_value_t = 8765)]
    http_port: u16,
    /// TCP port the fan connects to (the real fan always uses 6666).
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
    /// UDP port of the simulated fan.
    #[arg(long, default_value_t = 18988)]
    sim_udp_port: u16,
    /// Announce this server to Home Assistant (Supervisor discovery) when running as an add-on.
    #[arg(long)]
    discovery: bool,
    /// Only answer this machine and the Home Assistant Supervisor network (172.30.32.0/23),
    /// not the rest of the LAN.
    #[arg(long)]
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

fn allowed(addr: Option<&std::net::SocketAddr>) -> bool {
    match addr.map(|a| a.ip()) {
        Some(std::net::IpAddr::V4(ip)) => ip.is_loopback() || (ip.octets()[0] == 172 && ip.octets()[1] == 30 && (ip.octets()[2] & 0xFE) == 32),
        Some(std::net::IpAddr::V6(ip)) => ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v4| allowed(Some(&std::net::SocketAddr::from((v4, 0))))),
        None => true,
    }
}

type Subscribers = Arc<Mutex<Vec<Sender<String>>>>;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).format_timestamp_secs().init();
    let args = Args::parse();
    let subs: Subscribers = Arc::new(Mutex::new(Vec::new()));
    let svc = start_service(&args, subs.clone());
    let server = Server::http((Ipv4Addr::UNSPECIFIED, args.http_port)).unwrap_or_else(|e| {
        eprintln!("cannot open HTTP port {}: {e}", args.http_port);
        std::process::exit(1)
    });
    log::info!("HTTP API and web UI on :{}", args.http_port);
    match (args.local_only, args.token()) {
        (true, _) => log::info!("API limited to this machine and Home Assistant"),
        (false, Some(_)) => log::info!("API open to the network, with an API token"),
        (false, None) => log::warn!("API open to the whole network WITHOUT a token: anyone on it can control the fans; set an API token"),
    }
    if args.discovery {
        let port = args.http_port;
        thread::spawn(move || announce(port));
    }
    let token = args.token().map(str::to_string);
    for req in server.incoming_requests() {
        let trusted = allowed(req.remote_addr());
        if args.local_only && !trusted {
            let _ = req.respond(json_response(&json!({ "error": "Forbidden" }), 403));
            continue;
        }
        if let Some(t) = &token {
            if !trusted && !public(req.method(), req.url()) && !has_token(&req, t) {
                log::debug!("401 for {} {} from {:?}", req.method(), req.url().split('?').next().unwrap_or(""), req.remote_addr());
                let _ =
                    req.respond(json_response(&json!({ "error": "Unauthorized" }), 401).with_header(Header::from_bytes("WWW-Authenticate", "Bearer").unwrap()));
                continue;
            }
        }
        let svc = svc.clone();
        let subs = subs.clone();
        thread::spawn(move || handle(req, &svc, &subs));
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
    if args.simulate || args.options_file.is_some() {
        targets.push(format!("127.0.0.1:{}", args.sim_udp_port));
    }
    let svc = Service::new(emit, Defaults { ip: args.ip.clone(), targets, port: Some(args.tcp_port), media_dirs: args.media_dirs.clone() });
    match svc.start_link(None, None, None) {
        Ok(ip) => log::info!("fan link started on TCP {} (advertising {ip})", args.tcp_port),
        Err(e) => log::error!("fan link not started: {e}"),
    }
    let virtual_fan = Arc::new(VirtualFan { udp_port: args.sim_udp_port, tcp_port: args.tcp_port, sim: Mutex::new(None) });
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
    let rest = url.strip_prefix("http://")?;
    let (hostport, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let addr = if hostport.contains(':') { hostport.to_string() } else { format!("{hostport}:80") };
    let sock = std::net::ToSocketAddrs::to_socket_addrs(&addr).ok()?.next()?;
    let mut s = TcpStream::connect_timeout(&sock, Duration::from_secs(3)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let auth = std::env::var("SUPERVISOR_TOKEN").map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    write!(s, "GET {path} HTTP/1.0\r\nHost: {hostport}\r\n{auth}Accept: application/json\r\n\r\n").ok()?;
    let mut resp = String::new();
    s.read_to_string(&mut resp).ok()?;
    let (head, body) = resp.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.") || !head.split_whitespace().nth(1).is_some_and(|c| c == "200") {
        return None;
    }
    serde_json::from_str(body).ok()
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
            let cfg = p56_sim::SimConfig { name: "VIRTUAL-P56".into(), udp_port: self.udp_port, pc_port: self.tcp_port, ..Default::default() };
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
    let _ = req.as_reader().take(64 * 1024 * 1024).read_to_string(&mut s);
    serde_json::from_str(&s).unwrap_or(Value::Null)
}

fn handle(mut req: Request, svc: &Arc<Service>, subs: &Subscribers) {
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
            sse(req, subs);
            return;
        }
        (Method::Get, "/api/state") => json_response(&state(svc), 200),
        (Method::Post, p) if p.starts_with("/api/call/") => {
            let cmd = p.trim_start_matches("/api/call/").to_string();
            let args = body_json(&mut req);
            match svc.call(&cmd, &args) {
                Ok(v) => json_response(&json!({ "ok": true, "result": v }), 200),
                Err(e) => json_response(&json!({ "ok": false, "error": e }), 200),
            }
        }
        (Method::Post, "/api/show") => show(svc, body_json(&mut req)),
        (Method::Get, p) if p.starts_with("/api/shown/") => {
            let mac = p.trim_start_matches("/api/shown/").replace("%3A", ":").replace("%3a", ":");
            match svc.shown(&mac) {
                Some(png) => {
                    let r = Response::from_data(png)
                        .with_header(Header::from_bytes("Content-Type", "image/png").unwrap())
                        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
                    let _ = req.respond(r);
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
    let _ = req.respond(resp);
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
fn sse(req: Request, subs: &Subscribers) {
    let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    subs.lock().unwrap().push(tx);
    let mut w = req.into_writer();
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\nX-Accel-Buffering: no\r\n\r\n";
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
