//! Gauntlet: the real `p56_core::Link` against the simulated fan, for every feature.
//! Every test also asserts the simulator saw zero protocol errors.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use p56_core::protocol::{ColorMode, FanError};
use p56_core::transcode::{self, FanType, Placement};
use p56_core::{Config, Error, Event, Link};
use p56_sim::{Sim, SimConfig, SimState};

const T: Duration = Duration::from_secs(5);

struct Rig {
    link: Link,
    _rx: Receiver<Event>,
    sim: Sim,
    id: u32,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.sim.stop();
        self.link.shutdown();
    }
}

fn start_link(targets: Vec<SocketAddr>, fan_udp: u16) -> (Link, Receiver<Event>) {
    start_link_allow(targets, fan_udp, Vec::new())
}

fn start_link_allow(targets: Vec<SocketAddr>, fan_udp: u16, allow: Vec<std::net::IpAddr>) -> (Link, Receiver<Event>) {
    Link::start(Config {
        local_ip: None,
        tcp_port: 0,
        udp_port: 0,
        fan_udp_port: fan_udp,
        broadcast_interval: Duration::from_millis(200),
        heartbeat: true,
        targets,
        allow,
    })
    .expect("link start")
}

fn rig_with(f: impl FnOnce(&mut SimConfig)) -> Rig {
    let mut cfg = SimConfig { udp_port: 0, ..Default::default() };
    f(&mut cfg);
    // Sim first on a free UDP port, then the link targeting it, then the link's TCP port to the sim.
    let sim = Sim::start(cfg).expect("sim start");
    let (link, rx) = start_link(vec![SocketAddr::from((Ipv4Addr::LOCALHOST, sim.udp_port))], sim.udp_port);
    sim.set_pc_port(link.tcp_port);
    assert!(link.wait_connected(T), "fan did not connect");
    let id = link.ids()[0];
    let rig = Rig { link, _rx: rx, sim, id };
    rig.wait_dev(|d| d.info.is_some() && d.status.is_some());
    rig
}

fn rig() -> Rig {
    rig_with(|_| {})
}

impl Rig {
    fn wait_dev(&self, pred: impl Fn(&p56_core::Device) -> bool) {
        let deadline = std::time::Instant::now() + T;
        loop {
            if let Some(d) = self.link.device(self.id) {
                if pred(&d) {
                    return;
                }
            }
            assert!(std::time::Instant::now() < deadline, "device state never matched: {:?}", self.link.device(self.id));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn wait_sim(&self, pred: impl Fn(&SimState) -> bool) {
        assert!(self.sim.wait_until(T, pred), "sim state never matched: {:?}", strip(self.sim.state()));
    }
    fn clean(&self) {
        let e = self.sim.state().protocol_errors;
        assert!(e.is_empty(), "protocol errors: {e:#?}");
    }
    fn jpeg(&self) -> Vec<u8> {
        transcode::encode(&transcode::test_card(), &FanType::P56, &Placement::default()).jpeg
    }
}

fn strip(mut s: SimState) -> SimState {
    s.commands.clear();
    s
}

#[test]
fn connects_and_reports_info_and_status() {
    let r = rig_with(|c| c.name = "LivingRoom".into());
    let d = r.link.device(r.id).unwrap();
    let info = d.info.unwrap();
    assert_eq!(info.name, "LivingRoom");
    assert_eq!(info.mac, "02:e0:e0:56:00:f5");
    assert_eq!(info.hotspot, "LivingRoom-AP");
    let st = d.status.unwrap();
    assert_eq!(st.fan_type, 8);
    assert!(st.power && st.playing);
    assert_eq!(st.brightness, 16);
    assert_eq!(st.angle, 180);
    r.wait_sim(|s| s.heartbeats >= 1);
    r.clean();
}

#[test]
fn power_play_pause_repeat() {
    let r = rig();
    r.link.off(r.id).unwrap();
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| !s.power));
    r.link.on(r.id).unwrap();
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| s.power));
    r.link.pause(r.id).unwrap();
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| !s.playing));
    r.link.play(r.id).unwrap();
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| s.playing));
    r.link.repeat(r.id, false).unwrap();
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| !s.repeat_all));
    r.link.repeat(r.id, true).unwrap();
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| s.repeat_all));
    r.clean();
}

