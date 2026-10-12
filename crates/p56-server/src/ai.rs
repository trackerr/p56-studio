//! AI pictures through Home Assistant's AI Task (only in the add-on): a prompt becomes a still,
//! a turning picture or a short animation for the fan. Which AI makes them (Google, OpenAI, …)
//! is whatever the user set up in Home Assistant; no key or account is kept here.

use std::time::Duration;

use image::RgbImage;
use p56_core::transcode;
use serde_json::{json, Value};

/// `AITaskEntityFeature.GENERATE_IMAGE` in Home Assistant.
const GENERATE_IMAGE: u64 = 4;
/// How long one picture may take (the AI services answer in 5–30 s).
const GENERATE_TIMEOUT: Duration = Duration::from_secs(120);
/// Frames of a turning picture: one turn in 3.6 s.
const SPIN_FRAMES: u32 = 36;
const SPIN_MS: u32 = 100;
/// A drawn animation: a 4×4 sheet of frames.
const SHEET: u32 = 4;
const SHEET_MS: u32 = 125;

/// What to make of the prompt.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// One picture.
    Still,
    /// One picture, turned round by the server.
    Spin,
    /// 16 frames drawn by the AI (a sprite sheet).
    Anim,
}

impl Kind {
    pub fn parse(s: &str) -> Result<Kind, String> {
        match s {
            "" | "still" => Ok(Kind::Still),
            "spin" => Ok(Kind::Spin),
            "anim" | "animation" => Ok(Kind::Anim),
            _ => Err(format!("unknown kind '{s}' (still, spin or anim)")),
        }
    }
}

/// How a picture looks good on the fan, added to every prompt.
const LOOK: &str = "made for a round holographic fan display (a spinning LED blade: black shows as \
transparent, only bright pixels are seen). Pure solid black background (#000000) all around the \
subject, no floor, no shadow, no frame, no border, no text, letters or numbers. One bold subject, \
centred, filling most of the space, bright saturated colours, thick shapes, high contrast, little \
fine detail.";

/// The instructions sent to the AI for the user's prompt.
pub fn instructions(prompt: &str, kind: Kind) -> String {
    let prompt = prompt.trim();
    match kind {
        Kind::Still | Kind::Spin => format!("A square image {LOOK}\nSubject: {prompt}"),
        Kind::Anim => format!(
            "A square sprite sheet: a {SHEET} by {SHEET} grid of {} equal square cells, the frames of one short \
             looping animation in reading order (left to right, top to bottom), the last frame leading back into \
             the first. Every cell shows the same subject at the same size and position; only the motion changes. \
             No lines, gaps or numbers between the cells. Each cell is {LOOK}\nSubject: {prompt}",
            SHEET * SHEET
        ),
    }
}

/// The AI Task entities that can make pictures: `[{entity_id, name}]`.
pub fn entities(base: &str, token: &str) -> Result<Vec<Value>, String> {
    let states = crate::http_get(&format!("{base}/states"), Some(token), "application/json").and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let states = states.as_ref().and_then(|v| v.as_array()).ok_or("Home Assistant did not list its entities (is homeassistant_api on?)")?;
    Ok(states
        .iter()
        .filter(|e| e["attributes"]["supported_features"].as_u64().is_some_and(|f| f & GENERATE_IMAGE != 0))
        .filter_map(|e| {
            let id = e["entity_id"].as_str().filter(|id| id.starts_with("ai_task."))?;
            Some(json!({ "entity_id": id, "name": e["attributes"]["friendly_name"].as_str().unwrap_or(id) }))
        })
        .collect())
}

/// Ask the AI for a picture and turn it into frames for the fan: (pictures, ms per frame).
pub fn generate(base: &str, token: &str, entity: Option<&str>, prompt: &str, kind: Kind) -> Result<(Vec<RgbImage>, u32), String> {
    if prompt.trim().is_empty() {
        return Err("describe the picture first".into());
    }
    let entity = match entity.filter(|e| !e.is_empty()) {
        Some(e) if e.starts_with("ai_task.") && e[8..].chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') => e.to_string(),
        Some(e) => return Err(format!("not an AI Task entity: {e}")),
        None => entities(base, token)?
            .first()
            .and_then(|e| e["entity_id"].as_str().map(String::from))
            .ok_or("no AI in Home Assistant can make pictures: add one (e.g. Google Generative AI) and its AI Task")?,
    };
    let body = json!({ "task_name": "P56 picture", "entity_id": entity, "instructions": instructions(prompt, kind) });
    let url = format!("{base}/services/ai_task/generate_image?return_response");
    let (code, answer) =
        crate::http_request("POST", &url, Some(token), "application/json", Some(&serde_json::to_vec(&body).unwrap_or_default()), GENERATE_TIMEOUT)?;
    let answer: Value = serde_json::from_slice(&answer).unwrap_or(Value::Null);
    if code != 200 {
        let msg = answer["message"].as_str().map(String::from).unwrap_or_else(|| format!("HTTP {code}"));
        return Err(format!("the AI made no picture: {msg}"));
    }
    let res = &answer["service_response"];
    let path = res["url"].as_str().ok_or("the AI answered without a picture")?;
    let bytes = fetch_picture(base, token, path)?;
    let img = transcode::decode_rgb(&bytes).map_err(|e| format!("cannot read the AI's picture: {e}"))?;
    log::info!("AI picture from {entity}: {}×{}", img.width(), img.height());
    Ok(match kind {
        Kind::Still => (vec![transcode::fit(img, p56_service::MAX_SIDE)], 0),
        Kind::Spin => (spin(&square(img, 480), SPIN_FRAMES), SPIN_MS),
        Kind::Anim => (sheet(&img, SHEET), SHEET_MS),
    })
}

