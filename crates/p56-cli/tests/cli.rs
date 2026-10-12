//! The `p56` binary against the fan simulator.

use std::cell::RefCell;
use std::net::{Ipv4Addr, SocketAddr};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use p56_sim::{Sim, SimConfig};

/// A simulated fan on a free UDP port that knows no PC yet: [`p56`] tells it where to connect.
fn sim(cfg: SimConfig) -> Sim {
    Sim::start(SimConfig { udp_port: 0, pc_port: 0, ..cfg }).unwrap()
}

/// Run `p56` against these simulated fans. It opens a free TCP port itself (`--port 0`) and
/// writes it to a file, and the fans are then sent to that port: a port picked here could be
/// taken by another test before `p56` opens it.
fn p56(fans: &[&Sim], args: &[&str]) -> (bool, String) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let port_file = std::env::temp_dir().join(format!("p56cli-{}-{}.port", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_file(&port_file);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_p56"));
    cmd.args(["--port", "0", "--port-file", port_file.to_str().unwrap(), "--wait", "10"]);
    for f in fans {
        cmd.args(["--target", &format!("127.0.0.1:{}", f.udp_port)]);
    }
    let mut child = cmd.args(args).env("RUST_LOG", "warn").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("run p56");
    // until p56 has its port open, or stops without one (a refused command)
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut killed = "";
    while child.try_wait().unwrap().is_none() {
        let port = std::fs::read_to_string(&port_file).ok().and_then(|s| s.strip_suffix('\n')?.parse::<u16>().ok());
        if let Some(port) = port {
            for f in fans {
                f.set_pc_port(port);
                f.connect_to(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
            }
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            killed = "(killed: no port file after 10 s)\n";
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().expect("run p56");
    // the port is closed now and free for anyone: a late broadcast must not send a fan there
    for f in fans {
        f.set_pc_port(0);
    }
    let _ = std::fs::remove_file(&port_file);
    let text = format!("{}{}{killed}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

struct Env {
    sim: Sim,
    /// every `p56` run with its output, shown when the test fails
    log: RefCell<String>,
}

fn env() -> Env {
    Env::new(SimConfig { name: "CLI-FAN".into(), ..Default::default() })
}

impl Env {
    fn new(cfg: SimConfig) -> Env {
        Env { sim: sim(cfg), log: RefCell::default() }
    }
    fn p56(&self, args: &[&str]) -> (bool, String) {
        let (ok, text) = p56(&[&self.sim], args);
        self.log.borrow_mut().push_str(&format!("$ p56 {} ({})\n{text}", args.join(" "), if ok { "ok" } else { "failed" }));
        (ok, text)
    }
    fn ok(&self, args: &[&str]) -> String {
        let (ok, t) = self.p56(args);
        assert!(ok, "p56 {args:?} failed:\n{t}");
        t
    }
    fn wait(&self, pred: impl Fn(&p56_sim::SimState) -> bool) {
        assert!(self.sim.wait_until(Duration::from_secs(5), pred), "sim: {:?}", self.sim.state().commands);
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("---- p56 runs ----\n{}", self.log.borrow());
        }
    }
}

#[test]
fn cli_full_workflow() {
    let e = env();
    let t = e.ok(&["info"]);
    assert!(t.contains("name CLI-FAN") && t.contains("type 8"), "{t}");
    e.ok(&["off"]);
    e.wait(|s| !s.power);
    e.ok(&["on"]);
    e.wait(|s| s.power);
    e.ok(&["brightness", "5"]);
    e.wait(|s| s.brightness == 5);
    e.ok(&["angle", "-90"]);
    e.wait(|s| s.angle == 90);
    e.ok(&["color", "true"]);
    e.wait(|s| s.color_mode == 1);
    e.ok(&["breath", "on"]);
    e.wait(|s| s.breath);
    e.ok(&["repeat", "one"]);
    e.wait(|s| !s.repeat_all);
    e.ok(&["name", "Kitchen"]);
    e.wait(|s| s.device_name == "Kitchen");
    e.ok(&["router", "Home", "password1"]);
    e.wait(|s| s.router.0 == "Home");
    e.ok(&["upload", "test", "--name", "card", "--slot", "1"]);
    e.wait(|s| s.files.len() == 1 && s.files[0].name == "card.mp4");
    e.ok(&["rename", "1", "ring"]);
    e.wait(|s| s.files[0].name == "ring.mp4");
    let t = e.ok(&["cast", "test", "--seconds", "1.5", "--fps", "10"]);
    assert!(t.contains("frames cast"), "{t}");
    e.wait(|s| s.cast_ok >= 5);
    e.ok(&["delete", "1"]);
    e.wait(|s| s.files.is_empty());
    assert!(e.sim.state().protocol_errors.is_empty(), "{:?}", e.sim.state().protocol_errors);
}

#[test]
fn cli_refuses_without_confirmation_and_bad_values() {
    let e = env();
    let (ok, t) = e.p56(&["format"]);
    assert!(!ok && t.contains("--yes"), "{t}");
    let (ok, t) = e.p56(&["brightness", "40"]);
    assert!(!ok && t.contains("1–16"), "{t}");
    let (ok, t) = e.p56(&["hotspot", "x", "short"]);
    assert!(!ok && t.contains("8 characters"), "{t}");
    e.ok(&["format", "--yes"]);
    e.wait(|s| s.formats == 1);
}

#[test]
fn cli_reports_fan_errors() {
    let e = env();
    e.sim.modify(|s| s.files.push(p56_sim::SimFile { name: "a.mp4".into(), frames: 1, sound_frames: 0, width: 688, height: 240, jpegs: vec![] }));
    let (ok, t) = e.p56(&["upload", "test", "--name", "a"]);
    assert!(!ok && t.contains("already exists"), "{t}");
}

fn preview_type(t: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("p56cli-type{t}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_p56")).args(["--type", t, "preview", "test", dir.join("pv").to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

#[test]
fn cli_rejects_unknown_fan_type() {
    let (ok, t) = preview_type("255");
    assert!(!ok && t.contains("unknown fan type 255"), "{t}");
}

#[test]
fn cli_rejects_unsupported_fan_type() {
    let (ok, t) = preview_type("32");
    assert!(!ok && t.contains("fan type 32") && t.contains("not supported"), "{t}");
}

#[test]
fn cli_accepts_supported_fan_type() {
    let (ok, t) = preview_type("8");
    assert!(ok && t.contains("688x240"), "{t}");
}

#[test]
fn cli_rejects_unsupported_type_reported_by_the_fan() {
    let e = Env::new(SimConfig { fan_type: 32, name: "ODD-FAN".into(), ..Default::default() });
    let (ok, t) = e.p56(&["upload", "test", "--name", "x"]);
    assert!(!ok && t.contains("fan type 32") && t.contains("not supported"), "{t}");
    assert_eq!(e.sim.state().uploads, 0);
    // commands that send no picture still work on such a fan
    e.ok(&["off"]);
    e.wait(|s| !s.power);
}

#[test]
fn cli_cast_reaches_only_the_selected_fan() {
    let start = |name: &str, last: u8| sim(SimConfig { name: name.into(), mac: [0x02, 0xE0, 0xE0, 0x56, 0x00, last], ..Default::default() });
    let (a, b) = (start("FAN-A", 1), start("FAN-B", 2));
    let (ok, t) = p56(&[&a, &b], &["--fan", "2", "cast", "test", "--seconds", "1.5", "--fps", "10"]);
    assert!(ok, "{t}");
    std::thread::sleep(Duration::from_millis(300));
    let (na, nb) = (a.state().cast_ok, b.state().cast_ok);
    assert!((na > 0) != (nb > 0), "exactly one fan must receive the cast: FAN-A {na}, FAN-B {nb}\n{t}");
}

#[test]
fn preview_and_types_work_offline() {
    let dir = std::env::temp_dir().join(format!("p56cli{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let prefix = dir.join("pv");
    let out = Command::new(env!("CARGO_BIN_EXE_p56")).args(["preview", "test", prefix.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("688x240"));
    let out = Command::new(env!("CARGO_BIN_EXE_p56")).arg("types").output().unwrap();
    let t = String::from_utf8_lossy(&out.stdout);
    assert!(t.contains("type  8: 960×172") && t.contains("Unsupported"), "{t}");
}
