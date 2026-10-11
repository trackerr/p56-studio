//! Networking: the PC is the TCP *server* (port 6666); the fan connects to it after
//! hearing our UDP broadcast `"<pc-ip>HS"` on 8988. One [`Link`] serves any number of fans.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::{self as proto, ColorMode, FanError, MachineInfo, Packet, Status, CAST_CHUNK, TCP_PORT, UDP_PORT, UPLOAD_CHUNK};
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
    pub last_error: Option<FanError>,
    /// Latest `8F` list per kind: (index, items).
    pub lists: HashMap<u8, (u8, Vec<String>)>,
    pub last_seen: Option<Instant>,
}

/// Recent packets of one connection, so callers can wait for a specific reply.
#[derive(Default)]
struct Inbox {
    seq: u64,
    bytes: u64,
    recent: VecDeque<(u64, Packet)>,
}

struct Conn {
    stream: TcpStream,
    dev: Device,
    inbox: Arc<(Mutex<Inbox>, Condvar)>,
}

struct Inner {
    conns: HashMap<u32, Conn>,
    next_id: u32,
    seen: HashMap<IpAddr, Instant>,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// IP to advertise in the broadcast. `None` = every interface advertises its own address.
    pub local_ip: Option<Ipv4Addr>,
    /// TCP port the fan connects to. The real fan always uses 6666; 0 = pick a free one (tests).
    pub tcp_port: u16,
    /// UDP port we listen on for the fan's `"<ip>HS"` replies.
    pub udp_port: u16,
    /// UDP port the fan listens on.
    pub fan_udp_port: u16,
    pub broadcast_interval: Duration,
    pub heartbeat: bool,
    /// Extra unicast discovery targets (a fan whose IP is known, or a simulator).
    pub targets: Vec<SocketAddr>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            local_ip: None,
            tcp_port: TCP_PORT,
            udp_port: UDP_PORT,
            fan_udp_port: UDP_PORT,
            broadcast_interval: Duration::from_secs(3),
            heartbeat: true,
            targets: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct Link {
    inner: Arc<Mutex<Inner>>,
    events: Sender<Event>,
    connected: Arc<(Mutex<()>, Condvar)>,
    stop: Arc<AtomicBool>,
    /// Address shown to the user (first private IPv4, or the configured one).
    pub local_ip: Ipv4Addr,
    pub tcp_port: u16,
    pub tx_lock: Arc<Mutex<()>>,
}

/// One IPv4 interface: (address, broadcast address).
pub fn interfaces() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    let mut out = Vec::new();
    if let Ok(list) = if_addrs::get_if_addrs() {
        for i in list {
            if let if_addrs::IfAddr::V4(v4) = i.addr {
                if v4.ip.is_loopback() || v4.ip.is_link_local() { continue; }
                let bc = v4.broadcast.unwrap_or_else(|| {
                    let m = u32::from(v4.netmask);
                    Ipv4Addr::from(u32::from(v4.ip) | !m)
                });
                if !out.iter().any(|(a, _)| *a == v4.ip) { out.push((v4.ip, bc)); }
            }
        }
    }
    out
}

/// Best-effort local IPv4 as seen from the fan's network (default AP is 192.168.4.1).
pub fn local_ip() -> Ipv4Addr {
    let ifs = interfaces();
    if let Some((ip, _)) = ifs.iter().find(|(ip, _)| ip.octets()[..3] == [192, 168, 4]) { return *ip; }
    if let Ok(s) = UdpSocket::bind("0.0.0.0:0") {
        if s.connect("192.168.4.1:1").is_ok() {
            if let Ok(SocketAddr::V4(a)) = s.local_addr() {
                if !a.ip().is_loopback() && !a.ip().is_unspecified() { return *a.ip(); }
            }
        }
    }
    ifs.first().map(|(ip, _)| *ip).unwrap_or(Ipv4Addr::new(192, 168, 4, 2))
}

fn fresh_ack_window(dur: u64) -> Duration { Duration::from_millis(dur) }

