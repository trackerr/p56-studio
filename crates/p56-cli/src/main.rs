//! `p56` — command-line client for Holoscope / P56 hologram fans.
//! Replaces Holoscope for everything except firmware flashing and the vendor cloud.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use p56_core::{transcode, Config, Event, FanType, Link, Placement};

#[derive(Parser)]
#[command(name = "p56", version, about = "Holoscope / P56 POV hologram fan client")]
struct Cli {
    /// Fan type (resolution/arms). 8 = P56 (688×240).
    #[arg(long, default_value_t = 8, global = true)]
    r#type: u8,
    /// IP to advertise (default: auto-detect).
    #[arg(long, global = true)]
    ip: Option<std::net::Ipv4Addr>,
    /// Seconds to wait for a fan to connect.
    #[arg(long, default_value_t = 20, global = true)]
    wait: u64,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Power on
    On,
    /// Power off
    Off,
    Play,
    Pause,
    /// Show playlist slot N (1-based)
    Select { slot: u8 },
    /// Delete playlist slot N
    Delete { slot: u8 },
    Rename { old: String, new: String },
    /// Set the fan's Wi-Fi (it will only be reachable on that network afterwards!)
    Wifi { ssid: String, password: String },
    /// Print machine info, status and playlist
    Info,
    /// Upload an image or video as a playlist entry and show it
    Upload {
        file: PathBuf,
        #[arg(long)] name: Option<String>,
        #[arg(long, default_value_t = 1)] slot: u8,
        /// Video: limit number of frames (0 = all)
        #[arg(long, default_value_t = 0)] max_frames: usize,
        #[arg(long, default_value_t = 0.0)] x: f64,
        #[arg(long, default_value_t = 0.0)] y: f64,
        #[arg(long, default_value_t = 1.0)] scale: f64,
        #[arg(long)] mirror: bool,
    },
    /// Live cast an image / video / 'test' in a loop
    Cast {
        /// Path, or 'test' for the built-in test card
        source: String,
        #[arg(long, default_value_t = 15.0)] fps: f64,
        #[arg(long, default_value_t = 0.0)] x: f64,
        #[arg(long, default_value_t = 0.0)] y: f64,
        #[arg(long, default_value_t = 1.0)] scale: f64,
        #[arg(long)] mirror: bool,
    },
    /// Transcode only: write <prefix>_layout.png and <prefix>.jpg, no network
    Preview { file: String, #[arg(default_value = "preview")] prefix: String },
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).format_timestamp_millis().init();
    let cli = Cli::parse();
    let ft = FanType::by_id(cli.r#type);

    if let Cmd::Preview { file, prefix } = &cli.cmd {
        let img = load(file).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1) });
        let f = transcode::encode(&img, &ft, &Placement::default());
        f.layout.save(format!("{prefix}_layout.png")).unwrap();
        std::fs::write(format!("{prefix}.jpg"), &f.jpeg).unwrap();
        let (w, h) = f.layout.dimensions();
        println!("{prefix}_layout.png {w}x{h}, {prefix}.jpg {} bytes (q{})", f.jpeg.len(), f.quality);
        return;
    }

    let (link, rx) = Link::start(Config { local_ip: cli.ip, ..Default::default() }).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1) });
    std::thread::spawn(move || for ev in rx {
        match ev {
            Event::Packet { id, packet } => log::debug!("#{id} <- {packet:?}"),
            Event::Log(_) => {}
            other => log::info!("{other:?}"),
        }
    });
    if !link.wait_connected(Duration::from_secs(cli.wait)) {
        eprintln!("no fan connected within {} s (is Holoscope closed? same Wi-Fi as the fan?)", cli.wait);
        std::process::exit(2);
    }
    let id = link.ids()[0];
    let r = match cli.cmd {
        Cmd::On => link.on(id),
        Cmd::Off => link.off(id),
        Cmd::Play => link.play(id),
        Cmd::Pause => link.pause(id),
        Cmd::Select { slot } => link.select(id, slot),
        Cmd::Delete { slot } => link.delete(id, slot),
        Cmd::Rename { old, new } => link.rename(id, &old, &new),
        Cmd::Wifi { ssid, password } => link.set_wifi(id, &ssid, &password),
        Cmd::Info => {
            link.refresh(id).ok();
            std::thread::sleep(Duration::from_millis(600));
            link.send_to(id, &p56_core::protocol::heartbeat()).ok();
            std::thread::sleep(Duration::from_millis(600));
            for d in link.devices() {
                println!("fan #{} {}", d.id, d.ip.map(|i| i.to_string()).unwrap_or_default());
                if let Some(i) = &d.info { println!("  name {}  mac {}  wifi {}  fw {}", i.name, i.mac, i.wifi, i.version); }
                if let Some(s) = &d.status { println!("  type {}  {}×{}  power {}  playing {}  slot {}", s.fan_type, s.angles, s.leds_per_arm, s.power, s.playing, s.play_index); }
                for (n, f) in &d.files { println!("  [{n}] {f}"); }
            }
            Ok(())
        }
        Cmd::Upload { file, name, slot, max_frames, x, y, scale, mirror } => {
            let pl = Placement { x_offset: x, y_offset: y, scale, mirror };
            let frames = frames_from(&file, &ft, &pl, max_frames).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1) });
            let name = name.unwrap_or_else(|| file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("upload".into()));
            link.upload(id, &frames, &name, Some(slot), 1)
        }
        Cmd::Cast { source, fps, x, y, scale, mirror } => {
            let pl = Placement { x_offset: x, y_offset: y, scale, mirror };
            cast_loop(&link, &ft, &pl, &source, fps);
            Ok(())
        }
        Cmd::Preview { .. } => unreachable!(),
    };
    std::thread::sleep(Duration::from_millis(300));
    if let Err(e) = r { eprintln!("error: {e}"); std::process::exit(1); }
}

