//! Networking: the PC is the TCP *server* (port 6666); the fan connects to it after
//! hearing our UDP broadcast `"<pc-ip>HS"` on 8988. One [`Link`] serves any number of fans.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::{self as proto, ColorMode, FanError, MachineInfo, Packet, Status, MAX_UPLOAD_JPEG, TCP_PORT, UDP_PORT, UPLOAD_CHUNK};
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
    /// Speaker volume (0–10), from the latest `8F` report (the only place the fan reports it).
    pub volume: Option<u8>,
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
    /// One sender at a time per fan; other fans are never held up by this one.
    tx: Arc<Mutex<()>>,
    /// Cast frames in a row that could not be written to this fan.
    cast_misses: u32,
    /// When the last cast frame went out: a live session runs while this is recent.
    last_cast: Option<Instant>,
}

/// What an upload achieved. An error is returned instead when the fan refused it or the
/// connection dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UploadOutcome {
    /// The fan acknowledged the file data.
    pub acked: bool,
    /// The file showed up in the fan's playlist afterwards.
    pub listed: bool,
}

/// Cast frames in a row that could not be written, after which the connection is dropped (the
/// fan reconnects by itself on the next broadcast).
const CAST_MAX_MISSES: u32 = 5;
/// How long writing one cast frame may take before it counts as missed.
const CAST_WRITE: Duration = Duration::from_secs(2);
/// A pause in the frames this long ends a live session: the next frame starts a new one with
/// `5A 26 26 F5`.
const CAST_SESSION: Duration = Duration::from_millis(1000);
/// No heartbeat or status poll goes out until the last cast frame is this old (Holoscope.exe
/// sends none during a cast); longer than [`CAST_SESSION`], so the fan has left live mode.
const CAST_QUIET: Duration = Duration::from_millis(2000);
/// How long the fan may take to answer the cast start with `5A 29 29 F5`.
const CAST_START_ACK: Duration = Duration::from_millis(1000);
/// Fan addresses remembered for directed discovery (the most recent ones).
const KNOWN_FANS: usize = 16;
/// Discovery rounds in a row in which nothing could be sent, after which the UDP socket is
/// opened again.
const UDP_FAILED_ROUNDS: u32 = 10;
/// An answer to the discovery after this much silence from that address is logged again.
const ANSWER_AGAIN: Duration = Duration::from_secs(60);
/// How often discovery reads the network cards and the address to advertise again.
const IFS_REFRESH: Duration = Duration::from_secs(1);

struct Inner {
    conns: HashMap<u32, Conn>,
    next_id: u32,
    seen: HashMap<IpAddr, Instant>,
    /// Fans that connected or answered the discovery, most recent last: they also get the
    /// discovery packet directly, in case broadcasts do not reach them.
    known: Vec<IpAddr>,
}

impl Inner {
    fn remember(&mut self, ip: IpAddr) {
        self.known.retain(|k| *k != ip);
        self.known.push(ip);
        if self.known.len() > KNOWN_FANS {
            self.known.remove(0);
        }
    }
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
    /// Time between discovery rounds; it stays the same however long no fan shows up.
    pub broadcast_interval: Duration,
    /// Broadcast the discovery packet. Off, only the targets and the fans seen before get it,
    /// directly (tests use this to model a network that drops broadcasts).
    pub broadcast: bool,
    pub heartbeat: bool,
    /// Extra unicast discovery targets (a fan whose IP is known, or a simulator).
    pub targets: Vec<SocketAddr>,
    /// Only fans from these addresses may connect to the TCP port; empty = any address.
    pub allow: Vec<IpAddr>,
}

/// Most fans connected at once; further connections are refused.
pub const MAX_FANS: usize = 64;

