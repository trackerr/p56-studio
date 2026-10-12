//! Text → round picture for the fan: words wrapped and sized to fit inside the disc,
//! optionally with an icon (Material Design Icons, as in Home Assistant), an accent ring
//! along the rim and clock ticks.

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::{Rgb, RgbImage};

static FONT: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
static ICON_FONT: &[u8] = include_bytes!("../assets/materialdesignicons.ttf");
static ICON_NAMES: &str = include_str!("../assets/mdi-names.txt");

/// Side of the square picture the text is drawn on (the transcoder maps it onto the disc).
pub const SIDE: u32 = 720;

/// Text drawn on `bg` (a card or ticker frame) laid over a picture: the picture fills the square,
/// dimmed, and is darker still around every letter, so the text stays readable on a busy picture.
pub fn over_backdrop(text: &RgbImage, picture: &RgbImage, bg: [u8; 3]) -> RgbImage {
    let (w, h) = text.dimensions();
    let s = picture.width().min(picture.height());
    let sq = image::imageops::crop_imm(picture, (picture.width() - s) / 2, (picture.height() - s) / 2, s, s).to_image();
    let mut out = image::imageops::resize(&sq, w, h, image::imageops::FilterType::Triangle);
    let ink: Vec<bool> = text.pixels().map(|p| p.0 != bg).collect();
    let near = dilate(&ink, w as usize, h as usize, (w as usize / 90).max(2));
    for (i, (p, t)) in out.pixels_mut().zip(text.pixels()).enumerate() {
        if ink[i] {
            *p = *t;
        } else {
            let f = if near[i] { 0.15 } else { 0.6 };
            p.0 = p.0.map(|c| (c as f32 * f) as u8);
        }
    }
    out
}

/// Every pixel within `r` (a square) of a set one.
fn dilate(set: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let pass = |src: &[bool], along: usize, across: usize, at: &dyn Fn(usize, usize) -> usize| {
        let mut out = vec![false; src.len()];
        for b in 0..across {
            // running count of set pixels in the window [a - r, a + r]
            let mut count = (0..r.min(along)).filter(|&a| src[at(a, b)]).count();
            for a in 0..along {
                if a + r < along && src[at(a + r, b)] {
                    count += 1;
                }
                if a > r && src[at(a - r - 1, b)] {
                    count -= 1;
                }
                out[at(a, b)] = count > 0;
            }
        }
        out
    };
    let rows = pass(set, w, h, &|a, b| b * w + a);
    pass(&rows, h, w, &|a, b| a * w + b)
}

/// Default colours for [`color_scale`]: green → yellow → red.
pub const SCALE_GREEN_RED: [[u8; 3]; 3] = [[48, 209, 88], [255, 214, 10], [255, 59, 48]];

/// Parse `#rgb`, `#rrggbb`, `r,g,b` or a common colour name (English, or Dutch for
/// existing automations).
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().to_ascii_lowercase();
    let named = match s.as_str() {
        "white" | "wit" => Some([255, 255, 255]),
        "black" | "zwart" => Some([0, 0, 0]),
        "red" | "rood" => Some([255, 40, 40]),
        "green" | "groen" => Some([40, 230, 80]),
        "blue" | "blauw" => Some([50, 110, 255]),
        "yellow" | "geel" => Some([255, 220, 0]),
        "orange" | "oranje" => Some([255, 140, 0]),
        "cyan" => Some([0, 220, 255]),
        "magenta" | "paars" | "purple" => Some([200, 60, 255]),
        "pink" | "roze" => Some([255, 110, 180]),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    if let Some(h) = s.strip_prefix('#') {
        let v = u32::from_str_radix(h, 16).ok()?;
        return match h.len() {
            6 => Some([(v >> 16) as u8, (v >> 8) as u8, v as u8]),
            3 => Some([((v >> 8) & 0xF) as u8 * 17, ((v >> 4) & 0xF) as u8 * 17, (v & 0xF) as u8 * 17]),
            _ => None,
        };
    }
    let p: Vec<u8> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (p.len() == 3).then(|| [p[0], p[1], p[2]])
}

/// The glyph of a Material Design Icon, by Home Assistant name (`mdi:flash`) or bare name.
pub fn icon_char(name: &str) -> Option<char> {
    let name = name.trim().trim_start_matches("mdi:").to_ascii_lowercase();
    ICON_NAMES.lines().find_map(|l| {
        let (n, cp) = l.split_once(' ')?;
        (n == name).then(|| u32::from_str_radix(cp, 16).ok().and_then(char::from_u32)).flatten()
    })
}

