//! Wire protocol of the Holoscope / P56 POV hologram fan.
//!
//! Every packet is `5A <opcode> <body…> F5`. Text-carrying packets end with an
//! XOR checksum over `<len><ascii>` right before the `F5`. The upload header
//! (tag, size) is little-endian u32; everything else is big-endian.

use serde::{Deserialize, Serialize};

pub const UDP_PORT: u16 = 8988;
pub const TCP_PORT: u16 = 6666;

pub const SOF: u8 = 0x5A;
pub const EOF: u8 = 0xF5;

/// Live cast frame is always exactly this long.
pub const CAST_FRAME_SIZE: usize = 0x5000;
/// Largest JPEG that fits in a cast frame (`5A 28 … 55 F5`).
pub const MAX_JPEG: usize = 0x4FFC;
/// Holoscope sends the cast frame in chunks of this size and waits for an ack after each.
pub const CAST_CHUNK: usize = 0x3908;

/// Upload file header: 1600 zero bytes before the JPEG frames.
pub const UPLOAD_PADDING: usize = 1600;
pub const UPLOAD_TAG_BASE: u32 = 4950;
pub const UPLOAD_CHUNK: usize = 1440;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Op {
    On = 0x01,
    Off = 0x02,
    Pause = 0x03,
    Play = 0x04,
    Heartbeat = 0x0D,
    Ready = 0x0E,
    TransCancel = 0x10,
    GetMachine = 0x1C,
    SelectDevice = 0x23,
    CastFrame = 0x28,
    AngleAlignStart = 0x2A,
    AngleAlignStop = 0x2B,
    Delete = 0x80,
    Rename = 0x83,
    FileName = 0x84,
    Status = 0x86,
    Select = 0x87,
    Wifi = 0x8A,
    MachineInfo = 0x8D,
    TransStart = 0x90,
    Commit = 0x91,
    GroupSaved = 0x93,
    Charset = 0x94,
    GroupAdd = 0x95,
}

pub fn xorsum(b: &[u8]) -> u8 {
    b.iter().fold(0u8, |a, &v| a ^ v)
}

fn simple(op: u8) -> Vec<u8> {
    vec![SOF, op, op, EOF]
}

pub fn on() -> Vec<u8> { simple(Op::On as u8) }
pub fn off() -> Vec<u8> { simple(Op::Off as u8) }
pub fn pause() -> Vec<u8> { simple(Op::Pause as u8) }
pub fn play() -> Vec<u8> { simple(Op::Play as u8) }
pub fn get_machine() -> Vec<u8> { simple(Op::GetMachine as u8) }

/// `5A 0D 0D F5 3E` — Holoscope appends a trailing 0x3E byte.
pub fn heartbeat() -> Vec<u8> { vec![SOF, 0x0D, 0x0D, EOF, 0x3E] }

/// Select / show slot `n` (1-based).
pub fn select(n: u8) -> Vec<u8> { vec![SOF, Op::Select as u8, n, n, EOF] }
/// Delete slot `n` (1-based).
pub fn delete(n: u8) -> Vec<u8> { vec![SOF, Op::Delete as u8, n, n, EOF] }
/// Finish an upload so the firmware stores it in a playlist slot.
pub fn commit() -> Vec<u8> { vec![SOF, Op::Commit as u8, 0, 0, 0, 1, 1, EOF] }

fn named(op: u8, body: &[u8]) -> Vec<u8> {
    let body = body.get(..255).unwrap_or(body);
    let mut v = Vec::with_capacity(body.len() + 5);
    v.push(SOF);
    v.push(op);
    v.push(body.len() as u8);
    v.extend_from_slice(body);
    let x = xorsum(&v[2..]);
    v.push(x);
    v.push(EOF);
    v
}

/// Announce the name of the file about to be uploaded. Must end in `.mp4` or the
/// firmware will not add it to the playlist.
pub fn set_filename(name: &str) -> Vec<u8> { named(Op::FileName as u8, ascii(name).as_bytes()) }
pub fn rename(old: &str, new: &str) -> Vec<u8> {
    named(Op::Rename as u8, format!("{}\0{}", ascii(old), ascii(new)).as_bytes())
}
pub fn wifi(ssid: &str, password: &str) -> Vec<u8> {
    named(Op::Wifi as u8, format!("{}\0{}", ascii(ssid), ascii(password)).as_bytes())
}

