# P56 Studio

Open-source replacement for the **Holoscope** app for P56 / Holoscope POV "hologram"
fans, written in Rust. Windows and Android app, command line, simulator, and a
Home Assistant add-on + integration, all on one protocol core.

*Nederlands: open-source vervanger van de Holoscope-app voor P56-hologramfans: Windows-
en Android-app, opdrachtregel, simulator en Home Assistant. Het protocol staat in
[PROTOCOL.md](PROTOCOL.md).*

| Part | Folder | What |
|---|---|---|
| `p56-core` | `crates/p56-core` | protocol, picture conversion (polar warp, JPEG), networking; see [PROTOCOL.md](PROTOCOL.md) |
| `p56-service` | `crates/p56-service` | everything the apps do (upload, cast, text rendering, settings) behind one command API |
| P56 Studio | `app/` | Windows and Android app (Tauri 2 + React/shadcn UI) |
| `p56` | `crates/p56-cli` | command line |
| `p56-sim` | `crates/p56-sim` | simulated fan(s) for testing without hardware, with a web view |
| `p56-server` | `crates/p56-server` | headless server: HTTP API, live events, the web UI; used by the Home Assistant add-on |
| Home Assistant | `ha/` | add-on `p56_pov_display` + integration `p56_pov`, published as [home-assistant-p56](https://github.com/trackerr/home-assistant-p56) |

```
          ┌──────────── p56-core (protocol, transcoding, link) ────────────┐
          │                                                                │
   p56-service ──┬── P56 Studio (Windows, Android)                         │
                 ├── p56-server ── Home Assistant add-on ── integration    │
                 │                                                         │
   p56 (CLI) ────┘                               p56-sim (virtual fans) ───┘
```

<p align="center"><img src="ha/docs/panel.jpg" width="820" alt="P56 Studio: text on the round display, with the fan's own picture layout below"></p>

## Download

[Releases](https://github.com/trackerr/p56-studio/releases): Windows installer,
Android APK, `p56` command line, `p56-server` for Linux (x86_64/aarch64, static).

## Using the app

1. Connect the PC or phone to the fan's Wi-Fi, or put both on the same router.
2. Start **P56 Studio** and close Holoscope (only one program can serve a fan).
3. Windows asks to open the firewall on first start: allow it (private network).
4. The fan appears by itself. If not: tab **App** → enter the fan's IP.

Tabs: **Fan** (power, brightness, angle, colour, repeat, breath light, autostart),
**Playlist** (play, rename, delete), **Media** (photo, GIF, video or text),
**Cast** (test card, media, video, camera, screen), **Network** (name, hotspot,
router, Bluetooth). The round preview shows what the fan will display.

## Without a fan: the simulator

```
p56-sim                 # behaves like a P56 on UDP 8988
```

Then open P56 Studio (or `p56 info`): the simulated fan connects by itself.
http://localhost:8099/ shows what the fan displays, its settings and any protocol
errors. `p56-sim --count 3` simulates three fans; `--fragment` splits answers.

## Command line

```
p56 info
p56 upload photo.jpg --slot 1      # also .gif and (with ffmpeg) video
p56 cast test --seconds 10
p56 brightness 12
p56 angle -45
p56 router MyWifi password
p56 --help
```

## Home Assistant

See [ha/README.md](ha/README.md): the add-on runs `p56-server` (panel with the full
web app, HTTP API) and the integration adds light, switches, selects, sensors, a
text field, notifications, an image entity and the actions `p56_pov.show_text` /
`p56_pov.show_image`.

`p56-server` also runs on its own: `p56-server --http-port 8765` (add `--simulate`
for a virtual fan), then open http://localhost:8765/.

## Building

```
cargo build --release --workspace                  # p56, p56-sim, p56-server, P56 Studio
cd app/ui && pnpm install && pnpm run bundle       # only after UI changes → app/src/index.html
cd app && npx tauri build                          # Windows installer
powershell -File scripts/android.ps1 build         # Android APK (see the script for the SDK setup)
```

Release signing of the APK reads `app/src-tauri/gen/android/keystore.properties`
(not in git).

## Tests

```
cargo test --workspace                             # unit, simulator, CLI, server
powershell -File scripts/gauntlet.ps1 -Rounds 20   # everything repeated until something fails
node e2e/app-e2e.mjs                               # the real app against the simulator (CDP)
cd ha && pytest                                    # Home Assistant integration against p56-server
```

## License

MIT. Not affiliated with the maker of the fans or of Holoscope. The protocol was
worked out for interoperability with hardware you own; no vendor code or binaries
are included.