/// Colour for `value` on a scale from `min` to `max`, blending through `stops`
/// (e.g. green → yellow → red). Values outside the range take the end colours.
pub fn color_scale(value: f64, min: f64, max: f64, stops: &[[u8; 3]]) -> [u8; 3] {
    if stops.is_empty() {
        return [255, 255, 255];
    }
    if stops.len() == 1 || (max - min).abs() < f64::EPSILON {
        return stops[0];
    }
    let t = ((value - min) / (max - min)).clamp(0.0, 1.0) * (stops.len() - 1) as f64;
    let i = (t.floor() as usize).min(stops.len() - 2);
    let f = t - i as f64;
    let (a, b) = (stops[i], stops[i + 1]);
    [0, 1, 2].map(|k| (a[k] as f64 + (b[k] as f64 - a[k] as f64) * f).round() as u8)
}

/// Everything a text card can show.
#[derive(Clone, Debug)]
pub struct Card {
    pub text: String,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    /// Cap for the font size in pixels on the 720 px canvas (0 = as large as fits).
    pub max_size: f32,
    pub icon: Option<char>,
    pub icon_color: [u8; 3],
    /// Accent ring along the rim.
    pub ring: Option<[u8; 3]>,
    /// Twelve clock ticks inside the ring.
    pub ticks: bool,
    /// The ring as a meter: filled this share (0–1) clockwise from the top, the rest dim.
    pub gauge: Option<f32>,
}

impl Card {
    pub fn text(text: &str, fg: [u8; 3], bg: [u8; 3], max_size: f32) -> Card {
        Card { text: text.into(), fg, bg, max_size, icon: None, icon_color: fg, ring: None, ticks: false, gauge: None }
    }
}

fn line_width<F: Font, S: ScaleFont<F>>(font: &S, line: &str) -> f32 {
    let mut w = 0.0;
    let mut prev = None;
    for c in line.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = prev {
            w += font.kern(p, id);
        }
        w += font.h_advance(id);
        prev = Some(id);
    }
    w
}

