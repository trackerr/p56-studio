//! Wire protocol of the Holoscope / P56 POV hologram fan.
//!
//! Every packet is `5A <opcode> <body…> F5`. Text-carrying packets end with an
//! XOR checksum right before the `F5`. The u32 fields of an uploaded file (duration,
//! record lengths) are little-endian; everything else is big-endian.
//!
//! Sources: Holoscope.exe (Windows, Qt5) and Holoscope.apk (Android, Qt6,
//! `DeviceInterface::*` / `MyTcpServer::ReadData`). See `PROTOCOL.md`.

use serde::{Deserialize, Serialize};

pub const UDP_PORT: u16 = 8988;
pub const TCP_PORT: u16 = 6666;

pub const SOF: u8 = 0x5A;
pub const EOF: u8 = 0xF5;

/// Live cast frame is always exactly this long.
pub const CAST_FRAME_SIZE: usize = 0x5000;
/// Largest JPEG that fits in a cast frame (`5A 28 … 55 F5`).
pub const MAX_JPEG: usize = 0x4FFC;
/// Largest JPEG per frame of an uploaded file (the limit of Holoscope's own encoder).
pub const MAX_UPLOAD_JPEG: usize = 0xF000;
/// Holoscope sends the cast frame in chunks of this size and waits for an ack after each.
pub const CAST_CHUNK: usize = 0x3908;

/// Audio in every frame record of an uploaded file: 1/20 s of 16-bit mono PCM at 16 kHz (from the video's sound track, as Holoscope; silence for pictures).
pub const UPLOAD_PCM: usize = 1600;
/// The fan plays uploaded files at 20 frames per second.
pub const FRAME_MS: u32 = 50;
/// Duration Holoscope gives a still picture: 5 s at 20 fps, minus one frame.
pub const STILL_MS: u32 = 4950;
pub const UPLOAD_CHUNK: usize = 1440;

/// Fixed-size string fields in the name / Wi-Fi packets.
pub const NAME_FIELD: usize = 32;

/// Opcodes, PC → fan unless noted.
pub mod op {
    pub const ON: u8 = 0x01;
    pub const OFF: u8 = 0x02;
    pub const PAUSE: u8 = 0x03;
    pub const PLAY: u8 = 0x04;
    /// Loop through the whole playlist ("repeate", 顺序循环).
    pub const REPEAT_ALL: u8 = 0x05;
    /// Loop the current file ("repeateforever", 单循环).
    pub const REPEAT_ONE: u8 = 0x06;
    pub const FORMAT_CARD: u8 = 0x07;
    pub const FACTORY_RESET: u8 = 0x08;
    pub const BREATH_ON: u8 = 0x09;
    pub const BREATH_OFF: u8 = 0x0A;
    pub const AUTOSTART_ON: u8 = 0x0B;
    pub const AUTOSTART_OFF: u8 = 0x0C;
    pub const HEARTBEAT: u8 = 0x0D;
    /// Fan → PC: command / chunk acknowledged.
    pub const READY: u8 = 0x0E;
    pub const TRANS_CANCEL: u8 = 0x10;
    // 18–1B and 8E are the fan's Bluetooth commands. P56 Studio never sends them: Wi-Fi and
    // Bluetooth share one radio, and with a Bluetooth receiver connected the fan cannot rejoin
    // the home Wi-Fi (see PROTOCOL.md).
    pub const GET_MACHINE: u8 = 0x1C;
    /// Fan → PC: transfer step acknowledged / continue.
    pub const TRANS_CONTINUE: u8 = 0x21;
    /// Fan → PC: transfer aborted by the fan.
    pub const TRANS_BREAK: u8 = 0x22;
    pub const SELECT_DEVICE: u8 = 0x23;
    /// Start live mode (Holoscope.exe `samescreen`); the fan answers `5A 29 29 F5`.
    pub const CAST_START: u8 = 0x26;
    pub const CAST_FRAME: u8 = 0x28;
    /// Fan → PC: transfer stored (progress 100 %), and live mode ready after `CAST_START`.
    pub const TRANS_DONE: u8 = 0x29;
    pub const ANGLE_ALIGN_START: u8 = 0x2A;
    pub const ANGLE_ALIGN_STOP: u8 = 0x2B;
    pub const COLOR_TRUE: u8 = 0x2C;
    pub const COLOR_BRIGHT: u8 = 0x2D;
    pub const DELETE: u8 = 0x80;
    pub const BRIGHTNESS: u8 = 0x81;
    pub const ANGLE: u8 = 0x82;
    pub const RENAME: u8 = 0x83;
    /// PC → fan: upload file name. Fan → PC: playlist.
    pub const FILE_NAME: u8 = 0x84;
    /// Fan → PC: error code.
    pub const ERROR: u8 = 0x85;
    /// Fan → PC: status.
    pub const STATUS: u8 = 0x86;
    pub const SELECT: u8 = 0x87;
    pub const DEVICE_NAME: u8 = 0x89;
    pub const HOTSPOT: u8 = 0x8A;
    pub const ROUTER: u8 = 0x8B;
    /// Fan → PC: machine info.
    pub const MACHINE_INFO: u8 = 0x8D;
    /// Fan → PC: list report (Bluetooth state and the speaker volume).
    pub const LIST: u8 = 0x8F;
    pub const VOLUME: u8 = 0x90;
    pub const COMMIT: u8 = 0x91;
}

pub fn xorsum(b: &[u8]) -> u8 {
    b.iter().fold(0u8, |a, &v| a ^ v)
}

/// `5A op op F5`
fn simple(op: u8) -> Vec<u8> {
    vec![SOF, op, op, EOF]
}

