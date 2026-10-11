//! The `p56-server` binary end to end: HTTP API, add-on 1.x endpoints, SSE and the web UI,
//! against its built-in simulated fan.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream, UdpSocket};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn free_tcp() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port()
}
fn free_udp() -> u16 {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port()
}

struct Srv {
    child: Child,
    base: String,
    http: u16,
    tcp: u16,
}
impl Drop for Srv {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn start(simulate: bool) -> Srv {
    start_with(simulate, &[])
}

fn start_with(simulate: bool, extra: &[&str]) -> Srv {
    start_env(simulate, extra, &[])
}

fn start_env(simulate: bool, extra: &[&str], envs: &[(&str, &str)]) -> Srv {
    let (http, tcp) = (free_tcp(), free_tcp());
    let mut args = vec!["--http-port".to_string(), http.to_string(), "--tcp-port".into(), tcp.to_string()];
    if simulate {
        args.extend(["--simulate".into(), "--sim-udp-port".into(), free_udp().to_string()]);
    }
    args.extend(extra.iter().map(|s| s.to_string()));
    let child = Command::new(env!("CARGO_BIN_EXE_p56-server"))
        .args(&args)
        .env("RUST_LOG", "warn")
        .env_remove("P56_API_TOKEN")
        .env_remove("SUPERVISOR_TOKEN")
        .env_remove("P56_HA_URL")
        .env_remove("P56_HA_TOKEN")
        .envs(envs.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let base = format!("http://127.0.0.1:{http}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while ureq::get(&format!("{base}/api/ping")).call().is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(100));
    }
    Srv { child, base, http, tcp }
}

impl Srv {
    fn get(&self, p: &str) -> Value {
        ureq::get(&format!("{}{p}", self.base)).call().unwrap().into_json().unwrap()
    }
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
            if pred(&s) {
                return s;
            }
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
    let (code, _) = s.post("/text", json!({ "text": "hi" }));
    assert_eq!(code, 503);
    let (code, _) = s.post("/on", json!({}));
    assert_eq!(code, 503);
}

#[test]
fn api_controls_the_virtual_fan() {
    let s = start(true);
    let st = s.wait("virtual fan connects", |v| v["connected"] == true && v["devices"][0]["status"].is_object());
    let id = st["devices"][0]["id"].as_u64().unwrap();
    assert_eq!(st["devices"][0]["name"], "VIRTUAL-P56");
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
    let (code, v) = s.post("/api/show", json!({ "data": png, "name": "picture" }));
    assert_eq!(code, 200, "{v}");
    s.wait("image kept", |st| st["playlist"].as_array().is_some_and(|p| p.iter().any(|e| e["name"] == "picture.mp4")));
    // same name again replaces the file instead of adding a second one
    let (code, v) = s.post("/api/show", json!({ "text": "new", "name": "picture" }));
    assert_eq!(code, 200, "{v}");
    std::thread::sleep(Duration::from_millis(500));
    s.wait("replaced", |st| st["playlist"].as_array().is_some_and(|p| p.iter().filter(|e| e["name"] == "picture.mp4").count() == 1));
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
    let (code, v) = s.post("/api/show", json!({ "text": "LIVE", "mode": "cast", "show_seconds": 1, "color": "yellow" }));
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
        if line.starts_with("data:") && line.contains("\"status\"") {
            break;
        }
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
        .arg("--options-file")
        .arg(&opts)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let s = Srv { child, base: format!("http://127.0.0.1:{http}"), http, tcp };
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
        .arg("--options-file")
        .arg(&opts)
        .args(["--options-url", &format!("http://127.0.0.1:{sup_port}/addons/self/info")])
        .env("SUPERVISOR_TOKEN", "test-token")
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let s = Srv { child, base: format!("http://127.0.0.1:{http}"), http, tcp };
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

fn shown_png(s: &Srv) -> image::RgbImage {
    let mac = s.get("/api/state")["devices"][0]["mac"].as_str().unwrap().to_string();
    let mut png = Vec::new();
    ureq::get(&format!("{}/api/shown/{mac}", s.base)).call().unwrap().into_reader().read_to_end(&mut png).unwrap();
    image::load_from_memory(&png).unwrap().to_rgb8()
}

fn gif(frames: &[[u8; 3]]) -> Vec<u8> {
    use image::codecs::gif::{GifEncoder, Repeat};
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for c in frames {
            let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([c[0], c[1], c[2], 255]));
            enc.encode_frame(image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(80, 1))).unwrap();
        }
    }
    out
}

