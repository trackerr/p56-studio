//! Simulator of a Holoscope / P56 POV hologram fan.
//!
//! Behaves like the real device on the network: it answers the PC's `"<ip>HS"`
//! UDP broadcast, connects to the PC's TCP server, executes every command of the
//! protocol, stores uploads, checks every checksum and frame, and reports status,
//! machine info, playlist, list reports and error codes the way the firmware does.
//! Anything malformed is recorded in [`SimState::protocol_errors`].

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use p56_core::protocol::{self as proto, op, FanError, MachineInfo, Status, CAST_CHUNK, CAST_FRAME_SIZE, EOF, SOF};
use p56_core::transcode::FanType;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct SimConfig {
    pub name: String,
    pub fan_type: u8,
    /// UDP port to listen for the broadcast on (real fan: 8988, 0 = any free port).
    pub udp_port: u16,
    /// Connect straight to this PC address instead of waiting for a broadcast.
    pub pc: Option<SocketAddr>,
    /// TCP port of the PC (real fan: always 6666).
    pub pc_port: u16,
    /// IP the fan reports in its `HS` reply and machine info.
    pub ip: Ipv4Addr,
    pub mac: [u8; 6],
    pub max_files: usize,
    /// Split every reply into random small pieces (stress test for the PC's parser).
    pub fragment: bool,
    /// Reconnect after the PC closes the connection.
    pub reconnect: bool,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            name: "SIM-P56".into(),
            fan_type: 8,
            udp_port: proto::UDP_PORT,
            pc: None,
            pc_port: proto::TCP_PORT,
            ip: Ipv4Addr::new(192, 168, 4, 1),
            mac: [0x02, 0xE0, 0xE0, 0x56, 0x00, 0xF5],
            max_files: 20,
            fragment: false,
            reconnect: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SimFile {
    pub name: String,
    pub frames: usize,
    /// Frames whose 1600 PCM bytes are not silent (a video uploaded with its sound).
    pub sound_frames: usize,
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub jpegs: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimState {
    pub connected: bool,
    pub connections: u32,
    pub pc: Option<String>,
    pub power: bool,
    pub playing: bool,
    pub repeat_all: bool,
    pub breath: bool,
    pub autostart: bool,
    pub brightness: u8,
    pub color_mode: u8,
    pub angle: u16,
    pub volume: u8,
    pub group_device: u8,
    pub angle_align: bool,
    pub device_name: String,
    pub hotspot: (String, String),
    pub router: (String, String),
    pub files: Vec<SimFile>,
    /// Playing slot, 1-based (0 = none).
    pub current: u8,
    pub uploads: u32,
    pub cast_ok: u64,
    pub cast_bad: u64,
    /// Fault injection: take cast frames but never acknowledge them (a stalled fan).
    pub cast_mute: bool,
    /// Behave like a real P56 switched on: it starts at the first file and misses the next
    /// select (seen 2026-10-12 right after an upload, which switches the fan off and on).
    pub slow_start: bool,
    /// Selects still to be missed after switching on (with `slow_start`).
    pub missed_selects: u8,
    /// Behave like a real P56 seen on 2026-10-11: no 0E after the file data (a 29 instead) and
    /// no playlist after storing a file unless it is asked for (1C).
    pub quiet_upload: bool,
    pub formats: u32,
    pub resets: u32,
    pub heartbeats: u32,
    pub cancels: u32,
    /// Every command received, by name (most recent last, max 500).
    pub commands: Vec<String>,
    /// Malformed input from the PC. A correct client never causes any.
    pub protocol_errors: Vec<String>,
    #[serde(skip)]
    pub last_cast: Option<(Instant, Vec<u8>)>,
    /// Live mode: since the cast start (0x26) or the last cast frame, whichever is later.
    #[serde(skip)]
    pub live: Option<Instant>,
}

impl SimState {
    fn new(cfg: &SimConfig) -> SimState {
        SimState {
            connected: false,
            connections: 0,
            pc: None,
            power: true,
            playing: true,
            repeat_all: true,
            breath: false,
            autostart: true,
            brightness: 16,
            color_mode: 2,
            angle: 180,
            volume: 10,
            group_device: 0,
            angle_align: false,
            device_name: cfg.name.clone(),
            hotspot: (format!("{}-AP", cfg.name), "12345678".into()),
            router: (String::new(), String::new()),
            files: Vec::new(),
            current: 0,
            uploads: 0,
            cast_ok: 0,
            cast_bad: 0,
            cast_mute: false,
            slow_start: false,
            missed_selects: 0,
            quiet_upload: false,
            formats: 0,
            resets: 0,
            heartbeats: 0,
            cancels: 0,
            commands: Vec::new(),
            protocol_errors: Vec::new(),
            last_cast: None,
            live: None,
        }
    }

    fn factory_reset(&mut self, cfg: &SimConfig) {
        let keep = (
            self.connected,
            self.connections,
            self.pc.clone(),
            std::mem::take(&mut self.files),
            self.uploads,
            self.cast_ok,
            self.cast_bad,
            self.formats,
            std::mem::take(&mut self.commands),
            std::mem::take(&mut self.protocol_errors),
            self.heartbeats,
        );
        *self = SimState::new(cfg);
        (
            self.connected,
            self.connections,
            self.pc,
            self.files,
            self.uploads,
            self.cast_ok,
            self.cast_bad,
            self.formats,
            self.commands,
            self.protocol_errors,
            self.heartbeats,
        ) = keep;
        self.current = if self.files.is_empty() { 0 } else { 1 };
        self.resets += 1;
    }

    pub fn status(&self, fan_type: u8) -> Status {
        Status {
            flags: 0,
            power: self.power,
            playing: self.playing,
            repeat_all: self.repeat_all,
            breath: self.breath,
            autostart: self.autostart,
            play_index: self.current,
            brightness: self.brightness,
            color_mode: self.color_mode,
            angle: self.angle,
            group: self.group_device,
            fan_type,
            extra: 0,
        }
    }

    pub fn file_names(&self) -> Vec<String> {
        self.files.iter().map(|f| f.name.clone()).collect()
    }

    pub fn errors(&self) -> &[String] {
        &self.protocol_errors
    }
}

struct Shared {
    cfg: SimConfig,
    state: Mutex<SimState>,
    changed: Condvar,
    stop: AtomicBool,
    /// Set by [`Sim::drop_connection`]; the session closes on its next read tick.
    kick: AtomicBool,
    pc_port: Mutex<u16>,
    conn: Mutex<Option<TcpStream>>,
    want_pc: Mutex<Option<SocketAddr>>,
    rng: Mutex<u64>,
}

/// A running simulated fan. Dropping it stops the simulator.
pub struct Sim {
    shared: Arc<Shared>,
    pub udp_port: u16,
}

impl Sim {
    pub fn start(cfg: SimConfig) -> std::io::Result<Sim> {
        let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, cfg.udp_port))?;
        let _ = udp.set_broadcast(true);
        udp.set_read_timeout(Some(Duration::from_millis(200)))?;
        let udp_port = udp.local_addr()?.port();
        let seed = Instant::now().elapsed().as_nanos() as u64 ^ (udp_port as u64) << 17 ^ 0x9E37_79B9_7F4A_7C15;
        let shared = Arc::new(Shared {
            state: Mutex::new(SimState::new(&cfg)),
            changed: Condvar::new(),
            stop: AtomicBool::new(false),
            kick: AtomicBool::new(false),
            pc_port: Mutex::new(cfg.pc_port),
            conn: Mutex::new(None),
            want_pc: Mutex::new(cfg.pc),
            rng: Mutex::new(seed | 1),
            cfg,
        });
        {
            let s = shared.clone();
            thread::spawn(move || udp_loop(s, udp));
        }
        {
            let s = shared.clone();
            thread::spawn(move || connect_loop(s));
        }
        Ok(Sim { shared, udp_port })
    }

    pub fn config(&self) -> &SimConfig {
        &self.shared.cfg
    }

    /// TCP port of the PC to connect to after the next broadcast.
    pub fn set_pc_port(&self, port: u16) {
        *self.shared.pc_port.lock().unwrap() = port;
    }

    /// Connect to this PC as if its broadcast had arrived (for extra fans sharing one host).
    pub fn connect_to(&self, pc: SocketAddr) {
        let mut w = self.shared.want_pc.lock().unwrap();
        if w.is_none() || !self.shared.state.lock().unwrap().connected {
            *w = Some(pc);
        }
    }

    pub fn state(&self) -> SimState {
        self.shared.state.lock().unwrap().clone()
    }

    /// Change the simulated device state (e.g. fill the card).
    pub fn modify(&self, f: impl FnOnce(&mut SimState)) {
        f(&mut self.shared.state.lock().unwrap());
        self.shared.changed.notify_all();
    }

    /// Wait until `pred` holds for the state.
    pub fn wait_until(&self, timeout: Duration, pred: impl Fn(&SimState) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        let mut g = self.shared.state.lock().unwrap();
        loop {
            if pred(&g) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            g = self.shared.changed.wait_timeout(g, left.min(Duration::from_millis(50))).unwrap().0;
        }
    }

    /// Close the TCP connection as if Wi-Fi dropped; the fan reconnects on the next broadcast.
    pub fn drop_connection(&self) {
        self.shared.kick.store(true, Ordering::SeqCst);
        if let Some(c) = self.shared.conn.lock().unwrap().take() {
            let _ = c.shutdown(Shutdown::Both);
        }
    }

    /// Send raw bytes to the PC (to test the PC's handling of odd input).
    pub fn inject(&self, data: &[u8]) -> bool {
        match self.shared.conn.lock().unwrap().as_ref().and_then(|c| c.try_clone().ok()) {
            Some(mut c) => c.write_all(data).is_ok(),
            None => false,
        }
    }

    /// JPEG currently on the "blades": a live cast frame (< 1 s old) or the playing file.
    pub fn current_jpeg(&self) -> Option<Vec<u8>> {
        let g = self.shared.state.lock().unwrap();
        if let Some((t, j)) = &g.last_cast {
            if t.elapsed() < Duration::from_secs(1) {
                return Some(j.clone());
            }
        }
        if !g.power || g.current == 0 {
            return None;
        }
        let f = g.files.get(g.current as usize - 1)?;
        if f.jpegs.is_empty() {
            return None;
        }
        let i = if g.playing { (epoch().elapsed().as_millis() / 50) as usize % f.jpegs.len() } else { 0 };
        Some(f.jpegs[i].clone())
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.drop_connection();
    }
}

