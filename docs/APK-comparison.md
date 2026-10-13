# Holoscope.apk vs. p56-rust — comparison

Source: `Holoscope.apk` (Qt 6 / QML, C++ in `libappHoloscope_arm64-v8a.so`, not stripped).
Method: QML source extracted from the Qt resources (47 files), C++ disassembled with
`llvm-objdump`, packet layout traced per `DeviceInterface::*` function.
Not tested against a real fan.

> **Historical analysis** from the start of the project (October 2026). The "Us" column shows
> what was built *then*; nearly everything marked missing has been built and tested on a real P56
> since. For the current state see [PROTOCOL.md](../PROTOCOL.md) (with the Bluetooth and colour
> mode notes) and the [changelog](../CHANGELOG.md).

## 1. Commands app → fan

Notation: `X` = XOR checksum. "Us" = `crates/p56-core/src/protocol.rs`.

| Function (UI label)             | Holoscope packet                                   | Us                                   |
|---------------------------------|----------------------------------------------------|--------------------------------------|
| On / off                        | `5A 01 01 F5` / `5A 02 02 F5`                      | same                                 |
| Pause / play                    | `5A 03 03 F5` / `5A 04 04 F5`                      | same                                 |
| Repeat N times / always         | `5A 05 05 F5` / `5A 06 06 F5`                      | **missing**                          |
| Format SD card (Format)         | `5A 07 07 F5`                                      | **missing**                          |
| Factory reset (Reset)           | `5A 08 08 F5`                                      | **missing**                          |
| Breathing light on / off        | `5A 09 09 F5` / `5A 0A 0A F5`                      | **missing**                          |
| Autostart on / off              | `5A 0B 0B F5` / `5A 0C 0C F5`                      | **missing**                          |
| Heartbeat                       | `5A 0D 0D F5` (+ pad byte 00)                      | same (pad 3E, Windows variant)       |
| Cancel transfer                 | `5A 10 10 F5`                                      | enum present, unused                 |
| Bluetooth scan / disconnect     | `5A 18 18 F5` / `5A 19 19 F5`                      | not used on purpose (see PROTOCOL.md) |
| Bluetooth on / off              | `5A 1A 1A F5` / `5A 1B 1B F5`                      | not used on purpose (see PROTOCOL.md) |
| Request machine info            | `5A 1C 1C F5`                                      | same                                 |
| Group master on / off           | `5A 1F 1F F5` / `5A 20 20 F5`                      | **missing**                          |
| Select device in group          | `5A 23 nn F5`                                      | enum present, unused                 |
| Group angle alignment start/stop | `5A 2A .. F5` / `5A 2B .. F5`                     | enum present, unused                 |
| Colour mode True Color / High Brightness | `5A 2C 2C F5` / `5A 2D 2D F5`             | **missing**                          |
| Delete slot                     | `5A 80 n n F5` (n = 1-based)                       | same                                 |
| Brightness                      | `5A 81 v v F5`                                     | **missing**                          |
| Angle (Angle Control)           | `5A 82 hi lo X F5` (16-bit BE, X = hi^lo)          | **missing**                          |
| Rename                          | `5A 83 slot len name.mp4 X F5` (GB2312)            | **different**: we send `len old\0new` |
| File name (upload start)        | `5A 84 len name X F5`                              | same                                 |
| Show slot                       | `5A 87 n n F5`                                     | same                                 |
| Device name                     | `5A 89` + name[32] + `X F5` (36 bytes fixed)       | **missing**                          |
| Fan hotspot name + password ("Device Set") | `5A 8A` + ssid[32] + pw[32] + `X F5` (68 bytes) | **different**: we send a length prefix + `\0` |
| Join router ("Router Set")      | `5A 8B` + ssid[32] + pw[32] + `X F5` (68 bytes)    | **missing**                          |
| Connect Bluetooth device        | `5A 8E i i F5`                                     | not used on purpose (see PROTOCOL.md) |
| Volume                          | `5A 90 v v F5`                                     | **missing** (note: 0x90 is our `TransStart`) |
| Finish upload                   | `5A 91 u32be X F5` (u32 = frames for bin types)    | we always send 1 (fine for JPEG types) |

XOR for the fixed 36/68-byte packets = over byte 2 up to and including len-3.

## 2. Replies fan → app

