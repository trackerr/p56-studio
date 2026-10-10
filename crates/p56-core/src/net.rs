//! Networking: the PC is the TCP *server* (port 6666); the fan connects to it after
//! hearing our UDP broadcast on 8988. One [`Link`] serves any number of fans.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::{self as proto, MachineInfo, Packet, Status, CAST_CHUNK, CAST_FRAME_SIZE, TCP_PORT, UDP_PORT, UPLOAD_CHUNK};
use crate::Error;

/// Events emitted by the link, for UIs.
#[derive(Debug, Clone)]
pub enum Event {
    Discovered { ip: IpAddr },
    Connected { id: u32, ip: IpAddr },
    Disconnected { id: u32 },
    Packet { id: u32, packet: Packet },
    Log(String),
}

#[derive(Debug, Clone, Default)]
pub struct Device {
    pub id: u32,
    pub ip: Option<IpAddr>,
    pub status: Option<Status>,
    pub info: Option<MachineInfo>,
    pub files: Vec<(u8, String)>,
    pub last_seen: Option<Instant>,
}

struct Conn {
    stream: TcpStream,
    dev: Device,
    acks: Arc<(Mutex<u64>, Condvar)>,
}

struct Inner {
    conns: HashMap<u32, Conn>,
    next_id: u32,
    seen: HashMap<IpAddr, Instant>,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// IP to advertise in the broadcast. `None` = auto-detect.
    pub local_ip: Option<Ipv4Addr>,
    pub tcp_port: u16,
    pub udp_port: u16,
    pub broadcast_interval: Duration,
    pub heartbeat: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { local_ip: None, tcp_port: TCP_PORT, udp_port: UDP_PORT, broadcast_interval: Duration::from_secs(3), heartbeat: true }
    }
}

#[derive(Clone)]
pub struct Link {
    inner: Arc<Mutex<Inner>>,
    events: Sender<Event>,
    connected: Arc<(Mutex<()>, Condvar)>,
    pub local_ip: Ipv4Addr,
    pub tx_lock: Arc<Mutex<()>>,
}

/// Best-effort local IPv4 as seen from the fan's network (default AP is 192.168.4.1).
pub fn local_ip() -> Ipv4Addr {
    let s = UdpSocket::bind("0.0.0.0:0").ok();
    if let Some(s) = s {
        if s.connect("192.168.4.1:1").is_ok() {
            if let Ok(SocketAddr::V4(a)) = s.local_addr() {
                if !a.ip().is_loopback() { return *a.ip(); }
            }
        }
    }
    Ipv4Addr::new(192, 168, 4, 2)
}