/// `5A op v v F5`
fn with_value(op: u8, v: u8) -> Vec<u8> {
    vec![SOF, op, v, v, EOF]
}

pub fn on() -> Vec<u8> {
    simple(op::ON)
}
pub fn off() -> Vec<u8> {
    simple(op::OFF)
}
pub fn pause() -> Vec<u8> {
    simple(op::PAUSE)
}
pub fn play() -> Vec<u8> {
    simple(op::PLAY)
}
pub fn repeat_all() -> Vec<u8> {
    simple(op::REPEAT_ALL)
}
pub fn repeat_one() -> Vec<u8> {
    simple(op::REPEAT_ONE)
}
pub fn format_card() -> Vec<u8> {
    simple(op::FORMAT_CARD)
}
pub fn factory_reset() -> Vec<u8> {
    simple(op::FACTORY_RESET)
}
pub fn breath(on: bool) -> Vec<u8> {
    simple(if on { op::BREATH_ON } else { op::BREATH_OFF })
}
pub fn autostart(on: bool) -> Vec<u8> {
    simple(if on { op::AUTOSTART_ON } else { op::AUTOSTART_OFF })
}
pub fn trans_cancel() -> Vec<u8> {
    simple(op::TRANS_CANCEL)
}
pub fn get_machine() -> Vec<u8> {
    simple(op::GET_MACHINE)
}
pub fn angle_align(start: bool) -> Vec<u8> {
    simple(if start { op::ANGLE_ALIGN_START } else { op::ANGLE_ALIGN_STOP })
}

/// Colour mode: true colour (`5A 2C`) or high brightness (`5A 2D`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    True,
    Bright,
}

impl ColorMode {
    /// Value as the status packet reports it (1 = true colour, 2 = high brightness).
    pub fn from_status(v: u8) -> Option<ColorMode> {
        match v {
            1 => Some(ColorMode::True),
            2 => Some(ColorMode::Bright),
            _ => None,
        }
    }
}

pub fn color_mode(m: ColorMode) -> Vec<u8> {
    simple(match m {
        ColorMode::True => op::COLOR_TRUE,
        ColorMode::Bright => op::COLOR_BRIGHT,
    })
}

/// `5A 0D 0D F5 3E` — Holoscope.exe appends a trailing 0x3E byte.
pub fn heartbeat() -> Vec<u8> {
    vec![SOF, op::HEARTBEAT, op::HEARTBEAT, EOF, 0x3E]
}

/// Select / show slot `n` (1-based).
pub fn select(n: u8) -> Vec<u8> {
    with_value(op::SELECT, n)
}
/// Delete slot `n` (1-based).
pub fn delete(n: u8) -> Vec<u8> {
    with_value(op::DELETE, n)
}
/// Brightness 1–16.
pub fn brightness(v: u8) -> Vec<u8> {
    with_value(op::BRIGHTNESS, v.clamp(1, 16))
}
/// Volume 0–10.
pub fn volume(v: u8) -> Vec<u8> {
    with_value(op::VOLUME, v.min(10))
}
/// Make this fan the selected one of a group: `5A 23 23 F5`, sent to that fan
/// (Holoscope.apk `DeviceInterface::stitchselectdevice`).
pub fn select_device() -> Vec<u8> {
    simple(op::SELECT_DEVICE)
}

/// Display rotation in degrees, 0–360 (Holoscope's slider −180…180 + 180).
pub fn angle(deg: u16) -> Vec<u8> {
    let d = deg.min(360);
    let (hi, lo) = ((d >> 8) as u8, d as u8);
    vec![SOF, op::ANGLE, hi, lo, hi ^ lo, EOF]
}

/// Finish an upload so the firmware stores it in a playlist slot.
/// `frames` is what Holoscope puts in the u32 (frame count for the bin formats, 1 for JPEG).
pub fn commit_with(frames: u32) -> Vec<u8> {
    let b = frames.to_be_bytes();
    vec![SOF, op::COMMIT, b[0], b[1], b[2], b[3], xorsum(&b), EOF]
}
pub fn commit() -> Vec<u8> {
    commit_with(1)
}

/// Names the fan stores are ASCII; anything else becomes `_` so both of the
/// firmware's charsets (GB2312 / local 8-bit) see the same bytes.
pub fn ascii(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { '_' }).collect()
}

/// Longest file name stem sent to the fan, in bytes (before `.mp4`). Measured on a real P56: it
/// stores names up to 24 bytes; 25–27 are silently not stored, and from 28 on the upload also
/// breaks the card's file list (every later upload fails until it is formatted). Holoscope's own
/// UI allows only 12.
pub const MAX_NAME_STEM: usize = 24;

/// A safe file name for the fan, ending in `.mp4` (the firmware only lists `.mp4` names).
/// Only letters, digits, space, `-`, `_`, `.`, `(` and `)` are kept, anything else becomes `_`
/// (so no `/`, `\` or `:` can reach the memory card); no leading dots or spaces; the stem is at
/// most [`MAX_NAME_STEM`] bytes.
pub fn mp4_name(name: &str) -> String {
    let name = name.trim();
    let stem = if name.to_ascii_lowercase().ends_with(".mp4") { &name[..name.len() - 4] } else { name };
    let safe: String =
        stem.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') { c } else { '_' }).take(MAX_NAME_STEM).collect();
    let safe = safe.trim_start_matches(['.', ' ']).trim_end();
    let safe = if safe.trim_matches(['.', '_']).is_empty() { "upload" } else { safe };
    format!("{safe}.mp4")
}

