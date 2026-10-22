//! The `p56-server` binary end to end: HTTP API, add-on 1.x endpoints, SSE and the web UI,
//! against its built-in simulated fan.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

struct Srv {
    child: Child,
    base: String,
    http: u16,
    tcp: u16,
    /// everything the server printed, shown when the test fails
    log: PathBuf,
}
impl Drop for Srv {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if std::thread::panicking() {
            eprintln!("---- p56-server on :{} ----\n{}", self.http, std::fs::read_to_string(&self.log).unwrap_or_default());
        }
        let _ = std::fs::remove_file(&self.log);
    }
}

fn start(simulate: bool) -> Srv {
    start_with(simulate, &[])
}

fn start_with(simulate: bool, extra: &[&str]) -> Srv {
    start_env(simulate, extra, &[])
}

/// The server opens free ports itself (port 0) and writes them to a file: a port picked here
/// could be taken by another test before the server opens it.
fn start_env(simulate: bool, extra: &[&str], envs: &[(&str, &str)]) -> Srv {
    static N: AtomicUsize = AtomicUsize::new(0);
    let tmp = std::env::temp_dir().join(format!("p56-server-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let (ports, log) = (tmp.with_extension("ports"), tmp.with_extension("log"));
    let _ = std::fs::remove_file(&ports);
    let mut args = vec!["--http-port", "0", "--tcp-port", "0", "--sim-udp-port", "0", "--ports-file", ports.to_str().unwrap()];
    if simulate {
        args.push("--simulate");
    }
    args.extend(extra);
    let out = std::fs::File::create(&log).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_p56-server"))
        .args(&args)
        .env("RUST_LOG", "info")
        .env_remove("P56_API_TOKEN")
        .env_remove("SUPERVISOR_TOKEN")
        .env_remove("P56_HA_URL")
        .env_remove("P56_HA_TOKEN")
        .envs(envs.iter().copied())
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .spawn()
        .unwrap();
    let mut s = Srv { child, base: String::new(), http: 0, tcp: 0, log };
    let deadline = Instant::now() + Duration::from_secs(10);
    let p: Value = loop {
        if let Some(p) = std::fs::read(&ports).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
            break p;
        }
        if let Some(exit) = s.child.try_wait().unwrap() {
            panic!("server stopped: {exit}");
        }
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = std::fs::remove_file(&ports);
    (s.http, s.tcp) = (p["http"].as_u64().unwrap() as u16, p["tcp"].as_u64().unwrap() as u16);
    s.base = format!("http://127.0.0.1:{}", s.http);
    s
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

/// A 4×4 sprite sheet of 400×400: every cell a different shade of red.
fn sheet_png() -> Vec<u8> {
    let img = image::RgbImage::from_fn(400, 400, |x, y| image::Rgb([((y / 100 * 4 + x / 100) * 15 + 10) as u8, 0, 0]));
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
    let s = start_with(false, &["--options-file", opts.to_str().unwrap()]);
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
    use std::sync::atomic::AtomicBool;
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
    let url = format!("http://127.0.0.1:{sup_port}/addons/self/info");
    let s = start_env(false, &["--options-file", opts.to_str().unwrap(), "--options-url", &url], &[("SUPERVISOR_TOKEN", "test-token")]);
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

/// A preview of every file the server uploaded: by name, moved along on a rename and kept on
/// disk (--thumb-dir) so it survives a restart. The fan itself cannot send files back.
#[test]
fn previews_of_uploaded_files() {
    let dir = std::env::temp_dir().join(format!("p56-thumbs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = start_with(true, &["--thumb-dir", dir.to_str().unwrap()]);
    let id = connected_fan(&s);
    show_ok(&s, json!({ "text": "Hi", "name": "hello" }));
    s.wait("hello listed", |st| slot_of(st, "hello.mp4").is_some());
    let t = s.call("thumbs", json!({ "id": id }));
    let url = t["hello.mp4"].as_str().unwrap_or_default();
    assert!(url.starts_with("data:image/jpeg;base64,"), "{t}");
    let files = || walk(&dir);
    assert_eq!(files().len(), 1, "{:?}", files());
    let slot = slot_of(&s.get("/api/state"), "hello.mp4").unwrap();
    s.call("set_text", json!({ "id": id, "what": "rename", "a": "greeting", "slot": slot }));
    s.wait("renamed", |st| slot_of(st, "greeting.mp4").is_some());
    let t = s.call("thumbs", json!({ "id": id }));
    assert!(t["greeting.mp4"].is_string() && t.get("hello.mp4").is_none(), "{t}");
    assert!(files().iter().any(|f| f.ends_with("greeting.mp4.jpg")), "{:?}", files());
    let _ = std::fs::remove_dir_all(&dir);
}

fn walk(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().is_dir() {
            out.extend(walk(&e.path()));
        } else {
            out.push(e.path().display().to_string());
        }
    }
    out
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
    // two timed casts in a row: only the second one sends. A still is repeated every 100 ms so
    // the real P56 never falls out of live mode (it starts falling back after roughly 130 ms).
    s.post("/api/show", json!({ "text": "A", "mode": "cast", "show_seconds": 60 }));
    s.post("/api/show", json!({ "text": "B", "mode": "cast", "show_seconds": 60 }));
    std::thread::sleep(Duration::from_millis(600));
    let n = rate();
    assert!((10..=20).contains(&n), "one session at ~10 frames/s, got {n} frames in 1.5 s");
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
    // paused (as a P56 with an empty playlist): casting resumes it
    s.call("cast_stop", json!({}));
    s.call("control", json!({ "id": id, "action": "pause" }));
    s.wait("paused", |v| v["devices"][0]["status"]["playing"] == false);
    s.call("cast_start", json!({ "args": { "source": "test" } }));
    s.wait("playing by cast_start", |v| v["devices"][0]["status"]["playing"] == true);
    s.call("cast_stop", json!({}));
}

/// A stand-in for Home Assistant's REST API: two entities, one camera that needs the token.
fn fake_home_assistant() -> u16 {
    fake_home_assistant_counting().0
}

/// The fake Home Assistant, and how many pictures its AI made.
fn fake_home_assistant_counting() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let srv = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = srv.server_addr().to_ip().unwrap().port();
    let made = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = made.clone();
    std::thread::spawn(move || {
        for mut req in srv.incoming_requests() {
            let authed = req.headers().iter().any(|h| h.field.equiv("Authorization") && h.value.as_str() == "Bearer test-token");
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            let r = match (authed, req.url()) {
                // the AI's picture: a signed address, no token
                (_, "/ai_task/image/p56.png?authSig=signed") => tiny_http::Response::from_data(sheet_png()),
                (false, _) => tiny_http::Response::from_data(b"401".to_vec()).with_status_code(401),
                (true, "/states") => tiny_http::Response::from_data(
                    serde_json::to_vec(&json!([
                        { "entity_id": "light.kitchen", "attributes": { "friendly_name": "Kitchen" } },
                        { "entity_id": "camera.front_door", "attributes": { "friendly_name": "Front door" } },
                        { "entity_id": "ai_task.text_only", "attributes": { "friendly_name": "Text AI", "supported_features": 1 } },
                        { "entity_id": "ai_task.google_ai_task", "attributes": { "friendly_name": "Google AI Task", "supported_features": 7 } },
                    ]))
                    .unwrap(),
                ),
                (true, "/services/ai_task/generate_image?return_response") => {
                    let b: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                    let ok = b["entity_id"] == "ai_task.google_ai_task"
                        && b["instructions"]
                            .as_str()
                            .is_some_and(|i| (i.contains("black") && i.contains("Subject: a red fox")) || i.contains("situation: cheap power."));
                    if ok {
                        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        tiny_http::Response::from_data(
                            serde_json::to_vec(&json!({ "changed_states": [], "service_response": { "url": "/ai_task/image/p56.png?authSig=signed", "width": 400, "height": 400 } })).unwrap(),
                        )
                    } else {
                        tiny_http::Response::from_data(serde_json::to_vec(&json!({ "message": format!("bad request: {body}") })).unwrap()).with_status_code(400)
                    }
                }
                (true, "/camera_proxy/camera.front_door") => tiny_http::Response::from_data(image_png()),
                _ => tiny_http::Response::from_data(b"404".to_vec()).with_status_code(404),
            };
            let _ = req.respond(r);
        }
    });
    (port, made)
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

/// In the add-on a prompt becomes a picture through Home Assistant's AI Task: the entity that can
/// make pictures is found by itself, a still and a drawn animation become media items, and
/// `show_ai` puts one straight on the fan. Outside Home Assistant it fails clearly.
#[test]
fn ai_pictures_through_home_assistant() {
    let plain = start(true);
    let (_, v) = plain.post("/api/call/ai_status", json!({}));
    assert_eq!(v["ok"], false, "{v}");
    drop(plain);

    let ha = format!("http://127.0.0.1:{}", fake_home_assistant());
    let s = start_env(false, &[], &[("P56_HA_URL", &ha), ("P56_HA_TOKEN", "test-token")]);
    assert_eq!(s.call("ai_status", json!({})), json!({ "entities": [{ "entity_id": "ai_task.google_ai_task", "name": "Google AI Task" }] }));
    let still = s.call("ai_image", json!({ "prompt": "a red fox", "kind": "still", "art_style": "photo" }));
    assert_eq!((still["frames"].as_u64(), still["name"].as_str()), (Some(1), Some("ai a red fox")), "{still}");
    let anim = s.call("ai_image", json!({ "prompt": "a red fox", "kind": "anim", "name": "fox" }));
    assert_eq!(anim["frames"], 16, "{anim}");
    let spin = s.call("ai_image", json!({ "prompt": "a red fox", "kind": "spin" }));
    assert_eq!(spin["frames"], 24, "{spin}");
    for bad in [json!({ "prompt": "" }), json!({ "prompt": "fox", "kind": "video" }), json!({ "prompt": "fox", "entity_id": "light.kitchen" })] {
        let (_, v) = s.post("/api/call/ai_image", bad.clone());
        assert_eq!(v["ok"], false, "{bad}: {v}");
    }

    let pc = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, s.tcp));
    let fan = p56_sim::Sim::start(p56_sim::SimConfig { name: "FAN".into(), udp_port: 0, pc: Some(pc), ..Default::default() }).unwrap();
    s.wait("fan", |v| v["devices"].as_array().is_some_and(|d| d.len() == 1 && d[0]["status"].is_object()));
    let r = s.call("show_ai", json!({ "prompt": "a red fox", "kind": "anim", "name": "fox" }));
    assert_eq!(r["success"], true, "{r}");
    assert!(fan.state().files.iter().any(|f| f.name == "fox.mp4"), "{:?}", fan.state().files);
    assert!(fan.state().protocol_errors.is_empty(), "{:?}", fan.state().protocol_errors);
}

/// Text over a background the AI made (e.g. a price), and the same prompt again reuses that
/// background instead of paying for a new one, unless `fresh`.
#[test]
fn ai_background_behind_text_is_made_once() {
    let (port, made) = fake_home_assistant_counting();
    let ha = format!("http://127.0.0.1:{port}");
    let s = start_env(false, &[], &[("P56_HA_URL", &ha), ("P56_HA_TOKEN", "test-token")]);
    let pc = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, s.tcp));
    let fan = p56_sim::Sim::start(p56_sim::SimConfig { name: "FAN".into(), udp_port: 0, pc: Some(pc), ..Default::default() }).unwrap();
    s.wait("fan", |v| v["devices"].as_array().is_some_and(|d| d.len() == 1 && d[0]["status"].is_object()));
    let made = || made.load(std::sync::atomic::Ordering::SeqCst);
    for (price, fresh, total) in [("0.21", false, 1), ("0.23", false, 1), ("0.25", true, 2)] {
        let text = format!("€ {price}");
        let r = s.call("show_ai", json!({ "prompt": "cheap power", "text": text, "name": "price", "mode": "cast", "show_seconds": 1, "fresh": fresh }));
        assert_eq!(r["success"], true, "{r}");
        assert_eq!(made(), total, "{price}: pictures made");
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while fan.state().cast_ok == 0 {
        assert!(Instant::now() < deadline, "nothing was cast");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(fan.state().protocol_errors.is_empty(), "{:?}", fan.state().protocol_errors);
}

// ---- API token and media folders -------------------------------------------------------

/// This machine's address on the network (no packet is sent), if it has one.
fn lan_ip() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !(ip.is_loopback() || ip.is_unspecified() || ip.octets()[0] == 172 && ip.octets()[1] == 30) => Some(ip),
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

/// Temporary messages with a priority: a less important one waits for the one showing, a more
/// important one interrupts it and the interrupted one comes back for the time it had left; at
/// the end the fan plays the file it played before.
#[test]
fn messages_wait_or_interrupt_by_priority() {
    let s = start(true);
    let has =
        |name: &str| -> bool { s.get("/api/state")["devices"][0]["files"].as_array().is_some_and(|f| f.iter().any(|f| f[1] == name || f["name"] == name)) };
    let wait = |what: &str, cond: &dyn Fn() -> bool, secs: u64| {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while !cond() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    let show = |text: &str, prio: i32, secs: f64| {
        let (_, v) = s.post("/api/show", json!({ "text": text, "name": text, "priority": prio, "show_seconds": secs }));
        v
    };
    wait("virtual fan", &|| s.get("/api/state")["devices"].as_array().is_some_and(|d| !d.is_empty() && d[0]["status"].is_object()), 15);
    // a file that plays before any message
    assert_eq!(show("base", 0, 0.0)["success"], true);
    wait("base", &|| has("base.mp4"), 15);

    // a second file, so a fan that repeats the whole playlist would move on
    assert_eq!(show("photo", 0, 0.0)["success"], true);
    wait("photo", &|| has("photo.mp4"), 15);
    s.call("control", json!({ "id": s.get("/api/state")["devices"][0]["id"], "action": "repeat_all" }));
    s.call("set_value", json!({ "id": s.get("/api/state")["devices"][0]["id"], "what": "select", "value": 1 }));

    // an alarm, then a doorbell that is less important: it waits
    assert_eq!(show("alarm", 5, 4.0)["success"], true);
    // while the alarm shows, the fan repeats only the alarm
    wait("repeat one during a message", &|| s.get("/api/state")["devices"][0]["status"]["repeat_all"] == false, 10);
    let door = show("door", 1, 2.0);
    assert_eq!(door["queued"], true, "{door}");
    assert!(has("alarm.mp4") && !has("door.mp4"));
    wait("door after the alarm", &|| !has("alarm.mp4") && has("door.mp4"), 20);
    wait("door removed", &|| !has("door.mp4"), 15);

    // the weather, interrupted by a more important bell: comes back afterwards
    assert_eq!(show("weather", 0, 12.0)["success"], true);
    wait("weather", &|| has("weather.mp4"), 15);
    let bell = show("bell", 9, 2.0);
    assert!(bell["queued"].is_null(), "{bell}");
    wait("bell", &|| has("bell.mp4"), 15);
    wait("weather back after the bell", &|| !has("bell.mp4") && has("weather.mp4"), 20);
    wait("weather removed", &|| !has("weather.mp4"), 25);
    // back to the file that played before the messages, repeating the whole playlist again
    let st = s.get("/api/state");
    assert_eq!(st["devices"][0]["status"]["repeat_all"], true, "{st}");
    let d = &st["devices"][0];
    let playing = d["status"]["play_index"].as_u64().unwrap_or(0);
    let base = d["files"].as_array().unwrap().iter().find(|f| f[1] == "base.mp4" || f["name"] == "base.mp4").cloned().unwrap_or_default();
    let base_slot = base[0].as_u64().or(base["slot"].as_u64()).unwrap_or(99);
    assert_eq!(playing, base_slot, "{d}");
}

/// A real P56 switches itself off and on after an upload, starts at the first file and misses a
/// select sent right then. A temporary message must still end up showing (and repeating only
/// itself), not the first file of the playlist.
#[test]
fn a_message_shows_even_when_the_fan_misses_the_first_select() {
    let s = start_env(false, &[], &[]);
    let pc = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, s.tcp));
    let fan = p56_sim::Sim::start(p56_sim::SimConfig { name: "FAN".into(), udp_port: 0, pc: Some(pc), ..Default::default() }).unwrap();
    s.wait("fan", |v| v["devices"].as_array().is_some_and(|d| d.len() == 1 && d[0]["status"].is_object()));
    for name in ["spider", "photo"] {
        let (_, v) = s.post("/api/show", json!({ "text": name, "name": name }));
        assert_eq!(v["success"], true, "{v}");
    }
    fan.modify(|st| st.slow_start = true);
    let (_, v) = s.post("/api/show", json!({ "text": "€ 0.45", "name": "price", "show_seconds": 8 }));
    assert_eq!(v["success"], true, "{v}");
    let st = fan.state();
    let slot = st.files.iter().position(|f| f.name == "price.mp4").map(|i| i as u8 + 1).expect("price stored");
    assert_eq!(st.current, slot, "the fan shows the message, not the first file");
    assert!(!st.repeat_all, "and repeats only the message");
    // a text is a still picture (one frame, which the fan shows 5 s), not a 0.1 s animation
    // that a playlist would flash past
    assert!(st.files.iter().all(|f| f.frames == 1), "{:?}", st.files.iter().map(|f| (&f.name, f.frames)).collect::<Vec<_>>());
}

/// A message that is uploaded while a less important one is cast: the cast stops, the fan leaves
/// live mode, and the file is stored and shown (a real P56 in live mode stores nothing).
#[test]
fn an_upload_during_a_cast_is_stored() {
    let s = start_env(false, &[], &[]);
    let pc = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, s.tcp));
    let fan = p56_sim::Sim::start(p56_sim::SimConfig { name: "FAN".into(), udp_port: 0, pc: Some(pc), ..Default::default() }).unwrap();
    s.wait("fan", |v| v["devices"].as_array().is_some_and(|d| d.len() == 1 && d[0]["status"].is_object()));
    let (_, v) = s.post("/api/show", json!({ "text": "clock", "name": "clock", "mode": "cast", "show_seconds": 30, "priority": -10 }));
    assert_eq!(v["success"], true, "{v}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while fan.state().cast_ok < 5 {
        assert!(Instant::now() < deadline, "the clock was not cast");
        std::thread::sleep(Duration::from_millis(100));
    }
    let (_, v) = s.post("/api/show", json!({ "text": "€ 0.35", "name": "price", "show_seconds": 5, "priority": 5 }));
    assert_eq!(v["success"], true, "{v}");
    let st = fan.state();
    assert!(st.files.iter().any(|f| f.name == "price.mp4"), "{:?}", st.files);
    assert!(st.protocol_errors.is_empty(), "{:?}", st.protocol_errors);
}

