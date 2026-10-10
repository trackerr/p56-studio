# P56 / Holoscope hologramfan — volledig protocol

Reverse-engineered uit `Holoscope.exe` (Qt5/C++, FFmpeg, OpenCV 4.5.5) en bevestigd met
de eigen fan-emulator (`fan_emu.py`) en de werkende v25-testlab tegen de echte P56.
Status: alle functies van de app geanalyseerd; alles behalve firmware-flash en cloud is
geïmplementeerd en getest (emulator). Datum: 2026-10-10.

## 1. Netwerk
| Wat | Waarde |
|---|---|
| Rol | **PC is TCP-server** op poort 6666; de fan verbindt als client |
| Ontdekken | PC broadcast `"<pc-ip>HS"` naar `255.255.255.255:8988` elke 3 s |
| Antwoord | fan stuurt `"<fan-ip>HS"` terug (begint met `192`, eindigt op `HS`) |
| Standaard-AP | fan is wifi-accesspoint, IP `192.168.4.1`; PC krijgt `192.168.4.x` |
| Heartbeat | `5A 0D 0D F5 3E` elke 3 s; fan antwoordt met 86-status |

## 2. Frameformaat
Elk pakket: `5A <opcode> <body…> F5`. Pakketten met tekst eindigen op een
**XOR-checksum** over `<len><ascii>` vóór de `F5`. Lengtevelden zijn big-endian; de
upload-tag en -size zijn **little-endian** u32.

## 3. Opcodes PC → fan
| Op | Betekenis | Pakket |
|---|---|---|
| 01 | aan | `5A 01 01 F5` |
| 02 | uit | `5A 02 02 F5` |
| 03 | pauze | `5A 03 03 F5` |
| 04 | play | `5A 04 04 F5` |
| 0D | heartbeat/status-poll | `5A 0D 0D F5 3E` |
| 10 | verzending annuleren | `5A 10 … F5` |
| 1C | get machine info | `5A 1C 1C F5` → fan antwoordt 8D |
| 23 | select device (groep) | `5A 23 … F5` |
| 28 | **live cast-frame** | `5A 28 <jpeg> 00… 55 F5` (altijd 20480 B) |
| 2A/2B | hoek-uitlijning start/stop | `5A 2A … F5` / `5A 2B … F5` |
| 80 | wis slot n | `5A 80 n n F5` |
| 83 | hernoem | `5A 83 <len><oud\0nieuw> <xor> F5` |
| 84 | bestandsnaam zetten (upload-start) | `5A 84 <len><ascii> <xor> F5` |
| 87 | kies/toon slot n (1-based) | `5A 87 n n F5` |
| 8A | wifi instellen | `5A 8A <len><ssid\0pass> <xor> F5` |
| 90 | transstart (start transcode/verzend; ook firmware-trigger) | `5A 90 … F5` |
| 91 | commit (upload afsluiten → opslaan) | `5A 91 00 00 00 01 01 F5` |
| 93 | groep opgeslagen | `5A 93 … F5` |
| 94 | charset gbk/utf-8 | `5A 94 … F5` |
| 95 | apparaat aan groep toevoegen | `5A 95 … F5` |

## 4. Antwoorden fan → PC
| Op | Betekenis | Opbouw |
|---|---|---|
| 0E | "klaar / ontvangen" ack | `5A 0E 0E F5` (na elk upload-blok en commando) |
| 84 | bestandslijst | `5A 84 <len16> "1 naam/2 naam/…" <xor> F5` |
| 86 | status (11 B) | `5A 86 <flags> <playidx> <leds> <hoeken16> ? <fantype> ? F5` |
| 8D | machine-info (126 B) | naam@14 (32 B), MAC@46 (6 B), wifi@52, versie@84, F5@125 |
| 21/22/29/85 | firmware-stappen | zie §8 |

Status-flags (byte 2 van 86): bit1 = aan, bit2/3 = play-mode, bit4 = power, bit5 = ademen,
bit6 = autostart, bit7 = sync. `fantype` (byte 8) bepaalt de resolutie (§6).

## 5. Bestandsupload (foto of video in een slot)
```
AAN(01) → SETNAME(84 "naam.mp4") → [tag u32le][size u32le][1600×00][JPEG-frames…][00]
   (in stukken van 1440 B, 2 ms pauze)  → COMMIT(91) ×2 → UIT(02) → AAN(01) → IDX(87 n)
```
- `tag` = 4950 + (ms & 0xFFFF) + seq (willekeurig uniek is voldoende).
- `size` = 1600 + lengte(frames) + 1.
- Naam **moet** op `.mp4` eindigen, anders slaat de firmware hem niet op in de playlist.
- Eén JPEG = stilstaand beeld; meerdere JPEG's achter elkaar = video (fan scant op
  `FF D8 … FF D9`). Er is géén aparte fps/framecount-header; de 1600 bytes zijn nullen.