impl Default for Config {
    fn default() -> Self {
        Config {
            local_ip: None,
            tcp_port: TCP_PORT,
            udp_port: UDP_PORT,
            fan_udp_port: UDP_PORT,
            broadcast_interval: Duration::from_secs(3),
            broadcast: true,
            heartbeat: true,
            targets: Vec::new(),
            allow: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct Link {
    inner: Arc<Mutex<Inner>>,
    events: Sender<Event>,
    connected: Arc<(Mutex<()>, Condvar)>,
    stop: Arc<AtomicBool>,
    /// Address advertised now: looked up again during discovery (at most once a second), as the network can
    /// change after the start.
    advertised: Arc<Mutex<Ipv4Addr>>,
    /// Discovery rounds sent so far.
    rounds: Arc<AtomicU64>,
    /// Address advertised at the start (first private IPv4, or the configured one); see
    /// [`Link::advertised`] for the current one.
    pub local_ip: Ipv4Addr,
    pub tcp_port: u16,
}

/// One IPv4 interface: (address, broadcast address).
pub fn interfaces() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    let mut out = Vec::new();
    if let Ok(list) = if_addrs::get_if_addrs() {
        for i in list {
            if let if_addrs::IfAddr::V4(v4) = i.addr {
                if v4.ip.is_loopback() || v4.ip.is_link_local() {
                    continue;
                }
                let bc = v4.broadcast.unwrap_or_else(|| {
                    let m = u32::from(v4.netmask);
                    Ipv4Addr::from(u32::from(v4.ip) | !m)
                });
                if !out.iter().any(|(a, _)| *a == v4.ip) {
                    out.push((v4.ip, bc));
                }
            }
        }
    }
    out
}

/// Best-effort local IPv4 as seen from the fan's network (default AP is 192.168.4.1).
pub fn local_ip() -> Ipv4Addr {
    local_ip_of(&interfaces())
}

fn local_ip_of(ifs: &[(Ipv4Addr, Ipv4Addr)]) -> Ipv4Addr {
    if let Some((ip, _)) = ifs.iter().find(|(ip, _)| ip.octets()[..3] == [192, 168, 4]) {
        return *ip;
    }
    if let Some(ip) = source_ip(Ipv4Addr::new(192, 168, 4, 1)).filter(|ip| !ip.is_loopback()) {
        return ip;
    }
    ifs.first().map(|(ip, _)| *ip).unwrap_or(Ipv4Addr::new(192, 168, 4, 2))
}

/// The local address this machine sends from to reach `to` (nothing is sent).
pub fn source_ip(to: Ipv4Addr) -> Option<Ipv4Addr> {
    let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    s.connect((to, 1)).ok()?;
    match s.local_addr() {
        Ok(SocketAddr::V4(a)) if !a.ip().is_unspecified() => Some(*a.ip()),
        _ => None,
    }
}

fn fresh_ack_window(dur: u64) -> Duration {
    Duration::from_millis(dur)
}

impl Link {
    /// Start broadcasting and listening. Returns the link and its event stream.
    pub fn start(cfg: Config) -> Result<(Link, Receiver<Event>), Error> {
        let (tx, rx) = mpsc::channel();
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, cfg.tcp_port))
            .map_err(|e| Error::Io(format!("port {} is in use or blocked ({e}); close Holoscope or any other P56 app", cfg.tcp_port)))?;
        let tcp_port = listener.local_addr().map(|a| a.port()).unwrap_or(cfg.tcp_port);
        let ip = cfg.local_ip.unwrap_or_else(local_ip);
        let link = Link {
            inner: Arc::new(Mutex::new(Inner { conns: HashMap::new(), next_id: 1, seen: HashMap::new(), known: Vec::new() })),
            events: tx,
            connected: Arc::new((Mutex::new(()), Condvar::new())),
            stop: Arc::new(AtomicBool::new(false)),
            advertised: Arc::new(Mutex::new(ip)),
            rounds: Arc::new(AtomicU64::new(0)),
            local_ip: ip,
            tcp_port,
        };
        link.log(format!("TCP server on :{tcp_port} (advertising {ip})"));
        {
            let l = link.clone();
            let allow = cfg.allow.clone();
            thread::spawn(move || l.accept_loop(listener, allow));
        }
        {
            let l = link.clone();
            let c = cfg.clone();
            thread::spawn(move || l.discovery(c));
        }
        if cfg.heartbeat {
            let l = link.clone();
            thread::spawn(move || l.heartbeat_loop());
        }
        Ok((link, rx))
    }