/// The picture behind the AI's (signed) URL. Home Assistant serves it outside its REST API, so
/// it is fetched from Home Assistant itself: the add-on runs on the host network.
fn fetch_picture(base: &str, token: &str, path: &str) -> Result<Vec<u8>, String> {
    if path.starts_with("http://") {
        return crate::http_get(path, Some(token), "image/*").ok_or_else(|| format!("cannot fetch {path}"));
    }
    if !path.starts_with('/') {
        return Err(format!("unexpected picture address: {path}"));
    }
    let mut roots: Vec<String> = std::env::var("P56_HA_WEB").into_iter().collect();
    // P56_HA_URL in tests points at the fake Home Assistant itself
    if std::env::var("P56_HA_URL").is_ok() {
        roots.push(base.to_string());
    }
    roots.extend(["http://homeassistant:8123".to_string(), "http://127.0.0.1:8123".to_string()]);
    for root in &roots {
        if let Some(b) = crate::http_get(&format!("{}{path}", root.trim_end_matches('/')), None, "image/*") {
            return Ok(b);
        }
    }
    Err("the AI made a picture, but it could not be fetched from Home Assistant (port 8123, plain http)".into())
}

/// The middle square of a picture, `side` pixels wide.
fn square(img: RgbImage, side: u32) -> RgbImage {
    let s = img.width().min(img.height());
    let sq = image::imageops::crop_imm(&img, (img.width() - s) / 2, (img.height() - s) / 2, s, s).to_image();
    image::imageops::resize(&sq, side, side, image::imageops::FilterType::Triangle)
}

/// `n` frames of the picture turning once round its middle (clockwise).
fn spin(img: &RgbImage, n: u32) -> Vec<RgbImage> {
    let (w, h) = img.dimensions();
    let (cx, cy) = ((w as f32 - 1.0) / 2.0, (h as f32 - 1.0) / 2.0);
    (0..n)
        .map(|i| {
            let (s, c) = (std::f32::consts::TAU * i as f32 / n as f32).sin_cos();
            RgbImage::from_fn(w, h, |x, y| {
                // where this pixel comes from in the unturned picture
                let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                let (sx, sy) = (c * dx + s * dy + cx, -s * dx + c * dy + cy);
                bilinear(img, sx, sy)
            })
        })
        .collect()
}

fn bilinear(img: &RgbImage, x: f32, y: f32) -> image::Rgb<u8> {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let (x0, y0) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |x: i64, y: i64| if x < 0 || y < 0 || x >= w || y >= h { [0u8; 3] } else { img.get_pixel(x as u32, y as u32).0 };
    let (a, b, c, d) = (px(x0, y0), px(x0 + 1, y0), px(x0, y0 + 1), px(x0 + 1, y0 + 1));
    image::Rgb(std::array::from_fn(|i| {
        let top = a[i] as f32 * (1.0 - fx) + b[i] as f32 * fx;
        let bottom = c[i] as f32 * (1.0 - fx) + d[i] as f32 * fx;
        (top * (1.0 - fy) + bottom * fy).round() as u8
    }))
}

/// The frames of an `n`×`n` sprite sheet, in reading order. A small margin of each cell is left
/// out, so a thin line the AI drew between the cells does not flicker on the fan.
fn sheet(img: &RgbImage, n: u32) -> Vec<RgbImage> {
    let (cw, ch) = (img.width() / n, img.height() / n);
    let (mx, my) = (cw / 25, ch / 25);
    (0..n * n)
        .map(|i| {
            let (col, row) = (i % n, i / n);
            image::imageops::crop_imm(img, col * cw + mx, row * ch + my, cw - 2 * mx, ch - 2 * my).to_image()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_instructions() {
        assert_eq!(Kind::parse("").unwrap(), Kind::Still);
        assert_eq!(Kind::parse("anim").unwrap(), Kind::Anim);
        assert!(Kind::parse("video").is_err());
        let i = instructions("  a red fox ", Kind::Anim);
        assert!(i.contains("4 by 4") && i.ends_with("Subject: a red fox"), "{i}");
        assert!(instructions("fox", Kind::Spin).contains("black"));
    }

    #[test]
    fn a_turn_ends_where_it_started_and_a_sheet_has_sixteen_frames() {
        let img = RgbImage::from_fn(40, 40, |x, _| image::Rgb([if x < 20 { 255 } else { 0 }, 0, 0]));
        let frames = spin(&img, 4);
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0], img, "the first frame is the picture itself");
        // a quarter turn clockwise: the red left half is now the top half
        assert!(frames[1].get_pixel(20, 5).0[0] > 200 && frames[1].get_pixel(20, 35).0[0] < 50);
        let s = sheet(&RgbImage::new(400, 400), 4);
        assert_eq!((s.len(), s[0].width()), (16, 92));
    }
}
