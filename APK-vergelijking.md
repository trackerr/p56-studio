# Holoscope.apk vs. p56-rust — vergelijking

Bron: `Holoscope.apk` (Qt 6 / QML, C++ in `libappHoloscope_arm64-v8a.so`, niet gestript).
Methode: QML-broncode uit de Qt-resources gehaald (47 bestanden), C++ gedisassembleerd met
`llvm-objdump`, pakketopbouw per `DeviceInterface::*`-functie getraceerd.
Niet getest tegen een echte fan.

## 1. Commando's app → fan

Notatie: `X` = XOR-checksum. "Wij" = `crates/p56-core/src/protocol.rs`.

| Functie (UI-label)              | Holoscope-pakket                                   | Wij                                  |
|---------------------------------|----------------------------------------------------|--------------------------------------|
| Aan / uit                       | `5A 01 01 F5` / `5A 02 02 F5`                      | gelijk                               |
| Pauze / play                    | `5A 03 03 F5` / `5A 04 04 F5`                      | gelijk                               |
| Herhaal N keer / altijd         | `5A 05 05 F5` / `5A 06 06 F5`                      | **ontbreekt**                        |
| Formatteren SD-kaart (Format)   | `5A 07 07 F5`                                      | **ontbreekt**                        |
| Fabrieksreset (Reset)           | `5A 08 08 F5`                                      | **ontbreekt**                        |
| Ademlicht aan / uit             | `5A 09 09 F5` / `5A 0A 0A F5`                      | **ontbreekt**                        |
| Autostart aan / uit             | `5A 0B 0B F5` / `5A 0C 0C F5`                      | **ontbreekt**                        |
| Heartbeat                       | `5A 0D 0D F5` (+ pad-byte 00)                      | gelijk (pad 3E, Windows-variant)     |
| Overdracht annuleren            | `5A 10 10 F5`                                      | enum aanwezig, niet gebruikt         |
| Bluetooth scan / loskoppelen    | `5A 18 18 F5` / `5A 19 19 F5`                      | **ontbreekt**                        |
| Bluetooth aan / uit             | `5A 1A 1A F5` / `5A 1B 1B F5`                      | **ontbreekt**                        |
| Machine-info opvragen           | `5A 1C 1C F5`                                      | gelijk                               |
| Groep-master aan / uit          | `5A 1F 1F F5` / `5A 20 20 F5`                      | **ontbreekt**                        |
| Apparaat kiezen in groep        | `5A 23 nn F5`                                      | enum aanwezig, niet gebruikt         |
| Hoekuitlijning groep start/stop | `5A 2A .. F5` / `5A 2B .. F5`                      | enum aanwezig, niet gebruikt         |
| Kleurmodus True Color / High Brightness | `5A 2C 2C F5` / `5A 2D 2D F5`              | **ontbreekt**                        |
| Slot wissen                     | `5A 80 n n F5` (n = 1-based)                       | gelijk                               |
| Helderheid                      | `5A 81 v v F5`                                     | **ontbreekt**                        |
| Hoek (Angle Control)            | `5A 82 hi lo X F5` (16-bit BE, X = hi^lo)          | **ontbreekt**                        |
| Hernoemen                       | `5A 83 slot len naam.mp4 X F5` (GB2312)            | **anders**: wij sturen `len oud\0nieuw` |
| Bestandsnaam (upload start)     | `5A 84 len naam X F5`                              | gelijk                               |
| Slot tonen                      | `5A 87 n n F5`                                     | gelijk                               |
| Apparaatnaam                    | `5A 89` + naam[32] + `X F5` (36 bytes vast)        | **ontbreekt**                        |
| Hotspot-naam + wachtwoord fan ("Device Set") | `5A 8A` + ssid[32] + pw[32] + `X F5` (68 bytes) | **anders**: wij sturen lengteprefix + `\0` |
| Router koppelen ("Router Set")  | `5A 8B` + ssid[32] + pw[32] + `X F5` (68 bytes)    | **ontbreekt**                        |
| Bluetooth-apparaat verbinden    | `5A 8E i i F5`                                     | **ontbreekt**                        |
| Volume                          | `5A 90 v v F5`                                     | **ontbreekt** (let op: 0x90 is bij ons `TransStart`) |
| Upload afronden                 | `5A 91 u32be X F5` (u32 = frames bij bin-types)    | wij sturen altijd 1 (ok voor JPEG-types) |

XOR bij de vaste 36/68-byte pakketten = over byte 2 t/m len-3.

## 2. Antwoorden fan → app