/// Switching the fan off during a cast stops the cast, so it does not switch the fan on again.
#[test]
fn switching_off_stops_a_cast() {
    let s = start_env(false, &[], &[]);
    let pc = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, s.tcp));
    let fan = p56_sim::Sim::start(p56_sim::SimConfig { name: "FAN".into(), udp_port: 0, pc: Some(pc), ..Default::default() }).unwrap();
    s.wait("fan", |v| v["devices"].as_array().is_some_and(|d| d.len() == 1 && d[0]["status"].is_object()));
    let (_, v) = s.post("/api/show", json!({ "text": "clock", "name": "clock", "mode": "cast", "show_seconds": 30 }));
    assert_eq!(v["success"], true, "{v}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while fan.state().cast_ok < 5 {
        assert!(Instant::now() < deadline, "the clock was not cast");
        std::thread::sleep(Duration::from_millis(100));
    }
    s.call("control", json!({ "id": 1, "action": "off" }));
    s.wait("fan off", |v| v["devices"][0]["status"]["power"] == false);
    std::thread::sleep(Duration::from_millis(800));
    let cast = fan.state().cast_ok;
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(fan.state().cast_ok, cast, "the cast stopped");
    assert!(!fan.state().power, "the fan stays off");
}

/// A fan that is off shows a message and goes off again afterwards (stored or cast); one that
/// was on stays on.
#[test]
fn a_fan_that_was_off_goes_off_again_after_a_message() {
    let s = start_env(false, &[], &[]);
    let pc = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, s.tcp));
    let fan = p56_sim::Sim::start(p56_sim::SimConfig { name: "FAN".into(), udp_port: 0, pc: Some(pc), ..Default::default() }).unwrap();
    s.wait("fan", |v| v["devices"].as_array().is_some_and(|d| d.len() == 1 && d[0]["status"].is_object()));
    let wait = |what: &str, cond: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !cond() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    for mode in ["upload", "cast"] {
        s.call("control", json!({ "id": 1, "action": "off" }));
        wait("off", &|| !fan.state().power);
        let (_, v) = s.post("/api/show", json!({ "text": "INTRUDER", "name": "alarm", "mode": mode, "show_seconds": 3, "priority": 10 }));
        assert_eq!(v["success"], true, "{mode}: {v}");
        let offs = || fan.state().commands.iter().filter(|c| *c == "off").count();
        let before = offs();
        wait("on for the message", &|| fan.state().power);
        wait("off again afterwards", &|| !fan.state().power && !fan.state().files.iter().any(|f| f.name == "alarm.mp4"));
        // and once more a little later: a real P56 sometimes reports off but keeps turning
        wait("a second off", &|| offs() >= before + 2);
    }
    // a fan that was on stays on
    s.call("control", json!({ "id": 1, "action": "on" }));
    wait("on", &|| fan.state().power);
    // the server knows it too (a message reads the fan's state from the server)
    s.wait("server sees the fan on", |v| v["devices"][0]["status"]["power"] == true);
    let (_, v) = s.post("/api/show", json!({ "text": "hello", "name": "hello", "show_seconds": 2 }));
    assert_eq!(v["success"], true, "{v}");
    wait("message gone", &|| !fan.state().files.iter().any(|f| f.name == "hello.mp4"));
    std::thread::sleep(Duration::from_millis(800));
    assert!(fan.state().power, "it was on before, so it stays on");
    assert!(fan.state().protocol_errors.is_empty(), "{:?}", fan.state().protocol_errors);
}

