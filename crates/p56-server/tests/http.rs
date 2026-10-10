//! The `p56-server` binary end to end: HTTP API, add-on 1.x endpoints, SSE and the web UI,
//! against its built-in simulated fan.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream, UdpSocket};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn free_tcp() -> u16 { TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port() }
fn free_udp() -> u16 { UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port() }

struct Srv { child: Child, base: String }
impl Drop for Srv { fn drop(&mut self) { let _ = self.child.kill(); } }

fn start(simulate: bool) -> Srv {
    let (http, tcp) = (free_tcp(), free_tcp());
    let mut args = vec!["--http-port".to_string(), http.to_string(), "--tcp-port".into(), tcp.to_string()];
    if simulate { args.extend(["--simulate".into(), "--sim-udp-port".into(), free_udp().to_string()]); }
    let child = Command::new(env!("CARGO_BIN_EXE_p56-server")).args(&args).env("RUST_LOG", "warn")
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let base = format!("http://127.0.0.1:{http}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while ureq::get(&format!("{base}/api/ping")).call().is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(100));
    }
    Srv { child, base }
}

impl Srv {
    fn get(&self, p: &str) -> Value { ureq::get(&format!("{}{p}", self.base)).call().unwrap().into_json().unwrap() }
    fn post(&self, p: &str, body: Value) -> (u16, Value) {
        match ureq::post(&format!("{}{p}", self.base)).send_json(body) {
            Ok(r) => (r.status(), r.into_json().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(c, r)) => (c, r.into_json().unwrap_or(Value::Null)),
            Err(e) => panic!("{e}"),
        }
    }
    fn call(&self, cmd: &str, args: Value) -> Value {
        let (_, v) = self.post(&format!("/api/call/{cmd}"), args);
        assert_eq!(v["ok"], true, "{cmd}: {v}");
        v["result"].clone()
    }
    fn wait(&self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = self.get("/api/state");
            if pred(&s) { return s; }
            assert!(Instant::now() < deadline, "{what}: {s}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[test]
fn ui_and_status_without_fan() {
    let s = start(false);
    let html = ureq::get(&format!("{}/", s.base)).call().unwrap().into_string().unwrap();
    assert!(html.contains("P56 Studio") && html.len() > 100_000, "UI not served");
    let st = s.get("/status");
    assert_eq!(st["connected"], false);
    let h = s.get("/health");
    assert_eq!(h["status"], "waiting");
    let (code, _) = s.post("/text", json!({ "text": "hoi" }));
    assert_eq!(code, 503);
    let (code, _) = s.post("/on", json!({}));
    assert_eq!(code, 503);
}

#[test]
fn api_controls_the_virtual_fan() {
    let s = start(true);
    let st = s.wait("virtual fan connects", |v| v["connected"] == true && v["devices"][0]["status"].is_object());
    let id = st["devices"][0]["id"].as_u64().unwrap();
    assert_eq!(st["devices"][0]["name"], "VIRTUELE-P56");
    s.call("control", json!({ "id": id, "action": "off" }));
    s.wait("off", |v| v["devices"][0]["status"]["power"] == false);
    s.call("set_value", json!({ "id": id, "what": "brightness", "value": 6 }));
    s.wait("brightness", |v| v["devices"][0]["status"]["brightness"] == 6);
    s.call("set_value", json!({ "id": id, "what": "angle", "value": -45 }));
    s.wait("angle", |v| v["devices"][0]["status"]["angle"] == -45);
    s.call("control", json!({ "id": id, "action": "on" }));
    s.wait("on", |v| v["devices"][0]["status"]["power"] == true);
    let (_, v) = s.post("/api/call/nope", json!({}));
    assert_eq!(v["ok"], false);
}

#[test]
fn text_and_image_like_addon_1x() {
    let s = start(true);
    s.wait("connected", |v| v["connected"] == true);
    // 1.x /text with show_seconds: shown, then removed again
    let (code, v) = s.post("/text", json!({ "text": "0,23 euro/kWh", "show_seconds": 2 }));
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["success"], true, "{v}");
    let slot = v["results"][0]["slot"].as_u64().unwrap();
    assert!(slot >= 1);
    s.wait("text in playlist", |st| st["playlist"].as_array().is_some_and(|p| p.iter().any(|e| e["name"].as_str().unwrap_or("").starts_with("ha_"))));
    s.wait("text removed after 2 s", |st| st["playlist"].as_array().is_some_and(|p| p.is_empty()));
    // new API: image as data, kept
    let png = {
        let img = image_png();
        base64_encode(&img)
    };
    let (code, v) = s.post("/api/show", json!({ "data": png, "name": "plaatje" }));
    assert_eq!(code, 200, "{v}");
    s.wait("image kept", |st| st["playlist"].as_array().is_some_and(|p| p.iter().any(|e| e["name"] == "plaatje.mp4")));
    // same name again replaces the file instead of adding a second one
    let (code, v) = s.post("/api/show", json!({ "text": "nieuw", "name": "plaatje" }));
    assert_eq!(code, 200, "{v}");
    std::thread::sleep(Duration::from_millis(500));
    s.wait("replaced", |st| st["playlist"].as_array().is_some_and(|p| p.iter().filter(|e| e["name"] == "plaatje.mp4").count() == 1));
    let mac = s.get("/api/state")["devices"][0]["mac"].as_str().unwrap().to_string();
    let shown = ureq::get(&format!("{}/api/shown/{mac}", s.base)).call().unwrap();
    assert_eq!(shown.content_type(), "image/png");
    // the text really is on the disc: some, but far from all, pixels lit
    let mut png = Vec::new();
    shown.into_reader().read_to_end(&mut png).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgb8();
    let lit = img.pixels().filter(|p| p[0] > 128).count() as f64 / (img.width() * img.height()) as f64;
    assert!((0.02..0.5).contains(&lit), "shown picture is not text: {:.1}% lit", lit * 100.0);
    // live cast mode stores nothing
    let (code, v) = s.post("/api/show", json!({ "text": "LIVE", "mode": "cast", "show_seconds": 1, "color": "geel" }));
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["mode"], "cast");
    // 1.x /delete
    let (code, v) = s.post("/delete", json!({ "slot": 1 }));
    assert_eq!(code, 200, "{v}");
    s.wait("deleted", |st| st["playlist"].as_array().is_some_and(|p| p.is_empty()));
}

#[test]
fn events_stream() {
    let s = start(true);
    let st = s.wait("connected", |v| v["connected"] == true);
    let id = st["devices"][0]["id"].as_u64().unwrap();
    let addr = s.base.trim_start_matches("http://").to_string();
    let mut sock = TcpStream::connect(&addr).unwrap();
    write!(sock, "GET /api/events HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut rd = BufReader::new(sock);
    let mut line = String::new();
    rd.read_line(&mut line).unwrap();
    assert!(line.contains("200"), "{line}");
    s.call("control", json!({ "id": id, "action": "pause" }));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        line.clear();
        rd.read_line(&mut line).unwrap();
        if line.starts_with("data:") && line.contains("\"status\"") { break; }
        assert!(Instant::now() < deadline, "no status event");
    }
}

fn image_png() -> Vec<u8> {
    let img = image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([(x * 4) as u8, (y * 4) as u8, 200]));
    let mut v = Vec::new();
    image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).unwrap();
    v
}