- Een JPEG met het verkeerde formaat (niet 688×240 voor type 8) wordt stil genegeerd:
  naam verschijnt wél in de lijst, beeld blijft het oude. (Dit was het "nr. 1 blijft"-probleem.)

## 6. Transcode (beeld → wat de fan toont)
Per frame, identiek voor cast en upload (`holofan.encode()`):
1. `cv::warpPolar` rond het midden → (hoeken × straal). x/y-offset verschuift het
   midden, scale de radius. Resolutie per fantype:

| type | hoeken × straal | armen | JPEG-afm. |
|---|---|---|---|
| 1, 17 | 960 × 192 | 4 / 2 | 768×240 |
| 2 | 960 × 168 | 4 | 672×240 |
| 3 | 1440 × 240 | 1 | 240×1440 |
| 4 | 960 × 144 | 4 | 576×240 |
| 5 | 960 × 128 | 2 | 256×480 |
| 6 | 600 × 133 | — | 133×600 |
| 7, 15 | 490 × 122 | — | 122×490 |
| **8 (P56)** | **960 × 172** | **4** | **688×240** |
| 11 | 960 × 184 | 4 | 736×240 |
| 12 | 512 × 85 | — | 85×512 |
| 13 | 490 × 128 | — | 128×490 |
| 14 | 960 × 176 | 2 | 352×480 |

2. Arm-indeling: hoeken verdeeld over de armen, segmenten per rij naast elkaar in
   volgorde **0-3-2-1** (4 armen) of 0-1 (2 armen), straal van buiten naar binnen,
   rijen van onder naar boven.
3. Helderheid: automatische belichting (gemiddeld grijs × 1,5; max 150, type 14: 100);
   verzwakking per straalpositie (binnenste LEDs 0,73 → 0,98 bij pixel 16; types 5/14/17
   binnenste 3 op 0,6); zaagtand-zwart in het midden (rij r: `r mod 8` binnenste pixels op 0).
4. JPEG kwaliteit 80; zakt ×0,9 tot ≤ 20476 B (max 11 pogingen, anders "size err").
5. Kleurvolgorde: Holoscope werkt in RGB en draait bytes om → de JPEG bevat **normale**
   kleuren. (Niet nog eens swappen, anders oranje ↔ blauw.)

## 7. Live cast
- Bron: FFmpeg `gdigrab` op `desktop` (met muiscursor) of een videobestand.
- Elk frame = §6 → buffer van 20480 B: `5A 28 | JPEG | nullen | 55 F5`.
- Verzonden in 2 stukken (14600 + 5880 B); de PC wacht na elk stuk ~ op de fan-ack.
- ~10 ms tussen frames; in praktijk 15–25 fps.

## 8. Groepen, wifi, firmware, cloud (geanalyseerd, deels bewust niet gebouwd)
- **Groepen**: `groupprofile/1X3.grp` en `3X1.grp` zijn 132-byte lay-outs (3 × 44 B:
  apparaat-id + positie). Flow: addgroup → 95 (per apparaat) → 23 (select) → 93 (saved).
  Bij "groepsverzending" wordt een virtueel canvas opgedeeld en per fan apart getranscodeerd.
- **Wifi**: `8A <len><ssid\0pass> <xor>`; daarna is de fan alleen nog op het nieuwe
  netwerk te vinden (de app waarschuwt daar ook voor).
- **Firmware**: trigger via 90 + `*.bin`-upload op een aparte "UpdataSocket"; fan meldt
  voortgang met 22 (klaar) / 29 / 85 (fout, "filesexceeds"/"transfail"). **Niet
  geïmplementeerd** — een mislukte flash kan de fan onbruikbaar maken.
- **Cloud** (CloudLogin.qml): websocket naar de fabrikantserver, JSON
  `{"groupId":"1","type":"8","order":""}` als heartbeat; type 8 = ping. **Niet
  geïmplementeerd** — praat met externe servers, geen lokale functie.

## 9. Bestanden in deze map
| Bestand | Rol |
|---|---|
| `holofan.py` | transcode + live cast (`test` / `desktop` / foto / video) |
| `p56.py` | referentie-client: on/off/play/pause/select/delete/rename/wifi/upload/info |
| `fan_emu.py` | fan-emulator (gedraagt zich als P56, weergave op :8099) |
| `xref.py`, `rng.py`, `sx.py`, `allcmds.py` | disassembly-hulpscripts (capstone/pefile) |
| `*.txt` | ruwe analyse-dumps |

Snelstart zonder fan:
```
python fan_emu.py                       # emulator (wacht op broadcast)
python p56.py upload foto.png --slot 1  # upload + toon
python holofan.py desktop               # live scherm-mirroring
```