| Opcode | Betekenis (Holoscope)                                          | Wij          |
|--------|----------------------------------------------------------------|--------------|
| `0E`   | klaar/ack                                                      | ja           |
| `21`,`22` | overdracht-status ("continue"/"break")                      | nee          |
| `29`   | overdracht klaar (voortgang = 100 %)                           | nee          |
| `84`   | bestandslijst                                                  | ja           |
| `85`   | foutmeldingen bij overdracht (sub 0x0A, 0x06 → "connect device") | nee        |
| `86`   | status                                                         | ja (deels)   |
| `8D`   | machine-info                                                   | ja           |
| `8F`   | **gecombineerd statuspakket**, lengte 16-bit (bytes 4–5, max 2047): bestandslijst (`/`-gescheiden), machinestatus 1+2 (slot, play/stop, herhaalmodus, aan, ademlicht, autostart, fantype, LEDs, hoeken, kleurmodus), master, MAC, routernaam, Bluetooth-lijst, groepsinfo. Holoscope stelt hiermee automatisch het fantype in. | **nee** |

Velden die Holoscope uit de status haalt en wij niet: herhaalmodus, ademlicht, autostart,
helderheid, kleurmodus, master, SD-kaart totaal/vrij (`sendCardSizeChanged`).

## 3. Fantypes (`fileprocess::setparam` + `run`)

`[hoeken, LEDs/straal, ?, audio-samplerate]`, fps, formaat.

| Type | Holoscope            | fps | Formaat | Wij (`FanType::by_id`)      |
|------|----------------------|-----|---------|-----------------------------|
| 1,17,21,25 | 960×192        | 20  | JPEG    | 1,17 ok; 21,25 ontbreken     |
| 2    | 960×168              | 20  | JPEG    | ok                          |
| 3    | (niet meer in tabel) | –   | –       | 1440×240                    |
| 4    | 960×144              | 20  | JPEG    | ok                          |
| 5,22 | 960×128              | 20  | JPEG    | 5 ok; 22 ontbreekt          |
| 6    | **480×122**          | 12  | bin hc32l | 600×133 JPEG — **fout**   |
| 7,15 | 490×122              | 12  | bin hs42l | JPEG — **verkeerd formaat** |
| **8 (P56)** | **960×172**   | 20  | **JPEG** | **ok**                     |
| 11   | 960×184              | 20  | JPEG    | ok                          |
| 12   | 512×85               | 12  | bin hl20 | JPEG — verkeerd formaat    |
| 13   | **500×117**          | 12  | bin hc32l | 490×128 JPEG — **fout**   |
| 14,24,28 | 960×176          | 20  | JPEG    | 14 ok; 24,28 ontbreken      |
| 16   | 400×80               | 12  | bin hl18 | ontbreekt                  |
| 18   | 480×117              | 12  | bin hc32l | ontbreekt                 |
| 20,26 | 512×512             | 20  | JPEG    | ontbreken                   |
| 23   | 512×96               | 12  | bin hc32l | ontbreekt                 |
| 27   | 450×112              | 12  | bin hc32l | ontbreekt                 |
| 29   | 512×80               | 16  | bin hc32l | ontbreekt                 |
| 30   | 490×85               | 16  | bin hs42l | ontbreekt                 |
| 31   | 490×112              | 12  | bin hs42l | ontbreekt                 |
| 32   | 960×122              | 20  | JPEG    | ontbreekt                   |
| 33   | 960×133              | 20  | JPEG    | ontbreekt                   |
| 34   | 960×154              | 20  | JPEG    | ontbreekt                   |
| 36   | 448×69               | 12  | bin hc32l | ontbreekt                 |
| 37   | 704×128              | 20  | JPEG    | ontbreekt                   |

Onbekende types vallen bij ons terug op type 1 (960×192), dus verkeerde geometrie.

## 4. Netwerk

- **Discovery**: Holoscope zendt `<ip>HS` naar het subnet-broadcastadres van *elke* interface
  (`QNetworkInterface::allInterfaces`). Wij alleen naar `255.255.255.255` via de standaardroute.
- **Android**: Holoscope pakt een `MulticastLock` (nodig om UDP-broadcast te ontvangen),
  vraagt wake-lock + uitzondering batterijbesparing, en leest gekozen media via
  `ContentResolver` (`content://`-URI's).
- Upload-chunks: Holoscope 5840 bytes + `msleep`, wij 1440 bytes + 2 ms. Beide werken in principe.
- Holoscope detecteert een tweede app op hetzelfde LAN ("Other applications … have been opened").

## 5. Functies die wij helemaal niet hebben

- Groep/"Splice": meerdere fans als één groot beeld (stitch, master, hoekuitlijning, posities).
- Design: kubus, 3D-bol, roterende tekst, artistieke tekst, logo toevoegen.
- Cloud-mediabibliotheek (`http://bg.holoscope.tech:8081/sh/media/app/mediaList`).
- Camera: foto/video maken en direct sturen.
- Audio bij video (PCM, 8/16 kHz) + volumeregeling.
- Video bijsnijden (start/eindtijd), media roteren.
- Bluetooth-instellingen van de fan.
- Wekker/"Alarm"-scherm (UI aanwezig; pakket niet teruggevonden).
- Firmware-update: alleen teksten aanwezig, geen flash-routine in de C++ gevonden.