fn base64_encode(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}
#[test]
fn virtual_fan_follows_the_options_file() {
    let dir = std::env::temp_dir().join(format!("p56-opts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let opts = dir.join("options.json");
    std::fs::write(&opts, r#"{"simulate": false}"#).unwrap();
    let (http, tcp) = (free_tcp(), free_tcp());
    let child = Command::new(env!("CARGO_BIN_EXE_p56-server"))
        .args(["--http-port", &http.to_string(), "--tcp-port", &tcp.to_string(), "--sim-udp-port", &free_udp().to_string()])
        .arg("--options-file").arg(&opts)
        .env("RUST_LOG", "warn").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let s = Srv { child, base: format!("http://127.0.0.1:{http}") };
    let deadline = Instant::now() + Duration::from_secs(10);
    while ureq::get(&format!("{}/api/ping", s.base)).call().is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(s.get("/api/state")["connected"], false, "no virtual fan while the option is off");
    std::fs::write(&opts, r#"{"simulate": true, "log_level": "info"}"#).unwrap();
    s.wait("virtual fan switched on", |v| v["connected"] == true);
    std::fs::write(&opts, r#"{"simulate": false}"#).unwrap();
    s.wait("virtual fan switched off", |v| v["connected"] == false);
    std::fs::write(&opts, r#"{"simulate": true}"#).unwrap();
    s.wait("virtual fan switched on again", |v| v["connected"] == true);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn virtual_fan_follows_the_supervisor_options() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    // fake Supervisor: answers /addons/self/info like Home Assistant, only with the right token
    let simulate = Arc::new(AtomicBool::new(false));
    let sup = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let sup_port = sup.server_addr().to_ip().unwrap().port();
    let flag = simulate.clone();
    std::thread::spawn(move || {
        for req in sup.incoming_requests() {
            let authed = req.headers().iter().any(|h| h.field.equiv("Authorization") && h.value.as_str() == "Bearer test-token");
            let body = json!({ "result": "ok", "data": { "options": { "simulate": flag.load(Ordering::SeqCst), "lan_api": true } } }).to_string();
            let resp = tiny_http::Response::from_string(body).with_status_code(if authed { 200 } else { 401 });
            let _ = req.respond(resp);
        }
    });
    // the options file keeps saying false, as on Home Assistant until the add-on restarts
    let dir = std::env::temp_dir().join(format!("p56-sup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let opts = dir.join("options.json");
    std::fs::write(&opts, r#"{"simulate": false}"#).unwrap();
    let (http, tcp) = (free_tcp(), free_tcp());
    let child = Command::new(env!("CARGO_BIN_EXE_p56-server"))
        .args(["--http-port", &http.to_string(), "--tcp-port", &tcp.to_string(), "--sim-udp-port", &free_udp().to_string()])
        .arg("--options-file").arg(&opts)
        .args(["--options-url", &format!("http://127.0.0.1:{sup_port}/addons/self/info")])
        .env("SUPERVISOR_TOKEN", "test-token").env("RUST_LOG", "warn")
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let s = Srv { child, base: format!("http://127.0.0.1:{http}") };
    let deadline = Instant::now() + Duration::from_secs(10);
    while ureq::get(&format!("{}/api/ping", s.base)).call().is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(s.get("/api/state")["connected"], false);
    simulate.store(true, Ordering::SeqCst);
    s.wait("option saved on -> virtual fan", |v| v["connected"] == true);
    simulate.store(false, Ordering::SeqCst);
    s.wait("option saved off -> gone", |v| v["connected"] == false);
    let _ = std::fs::remove_dir_all(&dir);
}
