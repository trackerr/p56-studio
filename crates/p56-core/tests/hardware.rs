//! Tests against a real fan. Skipped unless asked for:
//!   P56_HARDWARE=1 cargo test -p p56-core --test hardware -- --ignored --nocapture --test-threads 1
//! Close Holoscope and P56 Studio first (this test opens TCP port 6666 itself) and put this
//! computer on the fan's Wi-Fi or the same router. Set P56_FAN=<ip> if broadcasts do not reach it.
//! The tests upload two files named `hwtest-*.mp4` and delete them again.

use std::time::Duration;

use p56_core::transcode::{self, FanType, Placement};
use p56_core::{Config, Link};

/// One link for all tests in this process (the fan port can only be opened once).
fn fan() -> Option<(Link, u32, FanType)> {
    static FAN: std::sync::OnceLock<Option<(Link, u32, FanType)>> = std::sync::OnceLock::new();
    FAN.get_or_init(connect).clone()
}

fn connect() -> Option<(Link, u32, FanType)> {
    if std::env::var("P56_HARDWARE").is_err() {
        eprintln!("skipped: set P56_HARDWARE=1 to test against a real fan");
        return None;
    }
    let targets = std::env::var("P56_FAN").ok().and_then(|ip| format!("{ip}:8988").parse().ok()).into_iter().collect();
    let (link, rx) = Link::start(Config { targets, ..Default::default() }).expect("TCP port 6666 is free (close Holoscope / P56 Studio)");
    assert!(link.wait_connected(Duration::from_secs(30)), "no fan connected within 30 s");
    let id = link.ids()[0];
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while link.device(id).and_then(|d| d.status).is_none() {
        assert!(std::time::Instant::now() < deadline, "the fan sent no status");
        std::thread::sleep(Duration::from_millis(100));
    }
    let ft = FanType::usable(link.device(id).unwrap().status.unwrap().fan_type).expect("supported fan type");
    // keep the event channel open for the whole test
    std::mem::forget(rx);
    Some((link, id, ft))
}

/// A rotating bar: easy to see whether the fan animates (and at what speed).
fn frames(ft: &FanType, n: usize) -> Vec<Vec<u8>> {
    (0..n)
        .map(|i| {
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            let img = image::RgbImage::from_fn(720, 720, |x, y| {
                let (dx, dy) = (x as f64 - 360.0, y as f64 - 360.0);
                let along = dx * a.cos() + dy * a.sin();
                let across = -dx * a.sin() + dy * a.cos();
                if along > 0.0 && along < 340.0 && across.abs() < 30.0 {
                    image::Rgb([255, 200, 0])
                } else {
                    image::Rgb([0, 0, 40])
                }
            });
            transcode::encode_upload(&img, ft, &Placement::default()).jpeg
        })
        .collect()
}

#[test]
#[ignore]
fn upload_still_and_animation() {
    let Some((link, id, ft)) = fan() else { return };
    remove_test_files(&link, id);
    let still = link.upload(id, &frames(&ft, 1), "hwtest-still", Some(1)).expect("still upload");
    eprintln!("still: {still:?}");
    assert!(still.listed, "the still picture is not in the playlist");
    // 40 frames = 2 s at the fan's 20 fps: the bar should make one turn every 2 seconds
    let anim = link.upload(id, &frames(&ft, 40), "hwtest-anim", None).expect("animation upload");
    eprintln!("animation: {anim:?}");
    assert!(anim.listed, "the animation is not in the playlist");
    let slot = link.device(id).unwrap().files.iter().find(|(_, n)| n == "hwtest-anim.mp4").map(|(s, _)| *s).unwrap();
    link.select(id, slot).unwrap();
    eprintln!("look at the fan: a yellow bar should turn once every 2 seconds, for 10 s");
    std::thread::sleep(Duration::from_secs(10));
    remove_test_files(&link, id);
}

/// Delete every hwtest-* file (also ones left by an earlier, failed run).
fn remove_test_files(link: &Link, id: u32) {
    for _ in 0..20 {
        let Some(s) = link.device(id).unwrap().files.iter().find(|(_, n)| n.starts_with("hwtest-")).map(|(s, _)| *s) else { return };
        link.delete(id, s).unwrap();
        std::thread::sleep(Duration::from_millis(1500));
    }
}