fn epoch() -> Instant {
    static E: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *E.get_or_init(Instant::now)
}

impl Drop for Sim {
    fn drop(&mut self) {
        self.stop();
    }
}

fn udp_loop(sh: Arc<Shared>, sock: UdpSocket) {
    let mut buf = [0u8; 256];
    while !sh.stop.load(Ordering::SeqCst) {
        let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
        let txt = String::from_utf8_lossy(&buf[..n]).trim().to_string();
        let Some(ip) = txt.strip_suffix("HS").and_then(|s| s.parse::<Ipv4Addr>().ok()) else { continue };
        let reply_ip = if ip.is_loopback() { Ipv4Addr::LOCALHOST } else { sh.cfg.ip };
        let _ = sock.send_to(format!("{reply_ip}HS").as_bytes(), from);
        let mut w = sh.want_pc.lock().unwrap();
        if w.is_none() || !sh.state.lock().unwrap().connected {
            *w = Some(SocketAddr::from((ip, *sh.pc_port.lock().unwrap())));
        }
    }
}

fn connect_loop(sh: Arc<Shared>) {
    while !sh.stop.load(Ordering::SeqCst) {
        let target = *sh.want_pc.lock().unwrap();
        let Some(addr) = target else {
            thread::sleep(Duration::from_millis(50));
            continue;
        };
        match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                sh.kick.store(false, Ordering::SeqCst);
                *sh.conn.lock().unwrap() = s.try_clone().ok();
                {
                    let mut g = sh.state.lock().unwrap();
                    g.connected = true;
                    g.connections += 1;
                    // live mode belongs to the connection that started it
                    g.live = None;
                    g.pc = Some(addr.to_string());
                }
                sh.changed.notify_all();
                log::info!("[sim {}] connected to {addr}", sh.cfg.name);
                session(&sh, s);
                *sh.conn.lock().unwrap() = None;
                sh.state.lock().unwrap().connected = false;
                sh.changed.notify_all();
                log::info!("[sim {}] connection to {addr} lost", sh.cfg.name);
                if !sh.cfg.reconnect {
                    break;
                }
                if sh.cfg.pc.is_none() {
                    *sh.want_pc.lock().unwrap() = None;
                }
                thread::sleep(Duration::from_millis(300));
            }
            Err(_) => thread::sleep(Duration::from_millis(300)),
        }
    }
}