#[test]
fn settings_roundtrip_through_status() {
    let r = rig();
    r.link.brightness(r.id, 7).unwrap();
    r.link.angle(r.id, 300).unwrap();
    r.link.color_mode(r.id, ColorMode::True).unwrap();
    r.link.breath(r.id, true).unwrap();
    r.link.autostart(r.id, false).unwrap();
    r.link.master(r.id, true).unwrap();
    r.link.volume(r.id, 4).unwrap();
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| s.brightness == 7 && s.angle == 300 && s.color_mode == 1 && s.breath && !s.autostart && s.master));
    r.wait_sim(|s| s.volume == 4);
    r.link.color_mode(r.id, ColorMode::Bright).unwrap();
    r.link.angle(r.id, 0).unwrap();
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| s.color_mode == 2 && s.angle == 0));
    r.clean();
}

#[test]
fn names_hotspot_router() {
    let r = rig();
    r.link.device_name(r.id, "Hal").unwrap();
    r.wait_dev(|d| d.info.as_ref().is_some_and(|i| i.name == "Hal"));
    r.link.set_hotspot(r.id, "HoloNet", "geheim123").unwrap();
    r.wait_sim(|s| s.hotspot == ("HoloNet".into(), "geheim123".into()));
    r.link.set_router(r.id, "HomeWifi", "pass word!").unwrap();
    r.wait_sim(|s| s.router == ("HomeWifi".into(), "pass word!".into()));
    r.wait_dev(|d| d.info.as_ref().is_some_and(|i| i.hotspot == "HoloNet" && i.router == "HomeWifi"));
    // 32-byte fields: longer values are cut to 31 bytes (the field keeps its terminating zero)
    r.link.set_router(r.id, &"R".repeat(40), &"P".repeat(40)).unwrap();
    r.wait_sim(|s| s.router == ("R".repeat(31), "P".repeat(31)));
    r.clean();
}

#[test]
fn upload_select_rename_delete() {
    let r = rig();
    let j = r.jpeg();
    r.link.upload(r.id, std::slice::from_ref(&j), "kat", Some(1)).unwrap();
    r.wait_sim(|s| s.files.len() == 1 && s.files[0].name == "kat.mp4" && (s.files[0].width, s.files[0].height) == (688, 240));
    r.link.upload(r.id, &[j.clone(), j.clone(), j], "hond.mp4", Some(2)).unwrap();
    r.wait_sim(|s| s.files.len() == 2 && s.files[1].frames == 3 && s.current == 2);
    r.wait_dev(|d| d.files == vec![(1, "kat.mp4".to_string()), (2, "hond.mp4".to_string())]);
    r.link.select(r.id, 1).unwrap();
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| s.play_index == 1));
    r.link.rename(r.id, 2, "vogel").unwrap();
    r.wait_dev(|d| d.files.get(1).is_some_and(|f| f.1 == "vogel.mp4"));
    r.link.delete(r.id, 1).unwrap();
    r.wait_dev(|d| d.files == vec![(1, "vogel.mp4".to_string())]);
    r.wait_sim(|s| s.uploads == 2 && s.files.len() == 1);
    r.clean();
}

#[test]
fn upload_reports_acknowledged_and_listed() {
    let r = rig();
    let o = r.link.upload(r.id, &[r.jpeg()], "seen", Some(1)).unwrap();
    assert!(o.acked && o.listed, "{o:?}");
    r.clean();
}

