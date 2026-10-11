//! Video frame source through an external `ffmpeg` binary (the Windows app ships
//! FFmpeg DLLs for the same job). If `ffmpeg` is not on PATH, [`open`] fails with
//! a clear message and callers fall back to still images.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use image::RgbImage;

use crate::Error;

pub struct VideoSource {
    child: Child,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// Probe the file with `ffprobe` and start decoding to raw RGB24 frames.
pub fn open(path: &Path, max_side: u32) -> Result<VideoSource, Error> {
    let probe = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,r_frame_rate", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .map_err(|e| Error::Io(format!("ffprobe not available: {e}")))?;
    let text = String::from_utf8_lossy(&probe.stdout);
    let mut it = text.trim().split(',');
    let w: u32 = it.next().and_then(|s| s.parse().ok()).ok_or_else(|| Error::Invalid("no video stream".into()))?;
    let h: u32 = it.next().and_then(|s| s.parse().ok()).ok_or_else(|| Error::Invalid("no video stream".into()))?;
    let fps = it
        .next()
        .and_then(|r| {
            let (a, b) = r.split_once('/')?;
            Some(a.parse::<f64>().ok()? / b.parse::<f64>().ok()?.max(1.0))
        })
        .unwrap_or(25.0);
    // Scale down so the polar warp stays cheap; keep aspect.
    let scale = (max_side as f64 / w.max(h) as f64).min(1.0);
    let (ow, oh) = (((w as f64 * scale) as u32) & !1, ((h as f64 * scale) as u32) & !1);
    let child = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vf", &format!("scale={ow}:{oh}"), "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| Error::Io(format!("ffmpeg not available: {e}")))?;
    Ok(VideoSource { child, width: ow, height: oh, fps })
}

impl VideoSource {
    /// Next decoded frame, or `None` at end of stream.
    pub fn next_frame(&mut self) -> Option<RgbImage> {
        let n = (self.width * self.height * 3) as usize;
        let mut buf = vec![0u8; n];
        let out = self.child.stdout.as_mut()?;
        out.read_exact(&mut buf).ok()?;
        RgbImage::from_raw(self.width, self.height, buf)
    }
}

impl Drop for VideoSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Sample rate of the sound in an uploaded file (16-bit mono; 1600 bytes per 20 fps frame).
pub const AUDIO_RATE: u32 = 16_000;

/// The sound track of a video as 16-bit little-endian mono PCM at [`AUDIO_RATE`], or `None`
/// when the file has no sound or `ffmpeg` is missing.
pub fn audio_pcm(path: &Path) -> Option<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", &AUDIO_RATE.to_string(), "-f", "s16le", "-"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
}