#[test]
#[ignore]
fn cast_frames_are_acknowledged() {
    let Some((link, id, ft)) = fan() else { return };
    let test = transcode::encode(&transcode::test_card(), &ft, &Placement::default()).jpeg;
    let acked = (0..50).filter(|_| link.cast_to(&[id], &test) == 1).count();
    eprintln!("{acked}/50 cast frames acknowledged");
    assert!(acked >= 45, "only {acked} of 50 cast frames were acknowledged");
}

/// How the fan answers the two chunks of a cast frame (for diagnosis): per frame, whether and
/// after how long it reacted to each chunk.
#[test]
#[ignore]
fn cast_chunk_answers() {
    let Some((link, id, ft)) = fan() else { return };
    let power = link.device(id).and_then(|d| d.status).is_some_and(|s| s.power);
    eprintln!("fan is {}", if power { "on" } else { "off: turning it on" });
    if !power {
        link.on(id).unwrap();
    }
    let test = transcode::encode(&transcode::test_card(), &ft, &Placement::default()).jpeg;
    let frame = p56_core::protocol::cast_frame(&test).unwrap();
    for n in 0..10 {
        let mut line = format!("frame {n}:");
        for (i, chunk) in frame.chunks(p56_core::protocol::CAST_CHUNK).enumerate() {
            let t = std::time::Instant::now();
            let answered = link.send_wait(id, chunk, Duration::from_millis(3500)).unwrap();
            line += &format!("  chunk {i} {} after {} ms", if answered { "answered" } else { "NO answer" }, t.elapsed().as_millis());
        }
        eprintln!("{line}");
    }
}

/// Every packet the fan sends while a still picture is uploaded, with the time (for diagnosis).
#[test]
#[ignore]
fn upload_trace() {
    let Some((link, id, ft)) = fan() else { return };
    let d = link.device(id).unwrap();
    eprintln!("before: status {:?}\nfiles {:?}", d.status, d.files);
    let start = link.packet_seq(id);
    let t0 = std::time::Instant::now();
    let watch = {
        let link = link.clone();
        std::thread::spawn(move || {
            let mut n = 1;
            while t0.elapsed() < Duration::from_secs(25) {
                if let Some(p) = link.wait_nth(id, start, n, Duration::from_millis(500), |_| true) {
                    eprintln!("{:>6} ms  {p:?}", t0.elapsed().as_millis());
                    n += 1;
                }
            }
        })
    };
    let r = link.upload(id, &frames(&ft, 1), "hwtest-trace", None);
    eprintln!("{:>6} ms  upload returned {r:?}", t0.elapsed().as_millis());
    watch.join().unwrap();
    let d = link.device(id).unwrap();
    eprintln!("after: files {:?}", d.files);
    remove_test_files(&link, id);
}

/// Live cast as Holoscope.exe starts it: `5A 26 26 F5`, wait for the fan's answer, then
/// stream the test card for 10 s. Prints every packet the fan sends after the 0x26.
#[test]
#[ignore]
fn cast_after_start_command() {
    let Some((link, id, ft)) = fan() else { return };
    let start = link.packet_seq(id);
    let t0 = std::time::Instant::now();
    link.send_to(id, &[0x5A, 0x26, 0x26, 0xF5]).unwrap();
    let mut n = 1;
    while t0.elapsed() < Duration::from_secs(3) {
        if let Some(p) = link.wait_nth(id, start, n, Duration::from_millis(300), |_| true) {
            eprintln!("{:>6} ms  {p:?}", t0.elapsed().as_millis());
            n += 1;
        }
    }
    let test = transcode::encode(&transcode::test_card(), &ft, &Placement::default()).jpeg;
    eprintln!("look at the fan: the test card for 10 s");
    let t1 = std::time::Instant::now();
    let mut frames = 0;
    while t1.elapsed() < Duration::from_secs(10) {
        frames += link.cast_to(&[id], &test);
        std::thread::sleep(Duration::from_millis(50));
    }
    eprintln!("{frames} frames acknowledged");
}

