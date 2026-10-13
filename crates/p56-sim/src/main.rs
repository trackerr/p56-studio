//! `p56-sim` — run one or more simulated P56 fans.
//!
//! The fans answer the PC's broadcast like real ones; open http://localhost:8099/
//! to see what each fan shows, its settings and any protocol errors.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::Parser;
use p56_core::transcode::{self, FanType};
use p56_sim::{Sim, SimConfig};

#[derive(Parser)]
#[command(name = "p56-sim", version, about = "Simulated Holoscope / P56 hologram fan(s)")]
struct Args {
    /// Number of fans to simulate.
    #[arg(long, default_value_t = 1)]
    count: usize,
    /// Fan type (8 = P56).
    #[arg(long = "type", default_value_t = 8)]
    fan_type: u8,
    #[arg(long, default_value = "SIM-P56")]
    name: String,
    /// Connect straight to this PC (ip:port) instead of waiting for its broadcast.
    #[arg(long)]
    pc: Option<SocketAddr>,
    /// TCP port of the PC app.
    #[arg(long, default_value_t = 6666)]
    pc_port: u16,
    /// UDP port the first fan listens on (others get free ports; give them --pc).
    #[arg(long, default_value_t = 8988)]
    udp_port: u16,
    /// Split replies into tiny pieces (stress test).
    #[arg(long)]
    fragment: bool,
    #[arg(long, default_value_t = 20)]
    max_files: usize,
    /// Web view port (0 = off).
    #[arg(long, default_value_t = 8099)]
    http: u16,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).format_timestamp_millis().init();
    let a = Args::parse();
    let mut sims = Vec::new();
    for i in 0..a.count.max(1) {
        let cfg = SimConfig {
            name: if a.count > 1 { format!("{}-{}", a.name, i + 1) } else { a.name.clone() },
            fan_type: a.fan_type,
            udp_port: if i == 0 { a.udp_port } else { 0 },
            pc: a.pc,
            pc_port: a.pc_port,
            ip: Ipv4Addr::new(192, 168, 4, 1 + i as u8),
            mac: [0x02, 0xE0, 0xE0, 0x56, 0x00, 1 + i as u8],
            max_files: a.max_files,
            fragment: a.fragment,
            reconnect: true,
        };
        match Sim::start(cfg) {
            Ok(s) => {
                log::info!("fan {} listening on UDP {}", s.config().name, s.udp_port);
                sims.push(s);
            }
            Err(e) => {
                eprintln!("cannot start simulator: {e} (is one already running on UDP {}?)", a.udp_port);
                std::process::exit(1);
            }
        }
    }
    let sims = Arc::new(sims);
    if sims.len() > 1 {
        // Only the first fan can own the discovery port on this host; the others follow it to
        // the same PC, like real fans that all hear the same broadcast.
        let s = sims.clone();
        thread::spawn(move || loop {
            let lead = s[0].state();
            if lead.connected {
                if let Some(pc) = lead.pc.as_deref().and_then(|p| p.parse().ok()) {
                    for other in &s[1..] {
                        other.connect_to(pc);
                    }
                }
            }
            thread::sleep(Duration::from_millis(500));
        });
    }
    if a.http > 0 {
        match TcpListener::bind((Ipv4Addr::LOCALHOST, a.http)) {
            Ok(l) => {
                log::info!("web view: http://localhost:{}/", a.http);
                let s = sims.clone();
                thread::spawn(move || {
                    for c in l.incoming().flatten() {
                        let s = s.clone();
                        thread::spawn(move || serve(c, &s));
                    }
                });
            }
            Err(e) => log::warn!("no web view on {}: {e}", a.http),
        }
    }
    loop {
        thread::sleep(Duration::from_secs(10));
        for s in sims.iter() {
            let st = s.state();
            log::info!(
                "[{}] {} | on={} playing={} slot={} files={} cast={} errors={}",
                s.config().name,
                if st.connected { "connected" } else { "waiting for pc" },
                st.power,
                st.playing,
                st.current,
                st.files.len(),
                st.cast_ok,
                st.protocol_errors.len()
            );
        }
    }
}

fn serve(mut c: TcpStream, sims: &[Sim]) {
    let mut line = String::new();
    if BufReader::new(&c).read_line(&mut line).is_err() {
        return;
    }
    let target = line.split_whitespace().nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/").to_string();
    let (ctype, body): (&str, Vec<u8>) = if let Some(i) = path.strip_prefix("/frame/").and_then(|s| s.trim_end_matches(".png").parse::<usize>().ok()) {
        let Some(sim) = sims.get(i) else { return };
        let ft = FanType::by_id(sim.config().fan_type);
        let img = sim.current_jpeg().and_then(|j| transcode::preview_from_jpeg(&j, &ft, 360).ok()).unwrap_or_else(|| image::RgbImage::new(360, 360));
        let mut v = Vec::new();
        let _ = image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png);
        ("image/png", v)
    } else if path == "/state.json" {
        let all: Vec<_> = sims.iter().map(|s| s.state()).collect();
        ("application/json", serde_json::to_vec_pretty(&all).unwrap_or_default())
    } else {
        let cards: String = (0..sims.len()).map(|i| format!("<div class=c><img id=f{i} src='/frame/{i}.png'><pre id=s{i}></pre></div>")).collect();
        ("text/html; charset=utf-8", format!(r#"<!doctype html><html><head><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1">
<title>P56 simulator</title><style>body{{background:#0b0d12;color:#cfd3dc;font:13px system-ui;margin:16px}}.c{{display:inline-block;vertical-align:top;margin:0 16px 16px 0}}
img{{width:360px;height:360px;border-radius:50%;background:#000;display:block}}pre{{width:360px;white-space:pre-wrap}}</style></head><body><h3>P56 simulator</h3>{cards}
<script>setInterval(async()=>{{const s=await (await fetch('/state.json')).json();s.forEach((x,i)=>{{document.getElementById('f'+i).src='/frame/'+i+'.png?'+Date.now();
document.getElementById('s'+i).textContent=`${{x.device_name}} — ${{x.connected?'connected to '+x.pc:'waiting for pc'}}
on ${{x.power}} · playing ${{x.playing}} · slot ${{x.current}} · brightness ${{x.brightness}} · angle ${{x.angle}} · colour ${{x.color_mode}}
breathing ${{x.breath}} · autostart ${{x.autostart}} · volume ${{x.volume}}
hotspot ${{x.hotspot[0]}} · router ${{x.router[0]||'-'}}
files: ${{x.files.map((f,j)=>(j+1)+' '+f.name+' ('+f.frames+')').join(', ')||'-'}}
cast frames ${{x.cast_ok}} (bad ${{x.cast_bad}}) · uploads ${{x.uploads}}
protocol errors: ${{x.protocol_errors.length?x.protocol_errors.join('\n'):'none'}}`}})}},500)</script></body></html>"#).into_bytes())
    };
    let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
    let _ = c.write_all(&body);
}