/// The picture last sent is reported live while it is on the fan, and served faded once it is not
/// (another file plays, the cast is over, or the fan is off).
#[test]
fn the_shown_picture_fades_when_it_is_not_on_the_fan() {
    let s = start(true);
    s.wait("virtual fan", |v| v["devices"].as_array().is_some_and(|d| !d.is_empty() && d[0]["status"].is_object()));
    let mac = s.get("/api/state")["devices"][0]["mac"].as_str().unwrap().to_string();
    let fetch = || {
        let r = ureq::get(&format!("{}/api/shown/{mac}", s.base)).call().unwrap();
        let live = r.header("X-P56-Live") == Some("1");
        let mut png = Vec::new();
        r.into_reader().read_to_end(&mut png).unwrap();
        (live, image::load_from_memory(&png).unwrap())
    };
    let live = |want: bool| move |v: &Value| v["devices"][0]["shown"]["live"] == want;
    show_ok(&s, json!({ "text": "ONE", "name": "one" }));
    show_ok(&s, json!({ "text": "TWO", "name": "two" }));
    let st = s.wait("two is live", live(true));
    assert!(st["devices"][0]["shown"]["at"].as_u64().unwrap() > 1_700_000_000, "{st}");
    let (on, img) = fetch();
    assert!(on && !img.color().has_alpha(), "live: the picture as sent");
    // another file plays: faded
    let slot = st["devices"][0]["files"].as_array().unwrap().iter().find(|f| f[1] == "one.mp4").unwrap()[0].as_u64().unwrap();
    s.call("set_value", json!({ "id": 1, "what": "select", "value": slot }));
    s.wait("two not live", live(false));
    let (on, img) = fetch();
    assert!(!on && img.color().has_alpha(), "not live: faded");
    assert!(img.to_rgba8().pixels().all(|p| p[3] < 128), "see-through");
    // a cast: live while it runs, not afterwards, and not while the fan is off
    show_ok(&s, json!({ "text": "CAST", "mode": "cast", "show_seconds": 2 }));
    s.wait("cast live", live(true));
    s.wait("cast over", live(false));
    show_ok(&s, json!({ "text": "THREE", "name": "three" }));
    s.wait("three live", live(true));
    s.call("control", json!({ "id": 1, "action": "off" }));
    s.wait("off: not live", live(false));
}

