# P56 / Holoscope hologram fan — protocol

Reverse-engineered from `Holoscope.exe` (Windows, Qt5) and `Holoscope.apk` (Android, Qt6:
`DeviceInterface::*`, `MyTcpServer::ReadData`, `fileprocess::*`). The APK is the newest
source; where it differed from the earlier Windows analysis, this document has been updated.
Everything below is implemented in `crates/p56-core` and emulated by
`crates/p56-sim`. Last updated: 2026-10-13.

## 1. Network
| What | Value |
|---|---|
| Role | **PC/phone is the TCP server** on port 6666; the fan connects as a client |
| Discovery | the app sends `"<own-ip>HS"` to UDP 8988 every 3 s, to the broadcast address of **every** network adapter (Holoscope.apk) and `255.255.255.255` |
| Reply | the fan sends `"<fan-ip>HS"` back and then connects to `<own-ip>:6666` |
| Default AP | the fan is a Wi-Fi access point, IP `192.168.4.1` |
| Heartbeat | `5A 0D 0D F5` (+1 padding byte) every 3 s; the fan answers with an `86` status. Holoscope also polls `1C` periodically |
| Android | Holoscope takes a `MulticastLock` and asks for a battery-optimisation exemption |

## 2. Frame format
`5A <opcode> <body…> F5`. Text packets end with an **XOR** over the body before `F5`.
Lengths are big-endian; the u32 fields of an uploaded file (duration, record lengths) are little-endian. 4-byte packets
are sometimes padded to 5 (Holoscope.exe: `3E`, Holoscope.apk: `00`).

## 3. Commands app → fan
| Op | Function (Holoscope label) | Packet |
|---|---|---|
| 01 / 02 | on / off | `5A 01 01 F5` / `5A 02 02 F5` |
| 03 / 04 | pause / play | `5A 03 03 F5` / `5A 04 04 F5` |
| 05 / 06 | repeat whole list / current file | `5A 05 05 F5` / `5A 06 06 F5` |
| 07 | format memory card | `5A 07 07 F5` |
| 08 | factory reset | `5A 08 08 F5` |
| 09 / 0A | breathing light on / off: a status light, all LEDs light up in one colour now and then to show the fan is on (most app languages call it "status light"; seen on a P56) | `5A 09 09 F5` / `5A 0A 0A F5` |
| 0B / 0C | autostart on / off | `5A 0B 0B F5` / `5A 0C 0C F5` |
| 0D | heartbeat / request status | `5A 0D 0D F5` |
| 10 | cancel transfer | `5A 10 10 F5` |
| 18 / 19 | Bluetooth scan / disconnect | `5A 18 18 F5` / `5A 19 19 F5` — not used by P56 Studio, see below |
| 1A / 1B | Bluetooth on / off | `5A 1A 1A F5` / `5A 1B 1B F5` — not used by P56 Studio, see below |
| 1C | request machine info | `5A 1C 1C F5` → `8D` |
| 1F / 20 | group master on / off (several fans showing one picture together); **not used** by P56 Studio, see below | `5A 1F 1F F5` / `5A 20 20 F5` |
| 23 | select this fan in its group | `5A 23 23 F5`, sent to that fan (`stitchselectdevice`) |
| 26 | start live cast (Holoscope.exe `samescreen`) | `5A 26 26 F5` → fan answers `5A 29 29 F5`; see §7 |
| 28 | live cast frame | `5A 28 <jpeg> 00… 55 F5`, always 20480 B |
| 2A / 2B | group angle alignment start / stop | `5A 2A 2A F5` / `5A 2B 2B F5` |
| 2C / 2D | colour: true colour / high brightness | `5A 2C 2C F5` / `5A 2D 2D F5`. On a real P56 switching made no visible difference and the status always reports `colour` 0, so P56 Studio and Home Assistant do not offer it; `p56 color` stays for other models |
| 80 | delete slot n (1-based) | `5A 80 n n F5` |
| 81 | brightness 1–16 | `5A 81 v v F5` |
| 82 | angle 0–360 (Holoscope slider −180…180 + 180) | `5A 82 hi lo hi^lo F5` |
| 83 | rename slot | `5A 83 slot len "name.mp4" xor F5` (xor over slot…name) |
| 84 | file name (starts an upload) | `5A 84 len "name.mp4" xor F5` |
| 87 | show slot n | `5A 87 n n F5` |
| 89 | device name | `5A 89` + name[32] + xor + `F5` (36 B) |
| 8A | the fan's hotspot: name + password | `5A 8A` + ssid[32] + pw[32] + xor + `F5` (68 B) |
| 8B | join a router | `5A 8B` + ssid[32] + pw[32] + xor + `F5` (68 B) |
| 8E | Bluetooth: connect to entry i | `5A 8E i i F5` — not used by P56 Studio, see below |
| 90 | volume 0–10 | `5A 90 v v F5` |
| 91 | finish upload | `5A 91 <u32 be> xor F5`; see §5 |
| 93 | group: delete a file by name on every fan | `5A 93` + name[32] + xor + `F5` (36 B; ".mp4" appended) — not implemented |
| 94 | group: rename a file by name on every fan | `5A 94` + old[32] + new[32] + xor + `F5` (68 B; field order not fully verified) — not implemented |
| 95 | group saved (Holoscope.exe) | `5A 95 01 01 F5` — not implemented |
| 96 | place in a group picture | `5A 96 group width height x y xor F5` (9 B, xor over bytes 2–6) — not implemented |