    /// Stop background threads and drop all fans.
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let mut g = self.inner.lock().unwrap();
        for c in g.conns.values() {
            let _ = c.stream.shutdown(Shutdown::Both);
        }
        g.conns.clear();
        drop(g);
        // unblock accept()
        let _ = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, self.tcp_port)), Duration::from_millis(200));
    }

    fn log(&self, s: String) {
        log::info!("{s}");
        let _ = self.events.send(Event::Log(s));
    }

    /// Keep discovery going for as long as the link runs: the UDP loop starts again when it
    /// stops (its socket failed or could not be opened, or it panicked), so a fan that shows up
    /// hours later is still found.
    fn discovery(&self, cfg: Config) {
        while !self.stop.load(Ordering::SeqCst) {
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.udp_loop(&cfg)));
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            if run.is_err() {
                self.log("discovery stopped unexpectedly; starting it again".into());
            }
            thread::sleep(cfg.broadcast_interval.min(Duration::from_secs(1)));
        }
    }

    fn udp_loop(&self, cfg: &Config) {
        let Ok(sock) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, cfg.udp_port)).or_else(|_| UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))) else {
            log::warn!("discovery: no UDP socket; trying again");
            return;
        };
        let me = sock.local_addr().map(|a| a.port()).unwrap_or(0);
        let _ = sock.set_broadcast(true);
        // rounds keep their pace: a short interval (tests) is not stretched by the coarse read
        // timeout of some systems, and a send that cannot go out (a stuck network card) does
        // not hold discovery up
        let poll = cfg.broadcast_interval < Duration::from_millis(20);
        if poll {
            let _ = sock.set_nonblocking(true);
        } else {
            let _ = sock.set_read_timeout(Some(cfg.broadcast_interval.min(Duration::from_millis(300))));
            let _ = sock.set_write_timeout(Some(Duration::from_millis(500)));
        }
        let mut last = Instant::now().checked_sub(cfg.broadcast_interval).unwrap_or_else(Instant::now);
        let mut buf = [0u8; 512];
        let mut adv = self.advertised();
        let mut ifs = Vec::new();
        let mut own: Vec<IpAddr> = vec![IpAddr::V4(adv)];
        let mut refreshed: Option<Instant> = None;
        let mut failed = 0u32;
        while !self.stop.load(Ordering::SeqCst) {
            if last.elapsed() >= cfg.broadcast_interval {
                last = Instant::now();
                // looked up again every round (at most every second): an address picked while
                // the network was still coming up, or before it changed, sends the fan somewhere
                // it cannot connect to
                if refreshed.is_none_or(|t| t.elapsed() >= IFS_REFRESH) {
                    refreshed = Some(Instant::now());
                    ifs = interfaces();
                    adv = cfg.local_ip.unwrap_or_else(|| local_ip_of(&ifs));
                    let was = std::mem::replace(&mut *self.advertised.lock().unwrap(), adv);
                    if was != adv {
                        self.log(format!("advertising {adv} now (was {was})"));
                    }
                    own = ifs.iter().map(|(ip, _)| IpAddr::V4(*ip)).chain([IpAddr::V4(adv)]).collect();
                }
                let (mut tried, mut sent) = (0u32, 0u32);
                let mut send = |msg: String, to: SocketAddr| {
                    tried += 1;
                    if sock.send_to(msg.as_bytes(), to).is_ok() {
                        sent += 1;
                    }
                };
                if cfg.broadcast {
                    match cfg.local_ip {
                        Some(ip) => {
                            send(format!("{ip}HS"), SocketAddr::from((Ipv4Addr::BROADCAST, cfg.fan_udp_port)));
                            for (_, bc) in &ifs {
                                send(format!("{ip}HS"), SocketAddr::from((*bc, cfg.fan_udp_port)));
                            }
                        }
                        None => {
                            for (ip, bc) in &ifs {
                                send(format!("{ip}HS"), SocketAddr::from((*bc, cfg.fan_udp_port)));
                            }
                            send(format!("{adv}HS"), SocketAddr::from((Ipv4Addr::BROADCAST, cfg.fan_udp_port)));
                        }
                    }
                }
                // directly to the targets and to fans seen before that are not connected now
                let (known, connected) = {
                    let g = self.inner.lock().unwrap();
                    (g.known.clone(), g.conns.values().filter_map(|c| c.dev.ip).collect::<Vec<_>>())
                };
                let seen_before = known
                    .into_iter()
                    .filter(|ip| !connected.contains(ip) && !cfg.targets.iter().any(|t| t.ip() == *ip))
                    // never this machine's own discovery port
                    .filter(|ip| !(cfg.fan_udp_port == me && (ip.is_loopback() || own.contains(ip))))
                    .map(|ip| SocketAddr::from((ip, cfg.fan_udp_port)));
                for t in cfg.targets.iter().copied().chain(seen_before) {
                    // the address this machine has towards that fan
                    let ip = match t.ip() {
                        IpAddr::V4(v) if v.is_loopback() => Ipv4Addr::LOCALHOST,
                        IpAddr::V4(v) => cfg.local_ip.or_else(|| source_ip(v)).unwrap_or(adv),
                        IpAddr::V6(_) => cfg.local_ip.unwrap_or(adv),
                    };
                    send(format!("{ip}HS"), t);
                }
                self.rounds.fetch_add(1, Ordering::SeqCst);
                failed = if tried > 0 && sent == 0 { failed + 1 } else { 0 };
                if failed >= UDP_FAILED_ROUNDS {
                    log::warn!("discovery: nothing could be sent for {failed} rounds; opening the UDP socket again");
                    return;
                }
            }
            let got = sock.recv_from(&mut buf);
            if poll && got.is_err() {
                thread::sleep(Duration::from_millis(1));
            }
            if let Ok((n, from)) = got {
                let d = &buf[..n];
                let mine = own.contains(&from.ip()) || (from.ip().is_loopback() && from.port() == me);
                if d.ends_with(b"HS") && !mine && d.first().is_some_and(|c| c.is_ascii_digit()) {
                    let (fresh, again) = {
                        let mut g = self.inner.lock().unwrap();
                        g.remember(from.ip());
                        match g.seen.insert(from.ip(), Instant::now()) {
                            None => (true, true),
                            Some(t) => (false, t.elapsed() >= ANSWER_AGAIN),
                        }
                    };
                    // in the log, so it shows whether a fan that does not connect hears us at all
                    if again {
                        self.log(format!("{} answered the discovery", from.ip()));
                    }
                    if fresh {
                        let _ = self.events.send(Event::Discovered { ip: from.ip() });
                    }
                }
            }
        }
    }

    fn accept_loop(&self, listener: TcpListener, allow: Vec<IpAddr>) {
        for stream in listener.incoming() {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let _ = stream.set_nodelay(true);
            let peer = stream.peer_addr().map(|a| a.ip()).ok();
            let peer_v4 = peer.map(|p| match p {
                IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(p),
                v4 => v4,
            });
            if !allow.is_empty() && !peer_v4.is_some_and(|p| allow.contains(&p)) {
                self.log(format!("refused a connection from {peer:?}: not in the allowed fan addresses"));
                let _ = stream.shutdown(Shutdown::Both);
                continue;
            }
            if self.inner.lock().unwrap().conns.len() >= MAX_FANS {
                self.log(format!("refused a connection from {peer:?}: already {MAX_FANS} fans connected"));
                let _ = stream.shutdown(Shutdown::Both);
                continue;
            }
            let Ok(reader) = stream.try_clone() else { continue };
            let inbox = Arc::new((Mutex::new(Inbox::default()), Condvar::new()));
            let id = {
                let mut g = self.inner.lock().unwrap();
                let id = g.next_id;
                g.next_id += 1;
                if let Some(p) = peer_v4 {
                    g.remember(p);
                }
                g.conns.insert(
                    id,
                    Conn {
                        stream,
                        dev: Device { id, ip: peer, last_seen: Some(Instant::now()), ..Default::default() },
                        inbox: inbox.clone(),
                        tx: Arc::new(Mutex::new(())),
                        cast_misses: 0,
                        last_cast: None,
                    },
                );
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
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
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
                    if ib.recent.len() > 64 {
                        ib.recent.pop_front();
                    }
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
                            Packet::Bluetooth(b) => c.dev.volume = Some(b.volume),
                            _ => {}
                        }
                    }
                }
                if let Packet::Error(e) = &pkt {
                    self.log(format!("fan #{id}: {e}"));
                }
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
                // skip a fan that is busy (upload, cast frame); the next tick catches up
                let Some(tx) = self.tx(id) else { continue };
                if let Ok(_g) = tx.try_lock() {
                    // Holoscope.exe sends neither heartbeat nor status poll during a live cast
                    // (checked under the send lock, so a cast cannot start in between)
                    if self.casting(id) {
                        continue;
                    }
                    let _ = self.send_to(id, &proto::heartbeat());
                    if tick.is_multiple_of(5) {
                        let _ = self.send_to(id, &proto::get_machine());
                    }
                };
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

    /// The address advertised in the discovery packet now.
    pub fn advertised(&self) -> Ipv4Addr {
        *self.advertised.lock().unwrap()
    }

    /// Discovery rounds sent since the start.
    pub fn discovery_rounds(&self) -> u64 {
        self.rounds.load(Ordering::SeqCst)
    }

    /// Block until at least one fan is connected.
    pub fn wait_connected(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let (m, cv) = &*self.connected;
        let mut g = m.lock().unwrap();
        while self.ids().is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            g = cv.wait_timeout(g, left.min(Duration::from_millis(200))).unwrap().0;
        }
        true
    }

    fn inbox(&self, id: u32) -> Option<Arc<(Mutex<Inbox>, Condvar)>> {
        self.inner.lock().unwrap().conns.get(&id).map(|c| c.inbox.clone())
    }

    /// The per-fan send lock.
    fn tx(&self, id: u32) -> Option<Arc<Mutex<()>>> {
        self.inner.lock().unwrap().conns.get(&id).map(|c| c.tx.clone())
    }

    fn stream(&self, id: u32) -> Result<TcpStream, Error> {
        self.inner.lock().unwrap().conns.get(&id).and_then(|c| c.stream.try_clone().ok()).ok_or(Error::NotConnected)
    }

    fn mark(&self, id: u32) -> (u64, u64) {
        self.inbox(id)
            .map(|b| {
                let g = b.0.lock().unwrap();
                (g.seq, g.bytes)
            })
            .unwrap_or((0, 0))
    }

    /// Send raw bytes to one fan.
    pub fn send_to(&self, id: u32, data: &[u8]) -> Result<(), Error> {
        let mut s = self.stream(id)?;
        s.write_all(data).map_err(|e| {
            if let Some(c) = self.inner.lock().unwrap().conns.remove(&id) {
                let _ = c.stream.shutdown(Shutdown::Both);
            }
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
            if left.is_zero() {
                return false;
            }
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
            if let Some((_, p)) = g.recent.iter().find(|(s, p)| *s > since && pred(p)) {
                return Some(p.clone());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            g = cv.wait_timeout(g, left).unwrap().0;
            if !self.inner.lock().unwrap().conns.contains_key(&id) {
                return None;
            }
        }
    }

    /// Wait for the `n`-th packet after `since` that matches `pred` and return it.
    pub fn wait_nth(&self, id: u32, since: u64, n: usize, timeout: Duration, pred: impl Fn(&Packet) -> bool) -> Option<Packet> {
        let b = self.inbox(id)?;
        let (m, cv) = &*b;
        let deadline = Instant::now() + timeout;
        let mut g = m.lock().unwrap();
        loop {
            if let Some((_, p)) = g.recent.iter().filter(|(s, p)| *s > since && pred(p)).nth(n.saturating_sub(1)) {
                return Some(p.clone());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            g = cv.wait_timeout(g, left).unwrap().0;
            if !self.inner.lock().unwrap().conns.contains_key(&id) {
                return None;
            }
        }
    }

    /// Wait until the fan has been silent for `silence` (at most `max`).
    pub fn quiesce(&self, id: u32, silence: Duration, max: Duration) {
        let deadline = Instant::now() + max;
        while Instant::now() < deadline {
            let s = self.packet_seq(id);
            if self.wait_packet(id, s, silence, |_| true).is_none() {
                return;
            }
        }
    }

    /// Sequence number of the newest packet received from `id` (for [`Link::wait_packet`]).
    pub fn packet_seq(&self, id: u32) -> u64 {
        self.mark(id).0
    }

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
        let tx = self.tx(id).ok_or(Error::NotConnected)?;
        let _g = tx.lock().unwrap();
        match self.send_ready(id, data, fresh_ack_window(600))? {
            Some(e) => Err(Error::Fan(e)),
            None => Ok(()),
        }
    }

    // ---- commands ---------------------------------------------------------
    pub fn on(&self, id: u32) -> Result<(), Error> {
        self.cmd(id, &proto::on())
    }
    pub fn off(&self, id: u32) -> Result<(), Error> {
        // a fan in live mode ignores "off": wait until a cast is over (up to 4 s)
        let quiet_by = Instant::now() + Duration::from_secs(4);
        while self.casting(id) && Instant::now() < quiet_by {
            thread::sleep(Duration::from_millis(100));
        }
        self.cmd(id, &proto::off())
    }
    pub fn play(&self, id: u32) -> Result<(), Error> {
        self.cmd(id, &proto::play())
    }
    pub fn pause(&self, id: u32) -> Result<(), Error> {
        self.cmd(id, &proto::pause())
    }
    pub fn select(&self, id: u32, slot: u8) -> Result<(), Error> {
        self.cmd(id, &proto::select(slot))
    }
    pub fn delete(&self, id: u32, slot: u8) -> Result<(), Error> {
        self.cmd(id, &proto::delete(slot))
    }
    pub fn rename(&self, id: u32, slot: u8, new_name: &str) -> Result<(), Error> {
        self.cmd(id, &proto::rename(slot, new_name))
    }
    pub fn repeat(&self, id: u32, all: bool) -> Result<(), Error> {
        self.cmd(id, &if all { proto::repeat_all() } else { proto::repeat_one() })
    }
    pub fn brightness(&self, id: u32, v: u8) -> Result<(), Error> {
        self.cmd(id, &proto::brightness(v))
    }
    pub fn angle(&self, id: u32, deg: u16) -> Result<(), Error> {
        self.cmd(id, &proto::angle(deg))
    }
    pub fn color_mode(&self, id: u32, m: ColorMode) -> Result<(), Error> {
        self.cmd(id, &proto::color_mode(m))
    }
    pub fn breath(&self, id: u32, on: bool) -> Result<(), Error> {
        self.cmd(id, &proto::breath(on))
    }
    pub fn autostart(&self, id: u32, on: bool) -> Result<(), Error> {
        self.cmd(id, &proto::autostart(on))
    }
    pub fn volume(&self, id: u32, v: u8) -> Result<(), Error> {
        self.cmd(id, &proto::volume(v))
    }
    pub fn device_name(&self, id: u32, name: &str) -> Result<(), Error> {
        self.cmd(id, &proto::device_name(name))
    }
    pub fn set_hotspot(&self, id: u32, ssid: &str, pw: &str) -> Result<(), Error> {
        self.cmd(id, &proto::hotspot(ssid, pw))
    }
    pub fn set_router(&self, id: u32, ssid: &str, pw: &str) -> Result<(), Error> {
        self.cmd(id, &proto::router(ssid, pw))
    }
    pub fn format_card(&self, id: u32) -> Result<(), Error> {
        self.cmd(id, &proto::format_card())
    }
    pub fn factory_reset(&self, id: u32) -> Result<(), Error> {
        self.cmd(id, &proto::factory_reset())
    }
    pub fn refresh(&self, id: u32) -> Result<(), Error> {
        // never during a live cast (Holoscope.exe does not poll then); the fan reports anyway.
        // Checked under the send lock, so a cast cannot start between the check and the poll.
        let tx = self.tx(id).ok_or(Error::NotConnected)?;
        let _g = tx.lock().unwrap();
        if self.casting(id) {
            return Ok(());
        }
        for data in [proto::get_machine(), proto::heartbeat()] {
            if let Some(e) = self.send_ready(id, &data, fresh_ack_window(600))? {
                return Err(Error::Fan(e));
            }
        }
        Ok(())
    }

    /// Upload one or more fan-layout JPEG frames as a playlist entry and show it.
    /// Mirrors the verified Holoscope / v25 sequence. Fails if a frame is not a complete JPEG
    /// within [`MAX_UPLOAD_JPEG`], if the fan reports an error or if the connection drops;
    /// otherwise reports whether the fan acknowledged the data and listed the file.
    pub fn upload(&self, id: u32, frames: &[Vec<u8>], name: &str, show_slot: Option<u8>) -> Result<UploadOutcome, Error> {
        self.upload_with_audio(id, frames, None, name, show_slot)
    }

    /// [`Link::upload`] with sound: 16-bit mono PCM at 16 kHz from the start of the animation
    /// (frame `i` plays bytes `i × 1600 ..`); the fan plays it on its speaker.
    pub fn upload_with_audio(&self, id: u32, frames: &[Vec<u8>], audio: Option<&[u8]>, name: &str, show_slot: Option<u8>) -> Result<UploadOutcome, Error> {
        if frames.is_empty() {
            return Err(Error::Invalid("no frames".into()));
        }
        if let Some(i) = frames.iter().position(|f| f.len() > MAX_UPLOAD_JPEG || !proto::jpeg_complete(f)) {
            return Err(Error::Invalid(format!("frame {} is not a complete JPEG of at most {MAX_UPLOAD_JPEG} bytes", i + 1)));
        }
        let name = proto::mp4_name(name);
        let tx = self.tx(id).ok_or(Error::NotConnected)?;
        let _g = tx.lock().unwrap();
        // a fan in live mode does not store a file: wait until a cast is over (up to 10 s). With
        // the link held no new cast frame can slip in meanwhile.
        let quiet_by = Instant::now() + Duration::from_secs(10);
        while self.casting(id) && Instant::now() < quiet_by {
            thread::sleep(Duration::from_millis(100));
        }
        // Let late answers to earlier commands arrive first, so they are not counted below.
        self.quiesce(id, Duration::from_millis(150), Duration::from_secs(2));
        let start = self.packet_seq(id);
        let gone = || self.device(id).is_none();
        // Only an 85 error code fails an upload. A 22 is ignored, as Holoscope.apk does (its
        // Windows version reads it as "firmware update finished").
        let fan_error = || -> Option<FanError> {
            match self.wait_packet(id, start, Duration::ZERO, |p| matches!(p, Packet::Error(_))) {
                Some(Packet::Error(e)) => Some(e),
                _ => None,
            }
        };
        // Every step is answered by exactly one 0E or error code; count them so a late
        // answer is never mistaken for the answer to a later step. A missing answer is allowed
        // (older firmware), a dropped connection is not. Returns whether the step was answered.
        let answers = std::cell::Cell::new(0usize);
        let is_answer = |p: &Packet| matches!(p, Packet::Ready | Packet::Error(_));
        let expect = |ms: u64| -> Result<bool, Error> {
            answers.set(answers.get() + 1);
            match self.wait_nth(id, start, answers.get(), Duration::from_millis(ms), is_answer) {
                Some(Packet::Error(e)) => Err(Error::Fan(e)),
                Some(_) => Ok(true),
                None if gone() => Err(Error::NotConnected),
                None => {
                    // nothing came: the next answer belongs to the next step, not to this one
                    answers.set(answers.get() - 1);
                    Ok(false)
                }
            }
        };
        let step = |d: &[u8], ms: u64| -> Result<bool, Error> {
            self.send_to(id, d)?;
            expect(ms)
        };
        step(&proto::on(), 1000)?;
        // Never send the file after the fan refused the name.
        step(&proto::set_filename(&name), 2000)?;
        if let Some(e) = fan_error() {
            return Err(Error::Fan(e));
        }
        let (head, records) = proto::upload_blob_with_audio(frames, audio);
        let data_mark = self.packet_seq(id);
        let mut s = self.stream(id)?;
        let io = |e: std::io::Error| Error::Io(e.to_string());
        // Like Holoscope: on an error code from the fan, stop sending at once and cancel the
        // transfer (repeated), so the card is not left with half a file.
        let abort = |e: FanError| {
            for _ in 0..4 {
                let _ = self.send_to(id, &proto::trans_cancel());
                thread::sleep(Duration::from_millis(500));
            }
            Error::Fan(e)
        };
        s.write_all(&head).map_err(io)?;
        for (i, chunk) in records.chunks(UPLOAD_CHUNK).enumerate() {
            s.write_all(chunk).map_err(io)?;
            thread::sleep(Duration::from_millis(2));
            if i % 64 == 63 {
                if let Some(e) = fan_error() {
                    return Err(abort(e));
                }
            }
        }
        // The fan acks the stored data (0E, or 29 "stored" / 21 on firmware that does not send
        // 0E here, as a real P56 does); give it time for large animations.
        let acked = expect(1000 + records.len() as u64 / 2000)?
            || self.wait_packet(id, data_mark, Duration::from_millis(500), |p| matches!(p, Packet::TransDone | Packet::TransContinue)).is_some();
        // Proof that the file is stored: the fan sends its playlist with the file in it, by itself,
        // once the file is on its card. Nothing earlier counts: on Linux (Home Assistant) the last
        // megabytes can still be in this socket's send buffer here, and the fan keeps reporting its
        // status while it receives them. Switching it off before the file is stored loses the file
        // and can leave the card refusing every upload until it is formatted. Meanwhile, like
        // Holoscope.apk (transfinish), COMMIT every 500 ms.
        let commit = if frames.len() > 1 { proto::commit_with(frames.len() as u32) } else { proto::commit() };
        let finish = self.packet_seq(id);
        // the fan stores about 300 kB/s; a whole file may still be on its way
        let deadline = Instant::now() + Duration::from_secs(30 + ((records.len() + head.len()) / 100_000) as u64);
        let stored_in = |p: &Packet| matches!(p, Packet::FileList(f) if f.iter().any(|(_, n)| n == &name));
        let mut stored = false;
        loop {
            self.send_to(id, &commit)?;
            if self.wait_packet(id, finish, Duration::from_millis(500), stored_in).is_some() {
                stored = true;
                break;
            }
            if let Some(e) = fan_error() {
                return Err(abort(e));
            }
            if gone() {
                return Err(Error::NotConnected);
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        if !stored {
            // no proof: leave the fan alone (no off/on/show), so it can still finish writing
            self.log(format!("fan #{id} did not confirm {name} (not in its playlist); the fan was left as it is"));
            return Ok(UploadOutcome { acked, listed: false });
        }
        if let Some(e) = fan_error() {
            return Err(abort(e));
        }
        // answers to the repeated COMMITs may still come: wait for them before the next steps
        self.quiesce(id, Duration::from_millis(300), Duration::from_secs(2));
        let ready = |d: &[u8], ms: u64| self.send_ready(id, d, Duration::from_millis(ms));
        ready(&proto::off(), 600)?;
        ready(&proto::on(), 600)?;
        if let Some(slot) = show_slot {
            ready(&proto::select(slot), 1000)?;
        }
        if let Some(e) = fan_error() {
            return Err(Error::Fan(e));
        }
        let listed = stored;
        if gone() {
            return Err(Error::NotConnected);
        }
        self.log(format!(
            "uploaded: {name}, {} frame(s), {} bytes{}{}",
            frames.len(),
            records.len() + head.len(),
            if listed { "" } else { ", not in the playlist (yet)" },
            if acked || listed { "" } else { ", not acknowledged" }
        ));
        Ok(UploadOutcome { acked, listed })
    }

    /// Push one live frame (a fan-layout JPEG) to every connected fan.
    /// Returns the number of fans that accepted it.
    pub fn cast(&self, jpeg: &[u8]) -> usize {
        self.cast_to(&self.ids(), jpeg)
    }

    /// Push one live frame (a fan-layout JPEG) to the fans in `ids` only (ids that are not
    /// connected are skipped). Returns the number of fans it was sent to.
    ///
    /// As Holoscope.exe: the first frame of a session starts live mode with `5A 26 26 F5` (the fan
    /// answers `5A 29 29 F5`; without it the fan keeps playing its playlist), every frame is
    /// written whole without waiting for an answer, and no heartbeat goes out while frames keep
    /// coming. The fan needs a frame about every 130 ms or it falls back. A frame that cannot be
    /// written within [`CAST_WRITE`] is missed; after [`CAST_MAX_MISSES`] in a row the connection
    /// is dropped so the fan reconnects. Fans are served in parallel, each under its own lock.
    pub fn cast_to(&self, ids: &[u32], jpeg: &[u8]) -> usize {
        let Some(frame) = proto::cast_frame(jpeg) else {
            self.log(format!("cast frame refused: not a complete JPEG of at most {} bytes", proto::MAX_JPEG));
            return 0;
        };
        let mut unique: Vec<u32> = Vec::with_capacity(ids.len());
        for &id in ids {
            if !unique.contains(&id) {
                unique.push(id);
            }
        }
        thread::scope(|sc| {
            let jobs: Vec<_> = unique
                .iter()
                .map(|&id| {
                    let frame = &frame;
                    sc.spawn(move || self.cast_one(id, frame))
                })
                .collect();
            jobs.into_iter().map(|j| j.join()).filter(|r| matches!(r, Ok(true))).count()
        })
    }

    /// How long ago the last cast frame went to fan `id`, if any did.
    fn since_cast(&self, id: u32) -> Option<Duration> {
        self.inner.lock().unwrap().conns.get(&id).and_then(|c| c.last_cast).map(|t| t.elapsed())
    }

    /// Whether fan `id` is (or just was) casting: no heartbeat or status poll then.
    fn casting(&self, id: u32) -> bool {
        self.since_cast(id).is_some_and(|d| d < CAST_QUIET)
    }

    /// Send one cast frame to one fan, starting a live session first when none runs.
    fn cast_one(&self, id: u32, frame: &[u8]) -> bool {
        let (Some(tx), Ok(mut s)) = (self.tx(id), self.stream(id)) else { return false };
        let _g = tx.lock().unwrap();
        if !self.since_cast(id).is_some_and(|d| d < CAST_SESSION) {
            let mark = self.packet_seq(id);
            if s.write_all(&proto::cast_start()).is_err() {
                return false;
            }
            if self.wait_packet(id, mark, CAST_START_ACK, |p| matches!(p, Packet::TransDone)).is_none() {
                self.log(format!("fan #{id} did not confirm the cast start (5A 29); casting anyway"));
            }
            // Holoscope's first frame follows the answer after about 70 ms
            thread::sleep(Duration::from_millis(70));
        }
        let _ = s.set_write_timeout(Some(CAST_WRITE));
        let sent = s.write_all(frame).is_ok();
        let _ = s.set_write_timeout(None);
        let mut g = self.inner.lock().unwrap();
        let Some(c) = g.conns.get_mut(&id) else { return false };
        if sent {
            c.cast_misses = 0;
            c.last_cast = Some(Instant::now());
        } else {
            c.cast_misses += 1;
        }
        if c.cast_misses >= CAST_MAX_MISSES {
            let _ = c.stream.shutdown(Shutdown::Both);
            drop(g);
            self.log(format!("fan #{id}: {CAST_MAX_MISSES} cast frames in a row could not be sent; reconnecting"));
        }
        sent
    }
}
