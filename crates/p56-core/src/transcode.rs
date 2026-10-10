//! Image → fan frame pipeline, bit-for-bit modelled on Holoscope.exe (and the
//! verified Python port `holofan.py`).
//!
//! 1. auto brightness (mean grey × factor, clamped)
//! 2. `warpPolar` around the (optionally shifted) centre → (angles × radius)
//! 3. radial attenuation of the innermost LEDs + sawtooth black centre
//! 4. arm layout: segments of all arms side by side per row, radius outside-in,
//!    rows bottom-up (order 0-3-2-1 for 4 arms, 0-1 for 2 arms)
//! 5. JPEG quality 80, backing off ×0.9 until it fits in a cast frame

use image::{ImageBuffer, Rgb, RgbImage};
use serde::{Deserialize, Serialize};

use crate::protocol::MAX_JPEG;

/// Geometry of one fan model, keyed by the `fan_type` the device reports.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FanType {
    pub id: u8,
    pub angles: u32,
    pub radius: u32,
    /// 0 = raw polar image (angles rows × radius cols), 1/2/4 = arm layout.
    pub arms: u32,
    /// Holoscope field 0xb70: auto-brightness gain.
    pub gain: f64,
}

/// How well we know a fan type's image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    /// JPEG arm layout taken from Holoscope (P56 = type 8 verified against the real fan).
    Jpeg,
    /// Raw polar JPEG as Holoscope.exe sends it; Holoscope.apk uses a binary format
    /// for these types instead, so newer firmware may ignore it.
    Legacy,
    /// Known from Holoscope.apk but the image layout is not implemented.
    Unsupported,
}

impl FanType {
    pub const P56: FanType = FanType { id: 8, angles: 960, radius: 172, arms: 4, gain: 1.5 };

    /// Types we can transcode for.
    pub fn known(id: u8) -> Option<FanType> {
        let (angles, radius, arms, gain) = match id {
            1 | 17 => (960, 192, if id == 1 { 4 } else { 2 }, 1.5),
            2 => (960, 168, 4, 1.5),
            3 => (1440, 240, 1, 1.5),
            4 => (960, 144, 4, 1.5),
            5 => (960, 128, 2, 1.5),
            6 => (600, 133, 0, 2.0),
            7 | 15 => (490, 122, 0, 1.6),
            8 => (960, 172, 4, 1.5),
            11 => (960, 184, 4, 1.5),
            12 => (512, 85, 0, 1.6),
            13 => (490, 128, 0, 1.6),
            14 => (960, 176, 2, 1.4),
            _ => return None,
        };
        Some(FanType { id, angles, radius, arms, gain })
    }

    /// Like [`FanType::known`] but falls back to type 1 for unknown ids.
    pub fn by_id(id: u8) -> FanType {
        FanType::known(id).unwrap_or(FanType { id: 1, angles: 960, radius: 192, arms: 4, gain: 1.5 })
    }

    pub fn support(id: u8) -> Support {
        match id {
            1 | 2 | 3 | 4 | 5 | 8 | 11 | 14 | 17 => Support::Jpeg,
            6 | 7 | 12 | 13 | 15 => Support::Legacy,
            _ => Support::Unsupported,
        }
    }

    /// Geometry Holoscope.apk uses (`fileprocess::setparam`): (angles, LEDs, fps).
    pub fn apk_geometry(id: u8) -> Option<(u32, u32, u32)> {
        Some(match id {
            1 | 17 | 21 | 25 => (960, 192, 20),
            2 => (960, 168, 20),
            4 => (960, 144, 20),
            5 | 22 => (960, 128, 20),
            6 => (480, 122, 12),
            7 | 15 => (490, 122, 12),
            8 => (960, 172, 20),
            11 => (960, 184, 20),
            12 => (512, 85, 12),
            13 => (500, 117, 12),
            14 | 24 | 28 => (960, 176, 20),
            16 => (400, 80, 12),
            18 => (480, 117, 12),
            20 | 26 => (512, 512, 20),
            23 => (512, 96, 12),
            27 => (450, 112, 12),
            29 => (512, 80, 16),
            30 => (490, 85, 16),
            31 => (490, 112, 12),
            32 => (960, 122, 20),
            33 => (960, 133, 20),
            34 => (960, 154, 20),
            36 => (448, 69, 12),
            37 => (704, 128, 20),
            _ => return None,
        })
    }