/// Greedy word wrap for `max_w` pixels; explicit newlines are kept.
fn wrap<F: Font, S: ScaleFont<F>>(font: &S, text: &str, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.lines() {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if line_width(font, &cand) <= max_w || cur.is_empty() {
                cur = cand;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        lines.push(cur);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Largest font size (≤ `start`) at which `text` wraps into a `box_w` × `box_h` box.
fn fit(font: &FontRef, text: &str, box_w: f32, box_h: f32, start: f32) -> (Vec<String>, f32) {
    let mut size = start;
    loop {
        let sf = font.as_scaled(PxScale::from(size));
        let lines = wrap(&sf, text, box_w);
        let widest = lines.iter().map(|l| line_width(&sf, l)).fold(0.0, f32::max);
        let height = lines.len() as f32 * (sf.ascent() - sf.descent() + sf.line_gap()) - sf.line_gap();
        if (widest <= box_w && height <= box_h) || size <= 10.0 {
            return (lines, size);
        }
        size *= 0.92;
    }
}

fn blend(img: &mut RgbImage, x: i32, y: i32, c: [u8; 3], cov: f32) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 || cov <= 0.0 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    let cov = cov.min(1.0);
    for k in 0..3 {
        p[k] = (p[k] as f32 * (1.0 - cov) + c[k] as f32 * cov).round() as u8;
    }
}

fn draw_glyph(img: &mut RgbImage, font: &FontRef, glyph: ab_glyph::Glyph, c: [u8; 3]) {
    if let Some(outline) = font.outline_glyph(glyph) {
        let b = outline.px_bounds();
        outline.draw(|gx, gy, cov| blend(img, b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32, c, cov));
    }
}

/// Draw centred lines starting with the top of the first line at `top`.
fn draw_lines(img: &mut RgbImage, font: &FontRef, lines: &[String], size: f32, top: f32, c: [u8; 3]) {
    let sf = font.as_scaled(PxScale::from(size));
    let line_h = sf.ascent() - sf.descent() + sf.line_gap();
    let mut y = top + sf.ascent();
    for line in lines {
        let mut x = (SIDE as f32 - line_width(&sf, line)) / 2.0;
        let mut prev = None;
        for ch in line.chars() {
            let id = sf.glyph_id(ch);
            if let Some(p) = prev {
                x += sf.kern(p, id);
            }
            draw_glyph(img, font, id.with_scale_and_position(PxScale::from(size), point(x, y)), c);
            x += sf.h_advance(id);
            prev = Some(id);
        }
        y += line_h;
    }
}

fn lines_height(font: &FontRef, lines: &[String], size: f32) -> f32 {
    let sf = font.as_scaled(PxScale::from(size));
    lines.len() as f32 * (sf.ascent() - sf.descent() + sf.line_gap()) - sf.line_gap()
}

/// Draw `icon` so that it fills (and is centred in) the box with centre (cx, cy) and size w × h.
fn draw_icon(img: &mut RgbImage, icon: char, cx: f32, cy: f32, w: f32, h: f32, c: [u8; 3]) {
    let font = FontRef::try_from_slice(ICON_FONT).expect("embedded icon font");
    let id = font.glyph_id(icon);
    let probe = font.outline_glyph(id.with_scale_and_position(PxScale::from(h), point(0.0, 0.0)));
    let Some(probe) = probe else { return };
    let b = probe.px_bounds();
    let (bw, bh) = ((b.max.x - b.min.x).max(1.0), (b.max.y - b.min.y).max(1.0));
    let scale = h * (w / bw).min(h / bh);
    let k = scale / h;
    let (mx, my) = ((b.min.x + b.max.x) / 2.0 * k, (b.min.y + b.max.y) / 2.0 * k);
    draw_glyph(img, &font, id.with_scale_and_position(PxScale::from(scale), point(cx - mx, cy - my)), c);
}

fn draw_ring(img: &mut RgbImage, c: [u8; 3], ticks: bool, gauge: Option<f32>) {
    let side = SIDE as f32;
    let centre = side / 2.0;
    // a meter ring is a little wider, so it reads at a glance
    let (ro, ri) = if gauge.is_some() { (side * 0.495, side * 0.44) } else { (side * 0.495, side * 0.46) };
    let dim = c.map(|v| (v as f32 * 0.22) as u8);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (dx, dy) = (x as f32 + 0.5 - centre, y as f32 + 0.5 - centre);
            let r = (dx * dx + dy * dy).sqrt();
            let cov = (ro - r + 0.5).clamp(0.0, 1.0).min((r - ri + 0.5).clamp(0.0, 1.0));
            if cov <= 0.0 {
                continue;
            }
            // clockwise from the top: the filled part in the colour, the rest dim
            let colour = match gauge {
                Some(f) => {
                    let a = dx.atan2(-dy).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
                    if a <= f {
                        c
                    } else {
                        dim
                    }
                }
                None => c,
            };
            blend(img, x as i32, y as i32, colour, cov);
        }
    }
    if ticks {
        for k in 0..12 {
            let a = k as f32 * std::f32::consts::TAU / 12.0;
            let (len, w) = if k % 3 == 0 { (side * 0.06, side * 0.016) } else { (side * 0.035, side * 0.01) };
            let (s, e) = (ri - side * 0.012 - len, ri - side * 0.012);
            let steps = (len * 2.0) as i32;
            for i in 0..=steps {
                let r = s + (e - s) * i as f32 / steps as f32;
                let (px, py) = (centre + r * a.sin(), centre - r * a.cos());
                let rad = w / 2.0;
                for dy in -(rad.ceil() as i32 + 1)..=(rad.ceil() as i32 + 1) {
                    for dx in -(rad.ceil() as i32 + 1)..=(rad.ceil() as i32 + 1) {
                        let (qx, qy) = (px.round() as i32 + dx, py.round() as i32 + dy);
                        let d = ((qx as f32 + 0.5 - px).powi(2) + (qy as f32 + 0.5 - py).powi(2)).sqrt();
                        blend(img, qx, qy, c, (rad - d + 0.5).clamp(0.0, 1.0));
                    }
                }
            }
        }
    }
}

/// Render `text` centred on a square picture so that it fits inside the round display.
/// `max_size` caps the font size in pixels (0 = as large as fits).
/// Text that moves, rendered as frames for the fan's 20 frames per second (50 ms each).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// One line moving across the middle, like a news ticker (a "lichtkrant").
    Scroll,
    /// Around the rim of the disc, turning.
    Orbit,
}