32-byte fields are zero padded and hold at most 31 bytes of text (Holoscope keeps a terminating
zero); the xor for `89/8A/8B` covers byte 2 up to and including len−3.
Names: ASCII (the firmware knows GB2312 and local 8-bit; ASCII is the same in both). **File
names: at most 24 bytes before `.mp4`** (measured on a real P56). With 25–27 bytes the fan answers
normally but does not store the file; from 28 bytes on it also breaks its file list, and every
later upload fails until the card is formatted. Holoscope's own UI allows 12 bytes. P56 Studio
keeps letters, digits, space and `-_.()` and at most 24 bytes before `.mp4`.
**Correction to the first analysis:** `8A` was built as `len + ssid\0pw` — wrong; and `8A`
sets the fan's *hotspot*, not a router (that is `8B`). `83` takes a slot number, not the old name.

**Bluetooth (`18`, `19`, `1A`, `1B`, `8E`) is not used by P56 Studio.** On the P56, Wi-Fi and
Bluetooth share one 2.4 GHz radio. With a Bluetooth audio receiver connected, a P56 could no
longer rejoin the home Wi-Fi and kept falling back to its own hotspot until Bluetooth was switched
off (2026-10-12). P56 Studio, `p56`, `p56-server` and the Home Assistant integration therefore
never send these commands, and the simulator treats them like any other unknown command. The
`8F` report is still read, because it is the only place the fan reports the speaker volume.

**OFF is not always obeyed.** Now and then (2026-10-21/22) a P56 answered `02` with a status
that says off, but kept turning with a black picture; a second `02` a few seconds later stopped
it. P56 Studio therefore sends "off" once more 3 s after switching a fan off after a message.

**Group master (`1F`, `20`) is not used by P56 Studio either.** In Holoscope a group of fans can
show one picture together, with one fan as master (status bit 1, `0x02`, reports it). P56 Studio
supports one fan per picture, so the app, `p56`, `p56-server` and Home Assistant offer no master
switch and never send these commands; the simulator treats them as unknown. Documented here for
whoever wants to build group display.

## 4. Replies fan → app
| Op | Meaning | Layout |
|---|---|---|
| 0E | ready / received | `5A 0E 0E F5`, after every command and every upload block |
| 21 / 22 | transfer: continue / (see note) | `5A 21 21 F5` / `5A 22 22 F5`. Holoscope.apk ignores `22`; Holoscope.exe reads it as "firmware update finished". P56 Studio ignores it during uploads. |
| 29 | transfer stored (progress 100 %) | `5A 29 29 F5` |
| 84 | playlist | `5A 84 <len16> "1 a/2 b" xor F5`: a real P56 lists the names without `.mp4` (names at most 32 characters). P56 Studio reads every name as `name.mp4` and also accepts names that already end in `.mp4`. |
| 85 | error code | `5A 85 c c F5`: `06` name already exists, `0A` too many files, other: transfer failed |
| 86 | status, 11 B | see below |
| 8D | machine info, 126 B | group picture width, height, x, y @6–9 (0 when not in a group; earlier read as an IP address), name@14 (32), MAC@46 (6), hotspot@52 (32), router@84 (32), `F5`@125 |
| 8F | Bluetooth report | `5A 8F flags volume <len16> "a/b/c" xor F5`: flags bit 7 on, bit 6 connected, bit 5 connecting; `volume` = speaker volume 0–10 (the only place the fan reports it); up to 8 device names. P56 Studio reads only the volume (it does not use Bluetooth, see §3). |

