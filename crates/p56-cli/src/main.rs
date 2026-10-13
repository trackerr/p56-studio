//! `p56` — command-line client for Holoscope / P56 hologram fans.
//! Replaces Holoscope for everything except firmware flashing and the vendor cloud.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use p56_core::{transcode, ColorMode, Config, Event, FanType, Link, Placement, Support};

#[derive(Parser)]
#[command(name = "p56", version, about = "Control Holoscope / P56 hologram fans from your PC")]
struct Cli {
    /// Fan type (resolution/arms). Default: whatever the fan reports (P56 = 8).
    #[arg(long = "type", global = true)]
    fan_type: Option<u8>,
    /// IP to advertise in the broadcast (default: each network adapter its own IP).
    #[arg(long, global = true)]
    ip: Option<std::net::Ipv4Addr>,
    /// TCP port the fan connects to (the real fan always uses 6666). 0: any free port.
    #[arg(long, default_value_t = 6666, global = true)]
    port: u16,
    /// Write the TCP port in use to this file once it is open: how tests find the port that
    /// port 0 picked.
    #[arg(long, hide = true, global = true)]
    port_file: Option<PathBuf>,
    /// Extra address to send the broadcast to, e.g. 192.168.4.1:8988 or the simulator.
    #[arg(long, global = true)]
    target: Vec<SocketAddr>,
    /// Seconds to wait for a fan to connect.
    #[arg(long, default_value_t = 20, global = true)]
    wait: u64,
    /// Which fan (1 = the first to connect) when there are several.
    #[arg(long, default_value_t = 1, global = true)]
    fan: usize,
    /// Send the command to all connected fans.
    #[arg(long, global = true)]
    all: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}
