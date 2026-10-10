# P56 Studio

Open-source vervanger van Holoscope voor P56 / Holoscope POV-hologramfans, in Rust.

| Onderdeel | Map | Wat |
|---|---|---|
| `p56-core` | `crates/p56-core` | protocol, beeldomzetting, netwerk (zie `PROTOCOL.md`) |
| `p56` | `crates/p56-cli` | opdrachtregel |
| `p56-sim` | `crates/p56-sim` | gesimuleerde fan, voor testen zonder hardware |
| P56 Studio | `app/` | desktop- en Android-app (Tauri + React) |

## De app gebruiken
1. Verbind de pc of telefoon met de wifi van de fan (of zet beide op dezelfde router).
2. Start **P56 Studio** en sluit Holoscope (er kan er maar één tegelijk luisteren).
3. Windows vraagt bij de eerste start om de firewall te openen: kies **Toestaan** (privé netwerk).
4. De fan verschijnt vanzelf. Lukt dat niet: tabblad **App** → vul het IP van de fan in.

Tabbladen: **Fan** (aan/uit, helderheid, hoek, kleur, herhalen, ademlicht, autostart),
**Afspeellijst** (tonen, hernoemen, wissen), **Media** (foto, GIF of video uploaden),
**Casten** (testkaart, media, video, camera, scherm), **Netwerk** (naam, hotspot, router, bluetooth).

## Zonder fan proberen: de simulator
```
p56-sim                 # gedraagt zich als een P56 op UDP 8988
```
Open daarna P56 Studio (of `p56 info`): de gesimuleerde fan verbindt vanzelf.
Op http://localhost:8099/ zie je wat de fan toont, z'n instellingen en eventuele protocolfouten.
`p56-sim --count 3` simuleert drie fans; `--fragment` knipt antwoorden in stukjes.

## Opdrachtregel
```
p56 info
p56 upload foto.jpg --slot 1          # ook .gif en (met ffmpeg) video
p56 cast test --fps 15
p56 brightness 12
p56 angle -45
p56 router MijnWifi wachtwoord
p56 --help
```

## Bouwen
```
cargo build --release --workspace                         # p56, p56-sim, P56 Studio
cd app/ui && pnpm install && pnpm exec vite build         # alleen na wijzigingen aan de UI
```
De UI (`app/ui`, React + shadcn/ui) wordt gebundeld tot één bestand `app/src/index.html`.

## Testen
```
cargo test --workspace                                    # unit + simulator + CLI
powershell -File scripts\gauntlet.ps1 -Rounds 20          # alles herhaald tot er iets faalt
node --experimental-websocket e2e/app-e2e.mjs             # echte app tegen de simulator
```