| Opcode | Meaning (Holoscope)                                            | Us           |
|--------|----------------------------------------------------------------|--------------|
| `0E`   | ready/ack                                                      | yes          |
| `21`,`22` | transfer status ("continue"/"break")                        | no           |
| `29`   | transfer done (progress = 100 %)                               | no           |
| `84`   | file list                                                      | yes          |
| `85`   | transfer errors (sub 0x0A, 0x06 → "connect device")            | no           |
| `86`   | status                                                         | yes (partly) |
| `8D`   | machine info                                                   | yes          |
| `8F`   | **combined status packet**, 16-bit length (bytes 4–5, max 2047): file list (`/`-separated), machine status 1+2 (slot, play/stop, repeat mode, on, breathing light, autostart, fan type, LEDs, angles, colour mode), master, MAC, router name, Bluetooth list, group info. Holoscope uses it to set the fan type automatically. | **no** |

Fields Holoscope reads from the status and we do not: repeat mode, breathing light, autostart,
brightness, colour mode, master, SD card total/free (`sendCardSizeChanged`).

## 3. Fan types (`fileprocess::setparam` + `run`)

`[angles, LEDs/radius, ?, audio sample rate]`, fps, format.

| Type | Holoscope            | fps | Format  | Us (`FanType::by_id`)       |
|------|----------------------|-----|---------|-----------------------------|
| 1,17,21,25 | 960×192        | 20  | JPEG    | 1,17 ok; 21,25 missing      |
| 2    | 960×168              | 20  | JPEG    | ok                          |
| 3    | (no longer in table) | –   | –       | 1440×240                    |
| 4    | 960×144              | 20  | JPEG    | ok                          |
| 5,22 | 960×128              | 20  | JPEG    | 5 ok; 22 missing            |
| 6    | **480×122**          | 12  | bin hc32l | 600×133 JPEG — **wrong**  |
| 7,15 | 490×122              | 12  | bin hs42l | JPEG — **wrong format**   |
| **8 (P56)** | **960×172**   | 20  | **JPEG** | **ok**                     |
| 11   | 960×184              | 20  | JPEG    | ok                          |
| 12   | 512×85               | 12  | bin hl20 | JPEG — wrong format        |
| 13   | **500×117**          | 12  | bin hc32l | 490×128 JPEG — **wrong**  |
| 14,24,28 | 960×176          | 20  | JPEG    | 14 ok; 24,28 missing        |
| 16   | 400×80               | 12  | bin hl18 | missing                    |
| 18   | 480×117              | 12  | bin hc32l | missing                   |
| 20,26 | 512×512             | 20  | JPEG    | missing                     |
| 23   | 512×96               | 12  | bin hc32l | missing                   |
| 27   | 450×112              | 12  | bin hc32l | missing                   |
| 29   | 512×80               | 16  | bin hc32l | missing                   |
| 30   | 490×85               | 16  | bin hs42l | missing                   |
| 31   | 490×112              | 12  | bin hs42l | missing                   |
| 32   | 960×122              | 20  | JPEG    | missing                     |
| 33   | 960×133              | 20  | JPEG    | missing                     |
| 34   | 960×154              | 20  | JPEG    | missing                     |
| 36   | 448×69               | 12  | bin hc32l | missing                   |
| 37   | 704×128              | 20  | JPEG    | missing                     |

Unknown types fall back to type 1 (960×192) in our code, so the geometry is wrong.

## 4. Network

- **Discovery**: Holoscope sends `<ip>HS` to the subnet broadcast address of *every* interface
  (`QNetworkInterface::allInterfaces`). We only send to `255.255.255.255` via the default route.
- **Android**: Holoscope takes a `MulticastLock` (needed to receive UDP broadcasts),
  requests a wake lock + battery-optimisation exemption, and reads picked media through
  `ContentResolver` (`content://` URIs).
- Upload chunks: Holoscope 5840 bytes + `msleep`, we 1440 bytes + 2 ms. Both work in principle.
- Holoscope detects a second app on the same LAN ("Other applications … have been opened").

## 5. Features we do not have at all

- Group/"Splice": several fans as one large picture (stitch, master, angle alignment, positions).
- Design: cube, 3D sphere, rotating text, artistic text, add a logo.
- Cloud media library (`http://bg.holoscope.tech:8081/sh/media/app/mediaList`).
- Camera: take a photo/video and send it straight away.
- Audio with video (PCM, 8/16 kHz) + volume control.
- Trim video (start/end time), rotate media.
- The fan's Bluetooth settings: left out on purpose (Wi-Fi and Bluetooth share one radio; see PROTOCOL.md).
- Alarm clock/"Alarm" screen (UI present; packet not found).
- Firmware update: only strings present, no flash routine found in the C++.