impl Motion {
    /// `scroll` / `orbit`, or nothing (`""`, `none`, `static`).
    pub fn parse(s: &str) -> Result<Option<Motion>, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "none" | "static" => Ok(None),
            "scroll" | "ticker" | "marquee" => Ok(Some(Motion::Scroll)),
            "orbit" | "rotate" | "circle" => Ok(Some(Motion::Orbit)),
            other => Err(format!("unknown motion '{other}': use scroll or orbit")),
        }
    }
}

/// Most frames a moving text may use (30 s at 20 fps); faster beyond that.
pub const MAX_MOTION_FRAMES: usize = 600;

/// `text` on a single line (newlines become spaces) at font `size`: (strip, text width).
/// The strip is one line high, `pad` px of background left and right.
fn strip(text: &str, size: f32, fg: [u8; 3], bg: [u8; 3], pad: u32) -> (RgbImage, u32) {
    let font = FontRef::try_from_slice(FONT).expect("embedded font");
    let sf = font.as_scaled(PxScale::from(size));
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let w = line_width(&sf, &line).ceil().max(1.0) as u32;
    let h = (sf.ascent() - sf.descent()).ceil() as u32;
    let mut img = RgbImage::from_pixel(w + 2 * pad, h, Rgb(bg));
    let (mut x, y) = (pad as f32, sf.ascent());
    let mut prev = None;
    for ch in line.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            x += sf.kern(p, id);
        }
        draw_glyph(&mut img, &font, id.with_scale_and_position(PxScale::from(size), point(x, y)), fg);
        x += sf.h_advance(id);
        prev = Some(id);
    }
    (img, w)
}

/// Frames for `text` in `motion`. `speed` 1.0 is normal (a ticker moves 14 px per frame of the
/// 720 px picture, an orbit turns once in 4 s); `reverse` moves left to right / anticlockwise.
pub fn render_motion(text: &str, motion: Motion, fg: [u8; 3], bg: [u8; 3], speed: f32, reverse: bool) -> Vec<RgbImage> {
    let speed = if speed.is_finite() && speed > 0.0 { speed.clamp(0.1, 10.0) } else { 1.0 };
    match motion {
        Motion::Scroll => scroll(text, fg, bg, speed, reverse),
        Motion::Orbit => orbit(text, fg, bg, speed, reverse),
    }
}

/// A news ticker: the text enters at one edge, crosses the middle of the disc and leaves at the other.
fn scroll(text: &str, fg: [u8; 3], bg: [u8; 3], speed: f32, reverse: bool) -> Vec<RgbImage> {
    let (strip, w) = strip(text, SIDE as f32 * 0.28, fg, bg, SIDE);
    let travel = (SIDE + w) as f32;
    let step = (14.0 * speed).max(travel / MAX_MOTION_FRAMES as f32);
    let n = (travel / step).ceil() as usize;
    let top = (SIDE - strip.height().min(SIDE)) / 2;
    (0..n)
        .map(|k| {
            let moved = (k as f32 * step).round() as u32;
            let off = if reverse { (strip.width() - SIDE).saturating_sub(moved) } else { moved };
            let mut img = RgbImage::from_pixel(SIDE, SIDE, Rgb(bg));
            image::imageops::replace(&mut img, &image::imageops::crop_imm(&strip, off, 0, SIDE, strip.height().min(SIDE)).to_image(), 0, top as i64);
            img
        })
        .collect()
}