fn load(file: &str) -> Result<image::RgbImage, String> {
    if file == "test" { return Ok(transcode::test_card()); }
    transcode::load_rgb(std::path::Path::new(file)).map_err(|e| format!("{file}: {e}"))
}

fn frames_from(file: &PathBuf, ft: &FanType, pl: &Placement, max: usize) -> Result<Vec<Vec<u8>>, String> {
    if let Ok(img) = transcode::load_rgb(file) {
        return Ok(vec![transcode::encode(&img, ft, pl).jpeg]);
    }
    let mut v = p56_core::video::open(file, 720).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(fr) = v.next_frame() {
        out.push(transcode::encode(&fr, ft, pl).jpeg);
        if max > 0 && out.len() >= max { break; }
    }
    if out.is_empty() { return Err("no frames decoded".into()); }
    log::info!("{} frames at {:.1} fps", out.len(), v.fps);
    Ok(out)
}

fn cast_loop(link: &Link, ft: &FanType, pl: &Placement, source: &str, fps: f64) {
    let period = Duration::from_secs_f64(1.0 / fps.max(0.1));
    let mut video = (source != "test" && transcode::load_rgb(std::path::Path::new(source)).is_err())
        .then(|| p56_core::video::open(std::path::Path::new(source), 720).ok()).flatten();
    let still = if video.is_none() { Some(load(source).unwrap_or_else(|e| { eprintln!("{e}"); std::process::exit(1) })) } else { None };
    let mut n = 0u64;
    loop {
        let t = Instant::now();
        let img = match (&mut video, &still) {
            (Some(v), _) => match v.next_frame() {
                Some(f) => f,
                None => { video = p56_core::video::open(std::path::Path::new(source), 720).ok(); continue; }
            },
            (None, Some(s)) => s.clone(),
            _ => return,
        };
        let f = transcode::encode(&img, ft, pl);
        if link.cast(&f.jpeg) > 0 {
            n += 1;
            if n % 50 == 0 { log::info!("{n} frames cast"); }
        } else if link.ids().is_empty() {
            log::warn!("no fan connected"); std::thread::sleep(Duration::from_secs(1));
        }
        if let Some(rest) = period.checked_sub(t.elapsed()) { std::thread::sleep(rest); }
    }
}
