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