/// Text along the rim, turning: a strip of text mapped onto a ring (the same polar idea as the
/// fan itself), repeated so it goes all the way round when it is short.
fn orbit(text: &str, fg: [u8; 3], bg: [u8; 3], speed: f32, reverse: bool) -> Vec<RgbImage> {
    let c = SIDE as f32 / 2.0;
    let (r1, band) = (c * 0.97, c * 0.24);
    let r0 = r1 - band;
    let circumference = std::f32::consts::TAU * (r1 - band / 2.0);
    // the line's ascent+descent fills the band
    let (one, _) = strip(&format!("{}  •  ", text.trim()), band * 0.82, fg, bg, 0);
    let copies = ((circumference / one.width() as f32).floor() as u32).max(1);
    let sw = one.width() * copies;
    let sh = one.height();
    let mut ring = RgbImage::from_pixel(sw, sh, Rgb(bg));
    for i in 0..copies {
        image::imageops::replace(&mut ring, &one, (i * one.width()) as i64, 0);
    }
    // arc length (in strip pixels) of one full turn: the text is spread evenly round the ring
    let per_turn = if sw as f32 >= circumference { circumference } else { sw as f32 };
    let n = ((80.0 / speed).round() as usize).clamp(10, MAX_MOTION_FRAMES);
    // where every ring pixel samples the strip (u along the text, v across it)
    let mut map: Vec<(u32, u32, f32, u32)> = Vec::new();
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let rho = (dx * dx + dy * dy).sqrt();
            if rho < r0 || rho >= r1 {
                continue;
            }
            let theta = dx.atan2(-dy).rem_euclid(std::f32::consts::TAU); // 0 at the top, clockwise
            let u = theta / std::f32::consts::TAU * per_turn;
            let v = (((r1 - rho) / band) * sh as f32).min(sh as f32 - 1.0) as u32;
            map.push((x, y, u, v));
        }
    }
    (0..n)
        .map(|k| {
            let shift = k as f32 / n as f32 * sw as f32 * if reverse { -1.0 } else { 1.0 };
            let mut img = RgbImage::from_pixel(SIDE, SIDE, Rgb(bg));
            for &(x, y, u, v) in &map {
                let su = ((u + shift).rem_euclid(sw as f32)) as u32 % sw;
                img.put_pixel(x, y, *ring.get_pixel(su, v));
            }
            img
        })
        .collect()
}

pub fn render(text: &str, fg: [u8; 3], bg: [u8; 3], max_size: f32) -> RgbImage {
    render_card(&Card::text(text, fg, bg, max_size))
}

