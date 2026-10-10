# P56 / Holoscope hologram fan — protocol

Reverse-engineered from `Holoscope.exe` (Windows, Qt5) and `Holoscope.apk` (Android, Qt6:
`DeviceInterface::*`, `MyTcpServer::ReadData`, `fileprocess::*`). The APK is the newest
source; where it differed from the earlier Windows analysis, this document has been updated.
Everything below is implemented in `crates/p56-core` and emulated by
`crates/p56-sim`. Date: 2026-10-10.

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
Lengths are big-endian; the upload tag and size are little-endian u32. 4-byte packets
are sometimes padded to 5 (Holoscope.exe: `3E`, Holoscope.apk: `00`).

## 3. Commands app → fan
| Op | Function (Holoscope label) | Packet |
|---|---|---|
| 01 / 02 | on / off | `5A 01 01 F5` / `5A 02 02 F5` |
| 03 / 04 | pause / play | `5A 03 03 F5` / `5A 04 04 F5` |
| 05 / 06 | repeat whole list / current file | `5A 05 05 F5` / `5A 06 06 F5` |
| 07 | format memory card | `5A 07 07 F5` |
| 08 | factory reset | `5A 08 08 F5` |
| 09 / 0A | breathing light on / off | `5A 09 09 F5` / `5A 0A 0A F5` |
| 0B / 0C | autostart on / off | `5A 0B 0B F5` / `5A 0C 0C F5` |
| 0D | heartbeat / request status | `5A 0D 0D F5` |
| 10 | cancel transfer | `5A 10 10 F5` |
| 18 / 19 | Bluetooth scan / disconnect | `5A 18 18 F5` / `5A 19 19 F5` |
| 1A / 1B | Bluetooth on / off | `5A 1A 1A F5` / `5A 1B 1B F5` |
| 1C | request machine info | `5A 1C 1C F5` → `8D` |
| 1F / 20 | group master on / off | `5A 1F 1F F5` / `5A 20 20 F5` |
| 23 | select device in group | `5A 23 n F5` |
| 28 | live cast frame | `5A 28 <jpeg> 00… 55 F5`, always 20480 B |
| 2A / 2B | group angle alignment start / stop | `5A 2A 2A F5` / `5A 2B 2B F5` |
| 2C / 2D | colour: true colour / high brightness | `5A 2C 2C F5` / `5A 2D 2D F5` |
| 80 | delete slot n (1-based) | `5A 80 n n F5` |
| 81 | brightness 1–16 | `5A 81 v v F5` |
| 82 | angle 0–360 (Holoscope slider −180…180 + 180) | `5A 82 hi lo hi^lo F5` |
| 83 | rename slot | `5A 83 slot len "name.mp4" xor F5` (xor over slot…name) |
| 84 | file name (starts an upload) | `5A 84 len "name.mp4" xor F5` |
| 87 | show slot n | `5A 87 n n F5` |
| 89 | device name | `5A 89` + name[32] + xor + `F5` (36 B) |
| 8A | the fan's hotspot: name + password | `5A 8A` + ssid[32] + pw[32] + xor + `F5` (68 B) |
| 8B | join a router | `5A 8B` + ssid[32] + pw[32] + xor + `F5` (68 B) |
| 8E | Bluetooth: connect to entry i | `5A 8E i i F5` |
| 90 | volume 0–10 | `5A 90 v v F5` |
| 91 | finish upload | `5A 91 <u32 be> xor F5`; u32 = 1 for JPEG types, the frame count for binary types |

32-byte fields are zero padded; the xor for `89/8A/8B` covers byte 2 up to and including len−3.
Names: ASCII (the firmware knows GB2312 and local 8-bit; ASCII is the same in both).
**Correction to the first analysis:** `8A` was built as `len + ssid\0pw` — wrong; and `8A`
sets the fan's *hotspot*, not a router (that is `8B`). `83` takes a slot number, not the old name.

## 4. Replies fan → app
| Op | Meaning | Layout |
|---|---|---|
| 0E | ready / received | `5A 0E 0E F5`, after every command and every upload block |
| 21 / 22 | transfer: continue / aborted | `5A 21 21 F5` / `5A 22 22 F5` |
| 29 | transfer stored (progress 100 %) | `5A 29 29 F5` |
| 84 | playlist | `5A 84 <len16> "1 a.mp4/2 b.mp4" xor F5` (names at most 32 characters) |
| 85 | error code | `5A 85 c c F5`: `06` name already exists, `0A` too many files, other: transfer failed |
| 86 | status, 11 B | see below |
| 8D | machine info, 126 B | ip@6 (4 B), name@14 (32), MAC@46 (6), hotspot@52 (32), router@84 (32), `F5`@125 |
| 8F | list (group / Bluetooth) | `5A 8F kind idx <len16> "a/b/c" xor F5`; `kind` bit7 = Bluetooth |

### Status `86`
`5A 86 flags slot brightness (colour<<4 | angle_hi) angle_lo group type extra F5`

| Bit of `flags` | Meaning |
|---|---|
| 6 (0x40) | on |
| 5 (0x20) | autostart |
| 4 (0x10) | breathing light |
| 3 (0x08) | playing (otherwise paused) |
| 2 (0x04) | repeat whole list (otherwise current file) |
| 1 (0x02) | group master |

`brightness` 1–16, `colour` 1 = true colour / 2 = high brightness, `angle` 0–360 (180 = neutral),
`type` = fan type (§6). **Correction:** byte 4 is the brightness and bytes 5–6 the configured angle
(not "LEDs per arm" and "number of angles"); the flag bits are as above.

## 5. File upload
```
ON(01)→0E · NAME(84)→0E or 85 · [tag u32le][size u32le][1600×00][JPEG…][00]→0E
· COMMIT(91)×2 · OFF(02) · ON(01) · SHOW(87 n)
```
- After an `85` error on the name, **never** send the blob (the fan is then in command mode).
- Every step is answered by exactly one `0E` or `85`; count them, so a late answer to a
  previous command can never be mistaken for the answer to the name.
- `size` = 1600 + length(frames) + 1; the name must end in `.mp4`.
- One JPEG = photo; several in a row = video/animation.

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

Transcode (identical for cast and upload): auto exposure, `warpPolar`, radial attenuation,
sawtooth black in the centre, arm layout 0-3-2-1, JPEG q80 → ×0.9 until ≤ 20476 B.
The fourth parameter in Holoscope.apk (16000/7992/8000) is an audio sample rate: videos can contain sound.

## 7. Live cast
Each frame = §6 → 20480 B: `5A 28 | JPEG | zeros | 55 F5`, in two chunks (14600 + 5880 B);
after each chunk, wait until the fan responds.

## 8. Not implemented
- **Firmware**: Holoscope.exe starts an update over a separate socket with a `.bin`; the APK
  only contains strings. No firmware file, chip or bootloader known → deliberately not built.
- **Cloud media library** (`bg.holoscope.tech`), **group display** (splitting an image across
  several fans), **3D text/sphere/cube**, **sound in videos**: analysed, not built yet.
- **Binary image formats** for fan types without JPEG.