### Status `86`
`5A 86 flags slot brightness (colour<<4 | angle_hi) angle_lo group type extra F5`

| Bit of `flags` | Meaning |
|---|---|
| 6 (0x40) | on |
| 5 (0x20) | autostart |
| 4 (0x10) | breathing light |
| 3 (0x08) | playing (otherwise paused) |
| 2 (0x04) | repeat whole list (otherwise current file) |
| 1 (0x02) | group master (not read by P56 Studio) |

`slot` is the playing file **0-based** (0 = the first file), while `SHOW (87 n)`, delete and
rename count from 1: Holoscope selects list position + 1 and compares the status value with its
0-based list positions. P56 Studio adds 1 when reading it (confirmed on a real P56). `brightness` 1–16, `colour` 1 = true colour / 2 = high brightness (a P56 always reports 0), `angle` 0–360 (180 = neutral),
`group` = number of the group picture this fan belongs to (1–16, Holoscope ignores larger values),
`type` = fan type (§6). **Correction:** byte 4 is the brightness and bytes 5–6 the configured angle
(not "LEDs per arm" and "number of angles"); the flag bits are as above.

## 5. File upload
```
ON(01)→0E · NAME(84)→0E or 85 · FILE · COMMIT(91) every 500 ms until a status 86 · OFF(02) · ON(01) · SHOW(87 n)

FILE   = [duration_ms u32le] RECORD × frames          (in pieces of 1440 B, 2 ms apart)
RECORD = [len u32le][PCM 1600 B][JPEG][00 × 1–4]
         len = 1600 + (jpeg_len & ~3) + 4             (everything after the length field)
```
- **One record per frame.** Each record carries 1/20 s of 16-bit mono PCM audio at 16 kHz
  (`32000 / fps` bytes: the video's sound, or silence) and exactly one complete JPEG (`FF D8 … FF D9`), padded
  with at least one zero byte to a multiple of 4.
- `duration_ms` = frames × 50 for an animation (the P56 plays files at 20 fps). A still
  picture gets 4950 (5 s at 20 fps minus one frame, Holoscope's default).
- `COMMIT` carries 1 for a still picture and the frame count for an animation (the value is a
  setting in Holoscope; the frame count is what earlier hardware tests used).
- Each JPEG of a file may be up to `0xF000` (61440) bytes; Holoscope lowers the quality
  (q² per step from 0.95, at most 21 steps) and never cuts a JPEG short.
- **Finish:** the fan may still be writing to its card after the last byte arrived, and sends no
  status meanwhile; once the file is stored it sends its playlist (`84`) with the file in it, by
  itself, and then its status. Holoscope.apk (`transfinish`) repeats COMMIT every 500 ms until then.
  P56 Studio waits for that playlist (a status alone is no proof: on Linux the last megabytes can
  still be in the socket buffer, and the fan reports while it receives). Switching the fan off earlier (as P56 Studio did before 0.3.3)
  loses large files and can leave the card unusable until it is formatted (85 code 8 on every
  upload). Holoscope sets no limit on length or size; a real P56 stored a 27 MB file this way.
- **An `85` during the upload:** stop sending at once and send CANCEL (`5A 10 10 F5`) every
  500 ms, as Holoscope does.
- After an `85` error on the name, **never** send the file (the fan is then in command mode).
- Every step is answered by exactly one `0E` or `85`; count them, so a late answer to a
  previous command can never be mistaken for the answer to the name.
- The name must end in `.mp4`.

Verified in the machine code of Holoscope.apk 2.5.2 and 2.5.4 (`fileprocess::picproces`,
`videoprocess`, `matToQbytes`, `setparam`, `transfinish` in libappHoloscope_arm64-v8a.so) and
Holoscope.exe 1.1.8 (same record builder). A still picture in this layout is byte-for-byte what
already worked on real fans; earlier versions of this project sent all JPEGs of an animation in
one record, which no Holoscope version does.

## 6. Fan types and image format
| type | angles × LEDs | fps | format | supported |
|---|---|---|---|---|
| 1, 17 | 960 × 192 | 20 | JPEG, 4 / 2 arms | yes |
| 2 | 960 × 168 | 20 | JPEG, 4 arms | yes |
| 3 | 1440 × 240 | – | JPEG, 1 arm (Holoscope.exe only) | yes |
| 4 | 960 × 144 | 20 | JPEG, 4 arms | yes |
| 5 | 960 × 128 | 20 | JPEG, 2 arms | yes |
| **8 (P56)** | **960 × 172** | 20 | **JPEG 688×240, 4 arms** | **yes** |
| 11 | 960 × 184 | 20 | JPEG, 4 arms | yes |
| 14 | 960 × 176 | 20 | JPEG, 2 arms | yes |
| 6, 7, 12, 13, 15 | various | 12 | Holoscope.exe: raw polar JPEG; Holoscope.apk: binary | legacy format |
| 16, 18, 23, 27, 29–31, 36 | various | 12–16 | binary (`hc32l`, `hs42l`, `hl20`, `hl18`) | no |
| 20–22, 24–26, 28, 32–34, 37 | various | 20 | JPEG, arm layout not verified | no |

Model names (Holoscope.exe): 1 HS65, 2 HS56, 3 HS80, 4 P50, 5 HD30, 6 P45, 7 P42, **8 P56**, 9 HS100,
10 HS130, 11 P62, 12 HL20, 13 HL33, 14 PD45, 15 PD42.

Transcode (identical for cast and upload): auto exposure, `warpPolar`, radial attenuation,
sawtooth black in the centre, arm layout 0-3-2-1, JPEG from q80, lowering the quality until it
fits: ≤ 20476 B for a cast frame, ≤ 61440 B for a file. A JPEG is never truncated.
The fourth parameter in Holoscope.apk (16000/7992/8000) is an audio sample rate: videos can contain sound.

## 7. Live cast
```
ON(01) · CAST_START(26) → 29 · ~70 ms · FRAME(28) FRAME(28) …   (no heartbeat, no 1C meanwhile)
```
- **Start with `5A 26 26 F5`.** The fan answers `5A 29 29 F5` (on a real P56 after ~200 ms).
  Without it a P56 answers every frame but keeps playing its playlist; after it, it shows the
  frames. Holoscope.exe (`samescreen(true)`) sends nothing else at the start and does not
  stream before the `29` arrives.
- **Each frame** = §6 → 20480 B: `5A 28 | JPEG | zeros | 55 F5`, written whole. Holoscope.exe
  hands it to the socket in pieces of 14600 + 5880 B and waits only for its own write to finish,
  never for an answer from the fan; the fan's answers during a cast are ignored.
- **Keep it coming:** the fan needs a frame about every 130 ms; slower, it shows a frame
  briefly and falls back. A real P56 shows the test card at 15 fps.
- **No heartbeat (0D) or status poll (1C)** while casting: Holoscope.exe suppresses both.
- **Stop:** nothing is sent; the frames simply stop.
- **OFF (02) is ignored in live mode**, also right after the last frame (seen on a real P56,
  2026-10-13): P56 Studio waits until 2 s after the last frame before switching a fan off.
- A P56 with an empty playlist pauses itself, and a paused fan shows no cast frames: P56 Studio
  sends PLAY (04) first when the fan reports it is paused (confirmed on a real P56).

P56 Studio starts a session with `26` before the first frame and again after a pause of 1 s,
waits at most 1 s for the `29`, sends no heartbeat or poll until 2 s after the last frame, and
drops the connection after five frames in a row that could not be written within 2 s.
Holoscope.apk (2.5.2 and 2.5.4) has **no** live cast: its projection button takes a photo or
video that is then uploaded. Verified in the machine code of Holoscope.exe 1.1.8, and live on a
P56 (2026-10-11).

## 8. Not implemented
- **Firmware**: Holoscope.exe starts an update over a separate socket with a `.bin`; the APK
  only contains strings. No firmware file, chip or bootloader known → deliberately not built.
- **Cloud media library** (`bg.holoscope.tech`), **group display** (splitting an image across
  several fans), **3D text/sphere/cube**: analysed, not built yet.
- **Bluetooth** (audio and the BT commands): deliberately not used, see §3 (one radio for
  Wi-Fi and Bluetooth).
- **Binary image formats** for fan types without JPEG.
