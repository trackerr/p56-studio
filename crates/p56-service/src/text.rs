//! Text → round picture for the fan: words wrapped and sized to fit inside the disc.

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::{Rgb, RgbImage};

static FONT: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");

/// Side of the square picture the text is drawn on (the transcoder maps it onto the disc).
pub const SIDE: u32 = 720;

/// Parse `#rgb`, `#rrggbb`, `r,g,b` or a common colour name.
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().to_ascii_lowercase();
    let named = match s.as_str() {
        "white" | "wit" => Some([255, 255, 255]), "black" | "zwart" => Some([0, 0, 0]),
        "red" | "rood" => Some([255, 40, 40]), "green" | "groen" => Some([40, 230, 80]),
        "blue" | "blauw" => Some([50, 110, 255]), "yellow" | "geel" => Some([255, 220, 0]),
        "orange" | "oranje" => Some([255, 140, 0]), "cyan" => Some([0, 220, 255]),
        "magenta" | "paars" | "purple" => Some([200, 60, 255]), "pink" | "roze" => Some([255, 110, 180]),
        _ => None,
    };
    if named.is_some() { return named; }
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

fn line_width<F: Font, S: ScaleFont<F>>(font: &S, line: &str) -> f32 {
    let mut w = 0.0;
    let mut prev = None;
    for c in line.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = prev { w += font.kern(p, id); }
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
            if line_width(font, &cand) <= max_w || cur.is_empty() { cur = cand; } else { lines.push(std::mem::take(&mut cur)); cur = word.to_string(); }
        }
        lines.push(cur);
    }
    while lines.last().is_some_and(|l| l.is_empty()) { lines.pop(); }
    if lines.is_empty() { lines.push(String::new()); }
    lines
}

/// Render `text` centred on a square picture so that it fits inside the round display.
/// `max_size` caps the font size in pixels (0 = as large as fits).
pub fn render(text: &str, fg: [u8; 3], bg: [u8; 3], max_size: f32) -> RgbImage {
    let font = FontRef::try_from_slice(FONT).expect("embedded font");
    let side = SIDE as f32;
    // Text block must stay inside the disc: use a centred box of 80 % × 62 % of the diameter.
    let (box_w, box_h) = (side * 0.80, side * 0.62);
    let mut size = if max_size > 0.0 { max_size.min(side * 0.45) } else { side * 0.45 };
    let (lines, scaled) = loop {
        let sf = font.as_scaled(PxScale::from(size));
        let lines = wrap(&sf, text, box_w);
        let widest = lines.iter().map(|l| line_width(&sf, l)).fold(0.0, f32::max);
        let height = lines.len() as f32 * (sf.ascent() - sf.descent() + sf.line_gap());
        if (widest <= box_w && height <= box_h) || size <= 10.0 { break (lines, size); }
        size *= 0.92;
    };
    let sf = font.as_scaled(PxScale::from(scaled));
    let line_h = sf.ascent() - sf.descent() + sf.line_gap();
    let total_h = lines.len() as f32 * line_h - sf.line_gap();
    let mut img = RgbImage::from_pixel(SIDE, SIDE, Rgb(bg));
    let mut y = (side - total_h) / 2.0 + sf.ascent();
    for line in &lines {
        let mut x = (side - line_width(&sf, line)) / 2.0;
        let mut prev = None;
        for c in line.chars() {
            let id = sf.glyph_id(c);
            if let Some(p) = prev { x += sf.kern(p, id); }
            let glyph = id.with_scale_and_position(PxScale::from(scaled), point(x, y));
            x += sf.h_advance(id);
            prev = Some(id);
            if let Some(outline) = font.outline_glyph(glyph) {
                let b = outline.px_bounds();
                outline.draw(|gx, gy, cov| {
                    let px = b.min.x as i32 + gx as i32;
                    let py = b.min.y as i32 + gy as i32;
                    if px < 0 || py < 0 || px >= SIDE as i32 || py >= SIDE as i32 { return; }
                    let p = img.get_pixel_mut(px as u32, py as u32);
                    for k in 0..3 { p[k] = (p[k] as f32 * (1.0 - cov) + fg[k] as f32 * cov).round() as u8; }
                });
            }
        }
        y += line_h;
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_color("#fff"), Some([255, 255, 255]));
        assert_eq!(parse_color("10, 20,30"), Some([10, 20, 30]));
        assert_eq!(parse_color("Rood"), Some([255, 40, 40]));
        assert_eq!(parse_color("nope"), None);
    }

    #[test]
    fn text_is_drawn_inside_the_disc() {
        let img = render("0,23 euro/kWh en een heel lange zin die moet afbreken", [255, 255, 255], [0, 0, 0], 0.0);
        let c = SIDE as f64 / 2.0;
        let (mut lit, mut outside) = (0, 0);
        for (x, y, p) in img.enumerate_pixels() {
            if p[0] > 128 {
                lit += 1;
                let r = ((x as f64 - c).powi(2) + (y as f64 - c).powi(2)).sqrt();
                if r > c * 0.98 { outside += 1; }
            }
        }
        assert!(lit > 2000, "too little text drawn: {lit}");
        assert_eq!(outside, 0, "text outside the round display");
    }
}