enum Mode {
    Commands,
    /// Receiving a file after a valid `84` file name: `[duration u32le]`, then frame records.
    Upload(Upload),
}

#[derive(Default)]
struct Upload {
    name: String,
    duration: Option<u32>,
    frames: Vec<Vec<u8>>,
    sound_frames: usize,
    problems: Vec<String>,
}

/// Check one frame record (`[len][1600 PCM][JPEG][1–4 × 00]`, `len` already removed) and
/// return its JPEG, or what is wrong with it.
fn frame_record(rec: &[u8]) -> Result<Vec<u8>, String> {
    let area = &rec[proto::UPLOAD_PCM..];
    let jpegs = jpegs_in(area);
    if jpegs.len() != 1 {
        return Err(format!("record holds {} JPEGs; the fan expects one record per frame", jpegs.len()));
    }
    let j = &jpegs[0];
    if !area.starts_with(&[0xFF, 0xD8]) {
        return Err("JPEG does not start right after the 1600 PCM bytes".into());
    }
    let pad = &area[j.len()..];
    if !(1..=4).contains(&pad.len()) || pad.iter().any(|&b| b != 0) || (j.len() & !3) + 4 != area.len() {
        return Err(format!("JPEG of {} bytes needs 1–4 zero bytes up to a multiple of 4, got {}", j.len(), pad.len()));
    }
    if j.len() > proto::MAX_UPLOAD_JPEG {
        return Err(format!("JPEG of {} bytes is over the {} byte limit", j.len(), proto::MAX_UPLOAD_JPEG));
    }
    Ok(j.clone())
}

struct Out<'a> {
    sh: &'a Shared,
    s: TcpStream,
}