impl OnOff {
    fn b(self) -> bool {
        matches!(self, OnOff::On)
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Repeat {
    All,
    One,
}

#[derive(Clone, Copy, ValueEnum)]
enum Color {
    True,
    Bright,
}

#[derive(Subcommand)]
enum Cmd {
    /// Turn on
    On,
    /// Turn off
    Off,
    Play,
    Pause,
    /// Show slot N (1-based)
    Select {
        slot: u8,
    },
    /// Delete slot N
    Delete {
        slot: u8,
    },
    /// Rename slot N (.mp4 is appended)
    Rename {
        slot: u8,
        name: String,
    },
    /// Repeat the playlist (all) or the current file (one)
    Repeat {
        mode: Repeat,
    },
    /// Brightness 1–16
    Brightness {
        value: u8,
    },
    /// Rotate the image: −180…180 degrees
    #[command(allow_negative_numbers = true)]
    Angle {
        degrees: i16,
    },
    /// Colour mode: true (true colour) or bright (high brightness)
    Color {
        mode: Color,
    },
    /// Breathing light on/off
    Breath {
        state: OnOff,
    },
    /// Start playing automatically at power-on, on/off
    Autostart {
        state: OnOff,
    },
    /// Volume 0–10 (only fans with a speaker)
    Volume {
        value: u8,
    },
    /// Set the device name
    Name {
        name: String,
    },
    /// Change the name and password of the fan's own Wi-Fi network
    Hotspot {
        ssid: String,
        password: String,
    },
    /// Make the fan join your router
    Router {
        ssid: String,
        password: String,
    },
    /// Format the memory card (erases all files!)
    Format {
        #[arg(long)]
        yes: bool,
    },
    /// Restore factory settings
    Reset {
        #[arg(long)]
        yes: bool,
    },
    /// Show machine info, status and playlist
    Info,
    /// Upload a photo, GIF or video as a playlist entry and show it
    Upload {
        file: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = 1)]
        slot: u8,
        /// Video/GIF: maximum number of frames (0 = all)
        #[arg(long, default_value_t = 0)]
        max_frames: usize,
        /// Video: leave out the sound (it is sent along by default; needs ffmpeg)
        #[arg(long)]
        no_sound: bool,
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        x: f64,
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        y: f64,
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        #[arg(long)]
        mirror: bool,
        /// Turn the picture clockwise, in degrees (e.g. 90)
        #[arg(long, default_value_t = 0)]
        rotate: u16,
    },
    /// Live cast: photo, GIF, video or 'test'
    Cast {
        source: String,
        #[arg(long, default_value_t = 15.0)]
        fps: f64,
        /// Stop after this many seconds (0 = keep going)
        #[arg(long, default_value_t = 0.0)]
        seconds: f64,
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        x: f64,
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        y: f64,
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        #[arg(long)]
        mirror: bool,
        /// Turn the picture clockwise, in degrees (e.g. 90)
        #[arg(long, default_value_t = 0)]
        rotate: u16,
    },
    /// Transcode only: writes <prefix>_layout.png and <prefix>.jpg, no network
    Preview {
        file: String,
        #[arg(default_value = "preview")]
        prefix: String,
    },
    /// List known fan types
    Types,
}

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
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
                    let jpeg = FanType::known(id)
                        .map(|f| {
                            let (w, h) = f.jpeg_size();
                            format!("{w}×{h}")
                        })
                        .unwrap_or("-".into());
                    println!("type {id:>2}: {a}×{l}  {fps} fps  JPEG {jpeg:<9} {s:?}");
                }
            }
            return;
        }
        Cmd::Preview { file, prefix } => {
            let ft = usable_type(cli.fan_type.unwrap_or(8), "--type").unwrap_or_else(|e| die(e));
            let img = load(file).unwrap_or_else(|e| die(e));
            let f = transcode::encode(&img, &ft, &Placement::default());
            f.layout.save(format!("{prefix}_layout.png")).unwrap_or_else(|e| die(e));
            std::fs::write(format!("{prefix}.jpg"), &f.jpeg).unwrap_or_else(|e| die(e));
            let (w, h) = f.layout.dimensions();
            println!("{prefix}_layout.png {w}x{h}, {prefix}.jpg {} bytes (q{})", f.jpeg.len(), f.quality);
            return;
        }
        Cmd::Format { yes: false } | Cmd::Reset { yes: false } => die("this erases data on the fan; add --yes to continue"),
        _ => {}
    }

    let cfg = Config { local_ip: cli.ip, tcp_port: cli.port, targets: cli.target.clone(), ..Default::default() };
    let (link, rx) = Link::start(cfg).unwrap_or_else(|e| die(e));
    if let Some(path) = &cli.port_file {
        // the newline ends the number, so a reader never takes a half-written file for a port
        std::fs::write(path, format!("{}\n", link.tcp_port)).unwrap_or_else(|e| die(format!("cannot write {}: {e}", path.display())));
    }
    std::thread::spawn(move || {
        for ev in rx {
            match ev {
                Event::Packet { id, packet } => log::debug!("#{id} <- {packet:?}"),
                Event::Log(_) => {}
                other => log::info!("{other:?}"),
            }
        }
    });
    if !link.wait_connected(Duration::from_secs(cli.wait)) {
        die(format!("no fan connected within {} s (Holoscope closed? same Wi-Fi as the fan? firewall: TCP {} + UDP 8988)", cli.wait, link.tcp_port));
    }
    // give other fans and the first status a moment
    std::thread::sleep(Duration::from_millis(if cli.all { 1500 } else { 400 }));
    let ids = link.ids();
    let targets: Vec<u32> = if cli.all {
        ids.clone()
    } else {
        vec![*ids.get(cli.fan.saturating_sub(1)).unwrap_or_else(|| die(format!("fan {} does not exist; there are {}", cli.fan, ids.len())))]
    };
    let mut failed = false;
    if let Cmd::Cast { source, fps, seconds, x, y, scale, mirror, rotate } = &cli.cmd {
        // One live stream to exactly the selected fans, encoded once per fan type.
        let mut groups: Vec<(FanType, Vec<u32>)> = Vec::new();
        for id in targets {
            match fan_type(&link, id, cli.fan_type) {
                Ok(ft) => match groups.iter_mut().find(|(f, _)| f.id == ft.id) {
                    Some((_, ids)) => ids.push(id),
                    None => groups.push((ft, vec![id])),
                },
                Err(e) => {
                    eprintln!("fan #{id}: error: {e}");
                    failed = true;
                }
            }
        }
        if !groups.is_empty() {
            let pl = Placement { x_offset: *x, y_offset: *y, scale: *scale, mirror: *mirror, rotation: *rotate };
            if let Err(e) = cast_loop(&link, &groups, &pl, source, *fps, *seconds) {
                eprintln!("error: {e}");
                failed = true;
            }
        }
    } else {
        let needs_type = matches!(cli.cmd, Cmd::Upload { .. });
        for id in targets {
            let ft = if needs_type {
                match fan_type(&link, id, cli.fan_type) {
                    Ok(ft) => ft,
                    Err(e) => {
                        eprintln!("fan #{id}: error: {e}");
                        failed = true;
                        continue;
                    }
                }
            } else {
                FanType::P56
            };
            if let Err(e) = run(&link, id, ft, &cli.cmd) {
                eprintln!("fan #{id}: error: {e}");
                failed = true;
            }
        }
    }
    std::thread::sleep(Duration::from_millis(300));
    link.shutdown();
    if failed {
        std::process::exit(1);
    }
}