    /// Error text when we cannot produce images for `id`, `None` when we can.
    pub fn check(id: u8) -> Option<String> {
        match FanType::support(id) {
            Support::Unsupported => Some(match FanType::apk_geometry(id) {
                Some((a, l, _)) => format!("fantype {id} ({a}×{l}) wordt nog niet ondersteund: het beeldformaat is onbekend"),
                None => format!("onbekend fantype {id}"),
            }),
            _ => None,
        }
    }

    /// Size of the JPEG the firmware expects: (width, height).
    pub fn jpeg_size(&self) -> (u32, u32) {
        match self.arms {
            4 | 2 => (self.radius * self.arms, self.angles / self.arms),
            _ => (self.radius, self.angles),
        }
    }

    pub fn all() -> &'static [u8] { &[1, 2, 3, 4, 5, 6, 7, 8, 11, 12, 13, 14, 15, 17] }
}

/// Where the image is projected: offsets shift the polar centre (fraction of the
/// shorter side), `scale` multiplies the radius. Holoscope's x/y-offset & scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub x_offset: f64,
    pub y_offset: f64,
    pub scale: f64,
    /// Mirror horizontally before projecting (needed when the fan is viewed from the back).
    pub mirror: bool,
}

impl Default for Placement {
    fn default() -> Self { Placement { x_offset: 0.0, y_offset: 0.0, scale: 1.0, mirror: false } }
}

/// Byte-wise attenuation along the radius (byte 0 = innermost LED, RGB per pixel).
const RADIAL: [f64; 48] = {
    let mut t = [0.0; 48];
    let steps: [(usize, f64); 10] = [
        (19, 0.73), (5, 0.74), (3, 0.77), (3, 0.80), (3, 0.83),
        (3, 0.86), (3, 0.89), (3, 0.92), (3, 0.95), (3, 0.98),
    ];
    let mut i = 0;
    let mut s = 0;
    while s < steps.len() {
        let mut k = 0;
        while k < steps[s].0 { t[i] = steps[s].1; i += 1; k += 1; }
        s += 1;
    }
    t
};

fn grey_mean(img: &RgbImage) -> f64 {
    if img.is_empty() { return 0.0; }
    let sum: f64 = img.pixels().map(|p| 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64).sum();
    sum / (img.width() * img.height()) as f64
}

/// Holoscope: target = mean × gain, clamped to 150 (100 for type 14); then convertScaleAbs.
pub fn auto_brightness(img: &RgbImage, ft: &FanType) -> RgbImage {
    let mean = grey_mean(img);
    if mean < 1.0 { return img.clone(); }
    let cap = if ft.id == 14 { 100.0 } else { 150.0 };
    let alpha = (mean * ft.gain).min(cap) / mean;
    let mut out = img.clone();
    for p in out.pixels_mut() {
        for c in 0..3 { p[c] = (p[c] as f64 * alpha).round().clamp(0.0, 255.0) as u8; }
    }
    out
}

#[inline]
fn cubic(t: f64) -> f64 {
    // OpenCV bicubic (a = -0.75)
    let a = -0.75;
    let t = t.abs();
    if t < 1.0 { (a + 2.0) * t * t * t - (a + 3.0) * t * t + 1.0 }
    else if t < 2.0 { a * t * t * t - 5.0 * a * t * t + 8.0 * a * t - 4.0 * a }
    else { 0.0 }
}