impl Link {
    /// Start broadcasting and listening. Returns the link and its event stream.
    pub fn start(cfg: Config) -> Result<(Link, Receiver<Event>), Error> {
        let (tx, rx) = mpsc::channel();
        let ip = cfg.local_ip.unwrap_or_else(local_ip);
        let link = Link {
            inner: Arc::new(Mutex::new(Inner { conns: HashMap::new(), next_id: 1, seen: HashMap::new() })),
            events: tx,
            connected: Arc::new((Mutex::new(()), Condvar::new())),
            local_ip: ip,
            tx_lock: Arc::new(Mutex::new(())),
        };
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, cfg.tcp_port))
            .map_err(|e| Error::Io(format!("bind {}: {e}", cfg.tcp_port)))?;
        link.log(format!("TCP server on :{} (advertising {})", cfg.tcp_port, ip));

        { let l = link.clone(); thread::spawn(move || l.accept_loop(listener)); }
        { let l = link.clone(); let c = cfg.clone(); thread::spawn(move || l.udp_loop(c)); }
        if cfg.heartbeat { let l = link.clone(); thread::spawn(move || l.heartbeat_loop()); }
        Ok((link, rx))
    }

    fn log(&self, s: String) {
        log::info!("{s}");
        let _ = self.events.send(Event::Log(s));
    }

    fn udp_loop(&self, cfg: Config) {
        let Ok(sock) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, cfg.udp_port))
            .or_else(|_| UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))) else { return };
        let _ = sock.set_broadcast(true);
        let _ = sock.set_read_timeout(Some(Duration::from_millis(500)));
        let msg = format!("{}HS", self.local_ip).into_bytes();
        let mut last = Instant::now() - cfg.broadcast_interval;
        let mut buf = [0u8; 256];
        loop {
            if last.elapsed() >= cfg.broadcast_interval {
                let _ = sock.send_to(&msg, (Ipv4Addr::BROADCAST, cfg.udp_port));
                last = Instant::now();
            }
            if let Ok((n, from)) = sock.recv_from(&mut buf) {
                let d = &buf[..n];
                if from.ip() != IpAddr::V4(self.local_ip) && d.ends_with(b"HS") && d.starts_with(b"1") {
                    let mut g = self.inner.lock().unwrap();
                    let fresh = g.seen.insert(from.ip(), Instant::now()).is_none();
                    drop(g);
                    if fresh { let _ = self.events.send(Event::Discovered { ip: from.ip() }); }
                }
            }
        }
    }

    fn accept_loop(&self, listener: TcpListener) {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_nodelay(true);
            let peer = stream.peer_addr().map(|a| a.ip()).ok();
            let Ok(reader) = stream.try_clone() else { continue };
            let acks = Arc::new((Mutex::new(0u64), Condvar::new()));
            let id = {
                let mut g = self.inner.lock().unwrap();
                let id = g.next_id;
                g.next_id += 1;
                g.conns.insert(id, Conn { stream, dev: Device { id, ip: peer, last_seen: Some(Instant::now()), ..Default::default() }, acks: acks.clone() });
                id
            };
            self.log(format!("fan #{id} connected from {}", peer.map(|p| p.to_string()).unwrap_or_default()));
            let _ = self.events.send(Event::Connected { id, ip: peer.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)) });
            self.connected.1.notify_all();
            let l = self.clone();
            thread::spawn(move || l.read_loop(id, reader, acks));
            // ask who it is
            let _ = self.send_to(id, &proto::get_machine());
        }
    }

    fn read_loop(&self, id: u32, mut stream: TcpStream, acks: Arc<(Mutex<u64>, Condvar)>) {
        let mut rd = proto::Reader::default();
        let mut buf = [0u8; 8192];
        loop {
            let n = match stream.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => n };
            // Holoscope treats *any* bytes from the fan as "go on" (QIODevice::readyRead).
            { let (m, cv) = &*acks; *m.lock().unwrap() += 1; cv.notify_all(); }
            for pkt in rd.push(&buf[..n]) {
                {
                    let mut g = self.inner.lock().unwrap();
                    if let Some(c) = g.conns.get_mut(&id) {
                        c.dev.last_seen = Some(Instant::now());
                        match &pkt {
                            Packet::Status(s) => c.dev.status = Some(s.clone()),
                            Packet::MachineInfo(i) => c.dev.info = Some(i.clone()),
                            Packet::FileList(f) => c.dev.files = f.clone(),
                            _ => {}
                        }
                    }
                }
                let _ = self.events.send(Event::Packet { id, packet: pkt });
            }
        }
        self.inner.lock().unwrap().conns.remove(&id);
        self.log(format!("fan #{id} disconnected"));
        let _ = self.events.send(Event::Disconnected { id });
    }

    fn heartbeat_loop(&self) {
        loop {
            thread::sleep(Duration::from_secs(3));
            let ids = self.ids();
            for id in ids {
                if self.tx_lock.try_lock().is_ok() { let _ = self.send_to(id, &proto::heartbeat()); }
            }
        }
    }

    // ---- public API ------------------------------------------------------

    pub fn ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.inner.lock().unwrap().conns.keys().copied().collect();
        v.sort();
        v
    }

    pub fn devices(&self) -> Vec<Device> {
        let g = self.inner.lock().unwrap();
        let mut v: Vec<Device> = g.conns.values().map(|c| c.dev.clone()).collect();
        v.sort_by_key(|d| d.id);
        v
    }

    pub fn discovered(&self) -> Vec<IpAddr> {
        self.inner.lock().unwrap().seen.keys().copied().collect()
    }

    /// Block until at least one fan is connected.
    pub fn wait_connected(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let (m, cv) = &*self.connected;
        let mut g = m.lock().unwrap();
        while self.ids().is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() { return false; }
            g = cv.wait_timeout(g, left).unwrap().0;
        }
        true
    }

    fn ack_count(&self, id: u32) -> Option<Arc<(Mutex<u64>, Condvar)>> {
        self.inner.lock().unwrap().conns.get(&id).map(|c| c.acks.clone())
    }

    fn stream(&self, id: u32) -> Result<TcpStream, Error> {
        self.inner.lock().unwrap().conns.get(&id).and_then(|c| c.stream.try_clone().ok()).ok_or(Error::NotConnected)
    }

    /// Send raw bytes to one fan.
    pub fn send_to(&self, id: u32, data: &[u8]) -> Result<(), Error> {
        let mut s = self.stream(id)?;
        s.write_all(data).map_err(|e| { self.inner.lock().unwrap().conns.remove(&id); Error::Io(e.to_string()) })
    }

    /// Send raw bytes to every connected fan.
    pub fn broadcast(&self, data: &[u8]) -> usize {
        self.ids().into_iter().filter(|&id| self.send_to(id, data).is_ok()).count()
    }

    /// Send and then wait up to `timeout` for *any* packet back (ack).
    pub fn send_wait(&self, id: u32, data: &[u8], timeout: Duration) -> Result<bool, Error> {
        let acks = self.ack_count(id).ok_or(Error::NotConnected)?;
        let before = *acks.0.lock().unwrap();
        self.send_to(id, data)?;
        Ok(self.wait_ack(&acks, before, timeout))
    }

    fn wait_ack(&self, acks: &Arc<(Mutex<u64>, Condvar)>, before: u64, timeout: Duration) -> bool {
        let (m, cv) = &**acks;
        let deadline = Instant::now() + timeout;
        let mut g = m.lock().unwrap();
        while *g <= before {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() { return false; }
            g = cv.wait_timeout(g, left).unwrap().0;
        }
        true
    }

    /// Convenience wrappers --------------------------------------------------
    pub fn on(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::on()) }
    pub fn off(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::off()) }
    pub fn play(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::play()) }
    pub fn pause(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::pause()) }
    pub fn select(&self, id: u32, slot: u8) -> Result<(), Error> { self.cmd(id, &proto::select(slot)) }
    pub fn delete(&self, id: u32, slot: u8) -> Result<(), Error> { self.cmd(id, &proto::delete(slot)) }
    pub fn rename(&self, id: u32, old: &str, new: &str) -> Result<(), Error> { self.cmd(id, &proto::rename(old, new)) }
    pub fn set_wifi(&self, id: u32, ssid: &str, pw: &str) -> Result<(), Error> { self.cmd(id, &proto::wifi(ssid, pw)) }
    pub fn refresh(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::get_machine()) }

    fn cmd(&self, id: u32, data: &[u8]) -> Result<(), Error> {
        let _g = self.tx_lock.lock().unwrap();
        self.send_wait(id, data, Duration::from_millis(400)).map(|_| ())
    }

    /// Upload one or more fan-layout JPEG frames as a playlist entry and show it.
    /// Mirrors the verified Holoscope / v25 sequence.
    pub fn upload(&self, id: u32, frames: &[Vec<u8>], name: &str, show_slot: Option<u8>, seq: u32) -> Result<(), Error> {
        if frames.is_empty() { return Err(Error::Invalid("no frames".into())); }
        let name = if name.to_ascii_lowercase().ends_with(".mp4") { name.to_string() } else { format!("{name}.mp4") };
        let _g = self.tx_lock.lock().unwrap();
        let w = |d: &[u8], t: u64| -> Result<(), Error> { self.send_wait(id, d, Duration::from_millis(t)).map(|_| ()) };
        w(&proto::on(), 150)?;
        w(&proto::set_filename(&name), 100)?;
        let (p1, p2) = proto::upload_blob(frames, proto::upload_tag(seq));
        let mut s = self.stream(id)?;
        s.write_all(&p1).map_err(|e| Error::Io(e.to_string()))?;
        for chunk in p2.chunks(UPLOAD_CHUNK) {
            s.write_all(chunk).map_err(|e| Error::Io(e.to_string()))?;
            thread::sleep(Duration::from_millis(2));
        }
        thread::sleep(Duration::from_millis(200));
        w(&proto::commit(), 150)?;
        w(&proto::commit(), 150)?;
        w(&proto::off(), 100)?;
        w(&proto::on(), 100)?;
        if let Some(slot) = show_slot { w(&proto::select(slot), 150)?; }
        self.log(format!("uploaded {name}: {} frame(s), {} bytes", frames.len(), p2.len()));
        Ok(())
    }

    /// Push one live frame (a fan-layout JPEG) to every connected fan.
    /// Returns the number of fans that accepted it.
    pub fn cast(&self, jpeg: &[u8]) -> usize {
        let frame = proto::cast_frame(jpeg);
        let _g = self.tx_lock.lock().unwrap();
        let mut ok = 0;
        for id in self.ids() {
            let Some(acks) = self.ack_count(id) else { continue };
            let Ok(mut s) = self.stream(id) else { continue };
            let mut good = true;
            for chunk in frame.chunks(CAST_CHUNK) {
                let before = *acks.0.lock().unwrap();
                if s.write_all(chunk).is_err() { good = false; break; }
                // Holoscope blocks here until the fan reacts; the emulator acks within ms.
                self.wait_ack(&acks, before, Duration::from_millis(3500));
            }
            if good { ok += 1; }
        }
        debug_assert_eq!(frame.len(), CAST_FRAME_SIZE);
        ok
    }
}