#[test]
fn styles_icons_colour_scale_and_animations() {
    let s = start(true);
    s.wait("connected", |v| v["connected"] == true);
    // ready-made style: red ring along the rim + alert icon
    let (code, v) = s.post("/api/show", json!({ "text": "Door open", "style": "alert", "name": "styled" }));
    assert_eq!(code, 200, "{v}");
    let img = shown_png(&s);
    let reddish = img.pixels().filter(|p| p[0] > 180 && p[1] < 110 && p[2] < 110).count();
    assert!(reddish > 2_000, "alert ring is red: {reddish} red pixels");
    // colour from a value: high price -> red text, low -> green text
    let (code, v) = s.post("/api/show", json!({ "text": "0.39", "color_scale": { "value": 0.39, "min": 0.20, "max": 0.35 }, "name": "price" }));
    assert_eq!(code, 200, "{v}");
    let img = shown_png(&s);
    let red = img.pixels().filter(|p| p[0] > 180 && p[1] < 110).count();
    let green = img.pixels().filter(|p| p[1] > 160 && p[0] < 110).count();
    assert!(red > 1_000 && green < 100, "red text for a high value: red {red}, green {green}");
    // icon only
    let (code, v) = s.post("/api/show", json!({ "icon": "mdi:washing-machine", "name": "icon" }));
    assert_eq!(code, 200, "{v}");
    // animated GIF keeps all its frames
    let (code, v) = s.post("/api/show", json!({ "data": base64_encode(&gif(&[[255, 0, 0], [0, 255, 0], [0, 0, 255]])), "name": "anim" }));
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["results"][0]["frames"], 3, "{v}");
    // a list of frames
    let frames = vec![base64_encode(&image_png()), base64_encode(&image_png())];
    let (code, v) = s.post("/api/show", json!({ "frames": frames, "name": "frames" }));
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["results"][0]["frames"], 2, "{v}");
    // mistakes are explained
    let (code, v) = s.post("/api/show", json!({ "text": "x", "icon": "mdi:not-an-icon" }));
    assert_eq!(code, 500);
    assert!(v["error"].as_str().unwrap_or("").contains("unknown icon"), "{v}");
    let (code, v) = s.post("/api/show", json!({ "text": "x", "style": "fancy" }));
    assert_eq!(code, 500);
    assert!(v["error"].as_str().unwrap_or("").contains("unknown style"), "{v}");
    s.wait("files on the fan", |st| {
        st["playlist"]
            .as_array()
            .is_some_and(|p| ["styled.mp4", "price.mp4", "icon.mp4", "anim.mp4", "frames.mp4"].iter().all(|n| p.iter().any(|e| e["name"] == *n)))
    });
}

// ---- auto-delete timers (show_seconds) --------------------------------------------------

fn names(st: &Value) -> Vec<String> {
    st["playlist"].as_array().map(|p| p.iter().filter_map(|e| e["name"].as_str().map(String::from)).collect()).unwrap_or_default()
}

fn slot_of(st: &Value, name: &str) -> Option<u64> {
    st["playlist"].as_array()?.iter().find(|e| e["name"] == name).and_then(|e| e["slot"].as_u64())
}

fn connected_fan(s: &Srv) -> u64 {
    let st = s.wait("virtual fan connects", |v| v["connected"] == true && v["devices"][0]["status"].is_object() && v["devices"][0]["mac"] != "");
    st["devices"][0]["id"].as_u64().unwrap()
}

fn show_ok(s: &Srv, body: Value) -> Value {
    let (code, v) = s.post("/api/show", body);
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["success"], true, "{v}");
    v
}

#[test]
fn stale_timer_keeps_a_newer_upload_with_the_same_name() {
    let s = start(true);
    connected_fan(&s);
    show_ok(&s, json!({ "text": "for a moment", "name": "x", "show_seconds": 3 }));
    // the same name again, now to keep: the first upload's timer must not remove it
    show_ok(&s, json!({ "text": "for good", "name": "x", "show_seconds": 0 }));
    std::thread::sleep(Duration::from_secs(5));
    let st = s.get("/api/state");
    assert_eq!(names(&st).iter().filter(|n| *n == "x.mp4").count(), 1, "the permanent x must stay: {st}");
}