/// `5A op len <bytes> xor F5`, xor over `len` and the bytes.
fn named(op: u8, body: &[u8]) -> Vec<u8> {
    let body = body.get(..255).unwrap_or(body);
    let mut v = Vec::with_capacity(body.len() + 5);
    v.push(SOF);
    v.push(op);
    v.push(body.len() as u8);
    v.extend_from_slice(body);
    v.push(xorsum(&v[2..]));
    v.push(EOF);
    v
}

/// Fixed-layout packet: `5A op <fields…> xor F5`, every field `NAME_FIELD` bytes, zero padded.
/// At most 31 bytes of text per field, so it always ends in a zero (as Holoscope limits it).
fn fixed(op: u8, fields: &[&str]) -> Vec<u8> {
    let mut v = vec![0u8; 2 + fields.len() * NAME_FIELD + 2];
    v[0] = SOF;
    v[1] = op;
    for (i, f) in fields.iter().enumerate() {
        let b = ascii(f).into_bytes();
        let n = b.len().min(NAME_FIELD - 1);
        v[2 + i * NAME_FIELD..2 + i * NAME_FIELD + n].copy_from_slice(&b[..n]);
    }
    let end = v.len();
    v[end - 2] = xorsum(&v[2..end - 2]);
    v[end - 1] = EOF;
    v
}

/// Announce the name of the file about to be uploaded (`.mp4` is appended if missing).
pub fn set_filename(name: &str) -> Vec<u8> {
    named(op::FILE_NAME, mp4_name(name).as_bytes())
}

/// Rename slot `slot` (1-based): `5A 83 slot len name.mp4 xor F5`.
pub fn rename(slot: u8, new_name: &str) -> Vec<u8> {
    let name = mp4_name(new_name);
    let b = name.as_bytes().get(..250).unwrap_or(name.as_bytes());
    let mut v = vec![SOF, op::RENAME, slot, b.len() as u8];
    v.extend_from_slice(b);
    v.push(xorsum(&v[2..]));
    v.push(EOF);
    v
}

/// Fan name shown in Holoscope (36 bytes).
pub fn device_name(name: &str) -> Vec<u8> {
    fixed(op::DEVICE_NAME, &[name])
}

/// Name and password of the fan's own Wi-Fi hotspot (68 bytes). After this the
/// default hotspot name/password no longer work.
pub fn hotspot(ssid: &str, password: &str) -> Vec<u8> {
    fixed(op::HOTSPOT, &[ssid, password])
}

/// Let the fan join a router (68 bytes). It searches for it after this.
pub fn router(ssid: &str, password: &str) -> Vec<u8> {
    fixed(op::ROUTER, &[ssid, password])
}

/// Build an uploaded file the way Holoscope does (`fileprocess::picproces` / `videoprocess`
/// in libappHoloscope): `[duration_ms u32le]`, then one record per frame (see [`upload_record`]).
/// The duration is [`STILL_MS`] for one frame and `frames × 50 ms` for an animation.
/// Returns `(header, records)`: Holoscope writes the 4-byte duration separately.
pub fn upload_blob(jpeg_frames: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
    upload_blob_with_audio(jpeg_frames, None)
}

/// The same with sound: `audio` is 16-bit little-endian mono PCM at 16 kHz from the start of
/// the animation; frame `i` carries bytes `i × 1600 .. (i + 1) × 1600` (silence where it ends).
pub fn upload_blob_with_audio(jpeg_frames: &[Vec<u8>], audio: Option<&[u8]>) -> (Vec<u8>, Vec<u8>) {
    let duration = if jpeg_frames.len() > 1 { FRAME_MS.saturating_mul(jpeg_frames.len() as u32) } else { STILL_MS };
    let size: usize = jpeg_frames.iter().map(|f| 8 + UPLOAD_PCM + f.len()).sum();
    let mut body = Vec::with_capacity(size);
    for (i, f) in jpeg_frames.iter().enumerate() {
        let pcm = audio.and_then(|a| a.get(i * UPLOAD_PCM..)).map(|a| &a[..a.len().min(UPLOAD_PCM)]).unwrap_or(&[]);
        upload_record_with_audio(&mut body, f, pcm);
    }
    (duration.to_le_bytes().to_vec(), body)
}

/// One frame record: `[len u32le][1600 B PCM][JPEG][1–4 × 00]`. The JPEG is padded with at
/// least one zero byte up to a multiple of 4; `len` counts everything after itself.
pub fn upload_record(out: &mut Vec<u8>, jpeg: &[u8]) {
    upload_record_with_audio(out, jpeg, &[]);
}

/// One frame record with up to 1600 bytes of sound (the rest is silence).
pub fn upload_record_with_audio(out: &mut Vec<u8>, jpeg: &[u8], pcm: &[u8]) {
    let padded = (jpeg.len() & !3) + 4;
    out.extend_from_slice(&((UPLOAD_PCM + padded) as u32).to_le_bytes());
    let pcm = &pcm[..pcm.len().min(UPLOAD_PCM)];
    out.extend_from_slice(pcm);
    out.resize(out.len() + UPLOAD_PCM - pcm.len(), 0);
    out.extend_from_slice(jpeg);
    out.resize(out.len() + padded - jpeg.len(), 0);
}

/// A complete JPEG: starts with SOI (`FF D8`) and ends with EOI (`FF D9`).
pub fn jpeg_complete(jpeg: &[u8]) -> bool {
    jpeg.len() >= 4 && jpeg.starts_with(&[0xFF, 0xD8]) && jpeg.ends_with(&[0xFF, 0xD9])
}

/// `5A 26 26 F5`: switch the fan to live mode before the first cast frame (it answers
/// `5A 29 29 F5`). Without it a P56 acknowledges the frames but keeps playing its playlist.
pub fn cast_start() -> Vec<u8> {
    simple(op::CAST_START)
}