/// Validate a fan type id before using its geometry: unknown and unsupported types are an
/// error (never a silent fallback to another layout); legacy types work with a warning.
fn usable_type(id: u8, source: &str) -> Result<FanType, String> {
    let ft = FanType::usable(id).map_err(|e| format!("{e} ({source})"))?;
    if FanType::support(id) == Support::Legacy {
        log::warn!("fan type {id} uses the older raw polar format; newer firmware may ignore it");
    }
    Ok(ft)
}

/// Fan type to transcode for: `--type`, else what the fan reports, else 8.
fn fan_type(link: &Link, id: u32, forced: Option<u8>) -> Result<FanType, String> {
    if let Some(t) = forced {
        return usable_type(t, "--type");
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Some(t) = link.device(id).and_then(|d| d.status).map(|s| s.fan_type).filter(|t| *t != 0) {
            return usable_type(t, "reported by the fan; use --type to override");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(FanType::P56)
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
            if !(1..=16).contains(value) {
                return Err("brightness must be 1–16".into());
            }
            e(link.brightness(id, *value))
        }
        Cmd::Angle { degrees } => {
            if !(-180..=180).contains(degrees) {
                return Err("angle must be −180…180".into());
            }
            e(link.angle(id, (*degrees + 180) as u16))
        }
        Cmd::Color { mode } => e(link.color_mode(
            id,
            match mode {
                Color::True => ColorMode::True,
                Color::Bright => ColorMode::Bright,
            },
        )),
        Cmd::Breath { state } => e(link.breath(id, state.b())),
        Cmd::Autostart { state } => e(link.autostart(id, state.b())),
        Cmd::Volume { value } => {
            if *value > 10 {
                return Err("volume must be 0–10".into());
            }
            e(link.volume(id, *value))
        }
        Cmd::Name { name } => e(link.device_name(id, name)),
        Cmd::Hotspot { ssid, password } => {
            if password.len() < 8 {
                return Err("Wi-Fi password must be at least 8 characters".into());
            }
            println!("note: after this the fan is only reachable via Wi-Fi {ssid:?} with the new password");
            e(link.set_hotspot(id, ssid, password))
        }
        Cmd::Router { ssid, password } => {
            println!("the fan now looks for router {ssid:?}; after that you will find it on your regular network");
            e(link.set_router(id, ssid, password))
        }
        Cmd::Format { .. } => e(link.format_card(id)),
        Cmd::Reset { .. } => e(link.factory_reset(id)),
        Cmd::Info => {
            let _ = link.refresh(id);
            std::thread::sleep(Duration::from_millis(800));
            let d = link.device(id).ok_or("fan gone")?;
            println!("fan #{} {}", d.id, d.ip.map(|i| i.to_string()).unwrap_or_default());
            if let Some(i) = &d.info {
                println!("  name {}  mac {}", i.name, i.mac);
                if i.stitch != [0; 4] {
                    let [w, h, x, y] = i.stitch;
                    println!("  group picture {w}×{h}, this fan at column {x}, row {y}");
                }
                println!("  hotspot {}  router {}", i.hotspot, if i.router.is_empty() { "-" } else { &i.router });
            }
            if let Some(s) = &d.status {
                println!(
                    "  type {}{} ({:?})  on {}  playing {}  slot {}  repeat {}",
                    s.fan_type,
                    FanType::model(s.fan_type).map(|m| format!(" {m}")).unwrap_or_default(),
                    FanType::support(s.fan_type),
                    s.power,
                    s.playing,
                    s.play_index,
                    if s.repeat_all { "all" } else { "one" }
                );
                println!(
                    "  brightness {}/16  angle {}°  colour {}  breathing {}  autostart {}",
                    s.brightness,
                    s.angle as i32 - 180,
                    match s.color_mode {
                        1 => "true color",
                        2 => "high brightness",
                        // a real P56 reports 0: it does not tell its colour mode
                        _ => "not reported",
                    },
                    s.breath,
                    s.autostart
                );
            }
            if d.files.is_empty() {
                println!("  playlist empty");
            }
            for (n, f) in &d.files {
                println!("  [{n}] {f}");
            }
            if let Some(err) = d.last_error {
                println!("  last error from the fan: {err}");
            }
            Ok(())
        }
        Cmd::Upload { file, name, slot, max_frames, no_sound, x, y, scale, mirror, rotate } => {
            let pl = Placement { x_offset: *x, y_offset: *y, scale: *scale, mirror: *mirror, rotation: *rotate };
            let frames = frames_from(file, &ft, &pl, *max_frames)?;
            let name = name.clone().unwrap_or_else(|| file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("upload".into()));
            let audio = if frames.len() > 1 && !*no_sound { p56_core::video::audio_pcm(file) } else { None };
            println!("uploading {} frame(s){} as {}…", frames.len(), if audio.is_some() { " with sound" } else { "" }, p56_core::protocol::mp4_name(&name));
            let o = link.upload_with_audio(id, &frames, audio.as_deref(), &name, Some(*slot)).map_err(|e| e.to_string())?;
            // a real P56 does not acknowledge the data: being in the playlist is the proof
            if !o.listed {
                eprintln!(
                    "warning: the file did not show up in the fan's playlist (yet){}",
                    if o.acked { "" } else { "; the fan did not acknowledge the data" }
                );
            }
            Ok(())
        }
        Cmd::Cast { .. } | Cmd::Preview { .. } | Cmd::Types => unreachable!(),
    }
}

