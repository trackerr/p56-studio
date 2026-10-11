//! The `p56` binary against the fan simulator.

use std::net::{Ipv4Addr, TcpListener};
use std::process::Command;
use std::time::Duration;

use p56_sim::{Sim, SimConfig};

fn free_tcp() -> u16 { TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port() }

struct Env { sim: Sim, port: u16, udp: u16 }

fn env() -> Env {
    let port = free_tcp();
    let sim = Sim::start(SimConfig { udp_port: 0, pc_port: port, name: "CLI-FAN".into(), ..Default::default() }).unwrap();
    let udp = sim.udp_port;
    Env { sim, port, udp }
}

impl Env {
    fn p56(&self, args: &[&str]) -> (bool, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_p56"))
            .args(["--port", &self.port.to_string(), "--target", &format!("127.0.0.1:{}", self.udp), "--wait", "10"])
            .args(args)
            .env("RUST_LOG", "warn")
            .output()
            .expect("run p56");
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.success(), text)
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

#[test]
fn cli_full_workflow() {
    let e = env();
    let t = e.ok(&["info"]);
    assert!(t.contains("name CLI-FAN") && t.contains("type 8"), "{t}");
    e.ok(&["off"]); e.wait(|s| !s.power);
    e.ok(&["on"]); e.wait(|s| s.power);
    e.ok(&["brightness", "5"]); e.wait(|s| s.brightness == 5);
    e.ok(&["angle", "-90"]); e.wait(|s| s.angle == 90);
    e.ok(&["color", "true"]); e.wait(|s| s.color_mode == 1);
    e.ok(&["breath", "on"]); e.wait(|s| s.breath);
    e.ok(&["repeat", "one"]); e.wait(|s| !s.repeat_all);
    e.ok(&["name", "Kitchen"]); e.wait(|s| s.device_name == "Kitchen");
    e.ok(&["router", "Home", "password1"]); e.wait(|s| s.router.0 == "Home");
    e.ok(&["upload", "test", "--name", "card", "--slot", "1"]); e.wait(|s| s.files.len() == 1 && s.files[0].name == "card.mp4");
    e.ok(&["rename", "1", "ring"]); e.wait(|s| s.files[0].name == "ring.mp4");
    let t = e.ok(&["cast", "test", "--seconds", "1.5", "--fps", "10"]);
    assert!(t.contains("frames cast"), "{t}");
    e.wait(|s| s.cast_ok >= 5);
    e.ok(&["ble", "on"]);
    let t = e.ok(&["ble", "scan"]);
    assert!(t.contains("BT-Speaker"), "{t}");
    e.ok(&["delete", "1"]); e.wait(|s| s.files.is_empty());
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
    e.ok(&["format", "--yes"]); e.wait(|s| s.formats == 1);
}

#[test]
fn cli_reports_fan_errors() {
    let e = env();
    e.sim.modify(|s| s.files.push(p56_sim::SimFile { name: "a.mp4".into(), frames: 1, width: 688, height: 240, jpegs: vec![] }));
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
    let port = free_tcp();
    let sim = Sim::start(SimConfig { udp_port: 0, pc_port: port, fan_type: 32, name: "ODD-FAN".into(), ..Default::default() }).unwrap();
    let e = Env { udp: sim.udp_port, sim, port };
    let (ok, t) = e.p56(&["upload", "test", "--name", "x"]);
    assert!(!ok && t.contains("fan type 32") && t.contains("not supported"), "{t}");
    assert_eq!(e.sim.state().uploads, 0);
    // commands that send no picture still work on such a fan
    e.ok(&["off"]);
    e.wait(|s| !s.power);
}

#[test]
fn cli_cast_reaches_only_the_selected_fan() {
    let port = free_tcp();
    let start = |name: &str, last: u8| Sim::start(SimConfig {
        udp_port: 0, pc_port: port, name: name.into(), mac: [0x02, 0xE0, 0xE0, 0x56, 0x00, last], ..Default::default()
    }).unwrap();
    let (a, b) = (start("FAN-A", 1), start("FAN-B", 2));
    let out = Command::new(env!("CARGO_BIN_EXE_p56"))
        .args(["--port", &port.to_string(), "--wait", "10"])
        .args(["--target", &format!("127.0.0.1:{}", a.udp_port), "--target", &format!("127.0.0.1:{}", b.udp_port)])
        .args(["--fan", "2", "cast", "test", "--seconds", "1.5", "--fps", "10"])
        .env("RUST_LOG", "warn").output().unwrap();
    let t = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{t}");
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