/// Wrap a JPEG into a 20480-byte live cast frame: `5A 28 | jpeg | 00… | 55 F5`.
/// The JPEG must be complete and at most [`MAX_JPEG`] bytes (it is never cut).
pub fn cast_frame(jpeg: &[u8]) -> Option<Vec<u8>> {
    if jpeg.len() > MAX_JPEG || !jpeg_complete(jpeg) {
        return None;
    }
    let mut f = vec![0u8; CAST_FRAME_SIZE];
    let n = jpeg.len();
    f[0] = SOF;
    f[1] = op::CAST_FRAME;
    f[2..2 + n].copy_from_slice(&jpeg[..n]);
    f[CAST_FRAME_SIZE - 2] = 0x55;
    f[CAST_FRAME_SIZE - 1] = EOF;
    Some(f)
}

// ---------------------------------------------------------------------------
// Packets coming back from the fan
// ---------------------------------------------------------------------------

/// `5A 86` status, 11 bytes:
/// `5A 86 flags slot brightness (color<<4 | angle_hi) angle_lo group type extra F5`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Status {
    pub flags: u8,
    pub power: bool,
    /// Holoscope's `play_stop_mode`: playing (true) or paused.
    pub playing: bool,
    /// Holoscope's `repeate_mode`: whole playlist (true) or current file.
    pub repeat_all: bool,
    pub breath: bool,
    pub autostart: bool,
    /// Slot of the playing file, 1-based like every slot (`select`, the playlist). The fan sends
    /// it 0-based: Holoscope compares it with its 0-based list positions and selects with
    /// position + 1 (verified in Holoscope.apk `DeviceInterface::playindex` and its QML).
    pub play_index: u8,
    /// 1–16.
    pub brightness: u8,
    /// 1 = true colour, 2 = high brightness.
    pub color_mode: u8,
    /// Display rotation 0–360 (180 = neutral).
    pub angle: u16,
    pub group: u8,
    pub fan_type: u8,
    pub extra: u8,
}

impl Status {
    pub fn parse(body: &[u8]) -> Option<Status> {
        if body.len() < 8 {
            return None;
        }
        let f = body[0];
        Some(Status {
            flags: f,
            power: f & 0x40 != 0,
            autostart: f & 0x20 != 0,
            breath: f & 0x10 != 0,
            playing: f & 0x08 != 0,
            repeat_all: f & 0x04 != 0,
            play_index: body[1].saturating_add(1),
            brightness: body[2],
            color_mode: body[3] >> 4,
            angle: (((body[3] & 0x0F) as u16) << 8) | body[4] as u16,
            group: body[5],
            fan_type: body[6],
            extra: body[7],
        })
    }

    /// Inverse of [`Status::parse`], used by the simulator.
    pub fn to_packet(&self) -> Vec<u8> {
        let mut f = 0u8;
        if self.power {
            f |= 0x40
        }
        if self.autostart {
            f |= 0x20
        }
        if self.breath {
            f |= 0x10
        }
        if self.playing {
            f |= 0x08
        }
        if self.repeat_all {
            f |= 0x04
        }
        let a = self.angle.min(0x0FFF);
        vec![
            SOF,
            op::STATUS,
            f,
            self.play_index.saturating_sub(1),
            self.brightness,
            (self.color_mode << 4) | (a >> 8) as u8,
            a as u8,
            self.group,
            self.fan_type,
            self.extra,
            EOF,
        ]
    }
}

/// `5A 8D` machine info, 126 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MachineInfo {
    /// Place in a group picture (bytes 6–9: width, height, x, y), all 0 when the fan is not in
    /// one. (Earlier taken for an IP address; Holoscope.apk reads it as this layout.)
    pub stitch: [u8; 4],
    /// Device name (bytes 14–45).
    pub name: String,
    pub mac: String,
    /// Name of the fan's own hotspot (bytes 52–83).
    pub hotspot: String,
    /// Router the fan joins (bytes 84–115); older firmware puts a version here.
    pub router: String,
}

pub const MACHINE_INFO_LEN: usize = 126;

impl MachineInfo {
    pub fn parse(pkt: &[u8]) -> Option<MachineInfo> {
        if pkt.len() < 116 {
            return None;
        }
        Some(MachineInfo {
            stitch: [pkt[6], pkt[7], pkt[8], pkt[9]],
            name: cstr(&pkt[14..46]),
            mac: pkt[46..52].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"),
            hotspot: cstr(&pkt[52..84]),
            router: cstr(&pkt[84..116]),
        })
    }

    pub fn to_packet(&self) -> Vec<u8> {
        let mut r = vec![0u8; MACHINE_INFO_LEN];
        r[0] = SOF;
        r[1] = op::MACHINE_INFO;
        r[125] = EOF;
        r[6..10].copy_from_slice(&self.stitch);
        put_str(&mut r[14..46], &self.name);
        for (i, p) in self.mac.split(':').take(6).enumerate() {
            r[46 + i] = u8::from_str_radix(p, 16).unwrap_or(0);
        }
        put_str(&mut r[52..84], &self.hotspot);
        put_str(&mut r[84..116], &self.router);
        r
    }
}

/// Fan-side error from `5A 85 code … F5`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanError {
    /// 0x06: a file with this name already exists.
    DuplicateName,
    /// 0x0A: playlist / card is full.
    TooManyFiles,
    /// Anything else: transfer failed (check the memory card).
    TransferFailed(u8),
}