impl Out<'_> {
    fn send(&mut self, data: &[u8]) {
        if !self.sh.cfg.fragment {
            let _ = self.s.write_all(data);
            return;
        }
        let mut i = 0;
        while i < data.len() {
            let r = {
                let mut x = self.sh.rng.lock().unwrap();
                *x ^= *x << 13;
                *x ^= *x >> 7;
                *x ^= *x << 17;
                *x
            };
            let n = (1 + (r % 7) as usize).min(data.len() - i);
            if self.s.write_all(&data[i..i + n]).is_err() {
                return;
            }
            i += n;
            if r % 3 == 0 {
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
    fn ready(&mut self) {
        self.send(&[SOF, op::READY, op::READY, EOF]);
    }
}

/// Length of the PC → fan packet at `b[0]` (= `5A`), `None` if more bytes are needed,
/// `Some(Err)` if the length cannot be derived from the opcode.
fn command_len(b: &[u8]) -> Option<Result<usize, ()>> {
    let o = *b.get(1)?;
    Some(Ok(match o {
        op::ON
        | op::OFF
        | op::PAUSE
        | op::PLAY
        | op::REPEAT_ALL
        | op::REPEAT_ONE
        | op::FORMAT_CARD
        | op::FACTORY_RESET
        | op::BREATH_ON
        | op::BREATH_OFF
        | op::AUTOSTART_ON
        | op::AUTOSTART_OFF
        | op::HEARTBEAT
        | op::TRANS_CANCEL
        | op::GET_MACHINE
        | op::SELECT_DEVICE
        | op::ANGLE_ALIGN_START
        | op::ANGLE_ALIGN_STOP
        | op::COLOR_TRUE
        | op::COLOR_BRIGHT => 4,
        op::DELETE | op::BRIGHTNESS | op::SELECT | op::VOLUME => 5,
        op::ANGLE => 6,
        op::COMMIT => 8,
        op::FILE_NAME => 2 + 1 + *b.get(2)? as usize + 2,
        op::RENAME => 2 + 2 + *b.get(3)? as usize + 2,
        op::DEVICE_NAME => 36,
        op::HOTSPOT | op::ROUTER => 68,
        op::CAST_FRAME => CAST_FRAME_SIZE,
        _ => return Some(Err(())),
    }))
}

fn op_name(o: u8) -> String {
    match o {
        op::ON => "on",
        op::OFF => "off",
        op::PAUSE => "pause",
        op::PLAY => "play",
        op::REPEAT_ALL => "repeat_all",
        op::REPEAT_ONE => "repeat_one",
        op::FORMAT_CARD => "format",
        op::FACTORY_RESET => "reset",
        op::BREATH_ON => "breath_on",
        op::BREATH_OFF => "breath_off",
        op::AUTOSTART_ON => "autostart_on",
        op::AUTOSTART_OFF => "autostart_off",
        op::HEARTBEAT => "heartbeat",
        op::TRANS_CANCEL => "cancel",
        op::GET_MACHINE => "get_machine",
        op::SELECT_DEVICE => "select_device",
        op::CAST_FRAME => "cast",
        op::ANGLE_ALIGN_START => "align_start",
        op::ANGLE_ALIGN_STOP => "align_stop",
        op::COLOR_TRUE => "color_true",
        op::COLOR_BRIGHT => "color_bright",
        op::DELETE => "delete",
        op::BRIGHTNESS => "brightness",
        op::ANGLE => "angle",
        op::RENAME => "rename",
        op::FILE_NAME => "filename",
        op::SELECT => "select",
        op::DEVICE_NAME => "device_name",
        op::HOTSPOT => "hotspot",
        op::ROUTER => "router",
        op::VOLUME => "volume",
        op::COMMIT => "commit",
        other => return format!("op_{other:02x}"),
    }
    .to_string()
}

fn field(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).to_string()
}

fn jpegs_in(data: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < data.len() {
        if data[i] == 0xFF && data[i + 1] == 0xD8 {
            if let Some(j) = data[i + 2..].windows(2).position(|w| w == [0xFF, 0xD9]) {
                let end = i + 2 + j + 2;
                out.push(data[i..end].to_vec());
                i = end;
                continue;
            }
            break;
        }
        i += 1;
    }
    out
}

/// Check a JPEG has the layout this fan type expects. Returns (w, h).
fn check_jpeg(jpeg: &[u8], fan_type: u8) -> Result<(u32, u32), String> {
    let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).map_err(|e| format!("JPEG unreadable: {e}"))?;
    let (w, h) = (img.width(), img.height());
    if let Some(ft) = FanType::known(fan_type) {
        if (w, h) != ft.jpeg_size() {
            return Err(format!("JPEG {w}×{h}, expected {:?} for type {fan_type}", ft.jpeg_size()));
        }
    }
    Ok((w, h))
}

/// The fan's `8F` report, the only place it reports the speaker volume. The simulator has no
/// Bluetooth (P56 Studio does not use it), so it always reports Bluetooth off.
fn volume_report(sh: &Shared) -> Vec<u8> {
    let volume = sh.state.lock().unwrap().volume;
    proto::bluetooth_packet(&proto::Bluetooth { volume, ..Default::default() })
}