/// The test card cast live for 10 s at ~15 fps (the link starts live mode by itself).
#[test]
#[ignore]
fn cast_live() {
    let Some((link, id, ft)) = fan() else { return };
    let test = transcode::encode(&transcode::test_card(), &ft, &Placement::default()).jpeg;
    eprintln!("look at the fan: the test card for 10 s");
    let t = std::time::Instant::now();
    let mut sent = 0;
    while t.elapsed() < Duration::from_secs(10) {
        let f = std::time::Instant::now();
        sent += link.cast_to(&[id], &test);
        if let Some(rest) = Duration::from_millis(66).checked_sub(f.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    eprintln!("{sent} frames sent in 10 s");
    assert!(sent >= 100, "only {sent} frames went out");
}

/// Where does a long video fail? Three uploads, each shown for 15 s: a red bar (160 frames,
/// silent), green (160 frames with a tone) and blue (500 frames, silent, ~14 MB). Detailed
/// pictures, so each JPEG is about as large as one from a real video.
#[test]
#[ignore]
fn upload_long_video() {
    let Some((link, id, ft)) = fan() else { return };
    remove_test_files(&link, id);
    let noisy = |n: usize, color: [u8; 3]| -> Vec<Vec<u8>> {
        let mut seed = 12345u32;
        (0..n)
            .map(|i| {
                let a = i as f64 / 40.0 * std::f64::consts::TAU;
                let img = image::RgbImage::from_fn(720, 720, |x, y| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let (dx, dy) = (x as f64 - 360.0, y as f64 - 360.0);
                    let along = dx * a.cos() + dy * a.sin();
                    let across = -dx * a.sin() + dy * a.cos();
                    if along > 0.0 && along < 340.0 && across.abs() < 40.0 {
                        image::Rgb(color)
                    } else {
                        let v = (seed >> 24) as u8 / 3;
                        image::Rgb([v, v, v])
                    }
                });
                transcode::encode_upload(&img, &ft, &Placement::default()).jpeg
            })
            .collect()
    };
    let tone: Vec<u8> = (0..160 * 800).flat_map(|i| (((i as f64 * 440.0 / 16000.0 * std::f64::consts::TAU).sin() * 8000.0) as i16).to_le_bytes()).collect();
    let runs: [(&str, usize, [u8; 3], bool); 3] =
        [("hwtest-red-160", 160, [255, 0, 0], false), ("hwtest-green-160-sound", 160, [0, 255, 0], true), ("hwtest-blue-500", 500, [0, 80, 255], false)];
    for (name, n, color, sound) in runs {
        let frames = noisy(n, color);
        let bytes: usize = frames.iter().map(|f| f.len() + 1604).sum();
        let t = std::time::Instant::now();
        let r = link.upload_with_audio(id, &frames, if sound { Some(&tone) } else { None }, name, None);
        eprintln!("{name}: {n} frames, {:.1} MB, {:.0} s → {r:?}", bytes as f64 / 1e6, t.elapsed().as_secs_f64());
        if let Some(slot) = link.device(id).unwrap().files.iter().find(|(_, f)| f == &format!("{name}.mp4")).map(|(s, _)| *s) {
            link.select(id, slot).unwrap();
            eprintln!("  showing {name} (slot {slot}) for 15 s");
            std::thread::sleep(Duration::from_secs(15));
        }
    }
    remove_test_files(&link, id);
}

/// Find the largest file the fan stores: detailed frames (~54 kB) in growing numbers; prints the
/// size, the time, whether the fan listed it, and what the fan sent besides status reports.
#[test]
#[ignore]
fn upload_size_limit() {
    let Some((link, id, ft)) = fan() else { return };
    remove_test_files(&link, id);
    let mut seed = 777u32;
    let frame = |i: usize, seed: &mut u32| {
        let img = image::RgbImage::from_fn(720, 720, |x, _| {
            *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let v = (*seed >> 24) as u8 / 3;
            if (x as usize + i * 9) % 200 < 30 {
                image::Rgb([255, 120, 0])
            } else {
                image::Rgb([v, v, v])
            }
        });
        transcode::encode_upload(&img, &ft, &Placement::default()).jpeg
    };
    let counts: Vec<usize> = std::env::var("P56_COUNTS").ok().map(|s| s.split(',').filter_map(|n| n.parse().ok()).collect()).unwrap_or(vec![40, 80, 120]);
    for n in counts {
        let frames: Vec<Vec<u8>> = (0..n).map(|i| frame(i, &mut seed)).collect();
        let bytes: usize = frames.iter().map(|f| f.len() + 1604).sum();
        let name = format!("hwtest-size-{n}");
        let start = link.packet_seq(id);
        let t = std::time::Instant::now();
        let r = link.upload(id, &frames, &name, None);
        let secs = t.elapsed().as_secs_f64();
        let mut other = Vec::new();
        let mut k = 1;
        while let Some(p) = link.wait_nth(id, start, k, Duration::ZERO, |_| true) {
            if !matches!(
                p,
                p56_core::protocol::Packet::Status(_)
                    | p56_core::protocol::Packet::Bluetooth(_)
                    | p56_core::protocol::Packet::MachineInfo(_)
                    | p56_core::protocol::Packet::FileList(_)
            ) {
                other.push(format!("{p:?}"));
            }
            k += 1;
        }
        eprintln!("{n} frames, {:.2} MB, {secs:.0} s → {r:?}; fan sent: {}", bytes as f64 / 1e6, other.join(", "));
    }
    remove_test_files(&link, id);
}

/// The largest JPEG per frame the fan accepts: 10-frame files with more and more detail.
#[test]
#[ignore]
fn upload_frame_size_limit() {
    let Some((link, id, ft)) = fan() else { return };
    remove_test_files(&link, id);
    let amps: Vec<u32> = std::env::var("P56_AMPS").ok().map(|s| s.split(',').filter_map(|n| n.parse().ok()).collect()).unwrap_or(vec![8, 16, 24, 32, 48, 64]);
    for amp in amps {
        let mut seed = 99u32;
        let frames: Vec<Vec<u8>> = (0..10)
            .map(|i| {
                let img = image::RgbImage::from_fn(720, 720, |x, _| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let v = ((seed >> 24) * amp / 255) as u8;
                    if (x as usize + i * 9) % 200 < 30 {
                        image::Rgb([255, 120, 0])
                    } else {
                        image::Rgb([v, v, v])
                    }
                });
                transcode::encode_upload(&img, &ft, &Placement::default()).jpeg
            })
            .collect();
        let max = frames.iter().map(|f| f.len()).max().unwrap();
        let name = format!("hwtest-jpeg-{amp}");
        let r = link.upload(id, &frames, &name, None);
        eprintln!("noise {amp}: largest JPEG {:.1} kB → {r:?}", max as f64 / 1024.0);
        std::thread::sleep(Duration::from_millis(500));
    }
    remove_test_files(&link, id);
}