/// Render a card: optional ring and ticks, an optional icon above the text, all inside the disc.
pub fn render_card(c: &Card) -> RgbImage {
    let font = FontRef::try_from_slice(FONT).expect("embedded font");
    let side = SIDE as f32;
    let mut img = RgbImage::from_pixel(SIDE, SIDE, Rgb(c.bg));
    if let Some(rc) = c.ring {
        draw_ring(&mut img, rc, c.ticks, c.gauge);
    }
    // Content stays inside the disc: a centred box of 80 % × 62 % of the diameter,
    // smaller when a ring takes the rim.
    let k = if c.ring.is_some() {
        if c.ticks {
            0.78
        } else {
            0.86
        }
    } else {
        1.0
    };
    let (box_w, box_h) = (side * 0.80 * k, side * 0.62 * k);
    let start = if c.max_size > 0.0 { c.max_size.min(side * 0.45) } else { side * 0.45 };
    let text = c.text.trim();
    match (c.icon, text.is_empty()) {
        (Some(icon), true) => {
            let s = side * 0.55 * k;
            draw_icon(&mut img, icon, side / 2.0, side / 2.0, s, s, c.icon_color);
        }
        (Some(icon), false) => {
            let gap = side * 0.03;
            let icon_h = (side * 0.30 * k).min(box_h * 0.45);
            let (lines, size) = fit(&font, text, box_w, box_h - icon_h - gap, start);
            let text_h = lines_height(&font, &lines, size);
            let top = (side - (icon_h + gap + text_h)) / 2.0;
            draw_icon(&mut img, icon, side / 2.0, top + icon_h / 2.0, box_w * 0.6, icon_h, c.icon_color);
            draw_lines(&mut img, &font, &lines, size, top + icon_h + gap, c.fg);
        }
        (None, _) => {
            let (lines, size) = fit(&font, text, box_w, box_h, start);
            let top = (side - lines_height(&font, &lines, size)) / 2.0;
            draw_lines(&mut img, &font, &lines, size, top, c.fg);
        }
    }
    img
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    #[test]
    fn a_gauge_ring_fills_clockwise_from_the_top() {
        let mut card = Card::text("42", [255, 0, 0], [0, 0, 0], 0.0);
        card.ring = Some([255, 0, 0]);
        card.gauge = Some(0.25);
        let img = render_card(&card);
        let at = |deg: f32| {
            let (r, a) = (SIDE as f32 * 0.47, deg.to_radians());
            img.get_pixel((360.0 + r * a.sin()) as u32, (360.0 - r * a.cos()) as u32).0[0]
        };
        assert_eq!(at(45.0), 255, "filled: a quarter, from the top clockwise");
        assert!(at(135.0) < 80 && at(135.0) > 20, "the rest is a dim track");
        assert!(at(315.0) < 80, "anticlockwise from the top is not filled");
    }

    fn lit(img: &RgbImage) -> Vec<(u32, u32)> {
        img.enumerate_pixels().filter(|(_, _, p)| p[0] > 128).map(|(x, y, _)| (x, y)).collect()
    }

    /// Set P56_DUMP=<folder> to write a few frames as PNG for a look.
    fn dump(name: &str, frames: &[RgbImage]) {
        if let Ok(dir) = std::env::var("P56_DUMP") {
            for (i, f) in frames.iter().enumerate().step_by((frames.len() / 4).max(1)) {
                f.save(format!("{dir}/{name}-{i:03}.png")).unwrap();
            }
        }
    }

    #[test]
    fn ticker_crosses_the_middle_band() {
        let frames = render_motion("Welkom thuis", Motion::Scroll, [255, 255, 255], [0, 0, 0], 1.0, false);
        dump("scroll", &frames);
        assert!(frames.len() > 20 && frames.len() <= MAX_MOTION_FRAMES, "{}", frames.len());
        assert!(lit(&frames[0]).is_empty(), "starts just outside the right edge");
        let mid = lit(&frames[frames.len() / 2]);
        assert!(mid.len() > 5_000, "text in view halfway");
        assert!(mid.iter().all(|(_, y)| (200..520).contains(y)), "a band across the middle");
        // it moves right to left (a short word stays fully in view): its left edge goes down
        let left = |f: &RgbImage| lit(f).iter().map(|(x, _)| *x).min().unwrap();
        let hi = render_motion("Hi", Motion::Scroll, [255, 255, 255], [0, 0, 0], 1.0, false);
        assert!(left(&hi[hi.len() / 2]) > left(&hi[hi.len() / 2 + 3]));
        let rev = render_motion("Hi", Motion::Scroll, [255, 255, 255], [0, 0, 0], 1.0, true);
        assert!(left(&rev[rev.len() / 2]) < left(&rev[rev.len() / 2 + 3]), "reverse: left to right");
        assert!(
            render_motion("x", Motion::Scroll, [255; 3], [0; 3], 2.0, false).len() < render_motion("x", Motion::Scroll, [255; 3], [0; 3], 1.0, false).len()
        );
    }

    #[test]
    fn orbit_turns_along_the_rim() {
        let frames = render_motion("P56 Studio", Motion::Orbit, [255, 200, 0], [0, 0, 0], 1.0, false);
        dump("orbit", &frames);
        assert_eq!(frames.len(), 80, "one turn in 4 s");
        let c = SIDE as f32 / 2.0;
        for f in [&frames[0], &frames[40]] {
            let px = lit(f);
            assert!(px.len() > 5_000);
            assert!(
                px.iter().all(|(x, y)| {
                    let r = ((*x as f32 + 0.5 - c).powi(2) + (*y as f32 + 0.5 - c).powi(2)).sqrt();
                    r >= c * 0.72 && r < c * 0.97
                }),
                "only on the ring along the rim"
            );
        }
        assert!(frames[0].as_raw() != frames[1].as_raw(), "it turns");
        assert_eq!(render_motion("x", Motion::Orbit, [255; 3], [0; 3], 2.0, false).len(), 40, "twice as fast");
    }

    #[test]
    fn motion_names() {
        assert_eq!(Motion::parse("scroll").unwrap(), Some(Motion::Scroll));
        assert_eq!(Motion::parse("Orbit").unwrap(), Some(Motion::Orbit));
        assert_eq!(Motion::parse("").unwrap(), None);
        assert!(Motion::parse("spin").unwrap_err().contains("scroll or orbit"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_over_a_picture_stays_readable() {
        let picture = RgbImage::from_pixel(100, 50, Rgb([200, 200, 200]));
        let mut text = RgbImage::from_pixel(90, 90, Rgb([0, 0, 0]));
        for y in 40..50 {
            for x in 40..50 {
                text.put_pixel(x, y, Rgb([255, 0, 0]));
            }
        }
        let out = over_backdrop(&text, &picture, [0, 0, 0]);
        assert_eq!(out.dimensions(), (90, 90));
        assert_eq!(out.get_pixel(45, 45).0, [255, 0, 0], "the text itself");
        assert_eq!(out.get_pixel(38, 45).0, [30, 30, 30], "dark right around the text");
        assert_eq!(out.get_pixel(5, 5).0, [120, 120, 120], "the picture, dimmed");
    }

    fn lit_outside(img: &RgbImage, radius_frac: f64) -> (usize, usize) {
        let c = SIDE as f64 / 2.0;
        let (mut lit, mut outside) = (0, 0);
        for (x, y, p) in img.enumerate_pixels() {
            if p.0.iter().any(|v| *v > 128) {
                lit += 1;
                let r = ((x as f64 + 0.5 - c).powi(2) + (y as f64 + 0.5 - c).powi(2)).sqrt();
                if r > c * radius_frac {
                    outside += 1;
                }
            }
        }
        (lit, outside)
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_color("#fff"), Some([255, 255, 255]));
        assert_eq!(parse_color("10, 20,30"), Some([10, 20, 30]));
        assert_eq!(parse_color("Red"), Some([255, 40, 40]));
        assert_eq!(parse_color("rood"), Some([255, 40, 40]));
        assert_eq!(parse_color("nope"), None);
    }

    #[test]
    fn text_is_drawn_inside_the_disc() {
        let img = render("0,23 euro/kWh and a very long sentence that has to wrap", [255, 255, 255], [0, 0, 0], 0.0);
        let (lit, outside) = lit_outside(&img, 0.98);
        assert!(lit > 2000, "too little text drawn: {lit}");
        assert_eq!(outside, 0, "text outside the round display");
    }

    #[test]
    fn icons_by_home_assistant_name() {
        assert_eq!(icon_char("mdi:flash"), Some('\u{F0241}'));
        assert_eq!(icon_char("washing-machine"), Some('\u{F072A}'));
        assert_eq!(icon_char("mdi:does-not-exist"), None);
    }

    #[test]
    fn scale_runs_green_yellow_red() {
        assert_eq!(color_scale(0.10, 0.20, 0.40, &SCALE_GREEN_RED), SCALE_GREEN_RED[0]);
        assert_eq!(color_scale(0.30, 0.20, 0.40, &SCALE_GREEN_RED), SCALE_GREEN_RED[1]);
        assert_eq!(color_scale(0.99, 0.20, 0.40, &SCALE_GREEN_RED), SCALE_GREEN_RED[2]);
        let mid = color_scale(0.25, 0.20, 0.40, &SCALE_GREEN_RED);
        assert!(mid[0] > 48 && mid[0] < 255, "{mid:?}");
    }

    #[test]
    fn card_with_icon_ring_and_ticks_stays_inside() {
        let card = Card {
            text: "Washing machine done".into(),
            fg: [255, 255, 255],
            bg: [0, 0, 0],
            max_size: 0.0,
            icon: icon_char("mdi:washing-machine"),
            icon_color: [0, 200, 255],
            ring: Some([255, 59, 48]),
            ticks: true,
            gauge: None,
        };
        let img = render_card(&card);
        let (lit, outside) = lit_outside(&img, 0.995);
        assert!(lit > 20_000, "ring, icon and text drawn: {lit}");
        assert_eq!(outside, 0, "nothing outside the round display");
        // the icon (cyan) sits above the text (white)
        let rows = |pred: &dyn Fn(&Rgb<u8>) -> bool| -> Vec<u32> {
            img.enumerate_pixels().filter(|(x, _, p)| *x > 150 && *x < 570 && pred(p)).map(|(_, y, _)| y).collect()
        };
        let cyan = rows(&|p| p[0] < 60 && p[1] > 150 && p[2] > 200);
        let white = rows(&|p| p[0] > 200 && p[1] > 200 && p[2] > 200);
        assert!(!cyan.is_empty() && !white.is_empty());
        let avg = |v: &Vec<u32>| v.iter().map(|y| *y as f64).sum::<f64>() / v.len() as f64;
        assert!(avg(&cyan) < avg(&white), "icon above text");
    }

    #[test]
    fn icon_alone_is_big() {
        let card = Card {
            text: String::new(),
            fg: [255, 255, 255],
            bg: [0, 0, 0],
            max_size: 0.0,
            icon: icon_char("flash"),
            icon_color: [255, 214, 10],
            ring: None,
            ticks: false,
            gauge: None,
        };
        let (lit, outside) = lit_outside(&render_card(&card), 0.98);
        assert!(lit > 30_000, "big icon: {lit}");
        assert_eq!(outside, 0);
    }
}
