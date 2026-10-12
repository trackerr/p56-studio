//! AI pictures through Home Assistant's AI Task (only in the add-on): a prompt becomes a still,
//! an object turning round its own axis, or a short animation for the fan. Which AI makes them (Google, OpenAI, …)
//! is whatever the user set up in Home Assistant; no key or account is kept here.

use std::time::Duration;

use image::RgbImage;
use p56_core::transcode;
use serde_json::{json, Value};

/// `AITaskEntityFeature.GENERATE_IMAGE` in Home Assistant.
const GENERATE_IMAGE: u64 = 4;
/// How long one picture may take (the AI services answer in 5–30 s).
const GENERATE_TIMEOUT: Duration = Duration::from_secs(120);
/// Frames of a turning object: one turn round its own upright axis in 2.4 s.
const SPIN_FRAMES: u32 = 24;
const SPIN_MS: u32 = 100;
/// A drawn animation: a 4×4 sheet of frames.
const SHEET: u32 = 4;
const SHEET_MS: u32 = 125;

/// What to make of the prompt.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// One picture.
    Still,
    /// One picture, turned round its own upright axis by the server, as on a turntable (the AI
    /// cannot draw the views of a turn consistently).
    Spin,
    /// 16 frames drawn by the AI (a sprite sheet).
    Anim,
    /// A background for text written over it (`show_ai` with `text`): calm and dark in the middle.
    Backdrop,
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

/// What the fan needs, added to every prompt. Only technical: the style is the user's.
const LOOK: &str = "made for a round holographic fan display (a spinning LED blade: black shows as \
transparent and only lit pixels are seen, so the subject itself is well lit). Pure black background \
(#000000) all around the subject and up to every edge of the image, no white or coloured margin, no \
floor, no frame, no border, no text, letters or numbers unless the subject asks for them. The \
subject in the middle, filling most of the space.";

/// Ready-made styles (`art_style`); any other text is used as the style itself.
const STYLES: [(&str, &str); 10] = [
    ("photo", "a realistic photograph"),
    ("3d", "a detailed 3D render"),
    ("cartoon", "a cartoon"),
    ("anime", "anime"),
    ("painting", "an oil painting"),
    ("watercolor", "a watercolour painting"),
    ("pixel", "pixel art"),
    ("neon", "glowing neon lines"),
    ("line", "clean line art"),
    ("comic", "a comic book drawing"),
];

/// The style line of the instructions: a ready-made style, the user's own words, or nothing
/// (the AI follows the prompt).
fn style_line(style: Option<&str>) -> String {
    match style.map(str::trim).filter(|s| !s.is_empty() && *s != "free") {
        Some(s) => {
            let words = STYLES.iter().find(|(k, _)| k.eq_ignore_ascii_case(s)).map_or_else(|| s.chars().take(80).collect(), |(_, w)| w.to_string());
            format!("\nStyle: {words}")
        }
        None => String::new(),
    }
}