/// Cancel a transfer the fan may still be waiting for (5A 10 10 F5), then upload a small still.
#[test]
#[ignore]
fn cancel_then_upload() {
    let Some((link, id, ft)) = fan() else { return };
    link.send_to(id, &p56_core::protocol::trans_cancel()).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let r = link.upload(id, &frames(&ft, 1), "hwtest-after-cancel", None);
    eprintln!("after cancel: {r:?}; files {:?}", link.device(id).unwrap().files);
    remove_test_files(&link, id);
}

/// Every change in the fan's status (and every other packet) during and after a ~15 MB upload,
/// to find the "file stored" signal.
#[test]
#[ignore]
fn upload_status_trace() {
    let Some((link, id, ft)) = fan() else { return };
    let mut seed = 5u32;
    let frames: Vec<Vec<u8>> = (0..260)
        .map(|i| {
            let img = image::RgbImage::from_fn(720, 720, |x, _| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let v = (seed >> 24) as u8 / 3;
                if (x as usize + i * 9) % 200 < 30 {
                    image::Rgb([0, 200, 255])
                } else {
                    image::Rgb([v, v, v])
                }
            });
            transcode::encode_upload(&img, &ft, &Placement::default()).jpeg
        })
        .collect();
    let start = link.packet_seq(id);
    let t0 = std::time::Instant::now();
    let watch = {
        let link = link.clone();
        std::thread::spawn(move || {
            let (mut n, mut last) = (1, String::new());
            while t0.elapsed() < Duration::from_secs(90) {
                if let Some(p) = link.wait_nth(id, start, n, Duration::from_millis(500), |_| true) {
                    n += 1;
                    let line = match &p {
                        p56_core::protocol::Packet::Status(s) => {
                            format!("status flags {:#04x} power {} playing {} slot {} extra {}", s.flags, s.power, s.playing, s.play_index, s.extra)
                        }
                        p56_core::protocol::Packet::Bluetooth(_) => continue,
                        other => format!("{other:?}").chars().take(120).collect(),
                    };
                    // every status as a dot, so silence during the upload shows
                    if line.starts_with("status") && line == last {
                        eprint!(".");
                    } else {
                        eprintln!(
                            "
{:>6} ms  {line}",
                            t0.elapsed().as_millis()
                        );
                        last = line;
                    }
                }
            }
        })
    };
    let r = link.upload(id, &frames, "hwtest-status", None);
    eprintln!("{:>6} ms  upload returned {r:?}", t0.elapsed().as_millis());
    watch.join().unwrap();
    remove_test_files(&link, id);
}