fn sample_bicubic(img: &RgbImage, x: f64, y: f64) -> [u8; 3] {
    let (w, h) = (img.width() as i64, img.height() as i64);
    if x < -0.5 || y < -0.5 || x > w as f64 - 0.5 || y > h as f64 - 0.5 { return [0, 0, 0]; }
    let x0 = x.floor() as i64;
    let y0 = y.floor() as i64;
    let fx = x - x0 as f64;
    let fy = y - y0 as f64;
    let mut acc = [0.0f64; 3];
    for j in -1..=2 {
        let wy = cubic(fy - j as f64);
        let yy = (y0 + j).clamp(0, h - 1) as u32;
        for i in -1..=2 {
            let wx = cubic(fx - i as f64);
            let xx = (x0 + i).clamp(0, w - 1) as u32;
            let p = img.get_pixel(xx, yy);
            let wgt = wx * wy;
            for c in 0..3 { acc[c] += p[c] as f64 * wgt; }
        }
    }
    [acc[0].round().clamp(0.0, 255.0) as u8, acc[1].round().clamp(0.0, 255.0) as u8, acc[2].round().clamp(0.0, 255.0) as u8]
}

/// `cv::warpPolar(src, (radius, angles), centre, R, LINEAR)` → image of `radius` columns × `angles` rows.
pub fn to_polar(img: &RgbImage, ft: &FanType, pl: &Placement) -> RgbImage {
    let (w, h) = (img.width() as f64, img.height() as f64);
    let short = w.min(h);
    let cx = (w / 2.0).floor() + pl.x_offset * short;
    let cy = (h / 2.0).floor() + pl.y_offset * short;
    let max_r = ((short / 2.0).floor() + 0.5) * pl.scale.max(0.05);
    let k_angle = ft.angles as f64 / (2.0 * std::f64::consts::PI);
    let k_mag = ft.radius as f64 / max_r;
    let mut out = RgbImage::new(ft.radius, ft.angles);
    for phi in 0..ft.angles {
        let angle = phi as f64 / k_angle;
        let (s, c) = angle.sin_cos();
        for rho in 0..ft.radius {
            let mag = rho as f64 / k_mag;
            let mut sx = cx + mag * c;
            let sy = cy + mag * s;
            if pl.mirror { sx = w - 1.0 - sx; }
            out.put_pixel(rho, phi, Rgb(sample_bicubic(img, sx, sy)));
        }
    }
    out
}

/// Attenuate the innermost LEDs and black out a sawtooth in the centre
/// (row r: `r mod 8` innermost pixels set to 0), as Holoscope does.
pub fn radial_correction(polar: &mut RgbImage, ft: &FanType) {
    let rad = polar.width() as usize;
    let mut f = RADIAL;
    if matches!(ft.id, 5 | 14 | 17) { f[0] = 0.6; f[1] = 0.6; f[2] = 0.6; }
    let n = f.len().min(rad * 3);
    for r in 0..polar.height() {
        let row = polar.as_mut().chunks_exact_mut(rad * 3).nth(r as usize).unwrap();
        for i in 0..n { row[i] = (row[i] as f64 * f[i]).trunc() as u8; }
        let k = ((r % 8) * 3) as usize;
        for b in row.iter_mut().take(k) { *b = 0; }
    }
}

/// Rearrange the polar image into the layout the fan's JPEG decoder expects.
pub fn layout(polar: &RgbImage, ft: &FanType) -> RgbImage {
    let (ang, rad) = (polar.height(), polar.width());
    match ft.arms {
        4 | 2 => {
            let order: &[u32] = if ft.arms == 4 { &[0, 3, 2, 1] } else { &[0, 1] };
            let q = ang / ft.arms;
            let mut out = RgbImage::new(rad * ft.arms, q);
            for i in 0..q {
                let y_out = q - 1 - i; // flipud
                for (seg, &k) in order.iter().enumerate() {
                    let src_row = k * q + i;
                    for x in 0..rad {
                        let p = polar.get_pixel(rad - 1 - x, src_row); // radius outside-in
                        out.put_pixel(seg as u32 * rad + x, y_out, *p);
                    }
                }
            }
            out
        }
        1 => {
            // flipud(polar[:, ::-1, ::-1]) — radius reversed, channels swapped
            let mut out = RgbImage::new(rad, ang);
            for y in 0..ang {
                for x in 0..rad {
                    let p = polar.get_pixel(rad - 1 - x, y);
                    out.put_pixel(x, ang - 1 - y, Rgb([p[2], p[1], p[0]]));
                }
            }
            out
        }
        _ => polar.clone(),
    }
}