fn load(file: &str) -> Result<image::RgbImage, String> {
    if file == "test" {
        return Ok(transcode::test_card());
    }
    transcode::load_rgb(Path::new(file)).map_err(|e| format!("{file}: {e}"))
}

/// Image (incl. animated GIF) or video → fan JPEGs at the fan's fixed 20 fps: a frame that
/// should show longer is repeated, a faster video is thinned out, so it plays at its own speed.
/// `max` limits the frames sent (0 = 1200, i.e. 60 s).
fn frames_from(file: &Path, ft: &FanType, pl: &Placement, max: usize) -> Result<Vec<Vec<u8>>, String> {
    let max = if max == 0 { 1200 } else { max };
    let step = p56_core::protocol::FRAME_MS as f64;
    if file.to_str() == Some("test") {
        return Ok(vec![transcode::encode_upload(&transcode::test_card(), ft, pl).jpeg]);
    }
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    if let Ok(frames) = transcode::decode_frames(&bytes, max) {
        let delays: Vec<u32> = frames.iter().map(|(_, ms)| *ms).collect();
        let jpegs: Vec<Vec<u8>> = frames.into_iter().map(|(f, _)| transcode::encode_upload(&transcode::fit(f, 720), ft, pl).jpeg).collect();
        return Ok(transcode::fan_rate_indices(&delays, max).into_iter().map(|i| jpegs[i].clone()).collect());
    }
    let mut v = p56_core::video::open(file, 720).map_err(|e| format!("{e} — install ffmpeg for video, or use a photo/GIF"))?;
    let ratio = 1000.0 / step / v.fps.max(1.0); // fan frames per source frame
    let mut out = Vec::new();
    let mut i = 0f64;
    while let Some(fr) = v.next_frame() {
        let n = ((i + 1.0) * ratio).floor() as usize - (i * ratio).floor() as usize;
        i += 1.0;
        if n == 0 {
            continue;
        }
        let jpeg = transcode::encode_upload(&fr, ft, pl).jpeg;
        for _ in 0..n {
            if out.len() < max {
                out.push(jpeg.clone());
            }
        }
        if out.len() >= max {
            break;
        }
    }
    if out.is_empty() {
        return Err("no frames decoded".into());
    }
    log::info!("{} frames at {:.1} fps", out.len(), v.fps);
    Ok(out)
}