/// As the real P56 behaved: no 0E after the data (a 29 instead), no playlist unless asked.
/// The upload must still count as acknowledged and listed, and the next one must work.
#[test]
fn upload_on_a_fan_that_answers_like_the_real_p56() {
    let r = rig();
    r.sim.modify(|s| s.quiet_upload = true);
    let t = std::time::Instant::now();
    let o = r.link.upload(r.id, &[r.jpeg()], "real1", Some(1)).unwrap();
    assert!(o.acked && o.listed, "{o:?}");
    let o = r.link.upload(r.id, &[r.jpeg(), r.jpeg()], "real2", None).unwrap();
    assert!(o.acked && o.listed, "{o:?}");
    assert!(t.elapsed() < Duration::from_secs(12), "no step waits for an answer that never comes: {:?}", t.elapsed());
    r.wait_sim(|s| s.file_names() == vec!["real1.mp4".to_string(), "real2.mp4".to_string()]);
    r.clean();
}

#[test]
fn upload_refuses_an_incomplete_jpeg() {
    let r = rig();
    let mut cut = r.jpeg();
    cut.truncate(cut.len() - 10); // lost its FF D9
    match r.link.upload(r.id, &[cut], "cut", None) {
        Err(Error::Invalid(m)) => assert!(m.contains("complete JPEG"), "{m}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(r.sim.state().uploads, 0, "nothing was sent");
    r.clean();
}

/// The format this project used before (one length, 1600 zero bytes, then all JPEGs back to
/// back) is not what Holoscope sends for an animation; the simulator must flag it.
#[test]
fn concatenated_animation_format_is_a_protocol_error() {
    use p56_core::protocol as proto;
    let r = rig();
    let j = r.jpeg();
    r.link.send_to(r.id, &proto::on()).unwrap();
    r.link.send_to(r.id, &proto::set_filename("old.mp4")).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let mut blob = 4950u32.to_le_bytes().to_vec();
    blob.extend(((proto::UPLOAD_PCM + 2 * j.len() + 1) as u32).to_le_bytes());
    blob.extend(vec![0u8; proto::UPLOAD_PCM]);
    blob.extend(&j);
    blob.extend(&j);
    blob.push(0);
    while !blob.len().is_multiple_of(4) {
        blob.push(0);
    } // keep the old length field 4-byte aligned
    let n = blob.len() - 8 - proto::UPLOAD_PCM - 2 * j.len();
    blob[4..8].copy_from_slice(&((proto::UPLOAD_PCM + 2 * j.len() + n) as u32).to_le_bytes());
    r.link.send_to(r.id, &blob).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    r.link.send_to(r.id, &proto::commit()).unwrap();
    r.wait_sim(|s| s.protocol_errors.iter().any(|e| e.contains("one record per frame")));
    assert!(r.sim.state().files.is_empty(), "the file must not be stored");
}

#[test]
fn animation_duration_and_records_match_holoscope() {
    let r = rig();
    let j = r.jpeg();
    let o = r.link.upload(r.id, &[j.clone(), j.clone(), j.clone(), j], "anim", Some(1)).unwrap();
    assert!(o.acked && o.listed, "{o:?}");
    // the simulator checks the duration (4 × 50 ms) and every record; any mismatch is an error
    r.wait_sim(|s| s.files.first().is_some_and(|f| f.frames == 4));
    r.clean();
}

#[test]
fn animation_with_sound() {
    let r = rig();
    let j = r.jpeg();
    // 0.12 s of a loud square wave: frames 1–2 get sound, the last 30 ms of frame 3 is silence
    let samples = 16_000 * 12 / 100;
    let pcm: Vec<u8> = (0..samples).flat_map(|i| if i % 40 < 20 { 12_000i16 } else { -12_000 }.to_le_bytes()).collect();
    let o = r.link.upload_with_audio(r.id, &[j.clone(), j.clone(), j.clone(), j], Some(&pcm), "tune", Some(1)).unwrap();
    assert!(o.acked && o.listed);
    r.wait_sim(|s| s.files.first().is_some_and(|f| f.frames == 4 && f.sound_frames == 3));
    r.clean();
}

#[test]
fn cast_without_acknowledgement_fails_fast_and_reconnects() {
    let r = rig();
    let j = r.jpeg();
    assert_eq!(r.link.cast_to(&[r.id], &j), 1, "acknowledged frame counts");
    r.sim.modify(|s| s.cast_mute = true);
    let t = std::time::Instant::now();
    assert_eq!(r.link.cast_to(&[r.id], &j), 0, "an unacknowledged frame is not counted as delivered");
    assert!(t.elapsed() < Duration::from_secs(3), "gave up within the ack window, took {:?}", t.elapsed());
    // after a few unacknowledged frames in a row the link drops the connection; the fan reconnects
    let before = r.sim.state().connections;
    for _ in 0..6 {
        r.link.cast_to(&r.link.ids(), &j);
    }
    r.sim.modify(|s| s.cast_mute = false);
    r.wait_sim(|s| s.connections > before && s.connected);
    assert!(r.link.wait_connected(T));
    let id = r.link.ids()[0];
    let deadline = std::time::Instant::now() + T;
    while r.link.cast_to(&[id], &j) != 1 {
        assert!(std::time::Instant::now() < deadline, "cast never recovered after reconnecting");
        std::thread::sleep(Duration::from_millis(100));
    }
    r.clean();
}

#[test]
fn fan_port_only_accepts_allowed_addresses() {
    let sim = Sim::start(SimConfig { udp_port: 0, ..Default::default() }).expect("sim");
    let other = std::net::IpAddr::from([192, 0, 2, 7]); // documentation address, never the sim
    let (link, _rx) = start_link_allow(vec![SocketAddr::from((Ipv4Addr::LOCALHOST, sim.udp_port))], sim.udp_port, vec![other]);
    sim.set_pc_port(link.tcp_port);
    assert!(!link.wait_connected(Duration::from_secs(2)), "a fan from an address that is not allowed got in");
    link.shutdown();
    let (link, _rx) = start_link_allow(vec![SocketAddr::from((Ipv4Addr::LOCALHOST, sim.udp_port))], sim.udp_port, vec![other, Ipv4Addr::LOCALHOST.into()]);
    sim.set_pc_port(link.tcp_port);
    assert!(link.wait_connected(T), "an allowed fan was refused");
    link.shutdown();
    sim.stop();
}

#[test]
fn upload_errors_from_fan() {
    let r = rig_with(|c| c.max_files = 2);
    let j = r.jpeg();
    r.link.upload(r.id, std::slice::from_ref(&j), "a", None).unwrap();
    match r.link.upload(r.id, std::slice::from_ref(&j), "a", None) {
        Err(Error::Fan(FanError::DuplicateName)) => {}
        other => panic!("expected duplicate name, got {other:?}"),
    }
    r.link.upload(r.id, std::slice::from_ref(&j), "b", None).unwrap();
    match r.link.upload(r.id, std::slice::from_ref(&j), "c", None) {
        Err(Error::Fan(FanError::TooManyFiles)) => {}
        other => panic!("expected too many files, got {other:?}"),
    }
    r.wait_dev(|d| d.last_error == Some(FanError::TooManyFiles));
    // still usable afterwards
    r.link.delete(r.id, 1).unwrap();
    r.link.upload(r.id, &[j], "c", Some(2)).unwrap();
    r.wait_sim(|s| s.file_names() == vec!["b.mp4".to_string(), "c.mp4".to_string()]);
    r.clean();
}

#[test]
fn gif_animation_uploads_all_frames() {
    let r = rig();
    let mut gif = Vec::new();
    {
        let mut enc = image::codecs::gif::GifEncoder::new(&mut gif);
        for i in 0..5u8 {
            let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([i * 50, 255 - i * 50, 80, 255]));
            enc.encode_frame(image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(80, 1))).unwrap();
        }
    }
    let frames = transcode::decode_frames(&gif, 0).unwrap();
    assert_eq!(frames.len(), 5);
    assert!(frames.iter().all(|(_, ms)| *ms == 80));
    let jpegs: Vec<_> = frames.iter().map(|(f, _)| transcode::encode(f, &FanType::P56, &Placement::default()).jpeg).collect();
    r.link.upload(r.id, &jpegs, "anim.gif", Some(1)).unwrap();
    r.wait_sim(|s| s.files.first().is_some_and(|f| f.frames == 5 && f.name == "anim.gif.mp4"));
    r.clean();
}