/// A file has been received: check it like the firmware would, store it and acknowledge.
/// Returns whether it was stored.
fn finish_upload(sh: &Shared, out: &mut Out, up: Upload, ft: u8) -> bool {
    let name = up.name;
    let mut ok = up.problems.is_empty() && !up.frames.is_empty();
    for p in &up.problems {
        sh.err(format!("upload {name}: {p}"));
    }
    if up.frames.is_empty() && up.problems.is_empty() {
        sh.err(format!("upload {name}: no frames"));
    }
    let n = up.frames.len() as u32;
    match up.duration {
        Some(0) | None => {
            sh.err(format!("upload {name}: no duration"));
            ok = false;
        }
        Some(d) if n > 1 && d != n * proto::FRAME_MS => {
            sh.err(format!("upload {name}: duration {d} ms, expected {} ms for {n} frames at 20 fps", n * proto::FRAME_MS));
            ok = false;
        }
        _ => {}
    }
    let mut dims = (0, 0);
    for (i, j) in up.frames.iter().enumerate() {
        match check_jpeg(j, ft) {
            Ok(d) => dims = d,
            Err(e) => {
                sh.err(format!("upload {name} frame {}: {e}", i + 1));
                ok = false;
            }
        }
    }
    if ok {
        let mut g = sh.state.lock().unwrap();
        g.files.push(SimFile { name, frames: up.frames.len(), sound_frames: up.sound_frames, width: dims.0, height: dims.1, jpegs: up.frames });
        g.current = g.files.len() as u8;
        g.uploads += 1;
    }
    sh.changed.notify_all();
    if sh.state.lock().unwrap().quiet_upload {
        out.send(&[SOF, op::TRANS_DONE, op::TRANS_DONE, EOF]);
    } else {
        out.ready();
    }
    ok
}

fn session(sh: &Shared, stream: TcpStream) {
    let Ok(mut rd) = stream.try_clone() else { return };
    let mut out = Out { sh, s: stream };
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut mode = Mode::Commands;
    let mut cast_acked = false;
    let mut just_uploaded = false;
    let mut pad = false;
    let mut tmp = vec![0u8; 64 * 1024];
    let ft = sh.cfg.fan_type;
    let _ = rd.set_read_timeout(Some(Duration::from_millis(100)));
    loop {
        if sh.kick.load(Ordering::SeqCst) || sh.stop.load(Ordering::SeqCst) {
            let _ = rd.shutdown(Shutdown::Both);
            return;
        }
        let n = match rd.read(&mut tmp) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                // a pause after whole records: the file is complete
                if let Mode::Upload(up) = &mut mode {
                    if buf.is_empty() && !up.frames.is_empty() {
                        just_uploaded = finish_upload(sh, &mut out, std::mem::take(up), ft);
                        mode = Mode::Commands;
                    }
                }
                continue;
            }
            Err(_) => return,
        };
        buf.extend_from_slice(&tmp[..n]);
        loop {
            if let Mode::Upload(up) = &mut mode {
                if up.duration.is_none() {
                    if buf.len() < 4 {
                        break;
                    }
                    up.duration = Some(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]));
                    buf.drain(..4);
                }
                if buf.len() < 4 {
                    break;
                }
                let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
                // Records are 4-byte aligned; a command (5A …) never is, so it ends the file.
                if !len.is_multiple_of(4) || !(proto::UPLOAD_PCM + 8..=proto::UPLOAD_PCM + proto::MAX_UPLOAD_JPEG + 4).contains(&len) {
                    if up.frames.is_empty() && up.problems.is_empty() {
                        up.problems.push(format!("first frame record has an impossible length {len}"));
                    }
                    just_uploaded = finish_upload(sh, &mut out, std::mem::take(up), ft);
                    mode = Mode::Commands;
                    continue;
                }
                if buf.len() < 4 + len {
                    break;
                }
                let rec: Vec<u8> = buf.drain(..4 + len).collect();
                match frame_record(&rec[4..]) {
                    Ok(j) => {
                        up.sound_frames += rec[4..4 + proto::UPLOAD_PCM].iter().any(|&b| b != 0) as usize;
                        up.frames.push(j)
                    }
                    Err(e) => up.problems.push(format!("frame {}: {e}", up.frames.len() + up.problems.len() + 1)),
                }
                continue;
            }
            // command mode
            if pad && !buf.is_empty() {
                // Holoscope pads 4-byte packets to 5 bytes (heartbeat `3E`).
                if buf[0] != SOF {
                    buf.remove(0);
                }
                pad = false;
                continue;
            }
            let Some(start) = buf.iter().position(|&b| b == SOF) else {
                if !buf.is_empty() {
                    sh.err(format!("unexpected bytes {:02x?}", &buf[..buf.len().min(16)]));
                }
                buf.clear();
                break;
            };
            if start > 0 {
                sh.err(format!("unexpected bytes {:02x?}", &buf[..start.min(16)]));
                buf.drain(..start);
            }
            let len = match command_len(&buf) {
                None => break,
                Some(Ok(l)) => l,
                Some(Err(())) => match buf.iter().skip(2).position(|&b| b == EOF) {
                    Some(p) => p + 3,
                    None => {
                        if buf.len() > 4096 {
                            sh.err("unknown packet without F5".into());
                            buf.clear();
                        }
                        break;
                    }
                },
            };
            if buf[1] == op::CAST_FRAME && !cast_acked && buf.len() >= CAST_CHUNK {
                cast_acked = true;
                if !sh.state.lock().unwrap().cast_mute {
                    out.ready();
                }
            }
            if buf.len() < len {
                break;
            }
            let pkt: Vec<u8> = buf.drain(..len).collect();
            if pkt[1] == op::CAST_FRAME {
                cast_acked = false;
            }
            pad = pkt.len() == 4;
            if let Some(m) = handle(sh, &mut out, &pkt, ft, &mut just_uploaded) {
                mode = m;
            }
        }
    }
}