/// Live cast to the fans in `groups` only (fan type, fan ids): every frame is encoded once
/// per fan type and sent to that group's fans.
fn cast_loop(link: &Link, groups: &[(FanType, Vec<u32>)], pl: &Placement, source: &str, fps: f64, seconds: f64) -> Result<(), String> {
    let period = Duration::from_secs_f64(1.0 / fps.clamp(0.5, 60.0));
    let started = Instant::now();
    let encode = |img: &image::RgbImage| -> Vec<Vec<u8>> { groups.iter().map(|(ft, _)| transcode::encode(img, ft, pl).jpeg).collect() };
    // Pre-encode stills and GIFs; stream videos.
    let mut frames: Vec<(Vec<Vec<u8>>, u32)> = Vec::new();
    let mut video = None;
    if source == "test" {
        frames.push((encode(&transcode::test_card()), 0));
    } else {
        let bytes = std::fs::read(source).map_err(|e| format!("{source}: {e}"))?;
        match transcode::decode_frames(&bytes, 0) {
            Ok(fr) => frames = fr.into_iter().map(|(f, ms)| (encode(&transcode::fit(f, 720)), ms)).collect(),
            Err(_) => video = Some(p56_core::video::open(Path::new(source), 720).map_err(|e| e.to_string())?),
        }
    }
    let mut n = 0u64;
    let mut i = 0usize;
    loop {
        if seconds > 0.0 && started.elapsed().as_secs_f64() >= seconds {
            break;
        }
        let t = Instant::now();
        let (jpegs, delay) = match &mut video {
            Some(v) => match v.next_frame() {
                Some(f) => (encode(&f), 0),
                None => {
                    video = Some(p56_core::video::open(Path::new(source), 720).map_err(|e| e.to_string())?);
                    continue;
                }
            },
            None => {
                let f = frames[i % frames.len()].clone();
                i += 1;
                f
            }
        };
        let sent: usize = groups.iter().zip(&jpegs).map(|((_, ids), jpeg)| link.cast_to(ids, jpeg)).sum();
        if sent > 0 {
            n += 1;
            if n.is_multiple_of(50) {
                log::info!("{n} frames cast");
            }
        } else if groups.iter().flat_map(|(_, ids)| ids).all(|id| link.device(*id).is_none()) {
            log::warn!("the selected fan(s) are no longer connected");
            std::thread::sleep(Duration::from_secs(1));
        }
        let wait = if delay > 0 { Duration::from_millis(delay as u64) } else { period };
        if let Some(rest) = wait.checked_sub(t.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    println!("{n} frames cast");
    Ok(())
}