fn ascii(s: &str) -> String { s.chars().filter(|c| c.is_ascii() && *c != '\0').collect() }

/// Build the upload blob: `[tag u32le][size u32le][1600×00][frames…][00]`.
/// Returns `(header_part, body_part)` — Holoscope writes the 4-byte tag separately.
pub fn upload_blob(jpeg_frames: &[Vec<u8>], tag: u32) -> (Vec<u8>, Vec<u8>) {
    let payload_len: usize = jpeg_frames.iter().map(|f| f.len()).sum::<usize>() + 1;
    let size = (UPLOAD_PADDING + payload_len) as u32;
    let part1 = tag.to_le_bytes().to_vec();
    let mut part2 = Vec::with_capacity(4 + UPLOAD_PADDING + payload_len);
    part2.extend_from_slice(&size.to_le_bytes());
    part2.resize(4 + UPLOAD_PADDING, 0);
    for f in jpeg_frames { part2.extend_from_slice(f); }
    part2.push(0);
    (part1, part2)
}

/// Unique-enough upload tag, as Holoscope computes it.
pub fn upload_tag(seq: u32) -> u32 {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0);
    UPLOAD_TAG_BASE.wrapping_add(ms & 0xFFFF).wrapping_add(seq)
}

/// Wrap a JPEG into a 20480-byte live cast frame: `5A 28 | jpeg | 00… | 55 F5`.
pub fn cast_frame(jpeg: &[u8]) -> [u8; CAST_FRAME_SIZE] {
    let mut f = [0u8; CAST_FRAME_SIZE];
    let n = jpeg.len().min(MAX_JPEG);
    f[0] = SOF;
    f[1] = Op::CastFrame as u8;
    f[2..2 + n].copy_from_slice(&jpeg[..n]);
    f[CAST_FRAME_SIZE - 2] = 0x55;
    f[CAST_FRAME_SIZE - 1] = EOF;
    f
}