impl Shared {
    fn err(&self, e: String) {
        log::warn!("[sim {}] protocol error: {e}", self.cfg.name);
        self.state.lock().unwrap().protocol_errors.push(e);
        self.changed.notify_all();
    }
    fn info(&self) -> MachineInfo {
        let g = self.state.lock().unwrap();
        MachineInfo {
            stitch: [0; 4],
            name: g.device_name.clone(),
            mac: self.cfg.mac.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"),
            hotspot: g.hotspot.0.clone(),
            router: g.router.0.clone(),
        }
    }
}

/// A real P56 stores names of at most 24 bytes before `.mp4`: longer ones are not stored, and from
/// 28 bytes on they break its file list (measured).
fn long_name(sh: &Shared, name: &str) {
    let stem = name.strip_suffix(".mp4").unwrap_or(name).len();
    if stem > 24 {
        sh.err(format!("file name {name:?}: {stem} bytes before .mp4, a real P56 stores at most 24"));
    }
}

/// How long live mode lasts without a cast frame (longer than the link's own session pause).
const LIVE_TIMEOUT: Duration = Duration::from_millis(1500);

fn live(sh: &Shared) -> bool {
    sh.state.lock().unwrap().live.is_some_and(|t| t.elapsed() < LIVE_TIMEOUT)
}

