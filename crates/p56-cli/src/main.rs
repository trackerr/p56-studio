//! `p56` — command-line client for Holoscope / P56 hologram fans.
//! Replaces Holoscope for everything except firmware flashing and the vendor cloud.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use p56_core::{transcode, ColorMode, Config, Event, FanType, Link, Placement};

#[derive(Parser)]
#[command(name = "p56", version, about = "Holoscope / P56 hologramfan bedienen vanaf de pc")]
struct Cli {
    /// Fantype (resolutie/armen). Standaard: wat de fan zelf meldt (P56 = 8).
    #[arg(long = "type", global = true)]
    fan_type: Option<u8>,
    /// IP dat in de broadcast wordt geadverteerd (standaard: elke netwerkkaart z'n eigen IP).
    #[arg(long, global = true)]
    ip: Option<std::net::Ipv4Addr>,
    /// TCP-poort waarop de fan verbindt (de echte fan gebruikt altijd 6666).
    #[arg(long, default_value_t = 6666, global = true)]
    port: u16,
    /// Extra adres om de broadcast naartoe te sturen, bv. 192.168.4.1:8988 of de simulator.
    #[arg(long, global = true)]
    target: Vec<SocketAddr>,
    /// Seconden wachten tot een fan verbindt.
    #[arg(long, default_value_t = 20, global = true)]
    wait: u64,
    /// Welke fan (1 = eerste die verbond), als er meerdere zijn.
    #[arg(long, default_value_t = 1, global = true)]
    fan: usize,
    /// Commando naar alle verbonden fans sturen.
    #[arg(long, global = true)]
    all: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff { On, Off }
impl OnOff { fn b(self) -> bool { matches!(self, OnOff::On) } }

#[derive(Clone, Copy, ValueEnum)]
enum Repeat { All, One }

#[derive(Clone, Copy, ValueEnum)]
enum Color { True, Bright }

#[derive(Subcommand)]
enum Ble {
    On,
    Off,
    /// Zoeken naar Bluetooth-apparaten en de lijst tonen
    Scan,
    /// Verbinden met apparaat N uit de lijst (0-based zoals de fan het meldt)
    Connect { index: u8 },
    Disconnect,
}

#[derive(Subcommand)]
enum Cmd {
    /// Aanzetten
    On,
    /// Uitzetten
    Off,
    Play,
    Pause,
    /// Slot N tonen (1-based)
    Select { slot: u8 },
    /// Slot N wissen
    Delete { slot: u8 },
    /// Slot N hernoemen (.mp4 wordt toegevoegd)
    Rename { slot: u8, name: String },
    /// Afspeellijst herhalen (all) of het huidige bestand (one)
    Repeat { mode: Repeat },
    /// Helderheid 1–16
    Brightness { value: u8 },
    /// Beeld draaien: −180…180 graden
    #[command(allow_negative_numbers = true)]
    Angle { degrees: i16 },
    /// Kleurmodus: true (true color) of bright (hoge helderheid)
    Color { mode: Color },
    /// Ademlicht aan/uit
    Breath { state: OnOff },
    /// Automatisch starten bij aanzetten aan/uit
    Autostart { state: OnOff },
    /// Groeps-master aan/uit
    Master { state: OnOff },
    /// Volume 0–10 (alleen fans met speaker)
    Volume { value: u8 },
    /// Apparaatnaam instellen
    Name { name: String },
    /// Naam en wachtwoord van het eigen wifi-netwerk van de fan wijzigen
    Hotspot { ssid: String, password: String },
    /// De fan laten verbinden met je router
    Router { ssid: String, password: String },
    /// Bluetooth van de fan
    #[command(subcommand)]
    Ble(Ble),
    /// Geheugenkaart formatteren (wist alle bestanden!)
    Format { #[arg(long)] yes: bool },
    /// Fabrieksinstellingen herstellen
    Reset { #[arg(long)] yes: bool },
    /// Machine-info, status en afspeellijst tonen
    Info,
    /// Foto, GIF of video uploaden als afspeellijst-item en tonen
    Upload {
        file: PathBuf,
        #[arg(long)] name: Option<String>,
        #[arg(long, default_value_t = 1)] slot: u8,
        /// Video/GIF: maximaal aantal frames (0 = alles)
        #[arg(long, default_value_t = 0)] max_frames: usize,
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)] x: f64,
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)] y: f64,
        #[arg(long, default_value_t = 1.0)] scale: f64,
        #[arg(long)] mirror: bool,
    },
    /// Live casten: foto, GIF, video of 'test'
    Cast {
        source: String,
        #[arg(long, default_value_t = 15.0)] fps: f64,
        /// Stoppen na zoveel seconden (0 = doorgaan)
        #[arg(long, default_value_t = 0.0)] seconds: f64,
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)] x: f64,
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)] y: f64,
        #[arg(long, default_value_t = 1.0)] scale: f64,
        #[arg(long)] mirror: bool,
    },
    /// Alleen transcoderen: schrijft <prefix>_layout.png en <prefix>.jpg, geen netwerk
    Preview { file: String, #[arg(default_value = "preview")] prefix: String },
    /// Bekende fantypes tonen
    Types,
}

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("fout: {msg}");
    std::process::exit(1)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).format_timestamp_millis().init();
    let cli = Cli::parse();

    match &cli.cmd {
        Cmd::Types => {
            for id in 1..=40u8 {
                let s = FanType::support(id);
                if let Some((a, l, fps)) = FanType::apk_geometry(id).or(FanType::known(id).map(|f| (f.angles, f.radius, 20))) {
                    let jpeg = FanType::known(id).map(|f| { let (w, h) = f.jpeg_size(); format!("{w}×{h}") }).unwrap_or("-".into());
                    println!("type {id:>2}: {a}×{l}  {fps} fps  JPEG {jpeg:<9} {s:?}");
                }
            }
            return;
        }
        Cmd::Preview { file, prefix } => {
            let ft = FanType::by_id(cli.fan_type.unwrap_or(8));
            if let Some(e) = FanType::check(ft.id) { die(e); }
            let img = load(file).unwrap_or_else(|e| die(e));
            let f = transcode::encode(&img, &ft, &Placement::default());
            f.layout.save(format!("{prefix}_layout.png")).unwrap_or_else(|e| die(e));
            std::fs::write(format!("{prefix}.jpg"), &f.jpeg).unwrap_or_else(|e| die(e));
            let (w, h) = f.layout.dimensions();
            println!("{prefix}_layout.png {w}x{h}, {prefix}.jpg {} bytes (q{})", f.jpeg.len(), f.quality);
            return;
        }
        Cmd::Format { yes: false } | Cmd::Reset { yes: false } => die("dit wist gegevens op de fan; voeg --yes toe om door te gaan"),
        _ => {}
    }

    let cfg = Config { local_ip: cli.ip, tcp_port: cli.port, targets: cli.target.clone(), ..Default::default() };
    let (link, rx) = Link::start(cfg).unwrap_or_else(|e| die(e));
    std::thread::spawn(move || for ev in rx {
        match ev {
            Event::Packet { id, packet } => log::debug!("#{id} <- {packet:?}"),
            Event::Log(_) => {}
            other => log::info!("{other:?}"),
        }
    });
    if !link.wait_connected(Duration::from_secs(cli.wait)) {
        die(format!("geen fan verbonden binnen {} s (Holoscope gesloten? zelfde wifi als de fan? firewall: TCP {} + UDP 8988)", cli.wait, cli.port));
    }
    // give other fans and the first status a moment
    std::thread::sleep(Duration::from_millis(if cli.all { 1500 } else { 400 }));
    let ids = link.ids();
    let targets: Vec<u32> = if cli.all { ids.clone() } else {
        vec![*ids.get(cli.fan.saturating_sub(1)).unwrap_or_else(|| die(format!("fan {} bestaat niet; er zijn er {}", cli.fan, ids.len())))]
    };
    let mut failed = false;
    for id in targets {
        let ft = fan_type(&link, id, cli.fan_type);
        if let Err(e) = run(&link, id, ft, &cli.cmd) { eprintln!("fan #{id}: fout: {e}"); failed = true; }
    }
    std::thread::sleep(Duration::from_millis(300));
    link.shutdown();
    if failed { std::process::exit(1); }
}