/// A stand-in for ComfyUI: Qwen Image 2.1 files, a queue that finishes at once, and a picture.
/// Returns its port and the workflows it was sent.
fn fake_comfyui() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
    let srv = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = srv.server_addr().to_ip().unwrap().port();
    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = sent.clone();
    std::thread::spawn(move || {
        for mut req in srv.incoming_requests() {
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            let url = req.url().to_string();
            let json_reply = |v: Value| tiny_http::Response::from_string(v.to_string());
            let r = match url.split('?').next().unwrap_or_default() {
                "/models/diffusion_models" => json_reply(json!([
                    "flux.safetensors",
                    "qwen_image_2.1_int8.safetensors",
                    "qwen_image_2.1_turbo_int8.safetensors",
                    "trellis_2_int8.safetensors"
                ])),
                "/models/text_encoders" => json_reply(json!(["qwen3vl_8b_int8.safetensors"])),
                "/models/vae" => {
                    json_reply(json!(["qwen_image_2.1_vae_bf16.safetensors", "trellis_2_shape_vae_bf16.safetensors", "trellis_2_texture_vae_bf16.safetensors"]))
                }
                "/models/clip_vision" => json_reply(json!(["dino_v3_L_naf_fp32.safetensors"])),
                "/models/background_removal" => json_reply(json!(["birefnet.safetensors"])),
                "/prompt" => {
                    seen.lock().unwrap().push(serde_json::from_str::<Value>(&body).unwrap_or(Value::Null));
                    json_reply(json!({ "prompt_id": "job1" }))
                }
                "/history/job1" => {
                    // a picture (node 9), and the views round a 3D model (nodes 300…)
                    let mut outputs = serde_json::Map::new();
                    for node in std::iter::once(9).chain(300..360) {
                        outputs.insert(node.to_string(), json!({ "images": [{ "filename": "p56 1.png", "subfolder": "", "type": "temp" }] }));
                    }
                    json_reply(json!({ "job1": { "status": { "status_str": "success" }, "outputs": outputs } }))
                }
                "/view" if url.contains("filename=p56%201.png") && url.contains("type=temp") => tiny_http::Response::from_data(image_png()),
                _ => tiny_http::Response::from_string("not found").with_status_code(404),
            };
            let _ = req.respond(r);
        }
    });
    (port, sent)
}

