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
}

impl Card {
    pub fn text(text: &str, fg: [u8; 3], bg: [u8; 3], max_size: f32) -> Card {
        Card { text: text.into(), fg, bg, max_size, icon: None, icon_color: fg, ring: None, ticks: false }
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
    if x < 0 || y < 0 || x >= SIDE as i32 || y >= SIDE as i32 || cov <= 0.0 {
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

fn draw_ring(img: &mut RgbImage, c: [u8; 3], ticks: bool) {
    let side = SIDE as f32;
    let centre = side / 2.0;
    let (ro, ri) = (side * 0.495, side * 0.46);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let r = ((x as f32 + 0.5 - centre).powi(2) + (y as f32 + 0.5 - centre).powi(2)).sqrt();
            let cov = (ro - r + 0.5).clamp(0.0, 1.0).min((r - ri + 0.5).clamp(0.0, 1.0));
            blend(img, x as i32, y as i32, c, cov);
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
pub fn render(text: &str, fg: [u8; 3], bg: [u8; 3], max_size: f32) -> RgbImage {
    render_card(&Card::text(text, fg, bg, max_size))
}

/// Render a card: optional ring and ticks, an optional icon above the text, all inside the disc.
pub fn render_card(c: &Card) -> RgbImage {
    let font = FontRef::try_from_slice(FONT).expect("embedded font");
    let side = SIDE as f32;
    let mut img = RgbImage::from_pixel(SIDE, SIDE, Rgb(c.bg));
    if let Some(rc) = c.ring {
        draw_ring(&mut img, rc, c.ticks);
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
mod tests {
    use super::*;

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
        };
        let (lit, outside) = lit_outside(&render_card(&card), 0.98);
        assert!(lit > 30_000, "big icon: {lit}");
        assert_eq!(outside, 0);
    }
}