#[test]
fn stale_timer_never_deletes_another_file_in_the_same_slot() {
    let s = start(true);
    let id = connected_fan(&s);
    let v = show_ok(&s, json!({ "text": "for a moment", "name": "tmp", "show_seconds": 4 }));
    let slot = v["results"][0]["slot"].as_u64().unwrap();
    // removed by hand before its timer runs ...
    s.call("set_value", json!({ "id": id, "what": "delete", "value": slot }));
    s.wait("tmp deleted by hand", |st| !names(st).contains(&"tmp.mp4".to_string()));
    // ... and another file takes that slot
    let v = show_ok(&s, json!({ "text": "other", "name": "other" }));
    assert_eq!(v["results"][0]["slot"].as_u64(), Some(slot), "{v}");
    std::thread::sleep(Duration::from_secs(6));
    let st = s.get("/api/state");
    assert!(names(&st).contains(&"other.mp4".to_string()), "the timer of tmp deleted another file: {st}");
}

#[test]
fn temporary_file_returns_to_the_previous_file_by_name() {
    let s = start(true);
    let id = connected_fan(&s);
    for n in ["a", "b", "c"] {
        show_ok(&s, json!({ "text": n, "name": n }));
    }
    s.call("set_value", json!({ "id": id, "what": "select", "value": 3 }));
    s.wait("c plays", |st| st["devices"][0]["status"]["play_index"] == 3 && slot_of(st, "c.mp4") == Some(3));
    show_ok(&s, json!({ "text": "note", "name": "note", "show_seconds": 4 }));
    // the slots shift while the note is shown: c moves from 3 to 2
    s.call("set_value", json!({ "id": id, "what": "delete", "value": 1 }));
    s.wait("a deleted", |st| names(st) == ["b.mp4", "c.mp4", "note.mp4"]);
    let st = s.wait("note removed, c plays again", |st| {
        !names(st).contains(&"note.mp4".to_string()) && st["devices"][0]["status"]["play_index"].as_u64() == slot_of(st, "c.mp4")
    });
    assert_eq!(names(&st), ["b.mp4", "c.mp4"], "{st}");
}

// ---- live cast to one fan -----------------------------------------------------------------

#[test]
fn cast_reaches_only_the_selected_fan() {
    let s = start(false);
    let pc = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, s.tcp));
    let sim = |name: &str, last: u8| {
        p56_sim::Sim::start(p56_sim::SimConfig {
            name: name.into(),
            udp_port: 0,
            pc: Some(pc),
            mac: [0x02, 0xE0, 0xE0, 0x56, 0x01, last],
            ..Default::default()
        })
        .unwrap()
    };
    let (a, b) = (sim("FAN-A", 0xA1), sim("FAN-B", 0xB2));
    s.wait("two fans", |v| {
        v["devices"].as_array().is_some_and(|d| d.len() == 2 && d.iter().all(|x| x["status"].is_object() && x["mac"].as_str().is_some_and(|m| !m.is_empty())))
    });
    let (code, v) = s.post("/api/show", json!({ "text": "LIVE", "mode": "cast", "show_seconds": 1, "mac": "02:E0:E0:56:01:A1" }));
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["fans"], 1, "{v}");
    std::thread::sleep(Duration::from_secs(2));
    let (na, nb) = (a.state().cast_ok, b.state().cast_ok);
    assert!(na > 0, "the selected fan got no cast frames");
    assert_eq!(nb, 0, "the other fan got {nb} cast frames");
    assert!(a.state().protocol_errors.is_empty() && b.state().protocol_errors.is_empty());
}

/// A timed cast from `show` is a cast session like any other: `cast_stop` ends it, and a new one
/// replaces it instead of running next to it.
#[test]
fn timed_cast_stops_and_is_replaced() {
    let s = start(false);
    let pc = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, s.tcp));
    let fan = p56_sim::Sim::start(p56_sim::SimConfig { name: "FAN".into(), udp_port: 0, pc: Some(pc), ..Default::default() }).unwrap();
    s.wait("fan", |v| v["devices"].as_array().is_some_and(|d| d.len() == 1 && d[0]["status"].is_object()));
    let rate = || {
        let n = fan.state().cast_ok;
        std::thread::sleep(Duration::from_millis(1500));
        fan.state().cast_ok - n
    };
    let (code, v) = s.post("/api/show", json!({ "text": "ONE", "mode": "cast", "show_seconds": 60 }));
    assert_eq!(code, 200, "{v}");
    assert!(rate() > 0, "the timed cast sends frames");
    s.call("cast_stop", json!({}));
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(rate(), 0, "cast_stop ends a timed cast");
    // two timed casts in a row: only the second one sends (one frame per 200 ms, not two)
    s.post("/api/show", json!({ "text": "A", "mode": "cast", "show_seconds": 60 }));
    s.post("/api/show", json!({ "text": "B", "mode": "cast", "show_seconds": 60 }));
    std::thread::sleep(Duration::from_millis(600));
    let n = rate();
    assert!((3..=10).contains(&n), "one session at ~5 frames/s, got {n} frames in 1.5 s");
    s.call("cast_stop", json!({}));
    assert!(fan.state().protocol_errors.is_empty(), "{:?}", fan.state().protocol_errors);
}