#[test]
fn live_cast_frames_are_valid() {
    let r = rig();
    let j = r.jpeg();
    for _ in 0..25 {
        assert_eq!(r.link.cast(&j), 1);
    }
    r.wait_sim(|s| s.cast_ok == 25);
    assert_eq!(r.sim.state().cast_bad, 0);
    assert!(r.sim.current_jpeg().is_some());
    r.clean();
}

#[test]
fn bluetooth_scan_connect_disconnect() {
    let r = rig();
    r.link.ble(r.id, true).unwrap();
    r.wait_sim(|s| s.ble_on);
    r.link.ble_scan(r.id).unwrap();
    r.wait_dev(|d| d.bluetooth.as_ref().is_some_and(|b| b.on && !b.connected && b.devices == vec!["BT-Speaker".to_string(), "Phone".to_string()]));
    r.link.ble_connect(r.id, 1).unwrap();
    r.wait_sim(|s| s.ble_connected == Some(1));
    r.wait_dev(|d| d.bluetooth.as_ref().is_some_and(|b| b.connected));
    r.link.volume(r.id, 6).unwrap();
    r.wait_dev(|d| d.bluetooth.as_ref().is_some_and(|b| b.volume == 6));
    r.link.ble_disconnect(r.id).unwrap();
    r.wait_sim(|s| s.ble_connected.is_none());
    r.wait_dev(|d| d.bluetooth.as_ref().is_some_and(|b| !b.connected));
    r.link.ble(r.id, false).unwrap();
    r.wait_sim(|s| !s.ble_on);
    r.clean();
}