/// AI pictures from ComfyUI (Qwen Image 2.1, the turbo model in 8 steps, the size set up); when
/// ComfyUI cannot be reached, Home Assistant's AI makes it unless ComfyUI was asked for by name.
#[test]
fn ai_pictures_from_comfyui_with_home_assistant_as_fallback() {
    let (port, sent) = fake_comfyui();
    let comfy = format!("http://127.0.0.1:{port}");
    let s = start_env(false, &["--comfyui", &comfy, "--comfyui-size", "500"], &[]);
    let st = s.call("ai_status", json!({}));
    assert_eq!(st["entities"][0]["entity_id"], "comfyui", "{st}");
    let still = s.call("ai_image", json!({ "prompt": "a red fox", "art_style": "photo" }));
    assert_eq!(still["frames"], 1, "{still}");
    let graph = sent.lock().unwrap()[0]["prompt"].clone();
    assert_eq!(graph["1"]["inputs"]["unet_name"], "qwen_image_2.1_turbo_int8.safetensors");
    assert_eq!(graph["7"]["inputs"]["steps"], 8);
    assert_eq!(graph["6"]["inputs"]["width"], 512, "500 rounded to a multiple of 64");
    let words = graph["5"]["inputs"]["prompt"].as_str().unwrap().to_lowercase();
    assert!(words.contains("a red fox") && words.contains("realistic photograph") && words.contains("black"), "{words}");
    assert!(!words.contains("fan") && !words.contains("holographic"), "the fan itself is not described: {words}");
    drop(s);

    // ComfyUI off: Home Assistant's AI, unless ComfyUI was asked for
    let ha = format!("http://127.0.0.1:{}", fake_home_assistant());
    let s = start_env(false, &["--comfyui", "http://127.0.0.1:9"], &[("P56_HA_URL", &ha), ("P56_HA_TOKEN", "test-token")]);
    let st = s.call("ai_status", json!({}));
    assert_eq!(st["entities"].as_array().map(|e| e.len()), Some(2), "{st}");
    let still = s.call("ai_image", json!({ "prompt": "a red fox" }));
    assert_eq!(still["frames"], 1, "{still}");
    let (_, v) = s.post("/api/call/ai_image", json!({ "prompt": "a red fox", "entity_id": "comfyui", "fresh": true }));
    assert_eq!(v["ok"], false, "{v}");
    assert!(v["error"].as_str().unwrap_or_default().contains("ComfyUI"), "{v}");
}