/// The instructions sent to the AI for the user's prompt.
pub fn instructions(prompt: &str, kind: Kind, style: Option<&str>) -> String {
    let prompt = prompt.trim();
    let style = style_line(style);
    let body = match kind {
        Kind::Still => format!("A square image {LOOK}\nSubject: {prompt}"),
        Kind::Spin => format!("A square image, the subject seen straight from the front, {LOOK}\nSubject: {prompt}"),
        Kind::Backdrop => format!(
            "A square background picture made for a round holographic fan display (a spinning LED blade: black \
             shows as transparent, only bright pixels are seen). Large text will be written over its middle, so \
             the middle stays dark and calm; the theme is shown around the edge, like a ring or a frame. Keep it \
             simple and not busy, so the text stays the main thing: a few bold, clean shapes in bright colours, \
             little fine detail. Pure black elsewhere, up to every edge of the image, no white margin, no text, \
             letters or numbers.\nTheme: {prompt}"
        ),
        Kind::Anim => format!(
            "A square sprite sheet that fills the whole image on pure black, with no white or coloured margin, \
             frame or border around it: a {SHEET} by {SHEET} grid of {} equal square cells with a little black space \
             between them and no lines or numbers. The cells are the frames of one short looping animation in \
             reading order (left to right, top to bottom), the last frame leading back into the first. Every frame \
             is a clearly different moment of the motion (for example legs, wings, arms or the body in a new \
             position), so that played quickly one after another the subject visibly moves; never the same \
             picture repeated. The subject stays in the middle of its cell at the same size and never travels \
             across it: an object turns once around its own upright axis over the {} frames, as on a turntable; \
             a creature or figure moves on the spot. Each cell is {LOOK}\nSubject: {prompt}",
            SHEET * SHEET,
            SHEET * SHEET
        ),
    };
    body + &style
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

/// Pictures made before, by entity and instructions: an automation that shows the same prompt
/// again (e.g. a price background every quarter of an hour) does not pay for a new picture.
static MADE: std::sync::Mutex<Vec<(String, Vec<RgbImage>, u32)>> = std::sync::Mutex::new(Vec::new());
const MADE_KEPT: usize = 8;

/// Ask the AI for a picture and turn it into frames for the fan: (pictures, ms per frame). The
/// same prompt again gives the same picture, unless `fresh`.
pub fn generate(
    base: &str, token: &str, entity: Option<&str>, prompt: &str, kind: Kind, style: Option<&str>, fresh: bool,
) -> Result<(Vec<RgbImage>, u32), String> {
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
    let instructions = instructions(prompt, kind, style);
    let key = format!("{entity}\n{instructions}");
    if !fresh {
        if let Some((_, images, ms)) = MADE.lock().unwrap().iter().find(|(k, _, _)| *k == key) {
            log::info!("AI picture from {entity}: made before");
            return Ok((images.clone(), *ms));
        }
    }
    let body = json!({ "task_name": "P56 picture", "entity_id": entity, "instructions": instructions });
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
    let mut img = transcode::decode_rgb(&bytes).map_err(|e| format!("cannot read the AI's picture: {e}"))?;
    blacken_background(&mut img);
    log::info!("AI picture from {entity}: {}×{}", img.width(), img.height());
    let made = match kind {
        Kind::Still | Kind::Backdrop => (vec![transcode::fit(img, p56_service::MAX_SIDE)], 0),
        Kind::Spin => {
            // turned round the middle of the object itself, not of the picture
            let sq = square(img, 480);
            let centred = lit_middle(&sq).map_or(sq.clone(), |(mx, my)| cut(&sq, mx, my, 480, 480));
            (turntable(&centred, SPIN_FRAMES), SPIN_MS)
        }
        Kind::Anim => (sheet(&img, SHEET), SHEET_MS),
    };
    let mut kept = MADE.lock().unwrap();
    kept.retain(|(k, _, _)| *k != key);
    if kept.len() >= MADE_KEPT {
        kept.remove(0);
    }
    kept.push((key, made.0.clone(), made.1));
    Ok(made)
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

/// `n` frames of the object turning once round its own upright axis, as on a turntable: it
/// narrows to its edge, shows its (mirrored) back and widens again, a little darker while turned
/// sideways. The picture itself never tilts, so the fan does not seem to twist.
fn turntable(img: &RgbImage, n: u32) -> Vec<RgbImage> {
    let (w, h) = img.dimensions();
    let cx = (w as f32 - 1.0) / 2.0;
    (0..n)
        .map(|i| {
            let c = (std::f32::consts::TAU * i as f32 / n as f32).cos();
            let (width, shade) = (c.abs().max(0.03), 0.55 + 0.45 * c.abs());
            RgbImage::from_fn(w, h, |x, y| {
                // where this pixel comes from in the front view (mirrored when seen from behind)
                let sx = (x as f32 - cx) / width * c.signum() + cx;
                if sx < 0.0 || sx > w as f32 - 1.0 {
                    return image::Rgb([0, 0, 0]);
                }
                let (x0, f) = (sx.floor() as u32, sx.fract());
                let (a, b) = (img.get_pixel(x0, y).0, img.get_pixel((x0 + 1).min(w - 1), y).0);
                image::Rgb(std::array::from_fn(|k| ((a[k] as f32 * (1.0 - f) + b[k] as f32 * f) * shade).round() as u8))
            })
        })
        .collect()
}

/// A light background that reaches the edge (a white margin round the picture, or a white backdrop
/// the AI drew anyway) becomes black: the fan shows black as nothing and white as a lit disc.
fn blacken_background(img: &mut RgbImage) {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let mut todo: Vec<(u32, u32)> = (0..w).flat_map(|x| [(x, 0), (x, h - 1)]).chain((0..h).flat_map(|y| [(0, y), (w - 1, y)])).collect();
    while let Some((x, y)) = todo.pop() {
        if !img.get_pixel(x, y).0.iter().all(|&c| c >= LIGHT) {
            continue;
        }
        img.put_pixel(x, y, image::Rgb([0, 0, 0]));
        todo.extend([(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)].into_iter().filter(|&(x, y)| x < w && y < h));
    }
}

/// Every channel at least this bright counts as white background.
const LIGHT: u8 = 190;
/// A channel above this is a lit pixel.
const LIT: u8 = 48;

/// The frames of an `n`×`n` sprite sheet, in reading order. The AI does not always draw the grid
/// edge to edge, so the cells are found from the dark gaps between them; every frame has the same
/// size and the subject in its middle, so it stays put on the fan even when the AI let it drift.
fn sheet(img: &RgbImage, n: u32) -> Vec<RgbImage> {
    let lit = |p: &image::Rgb<u8>| p.0.iter().any(|&c| c > LIT);
    let mut cols = vec![0u32; img.width() as usize];
    let mut rows = vec![0u32; img.height() as usize];
    for (x, y, p) in img.enumerate_pixels() {
        if lit(p) {
            cols[x as usize] += 1;
            rows[y as usize] += 1;
        }
    }
    let (xs, w) = cell_centres(&cols, n);
    let (ys, h) = cell_centres(&rows, n);
    ys.iter()
        .flat_map(|&cy| xs.iter().map(move |&cx| (cx, cy)))
        .map(|(cx, cy)| {
            let (cx, cy) = (cx as i64, cy as i64);
            let frame = cut(img, cx, cy, w, h);
            match lit_middle(&frame) {
                Some((mx, my)) => cut(img, cx + mx - w as i64 / 2, cy + my - h as i64 / 2, w, h),
                None => frame,
            }
        })
        .collect()
}

/// A `w`×`h` piece of the picture around (cx, cy); outside the picture is black.
fn cut(img: &RgbImage, cx: i64, cy: i64, w: u32, h: u32) -> RgbImage {
    let mut frame = RgbImage::new(w, h);
    image::imageops::overlay(&mut frame, img, w as i64 / 2 - cx, h as i64 / 2 - cy);
    frame
}

/// The middle of the lit part of a picture (of its bounding box), if anything is lit.
fn lit_middle(img: &RgbImage) -> Option<(i64, i64)> {
    let lit: Vec<(u32, u32)> = img.enumerate_pixels().filter(|(_, _, p)| p.0.iter().any(|&c| c > LIT)).map(|(x, y, _)| (x, y)).collect();
    let (x0, x1) = (lit.iter().map(|p| p.0).min()?, lit.iter().map(|p| p.0).max()?);
    let (y0, y1) = (lit.iter().map(|p| p.1).min()?, lit.iter().map(|p| p.1).max()?);
    Some(((x0 + x1).div_ceil(2) as i64, (y0 + y1).div_ceil(2) as i64))
}

/// The centres of `n` cells along one axis and their size, from the lit pixels per column (or
/// row): the borders between cells are the darkest places near where they are expected.
fn cell_centres(profile: &[u32], n: u32) -> (Vec<u32>, u32) {
    let len = profile.len() as u32;
    let first = profile.iter().position(|&c| c > 0).unwrap_or(0) as u32;
    let last = profile.iter().rposition(|&c| c > 0).map_or(len, |i| i as u32 + 1);
    // nearly nothing lit: the whole picture
    let (start, end) = if last > first + 4 * n { (first, last) } else { (0, len) };
    let span = end - start;
    let win = span / (4 * n);
    let peak = profile.iter().copied().max().unwrap_or(0);
    let borders: Vec<u32> = (1..n)
        .map(|k| {
            let expect = start + span * k / n;
            let best = (expect - win..=expect + win).min_by_key(|&i| (profile[i as usize], i.abs_diff(expect))).unwrap_or(expect);
            // a sheet without dark gaps (e.g. falling rain) is simply cut evenly
            if profile[best as usize] > peak / 10 {
                return expect;
            }
            // else the middle of that dark gap (a few stray pixels still count as dark)
            let dark = |i: u32| profile[i as usize] <= profile[best as usize] + 2;
            let (mut a, mut b) = (best, best);
            while a > start && dark(a - 1) {
                a -= 1;
            }
            while b + 1 < end && dark(b + 1) {
                b += 1;
            }
            (a + b) / 2
        })
        .collect();
    // the size of a cell from the inner borders (the outer ones are where the drawing starts)
    let size = match borders.len() {
        0 => span,
        1 => span / 2,
        k => (borders[k - 1] - borders[0]) / (k as u32 - 1),
    }
    .max(1);
    let mut centres: Vec<u32> = borders.iter().map(|&b| (b + size / 2).min(len)).collect();
    centres.insert(0, borders.first().map_or(start + size / 2, |&b| b.saturating_sub(size / 2)));
    (centres, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_instructions() {
        assert_eq!(Kind::parse("").unwrap(), Kind::Still);
        assert_eq!(Kind::parse("anim").unwrap(), Kind::Anim);
        assert!(Kind::parse("video").is_err());
        let i = instructions("  a red fox ", Kind::Anim, None);
        assert!(i.contains("4 by 4") && i.ends_with("Subject: a red fox"), "{i}");
        assert!(instructions("fox", Kind::Spin, None).contains("from the front"));
        // no style of our own: only what the fan needs
        let plain = instructions("fox", Kind::Still, None);
        assert!(plain.contains("black") && !plain.contains("neon") && !plain.contains("Style"), "{plain}");
        assert!(instructions("fox", Kind::Still, Some("photo")).ends_with("Style: a realistic photograph"));
        assert!(instructions("fox", Kind::Still, Some(" Van Gogh ")).ends_with("Style: Van Gogh"));
        assert!(!instructions("fox", Kind::Still, Some("free")).contains("Style"));
        // a background behind text (notifications, prices) stays simple
        assert!(instructions("cheap power", Kind::Backdrop, None).contains("simple and not busy"));
    }

    #[test]
    fn an_object_turns_round_its_own_axis() {
        let img = RgbImage::from_fn(40, 40, |x, _| image::Rgb([if x < 20 { 255 } else { 0 }, 0, 0]));
        let frames = turntable(&img, 4);
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0], img, "the first frame is the front view");
        // a quarter turn: seen from the side, only a thin strip in the middle is left
        assert!((0..40).all(|y| frames[1].get_pixel(2, y).0[0] == 0 && frames[1].get_pixel(37, y).0[0] == 0));
        // half a turn: the back, mirrored (red right), and nothing tilted (every row the same)
        assert!(frames[2].get_pixel(35, 5).0[0] > 200 && frames[2].get_pixel(4, 5).0[0] == 0);
        assert!((0..40).all(|y| frames[2].get_pixel(35, y) == frames[2].get_pixel(35, 0)));
    }

    /// What Gemini drew once: the grid on a black square with a white margin round it, the cells
    /// not edge to edge. Every frame must be the subject in its own cell, at the same place, and
    /// no white may be left.
    #[test]
    fn a_sheet_with_a_white_margin_is_cut_at_its_cells() {
        let mut img = RgbImage::from_pixel(400, 400, image::Rgb([255, 255, 255]));
        // black square 30..370, cells of 85 px with a 20×40 red block in the middle of each,
        // drifting up to 9 px right and 6 px down (the AI letting it wander)
        for y in 30..370 {
            for x in 30..370 {
                img.put_pixel(x, y, image::Rgb([0, 0, 0]));
            }
        }
        for i in 0..16u32 {
            let (cx, cy) = (30 + (i % 4) * 85 + 42 + i % 10, 30 + (i / 4) * 85 + 42 + i % 7);
            for y in cy - 20..cy + 20 {
                for x in cx - 10..cx + 10 {
                    img.put_pixel(x, y, image::Rgb([230, 30, 30]));
                }
            }
        }
        blacken_background(&mut img);
        let frames = sheet(&img, 4);
        assert_eq!(frames.len(), 16);
        let size = frames[0].dimensions();
        for (i, f) in frames.iter().enumerate() {
            assert_eq!(f.dimensions(), size, "frame {i}");
            assert!(!f.pixels().any(|p| p.0.iter().all(|&c| c >= LIGHT)), "white in frame {i}");
            // the block is whole and in the middle of the frame, whatever its drift
            let red: Vec<(u32, u32)> = f.enumerate_pixels().filter(|(_, _, p)| p.0[0] > 200).map(|(x, y, _)| (x, y)).collect();
            assert_eq!(red.len(), 800, "frame {i}: the block is cut");
            let mx = red.iter().map(|p| p.0).sum::<u32>() / 800;
            let my = red.iter().map(|p| p.1).sum::<u32>() / 800;
            assert!(mx.abs_diff(size.0 / 2) <= 1 && my.abs_diff(size.1 / 2) <= 1, "frame {i}: block at {mx},{my} in {size:?}");
        }
        // a sheet without gaps (e.g. falling rain) is cut evenly
        let full = RgbImage::from_fn(400, 400, |x, y| image::Rgb([((y / 100 * 4 + x / 100) * 15 + 10) as u8, 200, 0]));
        let frames = sheet(&full, 4);
        assert_eq!(frames[0].dimensions(), (100, 100));
        for (i, f) in frames.iter().enumerate() {
            assert!(f.pixels().all(|p| p.0[0] == (i * 15 + 10) as u8), "frame {i} is not exactly its cell");
        }
        // a white subject inside the black square is kept
        let mut solo = RgbImage::from_pixel(50, 50, image::Rgb([0, 0, 0]));
        solo.put_pixel(25, 25, image::Rgb([255, 255, 255]));
        blacken_background(&mut solo);
        assert_eq!(solo.get_pixel(25, 25).0, [255, 255, 255]);
    }
}