/// Upload as from Linux (Home Assistant): a relay between the fan and the link takes everything
/// the link writes at once and passes it on at the fan's pace, like a socket send buffer of many
/// megabytes, so the link is done writing long before the fan has the data. The file must still
/// be stored, and the card must still take uploads afterwards.
#[test]
#[ignore]
fn upload_through_a_big_send_buffer() {
    if std::env::var("P56_HARDWARE").is_err() {
        return;
    }
    // the relay listens on the fan's port 6666 and connects to the link on 6667
    let relay = std::net::TcpListener::bind(("0.0.0.0", 6666)).expect("port 6666 free");
    std::thread::spawn(move || {
        for fan in relay.incoming().flatten() {
            let Ok(link) = std::net::TcpStream::connect(("127.0.0.1", 6667)) else { continue };
            let (mut fan_r, mut link_w) = (fan.try_clone().unwrap(), link.try_clone().unwrap());
            std::thread::spawn(move || std::io::copy(&mut fan_r, &mut link_w));
            let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
            let mut link_r = link;
            std::thread::spawn(move || {
                let mut buf = vec![0u8; 1 << 16];
                while let Ok(n) = std::io::Read::read(&mut link_r, &mut buf) {
                    if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
            let mut fan_w = fan;
            std::thread::spawn(move || {
                for b in rx {
                    if std::io::Write::write_all(&mut fan_w, &b).is_err() {
                        break;
                    }
                }
            });
        }
    });
    let targets = std::env::var("P56_FAN").ok().and_then(|ip| format!("{ip}:8988").parse().ok()).into_iter().collect();
    let (link, _rx) = Link::start(Config { targets, tcp_port: 6667, ..Default::default() }).unwrap();
    assert!(link.wait_connected(Duration::from_secs(30)), "no fan through the relay");
    let id = link.ids()[0];
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while link.device(id).and_then(|d| d.status).is_none() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(100));
    }
    let ft = FanType::usable(link.device(id).unwrap().status.unwrap().fan_type).unwrap();
    let mut seed = 31u32;
    let frames: Vec<Vec<u8>> = (0..120)
        .map(|i| {
            let img = image::RgbImage::from_fn(720, 720, |x, _| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let v = (seed >> 24) as u8 / 3;
                if (x as usize + i * 9) % 200 < 30 {
                    image::Rgb([255, 0, 200])
                } else {
                    image::Rgb([v, v, v])
                }
            });
            transcode::encode_upload(&img, &ft, &Placement::default()).jpeg
        })
        .collect();
    let t = std::time::Instant::now();
    let r = link.upload(id, &frames, "hwtest-buffered", None);
    eprintln!("buffered upload: {:.0} s → {r:?}", t.elapsed().as_secs_f64());
    let small = link.upload(id, &frames[..1], "hwtest-after", None);
    eprintln!("small upload afterwards: {small:?}; files {:?}", link.device(id).unwrap().files);
    remove_test_files(&link, id);
    link.shutdown();
}