/// Text over an animation (e.g. a rain radar): every frame keeps its picture, with the text over
/// it, instead of the text alone.
#[test]
fn text_over_every_frame_of_an_animation() {
    let s = start(true);
    s.wait("virtual fan", |v| v["devices"].as_array().is_some_and(|d| !d.is_empty() && d[0]["status"].is_object()));
    let anim = base64_encode(&gif(&[[0, 0, 200], [0, 200, 0], [200, 0, 0]]));
    let r = show_ok(&s, json!({ "data": anim, "text": "Rain", "mode": "cast", "show_seconds": 2 }));
    assert_eq!(r["frames"], 3, "{r}");
    // the preview is the first frame: blue (darkened behind the letters), with white text over it
    let img = shown_png(&s);
    let blue = img.pixels().filter(|p| p[2] > 80 && p[0] < 40 && p[1] < 40).count();
    let white = img.pixels().filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200).count();
    assert!(blue > 1000 && white > 100, "blue {blue}, white {white}");
}

/// A fan that was off first spins up on black before a message: on its black file when it has
/// one, otherwise on black sent live.
#[test]
fn a_fan_that_was_off_spins_up_on_black_first() {
    let s = start_env(false, &[], &[]);
    let pc = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, s.tcp));
    let fan = p56_sim::Sim::start(p56_sim::SimConfig { name: "FAN".into(), udp_port: 0, pc: Some(pc), ..Default::default() }).unwrap();
    s.wait("fan", |v| v["devices"].as_array().is_some_and(|d| d.len() == 1 && d[0]["status"].is_object()));
    let off = || {
        s.call("control", json!({ "id": 1, "action": "off" }));
        s.wait("off", |v| v["devices"][0]["status"]["power"] == false);
    };
    // no black file: black is sent live first
    off();
    let cast_before = fan.state().cast_ok;
    let started = Instant::now();
    show_ok(&s, json!({ "text": "hello", "name": "hello", "show_seconds": 2 }));
    assert!(started.elapsed() >= Duration::from_millis(2500), "spun up first");
    assert!(fan.state().cast_ok > cast_before, "black sent live");
    s.wait("hello gone", |v| !v["playlist"].as_array().unwrap().iter().any(|f| f["name"] == "hello.mp4"));
    // with a black file: the fan plays it first, nothing is cast
    let black = {
        let img = image::RgbImage::new(32, 32);
        let mut v = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).unwrap();
        base64_encode(&v)
    };
    show_ok(&s, json!({ "data": black, "name": "nothing" }));
    assert!(fan.state().files.iter().any(|f| f.name == "nothing.mp4"), "stored");
    off();
    let cast_before = fan.state().cast_ok;
    let commands_before = fan.state().commands.len();
    show_ok(&s, json!({ "text": "again", "name": "again", "show_seconds": 2 }));
    let st = fan.state();
    assert_eq!(st.cast_ok, cast_before, "nothing cast");
    assert!(st.commands[commands_before.min(st.commands.len())..].iter().any(|c| c == "select"), "{:?}", st.commands);
    assert!(st.protocol_errors.is_empty(), "{:?}", st.protocol_errors);
}

