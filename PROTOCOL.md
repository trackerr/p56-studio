# P56 / Holoscope hologramfan — protocol

Reverse-engineered uit `Holoscope.exe` (Windows, Qt5) en `Holoscope.apk` (Android, Qt6:
`DeviceInterface::*`, `MyTcpServer::ReadData`, `fileprocess::*`). De APK is de nieuwste
bron; waar die afweek van de eerdere Windows-analyse is dit document bijgewerkt.
Alles hieronder is geïmplementeerd in `crates/p56-core` en wordt nagebootst door
`crates/p56-sim`. Datum: 2026-10-10.

## 1. Netwerk
| Wat | Waarde |
|---|---|
| Rol | **PC/telefoon is TCP-server** op poort 6666; de fan verbindt als client |
| Ontdekken | app zendt `"<eigen-ip>HS"` naar UDP 8988, elke 3 s, naar het broadcastadres van **elke** netwerkkaart (Holoscope.apk) en `255.255.255.255` |
| Antwoord | fan stuurt `"<fan-ip>HS"` terug en verbindt daarna met `<eigen-ip>:6666` |
| Standaard-AP | fan is wifi-accesspoint, IP `192.168.4.1` |
| Heartbeat | `5A 0D 0D F5` (+1 opvulbyte) elke 3 s; fan antwoordt met `86`-status. Holoscope vraagt ook periodiek `1C` |
| Android | Holoscope pakt een `MulticastLock` en vraagt uitzondering op batterijbesparing |

## 2. Frameformaat
`5A <opcode> <body…> F5`. Tekstpakketten eindigen met een **XOR** over de body vóór `F5`.
Lengtes zijn big-endian; upload-tag en -size zijn little-endian u32. Pakketten van 4 bytes
worden soms tot 5 opgevuld (Holoscope.exe: `3E`, Holoscope.apk: `00`).

## 3. Commando's app → fan
| Op | Functie (Holoscope-label) | Pakket |
|---|---|---|
| 01 / 02 | aan / uit | `5A 01 01 F5` / `5A 02 02 F5` |
| 03 / 04 | pauze / afspelen | `5A 03 03 F5` / `5A 04 04 F5` |
| 05 / 06 | herhaal hele lijst / huidig bestand | `5A 05 05 F5` / `5A 06 06 F5` |
| 07 | geheugenkaart formatteren | `5A 07 07 F5` |
| 08 | fabrieksinstellingen | `5A 08 08 F5` |
| 09 / 0A | ademlicht aan / uit | `5A 09 09 F5` / `5A 0A 0A F5` |
| 0B / 0C | automatisch starten aan / uit | `5A 0B 0B F5` / `5A 0C 0C F5` |
| 0D | heartbeat / status opvragen | `5A 0D 0D F5` |
| 10 | overdracht annuleren | `5A 10 10 F5` |
| 18 / 19 | bluetooth zoeken / loskoppelen | `5A 18 18 F5` / `5A 19 19 F5` |
| 1A / 1B | bluetooth aan / uit | `5A 1A 1A F5` / `5A 1B 1B F5` |
| 1C | machine-info opvragen | `5A 1C 1C F5` → `8D` |
| 1F / 20 | groeps-master aan / uit | `5A 1F 1F F5` / `5A 20 20 F5` |
| 23 | apparaat kiezen in groep | `5A 23 n F5` |
| 28 | live cast-frame | `5A 28 <jpeg> 00… 55 F5`, altijd 20480 B |
| 2A / 2B | hoekuitlijning groep start / stop | `5A 2A 2A F5` / `5A 2B 2B F5` |
| 2C / 2D | kleur: true color / hoge helderheid | `5A 2C 2C F5` / `5A 2D 2D F5` |
| 80 | slot n wissen (1-based) | `5A 80 n n F5` |
| 81 | helderheid 1–16 | `5A 81 v v F5` |
| 82 | hoek 0–360 (Holoscope-slider −180…180 + 180) | `5A 82 hi lo hi^lo F5` |
| 83 | slot hernoemen | `5A 83 slot len "naam.mp4" xor F5` (xor over slot…naam) |
| 84 | bestandsnaam (start upload) | `5A 84 len "naam.mp4" xor F5` |
| 87 | slot n tonen | `5A 87 n n F5` |
| 89 | apparaatnaam | `5A 89` + naam[32] + xor + `F5` (36 B) |
| 8A | hotspot van de fan: naam + wachtwoord | `5A 8A` + ssid[32] + pw[32] + xor + `F5` (68 B) |
| 8B | verbinden met router | `5A 8B` + ssid[32] + pw[32] + xor + `F5` (68 B) |
| 8E | bluetooth verbinden met item i | `5A 8E i i F5` |
| 90 | volume 0–10 | `5A 90 v v F5` |
| 91 | upload afronden | `5A 91 <u32 be> xor F5`; u32 = 1 voor JPEG-types, aantal frames bij binaire types |

Velden van 32 bytes worden met nullen aangevuld; xor bij `89/8A/8B` = over byte 2 t/m len−3.
Namen: ASCII (de firmware kent GB2312 en lokaal 8-bit; ASCII is in beide gelijk).
**Correctie t.o.v. de eerste analyse:** `8A` was als `len + ssid\0pw` gebouwd — fout; en `8A`
zet de *hotspot* van de fan, niet een router (dat is `8B`). `83` neemt een slotnummer, niet de oude naam.