#[test]
fn format_and_factory_reset() {
    let r = rig();
    r.link.upload(r.id, &[r.jpeg()], "x", Some(1)).unwrap();
    r.link.brightness(r.id, 3).unwrap();
    r.link.device_name(r.id, "Anders").unwrap();
    r.link.factory_reset(r.id).unwrap();
    r.wait_sim(|s| s.resets == 1 && s.brightness == 16 && s.device_name == "SIM-P56" && s.files.len() == 1);
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| s.brightness == 16) && d.info.as_ref().is_some_and(|i| i.name == "SIM-P56"));
    r.link.format_card(r.id).unwrap();
    r.wait_sim(|s| s.files.is_empty() && s.formats == 1);
    r.wait_dev(|d| d.files.is_empty());
    r.clean();
}

#[test]
fn reconnects_after_drop() {
    let r = rig();
    let old = r.id;
    r.sim.drop_connection();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let id = loop {
        let ids = r.link.ids();
        if r.sim.state().connected && r.sim.state().connections >= 2 && !ids.contains(&old) {
            if let Some(&id) = ids.first() {
                break id;
            }
        }
        assert!(std::time::Instant::now() < deadline, "no reconnect: ids={ids:?} sim={}", r.sim.state().connections);
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(id > old);
    r.link.brightness(id, 9).unwrap();
    r.wait_sim(|s| s.brightness == 9);
    r.clean();
}

#[test]
fn two_fans_cast_and_commands() {
    let a = Sim::start(SimConfig { udp_port: 0, name: "A".into(), ..Default::default() }).unwrap();
    let b = Sim::start(SimConfig { udp_port: 0, name: "B".into(), fragment: true, ..Default::default() }).unwrap();
    let (link, _rx) = start_link(vec![SocketAddr::from((Ipv4Addr::LOCALHOST, a.udp_port)), SocketAddr::from((Ipv4Addr::LOCALHOST, b.udp_port))], a.udp_port);
    a.set_pc_port(link.tcp_port);
    b.set_pc_port(link.tcp_port);
    let deadline = std::time::Instant::now() + T;
    while link.ids().len() < 2 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    let j = transcode::encode(&transcode::test_card(), &FanType::P56, &Placement::default()).jpeg;
    for _ in 0..10 {
        assert_eq!(link.cast(&j), 2);
    }
    assert!(a.wait_until(T, |s| s.cast_ok == 10));
    assert!(b.wait_until(T, |s| s.cast_ok == 10));
    for id in link.ids() {
        link.brightness(id, 5).unwrap();
    }
    assert!(a.wait_until(T, |s| s.brightness == 5));
    assert!(b.wait_until(T, |s| s.brightness == 5));
    assert!(a.state().protocol_errors.is_empty() && b.state().protocol_errors.is_empty());
    a.stop();
    b.stop();
    link.shutdown();
}

#[test]
fn fragmented_replies_are_parsed() {
    let r = rig_with(|c| c.fragment = true);
    for v in 1..=16 {
        r.link.brightness(r.id, v).unwrap();
    }
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| s.brightness == 16));
    r.link.upload(r.id, &[r.jpeg()], "frag", Some(1)).unwrap();
    r.wait_dev(|d| d.files.len() == 1);
    r.link.refresh(r.id).unwrap();
    r.wait_dev(|d| d.info.is_some());
    r.clean();
}