/// Inverse of [`layout`]: JPEG-shaped image back to (angles × radius). Used by tests/emulation.
pub fn unlayout(mat: &RgbImage, ft: &FanType) -> RgbImage {
    let rad = ft.radius;
    match ft.arms {
        4 | 2 => {
            let order: &[u32] = if ft.arms == 4 { &[0, 3, 2, 1] } else { &[0, 1] };
            let q = mat.height();
            let mut polar = RgbImage::new(rad, q * ft.arms);
            for i in 0..q {
                let y_in = q - 1 - i;
                for (seg, &k) in order.iter().enumerate() {
                    for x in 0..rad {
                        let p = mat.get_pixel(seg as u32 * rad + x, y_in);
                        polar.put_pixel(rad - 1 - x, k * q + i, *p);
                    }
                }
            }
            polar
        }
        1 => {
            let ang = mat.height();
            let mut polar = RgbImage::new(rad, ang);
            for y in 0..ang {
                for x in 0..rad {
                    let p = mat.get_pixel(x, ang - 1 - y);
                    polar.put_pixel(rad - 1 - x, y, Rgb([p[2], p[1], p[0]]));
                }
            }
            polar
        }
        _ => mat.clone(),
    }
}

/// Encode an already laid-out image as baseline JPEG at `quality` (1-100),
/// 4:2:0 chroma subsampling like libjpeg's default (what Holoscope/OpenCV emit).
pub fn jpeg(img: &RgbImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 * 1024);
    let mut enc = jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 100));
    enc.set_sampling_factor(jpeg_encoder::SamplingFactor::F_2_2);
    enc.encode(img.as_raw(), img.width() as u16, img.height() as u16, jpeg_encoder::ColorType::Rgb)
        .expect("jpeg encode");
    out
}

/// Result of transcoding one frame.
#[derive(Debug, Clone)]
pub struct Frame {
    /// JPEG in fan layout, ≤ [`MAX_JPEG`] bytes.
    pub jpeg: Vec<u8>,
    /// The laid-out image (for previews / debugging).
    pub layout: RgbImage,
    pub quality: u8,
}

/// Full pipeline: any RGB image → fan-ready JPEG.
pub fn encode(img: &RgbImage, ft: &FanType, pl: &Placement) -> Frame {
    let bright = auto_brightness(img, ft);
    let mut polar = to_polar(&bright, ft, pl);
    radial_correction(&mut polar, ft);
    let lay = layout(&polar, ft);
    let mut q = 80.0f64;
    let mut data = jpeg(&lay, q as u8);
    for _ in 0..11 {
        if data.len() <= MAX_JPEG { break; }
        q *= 0.9;
        data = jpeg(&lay, q.max(5.0) as u8);
    }
    data.truncate(MAX_JPEG);
    Frame { jpeg: data, layout: lay, quality: q as u8 }
}

/// Render a polar image (angles × radius) back to a round picture of `size`×`size`,
/// i.e. what the spinning fan shows. Used for previews.
pub fn round_preview(polar: &RgbImage, size: u32) -> RgbImage {
    let (rad, ang) = (polar.width() as f64, polar.height() as f64);
    let c = size as f64 / 2.0;
    let mut out = RgbImage::new(size, size);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 + 0.5 - c;
            let dy = y as f64 + 0.5 - c;
            let r = (dx * dx + dy * dy).sqrt() / c;
            if r >= 1.0 { continue; }
            let mut phi = dy.atan2(dx);
            if phi < 0.0 { phi += 2.0 * std::f64::consts::PI; }
            let pr = (r * rad).min(rad - 1.0) as u32;
            let pa = ((phi / (2.0 * std::f64::consts::PI)) * ang) as u32 % polar.height();
            out.put_pixel(x, y, *polar.get_pixel(pr, pa));
        }
    }
    out
}

/// Decode a fan-layout JPEG back to the round picture (what the fan will show).
pub fn preview_from_jpeg(jpeg: &[u8], ft: &FanType, size: u32) -> Result<RgbImage, image::ImageError> {
    let mat = image::load_from_memory(jpeg)?.to_rgb8();
    Ok(round_preview(&unlayout(&mat, ft), size))
}