fn handle(sh: &Shared, out: &mut Out, pkt: &[u8], ft: u8, just_uploaded: &mut bool) -> Option<Mode> {
    let o = pkt[1];
    let n = pkt.len();
    if pkt[n - 1] != EOF && o != op::CAST_FRAME {
        sh.err(format!("{}: last byte {:02x} instead of F5", op_name(o), pkt[n - 1]));
        return None;
    }
    {
        let mut g = sh.state.lock().unwrap();
        if o != op::CAST_FRAME && o != op::HEARTBEAT {
            g.commands.push(op_name(o));
            if g.commands.len() > 500 {
                g.commands.remove(0);
            }
        }
    }
    let status = |sh: &Shared| {
        let g = sh.state.lock().unwrap();
        g.status(ft).to_packet()
    };
    let files = |sh: &Shared| proto::filelist_packet(&sh.state.lock().unwrap().file_names());
    let check_dup = |pkt: &[u8], what: &str| {
        if pkt[2] != pkt[3] {
            sh.err(format!("{what}: value {:02x} ≠ check {:02x}", pkt[2], pkt[3]));
            false
        } else {
            true
        }
    };
    let xor_ok = |pkt: &[u8], what: &str| {
        let n = pkt.len();
        let x = proto::xorsum(&pkt[2..n - 2]);
        if x != pkt[n - 2] {
            sh.err(format!("{what}: checksum {:02x} ≠ {:02x}", pkt[n - 2], x));
            false
        } else {
            true
        }
    };
    let simple = |pkt: &[u8]| {
        if pkt.len() == 4 && pkt[2] != pkt[1] && pkt[1] != op::SELECT_DEVICE {
            sh.err(format!("{}: byte 2 is {:02x}", op_name(pkt[1]), pkt[2]));
        }
    };
    if n == 4 {
        simple(pkt);
    }

    let set = |f: &dyn Fn(&mut SimState)| {
        f(&mut sh.state.lock().unwrap());
        sh.changed.notify_all();
    };
    match o {
        op::ON => {
            set(&|g| {
                g.power = true;
                if g.slow_start && !g.files.is_empty() {
                    g.current = 1;
                    g.missed_selects = 1;
                }
            });
            out.ready();
            out.send(&status(sh));
        }
        op::OFF => {
            // a real P56 in live mode ignores it (seen 2026-10-13 right after a cast)
            if !live(sh) {
                set(&|g| g.power = false);
            }
            out.ready();
            out.send(&status(sh));
        }
        op::PAUSE => {
            set(&|g| g.playing = false);
            out.ready();
            out.send(&status(sh));
        }
        op::PLAY => {
            set(&|g| {
                g.playing = true;
                g.power = true;
            });
            out.ready();
            out.send(&status(sh));
        }
        op::REPEAT_ALL | op::REPEAT_ONE => {
            set(&|g| g.repeat_all = o == op::REPEAT_ALL);
            out.ready();
            out.send(&status(sh));
        }
        op::FORMAT_CARD => {
            set(&|g| {
                g.files.clear();
                g.current = 0;
                g.formats += 1;
            });
            out.ready();
            out.send(&files(sh));
            out.send(&status(sh));
        }
        op::FACTORY_RESET => {
            sh.state.lock().unwrap().factory_reset(&sh.cfg);
            sh.changed.notify_all();
            out.ready();
            out.send(&status(sh));
            out.send(&sh.info().to_packet());
        }
        op::BREATH_ON | op::BREATH_OFF => {
            set(&|g| g.breath = o == op::BREATH_ON);
            out.ready();
            out.send(&status(sh));
        }
        op::AUTOSTART_ON | op::AUTOSTART_OFF => {
            set(&|g| g.autostart = o == op::AUTOSTART_ON);
            out.ready();
            out.send(&status(sh));
        }
        op::HEARTBEAT => {
            if live(sh) {
                sh.err("heartbeat during a live cast (Holoscope.exe sends none)".into());
            }
            set(&|g| g.heartbeats += 1);
            out.send(&status(sh));
        }
        op::TRANS_CANCEL => {
            set(&|g| g.cancels += 1);
            out.ready();
        }
        op::GET_MACHINE => {
            if live(sh) {
                sh.err("status poll (1C) during a live cast (Holoscope.exe sends none)".into());
            }
            out.send(&sh.info().to_packet());
            out.send(&files(sh));
            out.send(&status(sh));
        }
        op::SELECT_DEVICE => {
            if pkt[2] != op::SELECT_DEVICE {
                sh.err(format!("select_device: expected 5A 23 23 F5, got byte 2 = {:02x}", pkt[2]));
            }
            set(&|g| g.group_device = 1);
            out.ready();
        }
        op::ANGLE_ALIGN_START | op::ANGLE_ALIGN_STOP => {
            set(&|g| g.angle_align = o == op::ANGLE_ALIGN_START);
            out.ready();
        }
        op::COLOR_TRUE | op::COLOR_BRIGHT => {
            set(&|g| g.color_mode = if o == op::COLOR_TRUE { 1 } else { 2 });
            out.ready();
            out.send(&status(sh));
        }
        op::DELETE => {
            if check_dup(pkt, "delete") {
                let i = pkt[2];
                let ok = {
                    let g = sh.state.lock().unwrap();
                    i >= 1 && (i as usize) <= g.files.len()
                };
                if ok {
                    set(&|g| {
                        g.files.remove(i as usize - 1);
                        if g.current == i {
                            g.current = if g.files.is_empty() { 0 } else { 1 };
                        } else if g.current > i {
                            g.current -= 1;
                        }
                    });
                }
                out.ready();
                out.send(&files(sh));
                out.send(&status(sh));
            }
        }
        op::SELECT => {
            if check_dup(pkt, "select") {
                let i = pkt[2];
                let ok = {
                    let g = sh.state.lock().unwrap();
                    i >= 1 && (i as usize) <= g.files.len()
                };
                // a fan just switched on (with `slow_start`) misses this select
                let missed = {
                    let mut g = sh.state.lock().unwrap();
                    let m = g.missed_selects > 0;
                    g.missed_selects = g.missed_selects.saturating_sub(1);
                    m
                };
                if ok && !missed {
                    set(&|g| {
                        g.current = i;
                        g.playing = true;
                        // a real P56 goes back to repeating the whole playlist (seen 2026-10-12)
                        g.repeat_all = true;
                    });
                }
                out.ready();
                out.send(&status(sh));
            }
        }
        op::BRIGHTNESS => {
            if check_dup(pkt, "brightness") {
                let v = pkt[2];
                if !(1..=16).contains(&v) {
                    sh.err(format!("brightness {v} outside 1–16"));
                }
                set(&|g| g.brightness = v.clamp(1, 16));
                out.ready();
                out.send(&status(sh));
            }
        }
        op::VOLUME => {
            if check_dup(pkt, "volume") {
                let v = pkt[2];
                if v > 10 {
                    sh.err(format!("volume {v} outside 0–10"));
                }
                set(&|g| g.volume = v.min(10));
                out.ready();
                out.send(&status(sh));
                // the volume is only reported in the 8F packet (assumed: sent after a change)
                out.send(&volume_report(sh));
            }
        }
        op::ANGLE => {
            if pkt[4] != pkt[2] ^ pkt[3] {
                sh.err("angle: bad checksum".into());
            }
            let a = u16::from_be_bytes([pkt[2], pkt[3]]);
            if a > 360 {
                sh.err(format!("angle {a} > 360"));
            }
            set(&|g| g.angle = a.min(360));
            out.ready();
            out.send(&status(sh));
        }
        op::RENAME => {
            if xor_ok(pkt, "rename") {
                let slot = pkt[2];
                let name = String::from_utf8_lossy(&pkt[4..4 + pkt[3] as usize]).to_string();
                long_name(sh, &name);
                if !name.ends_with(".mp4") {
                    sh.err(format!("rename: name {name:?} does not end in .mp4"));
                }
                let (exists, dup) = {
                    let g = sh.state.lock().unwrap();
                    (slot >= 1 && (slot as usize) <= g.files.len(), g.files.iter().enumerate().any(|(i, f)| f.name == name && i + 1 != slot as usize))
                };
                if dup {
                    out.send(&FanError::DuplicateName.to_packet());
                } else if exists {
                    set(&|g| g.files[slot as usize - 1].name = name.clone());
                    out.ready();
                } else {
                    out.ready();
                }
                out.send(&files(sh));
            }
        }
        op::DEVICE_NAME => {
            if xor_ok(pkt, "device_name") {
                let s = field(&pkt[2..34]);
                set(&|g| g.device_name = s.clone());
                out.ready();
                out.send(&sh.info().to_packet());
            }
        }
        op::HOTSPOT | op::ROUTER => {
            if xor_ok(pkt, op_name(o).as_str()) {
                let (a, b) = (field(&pkt[2..34]), field(&pkt[34..66]));
                if o == op::HOTSPOT {
                    set(&|g| g.hotspot = (a.clone(), b.clone()));
                } else {
                    set(&|g| g.router = (a.clone(), b.clone()));
                }
                out.ready();
                out.send(&sh.info().to_packet());
            }
        }
        op::COMMIT => {
            let x = proto::xorsum(&pkt[2..6]);
            if x != pkt[6] {
                sh.err("commit: bad checksum".into());
            }
            out.ready();
            if *just_uploaded {
                *just_uploaded = false;
                if !sh.state.lock().unwrap().quiet_upload {
                    out.send(&[SOF, op::TRANS_DONE, op::TRANS_DONE, EOF]);
                }
                // once the file is stored the fan sends its playlist and status by itself (a real
                // P56 is silent while it receives and writes)
                out.send(&files(sh));
                out.send(&status(sh));
            }
        }
        op::FILE_NAME => {
            if !xor_ok(pkt, "filename") {
                return None;
            }
            let name = String::from_utf8_lossy(&pkt[3..3 + pkt[2] as usize]).to_string();
            if !name.ends_with(".mp4") {
                sh.err(format!("filename {name:?} does not end in .mp4"));
            }
            long_name(sh, &name);
            // a real P56 in live mode does not store a file (seen 2026-10-12: an upload while
            // pages were cast was never listed)
            if live(sh) {
                sh.err(format!("upload of {name} during live mode: the cast must be over first"));
                return None;
            }
            let (full, dup) = {
                let g = sh.state.lock().unwrap();
                (g.files.len() >= sh.cfg.max_files, g.files.iter().any(|f| f.name == name))
            };
            if dup {
                out.send(&FanError::DuplicateName.to_packet());
                return None;
            }
            if full {
                out.send(&FanError::TooManyFiles.to_packet());
                return None;
            }
            out.ready();
            return Some(Mode::Upload(Upload { name, ..Default::default() }));
        }
        op::CAST_START => {
            set(&|g| g.live = Some(Instant::now()));
            out.send(&[SOF, op::TRANS_DONE, op::TRANS_DONE, EOF]);
        }
        op::CAST_FRAME => {
            // A real P56 shows cast frames only in live mode, which 0x26 starts and which ends
            // when no frame comes for a while; outside it the frames are answered but not shown.
            // The error goes in before the count: whoever waits for `cast_bad` must find it.
            if !live(sh) {
                sh.err("cast frame outside live mode: send 5A 26 26 F5 first (a real P56 keeps playing its playlist)".into());
                set(&|g| g.cast_bad += 1);
                if !sh.state.lock().unwrap().cast_mute {
                    out.ready();
                }
                return None;
            }
            let good = pkt[n - 2] == 0x55 && pkt[n - 1] == EOF;
            let jp = jpegs_in(&pkt[2..n - 2]);
            let res = if !good {
                Err("cast: tail is not 55 F5".to_string())
            } else if jp.len() != 1 {
                Err(format!("cast: {} JPEGs in frame", jp.len()))
            } else if pkt[2..4] != [0xFF, 0xD8] {
                Err("cast: JPEG does not start right after 5A 28".into())
            } else {
                check_jpeg(&jp[0], ft).map(|_| ())
            };
            match res {
                Ok(()) => {
                    let j = jp.into_iter().next().unwrap();
                    set(&|g| {
                        g.cast_ok += 1;
                        g.last_cast = Some((Instant::now(), j.clone()));
                        g.live = Some(Instant::now());
                    });
                }
                Err(e) => {
                    sh.err(e);
                    set(&|g| g.cast_bad += 1);
                }
            }
            if !sh.state.lock().unwrap().cast_mute {
                out.ready();
            }
        }
        other => {
            sh.err(format!("unknown command {other:02x}: {:02x?}", &pkt[..pkt.len().min(16)]));
            out.ready();
        }
    }
    None
}