#[test]
fn odd_input_from_fan_does_not_break_link() {
    let r = rig();
    assert!(r.sim.inject(&[0x00, 0xFF, 0x5A, 0x99, 0x01, 0x02]));
    assert!(r.sim.inject(&[0x5A, 0x0E, 0x0E, 0xF5, 0x00]));
    assert!(r.sim.inject(&[0x5A, 0x85, 0x0A, 0x0A, 0xF5]));
    r.wait_dev(|d| d.last_error == Some(FanError::TooManyFiles));
    r.link.brightness(r.id, 2).unwrap();
    r.wait_dev(|d| d.status.as_ref().is_some_and(|s| s.brightness == 2));
    r.clean();
}

/// Random command sequences checked against a simple model of the fan.
#[test]
fn random_sequences_match_model() {
    let r = rig_with(|c| {
        c.fragment = true;
        c.max_files = 6;
    });
    let j = r.jpeg();
    let mut seed: u64 = 0x1234_5678_9abc_def1;
    let mut rnd = |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut files: Vec<String> = Vec::new();
    for step in 0..120 {
        match rnd(10) {
            0 => {
                let v = 1 + rnd(16) as u8;
                r.link.brightness(r.id, v).unwrap();
                r.wait_sim(|s| s.brightness == v);
            }
            1 => {
                let a = rnd(361) as u16;
                r.link.angle(r.id, a).unwrap();
                r.wait_sim(|s| s.angle == a);
            }
            2 => {
                let on = rnd(2) == 0;
                r.link.breath(r.id, on).unwrap();
                r.wait_sim(|s| s.breath == on);
            }
            3 => {
                let name = format!("f{step}.mp4");
                let res = r.link.upload(r.id, std::slice::from_ref(&j), &name, None);
                if files.len() >= 6 {
                    assert!(matches!(res, Err(Error::Fan(FanError::TooManyFiles))), "{res:?}");
                } else {
                    res.unwrap();
                    files.push(name);
                }
            }
            4 if !files.is_empty() => {
                let i = rnd(files.len() as u64) as usize;
                r.link.delete(r.id, i as u8 + 1).unwrap();
                files.remove(i);
            }
            5 if !files.is_empty() => {
                let i = rnd(files.len() as u64) as usize;
                let n = format!("r{step}.mp4");
                r.link.rename(r.id, i as u8 + 1, &n).unwrap();
                files[i] = n;
            }
            6 => {
                let m = if rnd(2) == 0 { ColorMode::True } else { ColorMode::Bright };
                r.link.color_mode(r.id, m).unwrap();
            }
            7 => {
                assert_eq!(r.link.cast(&j), 1);
            }
            8 => {
                if rnd(2) == 0 {
                    r.link.pause(r.id).unwrap();
                } else {
                    r.link.play(r.id).unwrap();
                }
            }
            _ => {
                r.link.refresh(r.id).unwrap();
            }
        }
        let want = files.clone();
        assert!(r.sim.wait_until(T, |s| s.file_names() == want), "step {step}: sim {:?} vs model {want:?}", r.sim.state().file_names());
    }
    let want: Vec<(u8, String)> = files.iter().enumerate().map(|(i, n)| (i as u8 + 1, n.clone())).collect();
    r.link.refresh(r.id).unwrap();
    r.wait_dev(|d| d.files == want);
    r.clean();
}