/// A fan that is off shows nothing that is cast to it: every kind of cast turns it on.
#[test]
fn casting_turns_the_fan_on() {
    let s = start(true);
    let st = s.wait("virtual fan connects", |v| v["connected"] == true && v["devices"][0]["status"].is_object());
    let id = st["devices"][0]["id"].as_u64().unwrap();
    let off = || {
        s.call("control", json!({ "id": id, "action": "off" }));
        s.wait("off", |v| v["devices"][0]["status"]["power"] == false);
    };
    off();
    s.call("cast_start", json!({ "args": { "source": "test" } }));
    s.wait("on by cast_start", |v| v["devices"][0]["status"]["power"] == true);
    s.call("cast_stop", json!({}));
    off();
    s.call("cast_frame", json!({ "data": base64_encode(&image_png()) }));
    s.wait("on by a camera frame", |v| v["devices"][0]["status"]["power"] == true);
    s.call("cast_stop", json!({}));
    off();
    show_ok(&s, json!({ "text": "ON", "mode": "cast", "show_seconds": 2 }));
    s.wait("on by a timed cast", |v| v["devices"][0]["status"]["power"] == true);
}

/// A stand-in for Home Assistant's REST API: two entities, one camera that needs the token.
fn fake_home_assistant() -> u16 {
    let srv = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = srv.server_addr().to_ip().unwrap().port();
    std::thread::spawn(move || {
        for req in srv.incoming_requests() {
            let authed = req.headers().iter().any(|h| h.field.equiv("Authorization") && h.value.as_str() == "Bearer test-token");
            let r = match (authed, req.url()) {
                (false, _) => tiny_http::Response::from_data(b"401".to_vec()).with_status_code(401),
                (true, "/states") => tiny_http::Response::from_data(
                    serde_json::to_vec(&json!([
                        { "entity_id": "light.kitchen", "attributes": { "friendly_name": "Kitchen" } },
                        { "entity_id": "camera.front_door", "attributes": { "friendly_name": "Front door" } },
                    ]))
                    .unwrap(),
                ),
                (true, "/camera_proxy/camera.front_door") => tiny_http::Response::from_data(image_png()),
                _ => tiny_http::Response::from_data(b"404".to_vec()).with_status_code(404),
            };
            let _ = req.respond(r);
        }
    });
    port
}

/// In the add-on the server lists Home Assistant's cameras and casts one of them; outside Home
/// Assistant both calls fail clearly, and only camera entities are ever fetched.
#[test]
fn home_assistant_cameras_are_listed_and_cast() {
    let plain = start(true);
    let (_, v) = plain.post("/api/call/ha_cameras", json!({}));
    assert_eq!(v["ok"], false, "{v}");
    drop(plain);

    let ha = format!("http://127.0.0.1:{}", fake_home_assistant());
    let s = start_env(false, &[], &[("P56_HA_URL", &ha), ("P56_HA_TOKEN", "test-token")]);
    let pc = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, s.tcp));
    let fan = p56_sim::Sim::start(p56_sim::SimConfig { name: "FAN".into(), udp_port: 0, pc: Some(pc), ..Default::default() }).unwrap();
    s.wait("fan", |v| v["devices"].as_array().is_some_and(|d| d.len() == 1 && d[0]["status"].is_object()));
    assert_eq!(s.call("ha_cameras", json!({})), json!([{ "entity_id": "camera.front_door", "name": "Front door" }]));
    for bad in ["ha_camera:light.kitchen", "ha_camera:camera.x/../../states", "ha_camera:camera."] {
        let (_, v) = s.post("/api/call/cast_start", json!({ "args": { "source": bad } }));
        assert_eq!(v["ok"], false, "{bad}: {v}");
    }
    s.call("cast_start", json!({ "args": { "source": "ha_camera:camera.front_door", "fps": 10 } }));
    let deadline = Instant::now() + Duration::from_secs(10);
    while fan.state().cast_ok < 10 {
        assert!(Instant::now() < deadline, "the camera was not cast: {:?}", fan.state().protocol_errors);
        std::thread::sleep(Duration::from_millis(100));
    }
    s.call("cast_stop", json!({}));
    assert!(fan.state().protocol_errors.is_empty(), "{:?}", fan.state().protocol_errors);
}