/// A real 3D model (`kind: 3d`): only with ComfyUI, made by Trellis 2 and rendered from all round
/// (60 views), and kept: the same prompt again, also after a restart, is not made again.
#[test]
fn a_real_3d_model_from_comfyui_is_kept() {
    let (port, sent) = fake_comfyui();
    let comfy = format!("http://127.0.0.1:{port}");
    let dir = std::env::temp_dir().join(format!("p56-ai3d-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let args = ["--comfyui", comfy.as_str(), "--ai-dir", dir.to_str().unwrap()];
    let s = start_env(false, &args, &[]);
    let model = s.call("ai_image", json!({ "prompt": "a treasure chest", "kind": "3d" }));
    assert_eq!(model["frames"], 60, "{model}");
    let graph = sent.lock().unwrap()[0]["prompt"].clone();
    assert_eq!(graph["30"]["inputs"]["unet_name"], "trellis_2_int8.safetensors");
    assert_eq!(graph["51"]["class_type"], "RemeshMesh");
    assert_eq!(graph["359"]["class_type"], "PreviewImage");
    assert_eq!(graph["159"]["inputs"]["mode.yaw"], 354.0);
    let words = graph["5"]["inputs"]["prompt"].as_str().unwrap();
    assert!(words.contains("a treasure chest") && words.contains("slightly above"), "{words}");
    // the same again: from the store, nothing sent to ComfyUI
    assert_eq!(s.call("ai_image", json!({ "prompt": "a treasure chest", "kind": "3d" }))["frames"], 60);
    assert_eq!(sent.lock().unwrap().len(), 1);
    drop(s);
    let s = start_env(false, &args, &[]);
    assert_eq!(s.call("ai_image", json!({ "prompt": "a treasure chest", "kind": "3d" }))["frames"], 60);
    assert_eq!(sent.lock().unwrap().len(), 1, "kept across a restart");
    // only ComfyUI makes 3D models
    let (_, v) = s.post("/api/call/ai_image", json!({ "prompt": "a chest", "kind": "3d", "entity_id": "ai_task.google_ai_task" }));
    assert_eq!(v["ok"], false, "{v}");
    let _ = std::fs::remove_dir_all(&dir);
}