// ---------------------------------------------------------------------------
// Packets coming back from the fan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Status {
    pub flags: u8,
    pub power: bool,
    pub playing: bool,
    /// Currently playing slot (1-based, 0 = none).
    pub play_index: u8,
    pub leds_per_arm: u8,
    pub angles: u16,
    pub fan_type: u8,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MachineInfo {
    pub name: String,
    pub mac: String,
    pub wifi: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Packet {
    /// `5A 0E 0E F5`: command / chunk acknowledged.
    Ready,
    Status(Status),
    MachineInfo(MachineInfo),
    /// Playlist as the fan reports it: `(slot, name)`, 1-based.
    FileList(Vec<(u8, String)>),
    /// Any other `5A xx … F5` packet.
    Other { op: u8, body: Vec<u8> },
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

fn parse_filelist(body: &[u8]) -> Vec<(u8, String)> {
    // body = <len16 be> <ascii "1 a.mp4/2 b.mp4"> <xor>
    if body.len() < 3 { return vec![]; }
    let n = u16::from_be_bytes([body[0], body[1]]) as usize;
    let text = String::from_utf8_lossy(&body[2..(2 + n).min(body.len())]);
    let mut out = Vec::new();
    let mut next = 1u8;
    for tok in text.split('/').map(str::trim).filter(|t| !t.is_empty()) {
        let (idx, name) = match tok.split_once(' ') {
            Some((i, n)) if i.chars().all(|c| c.is_ascii_digit()) => (i.parse().unwrap_or(next), n.trim()),
            _ => (next, tok),
        };
        out.push((idx, name.to_string()));
        next = idx.wrapping_add(1);
    }
    out
}

pub fn parse_packet(pkt: &[u8]) -> Option<Packet> {
    if pkt.len() < 3 || pkt[0] != SOF || *pkt.last()? != EOF { return None; }
    let op = pkt[1];
    let body = &pkt[2..pkt.len() - 1];
    Some(match op {
        0x0E => Packet::Ready,
        0x86 if body.len() >= 8 => {
            let flags = body[0];
            Packet::Status(Status {
                flags,
                power: flags & 0x10 != 0 || flags & 0x02 != 0,
                playing: flags & 0x0C != 0,
                play_index: body[1],
                leds_per_arm: body[2],
                angles: u16::from_be_bytes([body[3], body[4]]),
                fan_type: body[6],
                raw: pkt.to_vec(),
            })
        }
        0x8D if pkt.len() >= 120 => Packet::MachineInfo(MachineInfo {
            name: cstr(&pkt[14..46]),
            mac: pkt[46..52].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"),
            wifi: cstr(&pkt[52..84]),
            version: cstr(&pkt[84..pkt.len().min(116)]),
        }),
        0x84 => Packet::FileList(parse_filelist(body)),
        _ => Packet::Other { op, body: body.to_vec() },
    })
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
            if start > 0 { self.buf.drain(..start); }
            if self.buf.len() < 3 { break; }
            // 8D info is fixed 126 bytes and may contain 0xF5 inside the name/MAC.
            let end = if self.buf[1] == 0x8D {
                if self.buf.len() < 126 { break; }
                125
            } else if self.buf[1] == 0x84 && self.buf.len() >= 4 {
                let n = u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize;
                let e = 4 + n + 1; // + xor, then F5
                if self.buf.len() <= e { break; }
                e
            } else {
                match self.buf[2..].iter().position(|&b| b == EOF) {
                    Some(p) => p + 2,
                    None => { if self.buf.len() > 4096 { self.buf.clear(); } break; }
                }
            };
            let pkt: Vec<u8> = self.buf.drain(..=end).collect();
            if let Some(p) = parse_packet(&pkt) { out.push(p); }
            // Holoscope pads 4-byte packets to 5 (e.g. heartbeat 3E); the fan does the same.
            if pkt.len() == 4 && self.buf.first().is_some_and(|&b| b != SOF) { self.buf.remove(0); }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_checksum_matches_v25() {
        // f_set_filename("eddie.mp4") from the verified v25 test lab
        let p = set_filename("eddie.mp4");
        assert_eq!(p[0], 0x5A);
        assert_eq!(p[1], 0x84);
        assert_eq!(p[2], 9);
        assert_eq!(&p[3..12], b"eddie.mp4");
        let x = xorsum(&p[2..12]);
        assert_eq!(p[12], x);
        assert_eq!(p[13], 0xF5);
    }

    #[test]
    fn status_parses() {
        let raw = [0x5A, 0x86, 0x1E, 0x02, 172, 0x03, 0xC0, 0, 8, 0, 0xF5];
        let p = parse_packet(&raw).unwrap();
        match p {
            Packet::Status(s) => {
                assert_eq!(s.play_index, 2);
                assert_eq!(s.leds_per_arm, 172);
                assert_eq!(s.angles, 960);
                assert_eq!(s.fan_type, 8);
                assert!(s.playing && s.power);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn filelist_parses_numbered() {
        let text = b"1 a.mp4/2 b.mp4";
        let mut body = vec![0, text.len() as u8];
        body.extend_from_slice(text);
        body.push(xorsum(&body));
        let mut pkt = vec![0x5A, 0x84];
        pkt.extend(body);
        pkt.push(0xF5);
        let mut r = Reader::default();
        let out = r.push(&pkt);
        assert_eq!(out, vec![Packet::FileList(vec![(1, "a.mp4".into()), (2, "b.mp4".into())])]);
    }

    #[test]
    fn cast_frame_shape() {
        let f = cast_frame(&[0xFF, 0xD8, 0xFF, 0xD9]);
        assert_eq!(f.len(), CAST_FRAME_SIZE);
        assert_eq!(&f[..6], &[0x5A, 0x28, 0xFF, 0xD8, 0xFF, 0xD9]);
        assert_eq!(&f[CAST_FRAME_SIZE - 2..], &[0x55, 0xF5]);
    }
}