// ---- API token and media folders -------------------------------------------------------

/// This machine's address on the network (no packet is sent), if it has one.
fn lan_ip() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() && !(ip.octets()[0] == 172 && ip.octets()[1] == 30) => Some(ip),
        _ => None,
    }
}

fn answer(r: Result<ureq::Response, ureq::Error>) -> (u16, Value) {
    match r {
        Ok(r) => (r.status(), if r.content_type() == "application/json" { r.into_json().unwrap_or(Value::Null) } else { Value::Null }),
        Err(ureq::Error::Status(c, r)) => (c, r.into_json().unwrap_or(Value::Null)),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn the_network_is_refused_unless_lan_is_given() {
    let Some(ip) = lan_ip() else {
        eprintln!("skipped: this machine has no non-loopback IPv4 address");
        return;
    };
    let s = start(true);
    let (code, _) = answer(ureq::get(&format!("http://{ip}:{}/api/state", s.http)).timeout(Duration::from_secs(10)).call());
    assert_eq!(code, 403, "without --lan the network gets no answer");
    assert!(s.get("/api/state")["version"].is_string(), "this machine does");
}

#[test]
fn moving_text_ticker_and_orbit() {
    let s = start(true);
    s.wait("virtual fan", |v| v["devices"].as_array().is_some_and(|d| !d.is_empty() && d[0]["status"].is_object()));
    let (code, v) = s.post("/api/show", json!({ "text": "Lichtkrant", "motion": "scroll", "name": "ticker" }));
    assert_eq!(code, 200, "{v}");
    let n = v["results"][0]["fan_frames"].as_u64().unwrap();
    assert!(n > 20, "a ticker is an animation at 20 fps: {v}");
    let (code, v) = s.post("/api/show", json!({ "text": "Rondje", "motion": "orbit", "speed": 2, "name": "orbit", "color": "orange" }));
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["results"][0]["fan_frames"], 40, "one turn in 2 s: {v}");
    let (code, v) = s.post("/api/show", json!({ "text": "x", "motion": "spin" }));
    assert_eq!(code, 500);
    assert!(v["error"].as_str().unwrap_or("").contains("scroll or orbit"), "{v}");
    s.wait("files on the fan", |st| st["playlist"].as_array().is_some_and(|p| ["ticker.mp4", "orbit.mp4"].iter().all(|n| p.iter().any(|e| e["name"] == *n))));
}

/// A web page on another site may not make this machine's browser call the API (CSRF through
/// localhost); P56 Studio's own origin gets CORS headers; clients without an Origin keep working.
#[test]
fn foreign_web_pages_are_refused_and_the_app_gets_cors() {
    let s = start(false);
    let url = format!("{}/api/call/devices", s.base);
    let (code, v) = answer(ureq::post(&url).set("Origin", "https://evil.example").send_json(json!({})));
    assert_eq!(code, 403, "{v}");
    assert_eq!(answer(ureq::get(&format!("{}/api/state", s.base)).set("Origin", "http://evil.example").call()).0, 403);
    // the page itself (same origin) and clients without an Origin
    let own = format!("http://127.0.0.1:{}", s.http);
    assert_eq!(answer(ureq::post(&url).set("Origin", &own).send_json(json!({}))).0, 200);
    assert_eq!(answer(ureq::post(&url).send_json(json!({}))).0, 200);
    // P56 Studio: CORS headers on the answer and on the preflight
    for app in ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"] {
        let r = ureq::post(&url).set("Origin", app).send_json(json!({})).unwrap();
        assert_eq!(r.header("Access-Control-Allow-Origin"), Some(app));
        let r = ureq::request("OPTIONS", &url).set("Origin", app).set("Access-Control-Request-Method", "POST").call().unwrap();
        assert_eq!(r.status(), 204);
        assert_eq!(r.header("Access-Control-Allow-Origin"), Some(app));
        assert!(r.header("Access-Control-Allow-Headers").is_some_and(|h| h.contains("Authorization")));
    }
}