/// Fan type to transcode for: `--type`, else what the fan reports, else 8.
fn fan_type(link: &Link, id: u32, forced: Option<u8>) -> FanType {
    if let Some(t) = forced { return FanType::by_id(t); }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Some(t) = link.device(id).and_then(|d| d.status).map(|s| s.fan_type).filter(|t| *t != 0) {
            return FanType::by_id(t);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    FanType::P56
}

fn run(link: &Link, id: u32, ft: FanType, cmd: &Cmd) -> Result<(), String> {
    let e = |r: Result<(), p56_core::Error>| r.map_err(|e| e.to_string());
    match cmd {
        Cmd::On => e(link.on(id)),
        Cmd::Off => e(link.off(id)),
        Cmd::Play => e(link.play(id)),
        Cmd::Pause => e(link.pause(id)),
        Cmd::Select { slot } => e(link.select(id, *slot)),
        Cmd::Delete { slot } => e(link.delete(id, *slot)),
        Cmd::Rename { slot, name } => e(link.rename(id, *slot, name)),
        Cmd::Repeat { mode } => e(link.repeat(id, matches!(mode, Repeat::All))),
        Cmd::Brightness { value } => {
            if !(1..=16).contains(value) { return Err("helderheid moet 1–16 zijn".into()); }
            e(link.brightness(id, *value))
        }
        Cmd::Angle { degrees } => {
            if !(-180..=180).contains(degrees) { return Err("hoek moet −180…180 zijn".into()); }
            e(link.angle(id, (*degrees + 180) as u16))
        }
        Cmd::Color { mode } => e(link.color_mode(id, match mode { Color::True => ColorMode::True, Color::Bright => ColorMode::Bright })),
        Cmd::Breath { state } => e(link.breath(id, state.b())),
        Cmd::Autostart { state } => e(link.autostart(id, state.b())),
        Cmd::Master { state } => e(link.master(id, state.b())),
        Cmd::Volume { value } => {
            if *value > 10 { return Err("volume moet 0–10 zijn".into()); }
            e(link.volume(id, *value))
        }
        Cmd::Name { name } => e(link.device_name(id, name)),
        Cmd::Hotspot { ssid, password } => {
            if password.len() < 8 { return Err("wifi-wachtwoord moet minstens 8 tekens zijn".into()); }
            println!("let op: daarna is de fan alleen nog bereikbaar via wifi {ssid:?} met het nieuwe wachtwoord");
            e(link.set_hotspot(id, ssid, password))
        }
        Cmd::Router { ssid, password } => {
            println!("de fan zoekt nu router {ssid:?}; daarna vind je hem via je gewone netwerk");
            e(link.set_router(id, ssid, password))
        }
        Cmd::Ble(b) => match b {
            Ble::On => e(link.ble(id, true)),
            Ble::Off => e(link.ble(id, false)),
            Ble::Disconnect => e(link.ble_disconnect(id)),
            Ble::Connect { index } => e(link.ble_connect(id, *index)),
            Ble::Scan => {
                let seq = link.packet_seq(id);
                e(link.ble_scan(id))?;
                match link.wait_packet(id, seq, Duration::from_secs(8), |p| matches!(p, p56_core::Packet::List { kind, .. } if kind & 0x80 != 0)) {
                    Some(p56_core::Packet::List { items, index, .. }) => {
                        for (i, n) in items.iter().enumerate() { println!("  [{i}] {n}{}", if index as usize == i + 1 { "  (verbonden)" } else { "" }); }
                        if items.is_empty() { println!("  geen apparaten gevonden (staat bluetooth aan? `p56 ble on`)"); }
                        Ok(())
                    }
                    _ => Err("geen bluetooth-lijst ontvangen".into()),
                }
            }
        },
        Cmd::Format { .. } => e(link.format_card(id)),
        Cmd::Reset { .. } => e(link.factory_reset(id)),
        Cmd::Info => {
            let _ = link.refresh(id);
            std::thread::sleep(Duration::from_millis(800));
            let d = link.device(id).ok_or("fan weg")?;
            println!("fan #{} {}", d.id, d.ip.map(|i| i.to_string()).unwrap_or_default());
            if let Some(i) = &d.info {
                println!("  naam {}  mac {}  ip {}", i.name, i.mac, i.ip);
                println!("  hotspot {}  router {}", i.hotspot, if i.router.is_empty() { "-" } else { &i.router });
            }
            if let Some(s) = &d.status {
                println!("  type {} ({:?})  aan {}  speelt {}  slot {}  herhaal {}", s.fan_type, FanType::support(s.fan_type), s.power, s.playing, s.play_index, if s.repeat_all { "alles" } else { "één" });
                println!("  helderheid {}/16  hoek {}°  kleur {}  ademlicht {}  autostart {}  master {}", s.brightness, s.angle as i32 - 180,
                    match s.color_mode { 1 => "true color", 2 => "hoge helderheid", _ => "?" }, s.breath, s.autostart, s.master);
            }
            if d.files.is_empty() { println!("  afspeellijst leeg"); }
            for (n, f) in &d.files { println!("  [{n}] {f}"); }
            if let Some(err) = d.last_error { println!("  laatste fout van de fan: {err}"); }
            Ok(())
        }
        Cmd::Upload { file, name, slot, max_frames, x, y, scale, mirror } => {
            if let Some(m) = FanType::check(ft.id) { return Err(m); }
            let pl = Placement { x_offset: *x, y_offset: *y, scale: *scale, mirror: *mirror };
            let frames = frames_from(file, &ft, &pl, *max_frames)?;
            let name = name.clone().unwrap_or_else(|| file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("upload".into()));
            println!("{} frame(s) uploaden als {}…", frames.len(), p56_core::protocol::mp4_name(&name));
            e(link.upload(id, &frames, &name, Some(*slot), 1))
        }
        Cmd::Cast { source, fps, seconds, x, y, scale, mirror } => {
            if let Some(m) = FanType::check(ft.id) { return Err(m); }
            let pl = Placement { x_offset: *x, y_offset: *y, scale: *scale, mirror: *mirror };
            cast_loop(link, &ft, &pl, source, *fps, *seconds)
        }
        Cmd::Preview { .. } | Cmd::Types => unreachable!(),
    }
}

fn load(file: &str) -> Result<image::RgbImage, String> {
    if file == "test" { return Ok(transcode::test_card()); }
    transcode::load_rgb(Path::new(file)).map_err(|e| format!("{file}: {e}"))
}

/// Image (incl. animated GIF) or video → fan JPEGs.
fn frames_from(file: &Path, ft: &FanType, pl: &Placement, max: usize) -> Result<Vec<Vec<u8>>, String> {
    if file.to_str() == Some("test") { return Ok(vec![transcode::encode(&transcode::test_card(), ft, pl).jpeg]); }
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    if let Ok(frames) = transcode::decode_frames(&bytes, max) {
        return Ok(frames.into_iter().map(|(f, _)| transcode::encode(&transcode::fit(f, 720), ft, pl).jpeg).collect());
    }
    let mut v = p56_core::video::open(file, 720).map_err(|e| format!("{e} — installeer ffmpeg voor video, of gebruik een foto/GIF"))?;
    let mut out = Vec::new();
    while let Some(fr) = v.next_frame() {
        out.push(transcode::encode(&fr, ft, pl).jpeg);
        if max > 0 && out.len() >= max { break; }
    }
    if out.is_empty() { return Err("geen frames gedecodeerd".into()); }
    log::info!("{} frames bij {:.1} fps", out.len(), v.fps);
    Ok(out)
}

fn cast_loop(link: &Link, ft: &FanType, pl: &Placement, source: &str, fps: f64, seconds: f64) -> Result<(), String> {
    let period = Duration::from_secs_f64(1.0 / fps.clamp(0.5, 60.0));
    let started = Instant::now();
    // Pre-encode stills and GIFs; stream videos.
    let mut frames: Vec<(Vec<u8>, u32)> = Vec::new();
    let mut video = None;
    if source == "test" {
        frames.push((transcode::encode(&transcode::test_card(), ft, pl).jpeg, 0));
    } else {
        let bytes = std::fs::read(source).map_err(|e| format!("{source}: {e}"))?;
        match transcode::decode_frames(&bytes, 0) {
            Ok(fr) => frames = fr.into_iter().map(|(f, ms)| (transcode::encode(&transcode::fit(f, 720), ft, pl).jpeg, ms)).collect(),
            Err(_) => video = Some(p56_core::video::open(Path::new(source), 720).map_err(|e| e.to_string())?),
        }
    }
    let mut n = 0u64;
    let mut i = 0usize;
    loop {
        if seconds > 0.0 && started.elapsed().as_secs_f64() >= seconds { break; }
        let t = Instant::now();
        let (jpeg, delay) = match &mut video {
            Some(v) => match v.next_frame() {
                Some(f) => (transcode::encode(&f, ft, pl).jpeg, 0),
                None => { video = Some(p56_core::video::open(Path::new(source), 720).map_err(|e| e.to_string())?); continue; }
            },
            None => { let f = frames[i % frames.len()].clone(); i += 1; f }
        };
        if link.cast(&jpeg) > 0 {
            n += 1;
            if n % 50 == 0 { log::info!("{n} frames gecast"); }
        } else if link.ids().is_empty() {
            log::warn!("geen fan verbonden");
            std::thread::sleep(Duration::from_secs(1));
        }
        let wait = if delay > 0 { Duration::from_millis(delay as u64) } else { period };
        if let Some(rest) = wait.checked_sub(t.elapsed()) { std::thread::sleep(rest); }
    }
    println!("{n} frames gecast");
    Ok(())
}