## 4. Antwoorden fan → app
| Op | Betekenis | Opbouw |
|---|---|---|
| 0E | klaar / ontvangen | `5A 0E 0E F5`, na elk commando en elk upload-blok |
| 21 / 22 | overdracht: verder / afgebroken | `5A 21 21 F5` / `5A 22 22 F5` |
| 29 | overdracht opgeslagen (voortgang 100 %) | `5A 29 29 F5` |
| 84 | afspeellijst | `5A 84 <len16> "1 a.mp4/2 b.mp4" xor F5` (namen max. 32 tekens) |
| 85 | foutcode | `5A 85 c c F5`: `06` naam bestaat al, `0A` te veel bestanden, overig: overdracht mislukt |
| 86 | status, 11 B | zie hieronder |
| 8D | machine-info, 126 B | ip@6 (4 B), naam@14 (32), MAC@46 (6), hotspot@52 (32), router@84 (32), `F5`@125 |
| 8F | lijst (groep / bluetooth) | `5A 8F kind idx <len16> "a/b/c" xor F5`; `kind` bit7 = bluetooth |

### Status `86`
`5A 86 flags slot helderheid (kleur<<4 | hoek_hi) hoek_lo groep type extra F5`

| Bit van `flags` | Betekenis |
|---|---|
| 6 (0x40) | aan |
| 5 (0x20) | automatisch starten |
| 4 (0x10) | ademlicht |
| 3 (0x08) | speelt (anders pauze) |
| 2 (0x04) | herhaal hele lijst (anders huidig bestand) |
| 1 (0x02) | groeps-master |

`helderheid` 1–16, `kleur` 1 = true color / 2 = hoge helderheid, `hoek` 0–360 (180 = neutraal),
`type` = fantype (§6). **Correctie:** byte 4 is de helderheid en byte 5–6 de ingestelde hoek
(niet "LEDs per arm" en "aantal hoeken"); de vlagbits staan zoals hierboven.

## 5. Bestandsupload
```
AAN(01)→0E · NAAM(84)→0E of 85 · [tag u32le][size u32le][1600×00][JPEG…][00]→0E
· AFRONDEN(91)×2 · UIT(02) · AAN(01) · TOON(87 n)
```
- Na een `85`-fout op de naam **nooit** het blok sturen (de fan staat dan in commandomodus).
- Elke stap wordt beantwoord met precies één `0E` of `85`; tel ze, dan kan een laat antwoord
  op een vorig commando nooit voor het antwoord op de naam worden aangezien.
- `size` = 1600 + lengte(frames) + 1; naam moet op `.mp4` eindigen.
- Eén JPEG = foto; meerdere achter elkaar = video/animatie.

## 6. Fantypes en beeldformaat
| type | hoeken × LEDs | fps | formaat | ondersteund |
|---|---|---|---|---|
| 1, 17 | 960 × 192 | 20 | JPEG, 4 / 2 armen | ja |
| 2 | 960 × 168 | 20 | JPEG, 4 armen | ja |
| 3 | 1440 × 240 | – | JPEG, 1 arm (alleen Holoscope.exe) | ja |
| 4 | 960 × 144 | 20 | JPEG, 4 armen | ja |
| 5 | 960 × 128 | 20 | JPEG, 2 armen | ja |
| **8 (P56)** | **960 × 172** | 20 | **JPEG 688×240, 4 armen** | **ja** |
| 11 | 960 × 184 | 20 | JPEG, 4 armen | ja |
| 14 | 960 × 176 | 20 | JPEG, 2 armen | ja |
| 6, 7, 12, 13, 15 | div. | 12 | Holoscope.exe: ruwe polaire JPEG; Holoscope.apk: binair | oud formaat |
| 16, 18, 23, 27, 29–31, 36 | div. | 12–16 | binair (`hc32l`, `hs42l`, `hl20`, `hl18`) | nee |
| 20–22, 24–26, 28, 32–34, 37 | div. | 20 | JPEG, armindeling niet geverifieerd | nee |

Transcode (identiek voor cast en upload): auto-belichting, `warpPolar`, radiale verzwakking,
zaagtand-zwart in het midden, arm-indeling 0-3-2-1, JPEG q80 → ×0,9 tot ≤ 20476 B.
De vierde parameter in Holoscope.apk (16000/7992/8000) is een audio-samplerate: video's kunnen geluid bevatten.

## 7. Live cast
Elk frame = §6 → 20480 B: `5A 28 | JPEG | nullen | 55 F5`, in twee stukken (14600 + 5880 B);
na elk stuk wachten tot de fan reageert.

## 8. Niet geïmplementeerd
- **Firmware**: Holoscope.exe start een update via een aparte socket met een `.bin`; in de APK
  zitten alleen teksten. Geen firmware-bestand, chip of bootloader bekend → bewust niet gebouwd.
- **Cloud-mediabibliotheek** (`bg.holoscope.tech`), **groepsweergave** (beeld verdelen over
  meerdere fans), **3D-tekst/-bol/kubus**, **geluid in video's**: geanalyseerd, nog niet gebouwd.
- **Binaire beeldformaten** voor fantypes zonder JPEG.