/// Convenience: load any image file (png/jpg/bmp/webp/gif) as RGB.
pub fn load_rgb(path: &std::path::Path) -> Result<RgbImage, image::ImageError> {
    Ok(image::open(path)?.to_rgb8())
}

/// Decode an image from memory (any supported format).
pub fn decode_rgb(bytes: &[u8]) -> Result<RgbImage, image::ImageError> {
    Ok(image::load_from_memory(bytes)?.to_rgb8())
}

/// Frames of an image file in memory: an animated GIF yields every frame with its
/// delay in ms, anything else one frame with delay 0. At most `max` frames (0 = all).
pub fn decode_frames(bytes: &[u8], max: usize) -> Result<Vec<(RgbImage, u32)>, image::ImageError> {
    use image::AnimationDecoder;
    if image::guess_format(bytes).ok() == Some(image::ImageFormat::Gif) {
        let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))?;
        let mut out = Vec::new();
        for f in dec.into_frames() {
            let f = f?;
            let (n, d) = f.delay().numer_denom_ms();
            let ms = if d == 0 { 100 } else { n / d };
            out.push((image::DynamicImage::ImageRgba8(f.into_buffer()).to_rgb8(), ms.max(20)));
            if max > 0 && out.len() >= max { break; }
        }
        if !out.is_empty() { return Ok(out); }
    }
    Ok(vec![(decode_rgb(bytes)?, 0)])
}

/// Downscale so the shorter side is at most `side` pixels (keeps the polar warp cheap).
pub fn fit(img: RgbImage, side: u32) -> RgbImage {
    let (w, h) = img.dimensions();
    let s = side as f64 / w.min(h).max(1) as f64;
    if s >= 1.0 { return img; }
    image::imageops::resize(&img, ((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1), image::imageops::FilterType::Triangle)
}

/// Build an RGB image from raw RGB bytes (e.g. a captured screen).
pub fn rgb_from_raw(w: u32, h: u32, rgb: Vec<u8>) -> Option<RgbImage> {
    ImageBuffer::from_raw(w, h, rgb)
}

/// A simple test card: three rings, a "12 o'clock" bar and the label, 720×720.
pub fn test_card() -> RgbImage {
    let n = 720u32;
    let mut img = RgbImage::new(n, n);
    let c = n as f64 / 2.0;
    for y in 0..n {
        for x in 0..n {
            let dx = x as f64 - c;
            let dy = y as f64 - c;
            let r = (dx * dx + dy * dy).sqrt();
            let px = if (r - 340.0).abs() < 12.5 { [255, 0, 0] }
                else if (r - 250.0).abs() < 12.5 { [0, 255, 0] }
                else if (r - 160.0).abs() < 12.5 { [0, 0, 255] }
                else if dx.abs() < 7.5 && dy < 0.0 { [255, 255, 255] }
                else { [0, 0, 0] };
            img.put_pixel(x, y, Rgb(px));
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p56_jpeg_is_688x240() {
        let ft = FanType::P56;
        assert_eq!(ft.jpeg_size(), (688, 240));
        let f = encode(&test_card(), &ft, &Placement::default());
        assert_eq!(f.layout.dimensions(), (688, 240));
        assert!(f.jpeg.len() <= MAX_JPEG);
        assert_eq!(&f.jpeg[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn layout_roundtrip() {
        let ft = FanType::P56;
        let polar = to_polar(&test_card(), &ft, &Placement::default());
        let back = unlayout(&layout(&polar, &ft), &ft);
        assert_eq!(polar.as_raw(), back.as_raw());
    }

    #[test]
    fn all_types_encode() {
        for &id in FanType::all() {
            let ft = FanType::by_id(id);
            let f = encode(&test_card(), &ft, &Placement::default());
            assert_eq!(f.layout.dimensions(), ft.jpeg_size(), "type {id}");
            assert!(f.jpeg.len() <= MAX_JPEG, "type {id}");
        }
    }
}