impl Link {
    /// Start broadcasting and listening. Returns the link and its event stream.
    pub fn start(cfg: Config) -> Result<(Link, Receiver<Event>), Error> {
        let (tx, rx) = mpsc::channel();
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, cfg.tcp_port))
            .map_err(|e| Error::Io(format!("port {} is in use or blocked ({e}); close Holoscope or any other P56 app", cfg.tcp_port)))?;
        let tcp_port = listener.local_addr().map(|a| a.port()).unwrap_or(cfg.tcp_port);
        let ip = cfg.local_ip.unwrap_or_else(local_ip);
        let link = Link {
            inner: Arc::new(Mutex::new(Inner { conns: HashMap::new(), next_id: 1, seen: HashMap::new() })),
            events: tx,
            connected: Arc::new((Mutex::new(()), Condvar::new())),
            stop: Arc::new(AtomicBool::new(false)),
            local_ip: ip,
            tcp_port,
            tx_lock: Arc::new(Mutex::new(())),
        };
        link.log(format!("TCP server on :{tcp_port} (advertising {ip})"));
        { let l = link.clone(); thread::spawn(move || l.accept_loop(listener)); }
        { let l = link.clone(); let c = cfg.clone(); thread::spawn(move || l.udp_loop(c)); }
        if cfg.heartbeat { let l = link.clone(); thread::spawn(move || l.heartbeat_loop()); }
        Ok((link, rx))
    }

    /// Stop background threads and drop all fans.
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let mut g = self.inner.lock().unwrap();
        for c in g.conns.values() { let _ = c.stream.shutdown(Shutdown::Both); }
        g.conns.clear();
        drop(g);
        // unblock accept()
        let _ = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, self.tcp_port)), Duration::from_millis(200));
    }

    fn log(&self, s: String) {
        log::info!("{s}");
        let _ = self.events.send(Event::Log(s));
    }

    fn udp_loop(&self, cfg: Config) {
        let Ok(sock) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, cfg.udp_port))
            .or_else(|_| UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))) else { return };
        let _ = sock.set_broadcast(true);
        let _ = sock.set_read_timeout(Some(Duration::from_millis(300)));
        let mut last = Instant::now() - cfg.broadcast_interval;
        let mut buf = [0u8; 512];
        let mut own: Vec<IpAddr> = vec![IpAddr::V4(self.local_ip)];
        while !self.stop.load(Ordering::SeqCst) {
            if last.elapsed() >= cfg.broadcast_interval {
                last = Instant::now();
                let ifs = interfaces();
                own = ifs.iter().map(|(ip, _)| IpAddr::V4(*ip)).chain([IpAddr::V4(self.local_ip)]).collect();
                let mut sent = Vec::new();
                match cfg.local_ip {
                    Some(ip) => {
                        let msg = format!("{ip}HS");
                        let _ = sock.send_to(msg.as_bytes(), (Ipv4Addr::BROADCAST, cfg.fan_udp_port));
                        for (_, bc) in &ifs { let _ = sock.send_to(msg.as_bytes(), (*bc, cfg.fan_udp_port)); }
                    }
                    None => {
                        for (ip, bc) in &ifs {
                            let msg = format!("{ip}HS");
                            if sock.send_to(msg.as_bytes(), (*bc, cfg.fan_udp_port)).is_ok() { sent.push(*ip); }
                        }
                        let _ = sock.send_to(format!("{}HS", self.local_ip).as_bytes(), (Ipv4Addr::BROADCAST, cfg.fan_udp_port));
                    }
                }
                for t in &cfg.targets {
                    let ip = match t.ip() {
                        IpAddr::V4(v) if v.is_loopback() => Ipv4Addr::LOCALHOST,
                        _ => cfg.local_ip.unwrap_or(self.local_ip),
                    };
                    let _ = sock.send_to(format!("{ip}HS").as_bytes(), t);
                }
            }
            if let Ok((n, from)) = sock.recv_from(&mut buf) {
                let d = &buf[..n];
                if d.ends_with(b"HS") && !own.contains(&from.ip()) && d.first().is_some_and(|c| c.is_ascii_digit()) {
                    let fresh = self.inner.lock().unwrap().seen.insert(from.ip(), Instant::now()).is_none();
                    if fresh { let _ = self.events.send(Event::Discovered { ip: from.ip() }); }
                }
            }
        }
    }

    fn accept_loop(&self, listener: TcpListener) {
        for stream in listener.incoming() {
            if self.stop.load(Ordering::SeqCst) { break; }
            let Ok(stream) = stream else { continue };
            let _ = stream.set_nodelay(true);
            let peer = stream.peer_addr().map(|a| a.ip()).ok();
            let Ok(reader) = stream.try_clone() else { continue };
            let inbox = Arc::new((Mutex::new(Inbox::default()), Condvar::new()));
            let id = {
                let mut g = self.inner.lock().unwrap();
                let id = g.next_id;
                g.next_id += 1;
                g.conns.insert(id, Conn { stream, dev: Device { id, ip: peer, last_seen: Some(Instant::now()), ..Default::default() }, inbox: inbox.clone() });
                id
            };
            self.log(format!("fan #{id} connected from {}", peer.map(|p| p.to_string()).unwrap_or_default()));
            let _ = self.events.send(Event::Connected { id, ip: peer.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)) });
            self.connected.1.notify_all();
            let l = self.clone();
            thread::spawn(move || l.read_loop(id, reader, inbox));
            let _ = self.send_to(id, &proto::get_machine());
            let _ = self.send_to(id, &proto::heartbeat());
        }
    }

    fn read_loop(&self, id: u32, mut stream: TcpStream, inbox: Arc<(Mutex<Inbox>, Condvar)>) {
        let mut rd = proto::Reader::default();
        let mut buf = [0u8; 8192];
        loop {
            let n = match stream.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => n };
            let pkts = rd.push(&buf[..n]);
            {
                let (m, cv) = &*inbox;
                let mut ib = m.lock().unwrap();
                // Holoscope treats *any* bytes from the fan as "go on" (QIODevice::readyRead).
                ib.bytes += 1;
                for p in &pkts {
                    ib.seq += 1;
                    let s = ib.seq;
                    ib.recent.push_back((s, p.clone()));
                    if ib.recent.len() > 64 { ib.recent.pop_front(); }
                }
                cv.notify_all();
            }
            for pkt in pkts {
                {
                    let mut g = self.inner.lock().unwrap();
                    if let Some(c) = g.conns.get_mut(&id) {
                        c.dev.last_seen = Some(Instant::now());
                        match &pkt {
                            Packet::Status(s) => c.dev.status = Some(s.clone()),
                            Packet::MachineInfo(i) => c.dev.info = Some(i.clone()),
                            Packet::FileList(f) => c.dev.files = f.clone(),
                            Packet::Error(e) => c.dev.last_error = Some(*e),
                            Packet::List { kind, index, items } => { c.dev.lists.insert(*kind, (*index, items.clone())); }
                            _ => {}
                        }
                    }
                }
                if let Packet::Error(e) = &pkt { self.log(format!("fan #{id}: {e}")); }
                let _ = self.events.send(Event::Packet { id, packet: pkt });
            }
        }
        if self.inner.lock().unwrap().conns.remove(&id).is_some() {
            self.log(format!("fan #{id} disconnected"));
            let _ = self.events.send(Event::Disconnected { id });
        }
    }

    fn heartbeat_loop(&self) {
        let mut tick = 0u64;
        while !self.stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(3));
            tick += 1;
            for id in self.ids() {
                if let Ok(_g) = self.tx_lock.try_lock() {
                    let _ = self.send_to(id, &proto::heartbeat());
                    if tick % 5 == 0 { let _ = self.send_to(id, &proto::get_machine()); }
                }
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

    pub fn device(&self, id: u32) -> Option<Device> {
        self.inner.lock().unwrap().conns.get(&id).map(|c| c.dev.clone())
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
            g = cv.wait_timeout(g, left.min(Duration::from_millis(200))).unwrap().0;
        }
        true
    }

    fn inbox(&self, id: u32) -> Option<Arc<(Mutex<Inbox>, Condvar)>> {
        self.inner.lock().unwrap().conns.get(&id).map(|c| c.inbox.clone())
    }

    fn stream(&self, id: u32) -> Result<TcpStream, Error> {
        self.inner.lock().unwrap().conns.get(&id).and_then(|c| c.stream.try_clone().ok()).ok_or(Error::NotConnected)
    }

    fn mark(&self, id: u32) -> (u64, u64) {
        self.inbox(id).map(|b| { let g = b.0.lock().unwrap(); (g.seq, g.bytes) }).unwrap_or((0, 0))
    }

    /// Send raw bytes to one fan.
    pub fn send_to(&self, id: u32, data: &[u8]) -> Result<(), Error> {
        let mut s = self.stream(id)?;
        s.write_all(data).map_err(|e| {
            if let Some(c) = self.inner.lock().unwrap().conns.remove(&id) { let _ = c.stream.shutdown(Shutdown::Both); }
            Error::Io(e.to_string())
        })
    }

    /// Send raw bytes to every connected fan.
    pub fn broadcast(&self, data: &[u8]) -> usize {
        self.ids().into_iter().filter(|&id| self.send_to(id, data).is_ok()).count()
    }

    /// Wait until the fan sent *anything* after `mark` (Holoscope's readyRead).
    fn wait_bytes(&self, id: u32, mark: u64, timeout: Duration) -> bool {
        let Some(b) = self.inbox(id) else { return false };
        let (m, cv) = &*b;
        let deadline = Instant::now() + timeout;
        let mut g = m.lock().unwrap();
        while g.bytes <= mark {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() { return false; }
            g = cv.wait_timeout(g, left).unwrap().0;
        }
        true
    }

    /// Wait for a packet after sequence `since` that matches `pred`.
    pub fn wait_packet(&self, id: u32, since: u64, timeout: Duration, pred: impl Fn(&Packet) -> bool) -> Option<Packet> {
        let b = self.inbox(id)?;
        let (m, cv) = &*b;
        let deadline = Instant::now() + timeout;
        let mut g = m.lock().unwrap();
        loop {
            if let Some((_, p)) = g.recent.iter().find(|(s, p)| *s > since && pred(p)) { return Some(p.clone()); }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() { return None; }
            g = cv.wait_timeout(g, left).unwrap().0;
            if !self.inner.lock().unwrap().conns.contains_key(&id) { return None; }
        }
    }

    /// Wait for the `n`-th packet after `since` that matches `pred` and return it.
    pub fn wait_nth(&self, id: u32, since: u64, n: usize, timeout: Duration, pred: impl Fn(&Packet) -> bool) -> Option<Packet> {
        let b = self.inbox(id)?;
        let (m, cv) = &*b;
        let deadline = Instant::now() + timeout;
        let mut g = m.lock().unwrap();
        loop {
            if let Some((_, p)) = g.recent.iter().filter(|(s, p)| *s > since && pred(p)).nth(n.saturating_sub(1)) { return Some(p.clone()); }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() { return None; }
            g = cv.wait_timeout(g, left).unwrap().0;
            if !self.inner.lock().unwrap().conns.contains_key(&id) { return None; }
        }
    }

    /// Wait until the fan has been silent for `silence` (at most `max`).
    pub fn quiesce(&self, id: u32, silence: Duration, max: Duration) {
        let deadline = Instant::now() + max;
        while Instant::now() < deadline {
            let s = self.packet_seq(id);
            if self.wait_packet(id, s, silence, |_| true).is_none() { return; }
        }
    }

    /// Sequence number of the newest packet received from `id` (for [`Link::wait_packet`]).
    pub fn packet_seq(&self, id: u32) -> u64 { self.mark(id).0 }

    /// Send and then wait up to `timeout` for *any* bytes back (ack).
    pub fn send_wait(&self, id: u32, data: &[u8], timeout: Duration) -> Result<bool, Error> {
        let (_, mark) = self.mark(id);
        self.send_to(id, data)?;
        Ok(self.wait_bytes(id, mark, timeout))
    }

    /// Send a command and wait for the fan's `0E` ready (or an error code) that answers it.
    /// Returns the error code if the fan sent one; a missing ack after `timeout` is not an error
    /// (older firmware does not ack everything).
    fn send_ready(&self, id: u32, data: &[u8], timeout: Duration) -> Result<Option<FanError>, Error> {
        let before = self.packet_seq(id);
        self.send_to(id, data)?;
        Ok(match self.wait_packet(id, before, timeout, |p| matches!(p, Packet::Ready | Packet::Error(_))) {
            Some(Packet::Error(e)) => Some(e),
            _ => None,
        })
    }

    fn cmd(&self, id: u32, data: &[u8]) -> Result<(), Error> {
        let _g = self.tx_lock.lock().unwrap();
        match self.send_ready(id, data, fresh_ack_window(600))? {
            Some(e) => Err(Error::Fan(e)),
            None => Ok(()),
        }
    }

    // ---- commands ---------------------------------------------------------
    pub fn on(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::on()) }
    pub fn off(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::off()) }
    pub fn play(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::play()) }
    pub fn pause(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::pause()) }
    pub fn select(&self, id: u32, slot: u8) -> Result<(), Error> { self.cmd(id, &proto::select(slot)) }
    pub fn delete(&self, id: u32, slot: u8) -> Result<(), Error> { self.cmd(id, &proto::delete(slot)) }
    pub fn rename(&self, id: u32, slot: u8, new_name: &str) -> Result<(), Error> { self.cmd(id, &proto::rename(slot, new_name)) }
    pub fn repeat(&self, id: u32, all: bool) -> Result<(), Error> { self.cmd(id, &if all { proto::repeat_all() } else { proto::repeat_one() }) }
    pub fn brightness(&self, id: u32, v: u8) -> Result<(), Error> { self.cmd(id, &proto::brightness(v)) }
    pub fn angle(&self, id: u32, deg: u16) -> Result<(), Error> { self.cmd(id, &proto::angle(deg)) }
    pub fn color_mode(&self, id: u32, m: ColorMode) -> Result<(), Error> { self.cmd(id, &proto::color_mode(m)) }
    pub fn breath(&self, id: u32, on: bool) -> Result<(), Error> { self.cmd(id, &proto::breath(on)) }
    pub fn autostart(&self, id: u32, on: bool) -> Result<(), Error> { self.cmd(id, &proto::autostart(on)) }
    pub fn master(&self, id: u32, on: bool) -> Result<(), Error> { self.cmd(id, &proto::master(on)) }
    pub fn volume(&self, id: u32, v: u8) -> Result<(), Error> { self.cmd(id, &proto::volume(v)) }
    pub fn device_name(&self, id: u32, name: &str) -> Result<(), Error> { self.cmd(id, &proto::device_name(name)) }
    pub fn set_hotspot(&self, id: u32, ssid: &str, pw: &str) -> Result<(), Error> { self.cmd(id, &proto::hotspot(ssid, pw)) }
    pub fn set_router(&self, id: u32, ssid: &str, pw: &str) -> Result<(), Error> { self.cmd(id, &proto::router(ssid, pw)) }
    pub fn ble(&self, id: u32, on: bool) -> Result<(), Error> { self.cmd(id, &proto::ble(on)) }
    pub fn ble_scan(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::ble_scan()) }
    pub fn ble_connect(&self, id: u32, i: u8) -> Result<(), Error> { self.cmd(id, &proto::ble_connect(i)) }
    pub fn ble_disconnect(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::ble_disconnect()) }
    pub fn format_card(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::format_card()) }
    pub fn factory_reset(&self, id: u32) -> Result<(), Error> { self.cmd(id, &proto::factory_reset()) }
    pub fn refresh(&self, id: u32) -> Result<(), Error> {
        self.cmd(id, &proto::get_machine())?;
        self.cmd(id, &proto::heartbeat())
    }

    /// Upload one or more fan-layout JPEG frames as a playlist entry and show it.
    /// Mirrors the verified Holoscope / v25 sequence; fails if the fan reports an error.
    pub fn upload(&self, id: u32, frames: &[Vec<u8>], name: &str, show_slot: Option<u8>, seq: u32) -> Result<(), Error> {
        if frames.is_empty() { return Err(Error::Invalid("no frames".into())); }
        let name = proto::mp4_name(name);
        let _g = self.tx_lock.lock().unwrap();
        // Let late answers to earlier commands arrive first, so they are not counted below.
        self.quiesce(id, Duration::from_millis(150), Duration::from_secs(2));
        let start = self.packet_seq(id);
        let fan_error = || -> Option<FanError> {
            match self.wait_packet(id, start, Duration::ZERO, |p| matches!(p, Packet::Error(_) | Packet::TransBreak)) {
                Some(Packet::Error(e)) => Some(e),
                Some(_) => Some(FanError::TransferFailed(0x22)),
                None => None,
            }
        };
        // Every step is answered by exactly one 0E or error code; count them so a late
        // answer is never mistaken for the answer to a later step.
        let answers = std::cell::Cell::new(0usize);
        let is_answer = |p: &Packet| matches!(p, Packet::Ready | Packet::Error(_));
        let expect = |ms: u64| -> Result<(), Error> {
            answers.set(answers.get() + 1);
            match self.wait_nth(id, start, answers.get(), Duration::from_millis(ms), is_answer) {
                Some(Packet::Error(e)) => Err(Error::Fan(e)),
                _ => Ok(()),
            }
        };
        let step = |d: &[u8], ms: u64| -> Result<(), Error> { self.send_to(id, d)?; expect(ms) };
        step(&proto::on(), 1000)?;
        // Never send the blob after the fan refused the name.
        step(&proto::set_filename(&name), 2000)?;
        if let Some(e) = fan_error() { return Err(Error::Fan(e)); }
        let (p1, p2) = proto::upload_blob(frames, proto::upload_tag(seq));
        let mut s = self.stream(id)?;
        s.write_all(&p1).map_err(|e| Error::Io(e.to_string()))?;
        for chunk in p2.chunks(UPLOAD_CHUNK) {
            s.write_all(chunk).map_err(|e| Error::Io(e.to_string()))?;
            thread::sleep(Duration::from_millis(2));
        }
        // The fan acks the stored blob; give it time for large videos.
        expect(1000 + p2.len() as u64 / 2000)?;
        let r = step(&proto::commit(), 1000).and_then(|_| step(&proto::commit(), 1000));
        if let Err(e) = r {
            let _ = self.send_to(id, &proto::trans_cancel());
            return Err(e);
        }
        if let Some(e) = fan_error() {
            let _ = self.send_to(id, &proto::trans_cancel());
            return Err(Error::Fan(e));
        }
        step(&proto::off(), 600)?;
        step(&proto::on(), 600)?;
        if let Some(slot) = show_slot { step(&proto::select(slot), 1000)?; }
        if let Some(e) = fan_error() { return Err(Error::Fan(e)); }
        self.log(format!("uploaded: {name}, {} frame(s), {} bytes", frames.len(), p2.len()));
        Ok(())
    }

    /// Push one live frame (a fan-layout JPEG) to every connected fan.
    /// Returns the number of fans that accepted it.
    pub fn cast(&self, jpeg: &[u8]) -> usize {
        self.cast_to(&self.ids(), jpeg)
    }

    /// Push one live frame (a fan-layout JPEG) to the fans in `ids` only (ids that are not
    /// connected are skipped). Returns the number of fans that accepted it.
    pub fn cast_to(&self, ids: &[u32], jpeg: &[u8]) -> usize {
        let frame = proto::cast_frame(jpeg);
        let _g = self.tx_lock.lock().unwrap();
        let mut ok = 0;
        let mut done: Vec<u32> = Vec::with_capacity(ids.len());
        for &id in ids {
            if done.contains(&id) { continue; }
            done.push(id);
            let Ok(mut s) = self.stream(id) else { continue };
            let mut good = true;
            for chunk in frame.chunks(CAST_CHUNK) {
                let (_, mark) = self.mark(id);
                if s.write_all(chunk).is_err() { good = false; break; }
                // Holoscope blocks here until the fan reacts.
                self.wait_bytes(id, mark, Duration::from_millis(3500));
            }
            if good { ok += 1; }
        }
        ok
    }
}