impl FanError {
    pub fn from_code(c: u8) -> FanError {
        match c {
            0x06 => FanError::DuplicateName,
            0x0A => FanError::TooManyFiles,
            c => FanError::TransferFailed(c),
        }
    }
    pub fn code(&self) -> u8 {
        match self {
            FanError::DuplicateName => 0x06,
            FanError::TooManyFiles => 0x0A,
            FanError::TransferFailed(c) => *c,
        }
    }
    pub fn to_packet(&self) -> Vec<u8> {
        vec![SOF, op::ERROR, self.code(), self.code(), EOF]
    }
}

impl std::fmt::Display for FanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FanError::DuplicateName => write!(f, "a file with this name already exists on the fan"),
            FanError::TooManyFiles => write!(f, "maximum number of files reached"),
            FanError::TransferFailed(c) => write!(f, "transfer failed (code {c:#04x}); check the memory card"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Packet {
    /// `5A 0E 0E F5`: command / chunk acknowledged.
    Ready,
    TransContinue,
    TransBreak,
    TransDone,
    Status(Status),
    MachineInfo(MachineInfo),
    /// Playlist as the fan reports it: `(slot, name)`, 1-based.
    FileList(Vec<(u8, String)>),
    Error(FanError),
    /// `5A 8F flags volume len16 "a/b/c" xor F5`: Bluetooth state, speaker volume and the
    /// devices found by the last scan. Still parsed because it is the only place the fan
    /// reports the speaker volume; P56 Studio does not use the fan's Bluetooth.
    Bluetooth(Bluetooth),
    /// Any other `5A xx … F5` packet.
    Other {
        op: u8,
        body: Vec<u8>,
    },
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

fn put_str(dst: &mut [u8], s: &str) {
    let b = ascii(s).into_bytes();
    let n = b.len().min(dst.len());
    dst[..n].copy_from_slice(&b[..n]);
}

fn split_items(text: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(text).split('/').map(|t| t.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string()).filter(|t| !t.is_empty()).collect()
}

pub fn parse_filelist(body: &[u8]) -> Vec<(u8, String)> {
    // body = <len16 be> <"1 a/2 b", "1 a.mp4/2 b.mp4" or "a.mp4/b.mp4"> <xor>
    // A real P56 lists names without ".mp4"; it is added back, so a name always matches the
    // name it was uploaded, renamed or deleted with.
    if body.len() < 2 {
        return vec![];
    }
    let n = u16::from_be_bytes([body[0], body[1]]) as usize;
    let mut out = Vec::new();
    let mut next = 1u8;
    for tok in split_items(&body[2..(2 + n).min(body.len())]) {
        let (idx, name) = match tok.split_once(' ') {
            Some((i, n)) if !i.is_empty() && i.chars().all(|c| c.is_ascii_digit()) => (i.parse().unwrap_or(next), n.trim().to_string()),
            _ => (next, tok),
        };
        let name = if name.to_ascii_lowercase().ends_with(".mp4") { name } else { format!("{name}.mp4") };
        out.push((idx, name));
        next = idx.wrapping_add(1);
    }
    out
}

/// Playlist packet as a real P56 sends it, names without ".mp4" (used by the simulator).
pub fn filelist_packet(names: &[String]) -> Vec<u8> {
    let stem = |n: &str| if n.to_ascii_lowercase().ends_with(".mp4") { n[..n.len() - 4].to_string() } else { n.to_string() };
    let text: String = names.iter().enumerate().map(|(i, n)| format!("{} {}", i + 1, ascii(&stem(n)))).collect::<Vec<_>>().join("/");
    let t = text.as_bytes().get(..2000).unwrap_or(text.as_bytes());
    let mut v = vec![SOF, op::FILE_NAME, (t.len() >> 8) as u8, t.len() as u8];
    v.extend_from_slice(t);
    v.push(xorsum(&v[2..]));
    v.push(EOF);
    v
}

/// What the fan reports in `5A 8F` (Holoscope.apk `MyTcpServer::ReadData`): byte 2 holds the
/// Bluetooth state (bit 7 on, bit 6 connected, bit 5 connecting), byte 3 the speaker volume
/// (0–10), then the names found by the last scan, `/`-separated (at most 8).
/// Only the volume is used: P56 Studio does not use the fan's Bluetooth.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Bluetooth {
    pub on: bool,
    pub connected: bool,
    pub connecting: bool,
    pub volume: u8,
    pub devices: Vec<String>,
}

impl Bluetooth {
    pub fn from_header(flags: u8, volume: u8, devices: Vec<String>) -> Bluetooth {
        let on = flags & 0x80 != 0;
        Bluetooth { on, connected: on && flags & 0x40 != 0, connecting: on && flags & 0x20 != 0, volume, devices }
    }
    fn flags(&self) -> u8 {
        (self.on as u8) << 7 | (self.connected as u8) << 6 | (self.connecting as u8) << 5
    }
}

/// `8F` report packet (used by the simulator to report the volume).
pub fn bluetooth_packet(b: &Bluetooth) -> Vec<u8> {
    let text = b.devices.iter().map(|s| ascii(s)).collect::<Vec<_>>().join("/");
    let t = text.as_bytes().get(..2000).unwrap_or(text.as_bytes());
    let mut v = vec![SOF, op::LIST, b.flags(), b.volume, (t.len() >> 8) as u8, t.len() as u8];
    v.extend_from_slice(t);
    v.push(xorsum(&v[2..]));
    v.push(EOF);
    v
}

pub fn parse_packet(pkt: &[u8]) -> Option<Packet> {
    if pkt.len() < 4 || pkt[0] != SOF || *pkt.last()? != EOF {
        return None;
    }
    let op = pkt[1];
    let body = &pkt[2..pkt.len() - 1];
    Some(match op {
        op::READY => Packet::Ready,
        op::TRANS_CONTINUE => Packet::TransContinue,
        op::TRANS_BREAK => Packet::TransBreak,
        op::TRANS_DONE => Packet::TransDone,
        op::STATUS => match Status::parse(body) {
            Some(s) => Packet::Status(s),
            None => Packet::Other { op, body: body.to_vec() },
        },
        op::MACHINE_INFO => match MachineInfo::parse(pkt) {
            Some(i) => Packet::MachineInfo(i),
            None => Packet::Other { op, body: body.to_vec() },
        },
        op::FILE_NAME => Packet::FileList(parse_filelist(body)),
        op::ERROR => Packet::Error(FanError::from_code(body[0])),
        op::LIST if body.len() >= 4 => {
            let n = u16::from_be_bytes([body[2], body[3]]) as usize;
            Packet::Bluetooth(Bluetooth::from_header(body[0], body[1], split_items(&body[4..(4 + n).min(body.len())])))
        }
        _ => Packet::Other { op, body: body.to_vec() },
    })
}

/// Total length of the packet starting at `buf[0]` (= `5A`), if it can be determined yet.
/// `Err(())` means "not a packet we can frame by length; scan for F5".
fn framed_len(buf: &[u8]) -> Result<Option<usize>, ()> {
    let need = |n: usize| if buf.len() >= n { Ok(Some(n)) } else { Ok(None) };
    match buf.get(1) {
        None => Ok(None),
        Some(&op::MACHINE_INFO) => need(MACHINE_INFO_LEN),
        Some(&op::STATUS) => need(11),
        Some(&op::ERROR) => need(5),
        Some(&(op::READY | op::TRANS_CONTINUE | op::TRANS_BREAK | op::TRANS_DONE)) => need(4),
        Some(&op::FILE_NAME) => {
            if buf.len() < 4 {
                return Ok(None);
            }
            need(4 + u16::from_be_bytes([buf[2], buf[3]]) as usize + 2)
        }
        Some(&op::LIST) => {
            if buf.len() < 6 {
                return Ok(None);
            }
            need(6 + u16::from_be_bytes([buf[4], buf[5]]) as usize + 2)
        }
        _ => Err(()),
    }
}

/// Incremental splitter for the fan → PC byte stream.
#[derive(Default)]
pub struct Reader {
    buf: Vec<u8>,
}

impl Reader {
    pub fn push(&mut self, data: &[u8]) -> Vec<Packet> {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            let Some(start) = self.buf.iter().position(|&b| b == SOF) else {
                self.buf.clear();
                break;
            };
            if start > 0 {
                self.buf.drain(..start);
            }
            let len = match framed_len(&self.buf) {
                Ok(Some(n)) => n,
                Ok(None) => break,
                Err(()) => match self.buf.iter().skip(2).position(|&b| b == EOF) {
                    Some(p) => p + 3,
                    None => {
                        if self.buf.len() > 4096 {
                            self.buf.clear();
                        }
                        break;
                    }
                },
            };
            if self.buf[len - 1] != EOF {
                // Not a real frame start: skip this 5A and resync.
                self.buf.remove(0);
                continue;
            }
            let pkt: Vec<u8> = self.buf.drain(..len).collect();
            if let Some(p) = parse_packet(&pkt) {
                out.push(p);
            }
            // The fan pads 4-byte packets to 5 bytes (like Holoscope's heartbeat 3E).
            if pkt.len() == 4 && self.buf.first().is_some_and(|&b| b != SOF) {
                self.buf.remove(0);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_checksum_matches_v25() {
        let p = set_filename("eddie.mp4");
        assert_eq!(&p[..3], &[0x5A, 0x84, 9]);
        assert_eq!(&p[3..12], b"eddie.mp4");
        assert_eq!(p[12], xorsum(&p[2..12]));
        assert_eq!(p[13], 0xF5);
        assert_eq!(set_filename("eddie"), p);
    }

    #[test]
    fn simple_commands_match_holoscope() {
        assert_eq!(on(), [0x5A, 0x01, 0x01, 0xF5]);
        assert_eq!(pause(), [0x5A, 0x03, 0x03, 0xF5]);
        assert_eq!(repeat_all(), [0x5A, 0x05, 0x05, 0xF5]);
        assert_eq!(format_card(), [0x5A, 0x07, 0x07, 0xF5]);
        assert_eq!(breath(false), [0x5A, 0x0A, 0x0A, 0xF5]);
        assert_eq!(autostart(true), [0x5A, 0x0B, 0x0B, 0xF5]);
        assert_eq!(color_mode(ColorMode::Bright), [0x5A, 0x2D, 0x2D, 0xF5]);
        assert_eq!(select(3), [0x5A, 0x87, 3, 3, 0xF5]);
        assert_eq!(brightness(40), [0x5A, 0x81, 16, 16, 0xF5]);
        assert_eq!(volume(7), [0x5A, 0x90, 7, 7, 0xF5]);
        assert_eq!(angle(300), [0x5A, 0x82, 0x01, 0x2C, 0x01 ^ 0x2C, 0xF5]);
        assert_eq!(commit(), [0x5A, 0x91, 0, 0, 0, 1, 1, 0xF5]);
    }

    #[test]
    fn fixed_layout_wifi() {
        let p = hotspot("MyFan", "secret12");
        assert_eq!(p.len(), 68);
        assert_eq!(&p[..2], &[0x5A, 0x8A]);
        assert_eq!(&p[2..7], b"MyFan");
        assert_eq!(&p[34..42], b"secret12");
        assert_eq!(p[66], xorsum(&p[2..66]));
        assert_eq!(p[67], 0xF5);
        assert_eq!(router("x", "y")[1], 0x8B);
        // 32 characters: cut to 31 so the field keeps its terminating zero
        let long = hotspot(&"S".repeat(32), &"p".repeat(40));
        assert_eq!(&long[2..33], "S".repeat(31).as_bytes());
        assert_eq!(long[33], 0);
        assert_eq!(&long[34..65], "p".repeat(31).as_bytes());
        assert_eq!(long[65], 0);
        let n = device_name("Fan 1");
        assert_eq!(n.len(), 36);
        assert_eq!(n[34], xorsum(&n[2..34]));
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(mp4_name("clip"), "clip.mp4");
        assert_eq!(mp4_name("Clip.MP4"), "Clip.mp4");
        assert_eq!(mp4_name("../../etc/passwd"), "_.._etc_passwd.mp4");
        assert_eq!(mp4_name("a/b\\c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j.mp4");
        assert_eq!(mp4_name("Café Ø"), "Caf_ _.mp4");
        assert_eq!(mp4_name(".hidden"), "hidden.mp4");
        assert_eq!(mp4_name(".."), "upload.mp4");
        assert_eq!(mp4_name("   "), "upload.mp4");
        assert_eq!(mp4_name(&"x".repeat(300)).len(), MAX_NAME_STEM + 4);
    }

    #[test]
    fn rename_layout() {
        let p = rename(2, "clip");
        assert_eq!(&p[..4], &[0x5A, 0x83, 2, 8]);
        assert_eq!(&p[4..12], b"clip.mp4");
        assert_eq!(p[12], xorsum(&p[2..12]));
    }

    #[test]
    fn status_roundtrip() {
        let s = Status {
            power: true,
            playing: true,
            repeat_all: true,
            breath: false,
            autostart: true,
            play_index: 2,
            brightness: 12,
            color_mode: 2,
            angle: 270,
            group: 1,
            fan_type: 8,
            extra: 0,
            flags: 0,
        };
        let pkt = s.to_packet();
        assert_eq!(pkt.len(), 11);
        let Some(Packet::Status(p)) = parse_packet(&pkt) else { panic!() };
        assert_eq!(Status { flags: p.flags, ..s.clone() }, p);
        assert_eq!(p.flags, 0x40 | 0x20 | 0x08 | 0x04);
    }

    #[test]
    fn status_with_f5_inside_frames_by_length() {
        // play_index 0xF6 goes out as 0xF5 (0-based)
        let s = Status { angle: 0xF5, play_index: 0xF6, ..Default::default() };
        let mut r = Reader::default();
        let mut stream = s.to_packet();
        stream.extend(vec![0x5A, 0x0E, 0x0E, 0xF5]);
        let out = r.push(&stream);
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0], Packet::Status(p) if p.angle == 0xF5 && p.play_index == 0xF6));
        assert_eq!(out[1], Packet::Ready);
    }

    #[test]
    fn machine_info_roundtrip() {
        let i = MachineInfo { stitch: [2, 1, 1, 0], name: "P56".into(), mac: "02:e0:e0:e0:e0:f5".into(), hotspot: "HOLO-1".into(), router: "home".into() };
        let pkt = i.to_packet();
        let mut r = Reader::default();
        // feed byte by byte: MAC contains F5
        let mut out = vec![];
        for b in &pkt {
            out.extend(r.push(&[*b]));
        }
        assert_eq!(out, vec![Packet::MachineInfo(i)]);
    }

    #[test]
    fn filelist_and_list_roundtrip() {
        let names = vec!["a.mp4".to_string(), "b c.mp4".to_string()];
        let mut r = Reader::default();
        let out = r.push(&filelist_packet(&names));
        assert_eq!(out, vec![Packet::FileList(vec![(1, "a.mp4".into()), (2, "b c.mp4".into())])]);
        // a real P56 lists the names without ".mp4" (as the simulator does)
        assert!(filelist_packet(&names).windows(4).all(|w| w != b".mp4"));
        let mut body = vec![0, 19];
        body.extend_from_slice(b"1 fazant/2 Clip.MP4");
        assert_eq!(parse_filelist(&body), vec![(1, "fazant.mp4".into()), (2, "Clip.MP4".into())]);
        let bt = Bluetooth { on: true, connected: true, connecting: false, volume: 7, devices: vec!["dev1".into(), "dev2".into()] };
        let out = r.push(&bluetooth_packet(&bt));
        assert_eq!(out, vec![Packet::Bluetooth(bt)]);
        // as Holoscope reads it: 0x80 on, 0xC0 connected, 0xA0 connecting; byte 3 the volume
        assert_eq!(Bluetooth::from_header(0xC0, 4, vec![]), Bluetooth { on: true, connected: true, volume: 4, ..Default::default() });
        assert_eq!(Bluetooth::from_header(0xA0, 0, vec![]), Bluetooth { on: true, connecting: true, ..Default::default() });
        assert_eq!(Bluetooth::from_header(0x40, 0, vec![]), Bluetooth::default(), "off: nothing else counts");
        let out = r.push(&FanError::DuplicateName.to_packet());
        assert_eq!(out, vec![Packet::Error(FanError::DuplicateName)]);
    }

    #[test]
    fn reader_resyncs_after_garbage_and_padding() {
        let mut r = Reader::default();
        let mut s = vec![0x00, 0x12, 0x5A, 0x0E, 0x0E, 0xF5, 0x3E];
        s.extend(Status::default().to_packet());
        let out = r.push(&s);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn cast_frame_shape() {
        let f = cast_frame(&[0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
        assert_eq!(f.len(), CAST_FRAME_SIZE);
        assert_eq!(&f[..6], &[0x5A, 0x28, 0xFF, 0xD8, 0xFF, 0xD9]);
        assert_eq!(&f[CAST_FRAME_SIZE - 2..], &[0x55, 0xF5]);
    }

    #[test]
    fn cast_frame_never_cuts_a_jpeg() {
        let mut big = vec![0xFF, 0xD8];
        big.resize(MAX_JPEG + 1 - 2, 0x11);
        big.extend([0xFF, 0xD9]);
        assert!(cast_frame(&big).is_none(), "too large: refused, not truncated");
        assert!(cast_frame(&[0xFF, 0xD8, 0x00, 0x00]).is_none(), "no EOI");
        big.truncate(MAX_JPEG - 2);
        big.extend([0xFF, 0xD9]);
        assert!(cast_frame(&big).is_some(), "exactly MAX_JPEG fits");
    }

    /// Golden bytes for an animation, written out by hand from Holoscope's
    /// `fileprocess::videoprocess` (libappHoloscope 2.5.2 and 2.5.4): one duration, then one
    /// record per frame with its own length, 1600 PCM bytes and padding.
    #[test]
    fn upload_blob_animation_golden() {
        let j1 = vec![0xFF, 0xD8, 0xAA, 0xFF, 0xD9];
        let j2 = vec![0xFF, 0xD8, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0xD9];
        let (head, body) = upload_blob(&[j1.clone(), j2.clone()]);
        assert_eq!(head, [0x64, 0, 0, 0], "2 frames × 50 ms");
        let mut want = vec![0x48, 0x06, 0, 0]; // 1600 + 8
        want.extend([0u8; 1600]);
        want.extend(&j1);
        want.extend([0, 0, 0]);
        want.extend([0x4C, 0x06, 0, 0]); // 1600 + 12
        want.extend([0u8; 1600]);
        want.extend(&j2);
        want.extend([0, 0, 0, 0]);
        assert_eq!(body, want);
    }

    #[test]
    fn upload_records_one_header_per_frame_aligned() {
        let frames: Vec<Vec<u8>> = (0..7usize)
            .map(|i| {
                let mut j = vec![0xFF, 0xD8];
                j.resize(2 + i * 3, 0x42);
                j.extend([0xFF, 0xD9]);
                j
            })
            .collect();
        let (head, body) = upload_blob(&frames);
        assert_eq!(u32::from_le_bytes(head.try_into().unwrap()), 7 * FRAME_MS);
        let mut at = 0;
        for f in &frames {
            let len = u32::from_le_bytes(body[at..at + 4].try_into().unwrap()) as usize;
            assert_eq!(len % 4, 0, "records stay 4-byte aligned");
            let rec = &body[at + 4..at + 4 + len];
            assert!(rec[..UPLOAD_PCM].iter().all(|&b| b == 0), "1600 PCM bytes (silence)");
            assert_eq!(&rec[UPLOAD_PCM..UPLOAD_PCM + f.len()], &f[..], "exactly one JPEG");
            let pad = &rec[UPLOAD_PCM + f.len()..];
            assert!((1..=4).contains(&pad.len()) && pad.iter().all(|&b| b == 0), "1–4 zero bytes of padding");
            at += 4 + len;
        }
        assert_eq!(at, body.len());
    }

    #[test]
    fn upload_records_carry_the_sound_in_time() {
        let j = vec![0xFF, 0xD8, 0x01, 0xFF, 0xD9];
        let audio: Vec<u8> = (0..UPLOAD_PCM * 5 / 2).map(|i| (i / UPLOAD_PCM + 1) as u8).collect(); // 2.5 frames
        let (_, body) = upload_blob_with_audio(&[j.clone(), j.clone(), j.clone(), j.clone()], Some(&audio));
        let rec = |n: usize| {
            let size = 4 + UPLOAD_PCM + 8;
            &body[n * size + 4..n * size + 4 + UPLOAD_PCM]
        };
        assert!(rec(0).iter().all(|&b| b == 1), "frame 1: the first 50 ms");
        assert!(rec(1).iter().all(|&b| b == 2), "frame 2: the next 50 ms");
        assert!(rec(2)[..UPLOAD_PCM / 2].iter().all(|&b| b == 3) && rec(2)[UPLOAD_PCM / 2..].iter().all(|&b| b == 0), "frame 3: the last half, then silence");
        assert!(rec(3).iter().all(|&b| b == 0), "frame 4: silence");
        assert_eq!(upload_blob_with_audio(std::slice::from_ref(&j), None), upload_blob(&[j]), "no sound = silence");
    }

    /// A still picture must stay byte-for-byte what already works on real fans
    /// (`[duration][1600 + padded JPEG][1600 × 00][JPEG 00 …]`, padded to 4).
    #[test]
    fn upload_blob_still_matches_hardware_proven_layout() {
        for extra in 0..8 {
            let mut j = vec![0xFF, 0xD8];
            j.resize(2 + extra, 0x7E);
            j.extend([0xFF, 0xD9]);
            let (head, body) = upload_blob(&[j.clone()]);
            assert_eq!(u32::from_le_bytes(head.try_into().unwrap()), STILL_MS);
            let mut payload = j.clone();
            payload.push(0);
            while !payload.len().is_multiple_of(4) {
                payload.push(0);
            }
            let mut want = ((UPLOAD_PCM + payload.len()) as u32).to_le_bytes().to_vec();
            want.extend(vec![0u8; UPLOAD_PCM]);
            want.extend(&payload);
            assert_eq!(body, want, "JPEG of {} bytes", j.len());
        }
    }
}