#[test]
fn too_large_bodies_get_413() {
    let s = start(false);
    let big = "x".repeat(64 * 1024 * 1024 + 1);
    let (code, _) = answer(ureq::post(&format!("{}/api/show", s.base)).set("Content-Type", "application/json").send_string(&big));
    assert_eq!(code, 413);
}

#[test]
fn api_token_is_required_from_the_network_only() {
    let Some(ip) = lan_ip() else {
        eprintln!("skipped: this machine has no non-loopback IPv4 address");
        return;
    };
    let token = "s3cret/+token";
    let s = start_with(true, &["--lan", "--api-token", token]);
    let lan = format!("http://{ip}:{}", s.http);
    let get = |p: &str, auth: Option<&str>| {
        let mut r = ureq::get(&format!("{lan}{p}")).timeout(Duration::from_secs(10));
        if let Some(a) = auth {
            r = r.set("Authorization", a);
        }
        answer(r.call())
    };
    // from the network without (or with a wrong) token: refused
    let (code, v) = get("/api/state", None);
    assert_eq!((code, v["error"].as_str()), (401, Some("Unauthorized")), "{v}");
    assert_eq!(get("/api/state", Some("Bearer wrong")).0, 401);
    assert_eq!(get("/api/state?token=wrong", None).0, 401);
    assert_eq!(get("/api/shown/02:e0:e0:56:00:f5", None).0, 401);
    for p in ["/status", "/health"] {
        assert_eq!(get(p, None).0, 401, "{p}");
    }
    for p in ["/api/call/devices", "/api/show", "/text", "/upload", "/delete", "/on", "/off"] {
        let (code, _) = answer(ureq::post(&format!("{lan}{p}")).send_json(json!({ "text": "x" })));
        assert_eq!(code, 401, "POST {p}");
    }
    // with the token, as a header or (for EventSource) in the query
    assert_eq!(get("/api/state", Some(&format!("Bearer {token}"))).0, 200);
    assert_eq!(get("/api/state?token=s3cret%2F%2Btoken", None).0, 200);
    let (code, v) = answer(ureq::post(&format!("{lan}/api/call/devices")).set("Authorization", &format!("bearer {token}")).send_json(json!({})));
    assert_eq!((code, &v["ok"]), (200, &json!(true)), "{v}");
    // the page itself and the ping stay open
    assert_eq!(get("/", None).0, 200);
    assert_eq!(get("/api/ping", None).0, 200);
    // this machine (loopback) needs no token
    assert!(s.get("/api/state")["version"].is_string());
    let (code, _) = s.post("/api/call/devices", json!({}));
    assert_eq!(code, 200);
}

#[test]
fn show_reads_files_only_inside_media_dirs() {
    let dir = std::env::temp_dir().join(format!("p56-media-{}", std::process::id()));
    let (media, outside) = (dir.join("media"), dir.join("outside"));
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(media.join("pic.png"), image_png()).unwrap();
    std::fs::write(outside.join("secret.png"), image_png()).unwrap();
    let p = |p: std::path::PathBuf| p.to_str().unwrap().to_string();
    let s = start_with(true, &["--media-dir", media.to_str().unwrap()]);
    connected_fan(&s);
    let v = show_ok(&s, json!({ "path": p(media.join("pic.png")), "name": "inside" }));
    assert_eq!(v["results"][0]["name"], "inside.mp4", "{v}");
    for bad in [outside.join("secret.png"), media.join("..").join("outside").join("secret.png"), media.join("missing.png")] {
        let (code, v) = s.post("/api/show", json!({ "path": p(bad.clone()) }));
        assert_eq!(code, 403, "{}: {v}", bad.display());
        assert!(v["error"].as_str().unwrap_or("").contains("not allowed"), "{v}");
    }
    // the 1.x endpoint and the command API follow the same rule
    let (code, _) = s.post("/upload", json!({ "image": p(outside.join("secret.png")) }));
    assert_eq!(code, 403);
    let (_, v) = s.post("/api/call/show_image", json!({ "path": p(outside.join("secret.png")) }));
    assert_eq!(v["ok"], false, "{v}");
    // without --media-dir no file is read at all
    let s2 = start(false);
    let (code, v) = s2.post("/api/show", json!({ "path": p(media.join("pic.png")) }));
    assert_eq!(code, 403, "{v}");
    assert!(v["error"].as_str().unwrap_or("").contains("disabled"), "{v}");
    let _ = std::fs::remove_dir_all(&dir);
}
