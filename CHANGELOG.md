# Changelog

P56 Studio (Windows, Linux, Android), `p56` (CLI), `p56-sim` and `p56-server`.
The Home Assistant add-on and integration have their own changelog in
[`ha/p56_pov_display/CHANGELOG.md`](ha/p56_pov_display/CHANGELOG.md).

## 0.3.32 — 2026-10-22

### Fixed
- A fan that was off now and then kept turning (black) after a message although it reported
  "off". After switching it off again, the server now sends "off" once more 3 s later (not when
  a new message or a hand on/off came in between); that second "off" stopped it on the real fan.

## 0.3.31 — 2026-10-17

### Fixed
- A still picture or slow animation shown live could briefly fall back to black or the playlist,
  because frames arrived every 200 ms while a real P56 may leave live mode after roughly 130 ms.
  Timed casts now repeat the current picture every 100 ms; animations keep their intended speed.

## 0.3.30 — 2026-10-14

### Fixed
- App tab: through a P56 server the app showed the server's version as its own (for example
  0.3.28 while the app was 0.3.29). It now shows both: "P56 Studio 0.3.30 · server 0.3.29 (linux)".

## 0.3.29 — 2026-10-14

### Fixed
- The server's own web page (also the Home Assistant panel) could stop and restart the server's
  fan link with the IP and fan addresses stored in that browser (App tab → *Save and reconnect*),
  changing the link for every device until the server restarted. Through a server the App tab
  now only says that these are set in the server's options, and the page never stops the link or
  sends its own settings. The app with its own engine works as before.

## 0.3.28 — 2026-10-13

### Removed
- **Group master** (`1F`/`20`, for several fans showing one picture together): no switch in the
  app, no `p56 master`, no `master_on`/`master_off` control, no `master` in the status. P56
  Studio supports one fan per picture; PROTOCOL.md still documents the commands.

## 0.3.27 — 2026-10-13

### Fixed
- Android: dragging the outer ring of the dial (turning the picture) stopped after the first
  bit of movement, because Android took the touch over as a page scroll (Chrome ignores
  `touch-action` inside an SVG). A touch that starts on the ring now stays with it; tried on a
  Galaxy S24.

## 0.3.26 — 2026-10-13

### Changed
- A real 3D model turns more slowly: one turn in 6 s instead of 3.6 s (100 ms per view), as
  liked on the real fan. Models made before turn at the new pace too.

## 0.3.25 — 2026-10-13

### Added
- **Real 3D models** (`kind: "3d"`, ComfyUI only): Qwen Image 2.1 draws the object, its background
  is cut away, Trellis 2 makes a coloured 3D model and ComfyUI renders it from all round (60 views,
  one turn in 3.6 s). About 2 minutes on a laptop graphics card (171 s on the real fan's test).
  `p56-server --ai-dir <folder>` keeps the views, so the same prompt again is there at once, also
  after a restart. The web app's AI box offers **Real 3D** when ComfyUI is there.

### Changed
- Docs: the breathing light is a status light (all LEDs light up in one colour now and then).

## 0.3.24 — 2026-10-13

### Changed
- Web app in the Home Assistant panel: the Cast tab no longer offers the camera of the device you
  look at the panel with, only Home Assistant's cameras.

### Fixed
- Home Assistant panel: when Home Assistant was still starting, the Cast tab showed no Home
  Assistant cameras until the panel was opened again; it now asks again (up to 30 s).

## 0.3.23 — 2026-10-13

### Added
- A fan that is off first spins up on black for 2.5 s before something is shown, so a message
  is steady at once: on a black file in its playlist when there is one (recognised by its
  preview, or by the name black, zwart, blank or leeg), otherwise on black sent live.

## 0.3.22 — 2026-10-13

### Added
- `/api/show` with `text` (or `icon`) and a picture or animation (`data`, `path`, `frames`):
  the text is written over every frame, on a darkened band, instead of the text alone. For
  example "Rain in 15 minutes" over a moving rain radar.

## 0.3.21 — 2026-10-13

### Added
- `p56-server --comfyui <url>` (and `--comfyui-size`): AI pictures from ComfyUI with Qwen Image
  2.1 on a computer in the house; Home Assistant's AI Task is the fallback. `entity_id:
  "comfyui"` picks it.

### Changed
- The instructions for AI pictures no longer describe the fan itself (Qwen Image painted a fan
  disc or a ring of light around the subject): only a black background, a lit subject in the
  middle, and no frame, ring or device.
- A background behind text is a whole picture of its theme (a washing machine for "the laundry
  is done"), bright on a darker background and without any writing, instead of a dark middle
  with shapes around the edge; the text goes over it on a darkened band as before.

## 0.3.20 — 2026-10-13

### Added
- `p56-server`: `/api/shown/<mac>` serves the last picture faded (grey, see-through) once it is no
  longer on the fan, with header `X-P56-Live`; `/api/state` has `shown: {at, live}` per fan.

### Changed
- Switching a fan off by hand is checked too: right after a cast a fan may ignore it, so it is
  sent once more when the fan stays on.

### Removed
- `queue: false` in `/api/show`: it was only there for the Home Assistant pages, which are gone.

### Fixed
- No status poll or heartbeat could slip in at the moment a cast starts.

## 0.3.19 — 2026-10-13

### Fixed
- A fan that was off and showed a message as a cast (`mode: cast`) went black afterwards but
  stayed on: a fan in live mode ignores "off". Switching off now waits until the cast is over,
  checks that the fan really went off and tries once more if not. `p56-sim` ignores "off" during
  a cast too.

## 0.3.18 — 2026-10-12

### Changed
- `show_ai` with `text`: over the AI background only the text is shown; an `icon` is left out.

## 0.3.17 — 2026-10-12

### Added
- **A fan that was off goes off again after a message**: a temporary message (stored or cast)
  switches the fan on, and after the last message it is switched off again; a fan that was on
  stays on. Tested on a real P56.

### Fixed
- **An upload during a live cast was not stored** (a real P56 in live mode stores nothing): the
  server now stops a running cast first, and the upload waits until the fan has left live mode.
  The simulator refuses an upload during live mode, like the fan.
- **Switching the fan off while something is cast** (e.g. rotating pages) works: the cast stops,
  and pages no longer switch a fan on that was switched off (they are skipped; messages may).

## 0.3.16 — 2026-10-12

### Fixed
- **Texts and messages were stored as a 0.1 s animation instead of a still picture**, so a
  playlist flashed past them and a selected message was gone before it could be repeated. One
  picture is now stored as a still (one frame, which the fan shows 5 s, as Holoscope does).
- A temporary message really stays: on a real P56 selecting a file sets the fan back to
  repeating the whole playlist, so "repeat one" is now sent after the fan reports that it plays
  the message (tested on a real P56: the price stayed on for its 30 s, then the playlist went on).
  The simulator now behaves the same way.

## 0.3.15 — 2026-10-12

### Fixed
- **A message showed only for a moment, then the first file stayed on** (e.g. the electricity
  price flashed by and a picture from the playlist stuck). After an upload the fan switches
  itself off and on, starts at its first file and can miss the select sent right then; the
  server then set "repeat one" on that first file. The server now waits until the fan really
  plays the new file (and selects it again if needed) before it repeats only that one. The
  simulator can miss a select the same way (`slow_start`).
- AI pictures right after Home Assistant restarts: listing its entities is tried once more.
- **A fan that comes on the network late is picked up** (field report: the Home Assistant
  add-on started without a fan, and the fan that joined two hours later was only connected after
  a restart). Discovery is made sturdier:
  - the address advertised in the discovery packet is looked up again while running (at most
    once a second), so an address picked while the network was still coming up, or before it
    changed, no longer sends the fan somewhere it cannot connect to; a change shows in the log;
  - a fan that connected or answered before gets the discovery packet directly every round as
    well (with the address this machine has towards it), also when broadcasts do not reach it;
  - discovery starts again by itself if its UDP socket fails or its thread stops, and a send that
    hangs can no longer hold it up; the rounds keep their 3 s pace for as long as no fan shows up;
  - an address that answers the discovery is logged ("… answered the discovery"), so the log
    shows whether a fan that does not connect hears the server at all.
- `/api/call/link_info` shows the address advertised now instead of the one from the start.

## 0.3.14 — 2026-10-12

### Added
- `/api/show`: `queue: false` — a message that would have to wait is skipped (`skipped: true`)
  and does not come back after an interruption; a cast message that is interrupted stops at
  once. For the Home Assistant pages.

### Removed
- **Bluetooth**: P56 Studio, `p56`, `p56-server` and the simulator no longer use the fan's
  Bluetooth. The Bluetooth section on the Network tab, `p56 ble`, and the `ble_on`, `ble_off`,
  `ble_scan`, `ble_disconnect` and `ble_connect` commands are gone; the device list has `volume`
  instead of `ble`. Reason: on the P56, Wi-Fi and Bluetooth share one 2.4 GHz radio. With a
  Bluetooth audio receiver connected, the fan could no longer rejoin the home Wi-Fi and kept
  falling back to its own hotspot until Bluetooth was switched off. The volume slider still
  works: the fan's `8F` report is still read for the speaker volume.
- **Colour mode** (true colour / high brightness): P56 Studio no longer shows the colour readout
  or the colour choice on the Fan tab. Reason: on a real P56, switching it made no visible
  difference, and the fan never reports which mode it is in (it sends 0). The `color_true` and
  `color_bright` commands and `p56 color` stay, for fan models that may use it.

## 0.3.13 — 2026-10-12

### Added
- **Priority of messages** (`priority` in `/api/show`): while a temporary message shows, a less
  important one waits and comes afterwards, a more important one interrupts it and the
  interrupted one comes back for the time it had left.
- **Meter ring** (`gauge` with `color_scale`): the ring along the rim fills clockwise as far as
  the value is between min and max.

### Fixed
- **A temporary message stays on until its time is up**: the fan repeats only that file
  meanwhile (it moved on to the next file of the playlist), then returns to the file and the
  repeat mode from before.
- Two temporary messages after each other: the fan now returns to the file that played before
  the first one (it could end on nothing).

## 0.3.12 — 2026-10-12

### Changed
- **AI pictures in any style.** The instructions no longer ask for bright neon-like pictures;
  only what the fan needs is added (black background, subject in the middle, well lit). A
  **Style** choice (photo, 3D render, cartoon, anime, painting, watercolour, pixel art, neon, line
  drawing, comic, or as described) is in the AI box and `art_style` in the API.
- Backgrounds behind text (`show_ai` with `text`, e.g. notifications and prices) stay simple:
  a few bold, clean shapes, not busy, so the text stays readable.
- The AI box offers Still and Turning; drawn animations are no longer offered, as the AI could
  not draw them consistently (the API still accepts `kind: anim`).

## 0.3.11 — 2026-10-12

### Added
- `p56-server`: `show_ai` with `text` (or `icon`): the AI makes a background with a calm middle
  and the text is written over it, sharp and with a dark edge. The same prompt reuses the picture
  made before (`fresh` makes a new one), so a price updated every quarter of an hour does not pay
  for a new picture each time.
- `/api/show`: `backdrop`, a picture behind the text.

## 0.3.10 — 2026-10-12

### Changed
- **AI pictures, Turning:** the object turns round its own upright axis, as on a turntable,
  instead of the whole picture turning round on the display.

### Fixed
- **AI animations:** a white margin the AI sometimes draws round the frames becomes black, the
  frames are cut where the AI really drew them (not blindly in 16 equal parts), and every frame is
  centred, so the subject stays in place. The AI is asked more clearly for frames that differ and
  for a subject that moves on the spot.
- The preview in the Media tab plays animations (with a pause button), instead of showing one
  frame.

## 0.3.9 — 2026-10-12

### Added
- **AI pictures** (in Home Assistant): the Media tab gets an **AI picture** box when Home
  Assistant has an AI that makes pictures (AI Task, e.g. Google Gemini). Describe a picture and
  choose still, turning or animation; the AI draws it with a black background made for the fan.
  Also for P56 Studio on a phone or computer that goes through Home Assistant.
- `p56-server`: the calls `ai_status`, `ai_image` and `show_ai`.

## 0.3.8 — 2026-10-12

### Changed
- **Android: new signing key.** Remove P56 Studio from the phone once and install this version
  (an update over an older version is refused because the key changed); settings such as the
  server address have to be entered again.
- The Windows installer's publisher is the project's GitHub name.
- Live cast runs at **25 frames per second** by default (was 15): on a real P56 it looks
  smoother than 15 or 20, and casting through Home Assistant keeps up.

## 0.3.7 — 2026-10-12

### Added
- **Overwrite**: when the fan already has a file with that name, the app asks whether to
  overwrite it and then replaces it (upload takes `replace`).

### Fixed
- Dialogs: a long file name (in "Delete …?") wraps instead of pushing the dialog off the screen
  on a phone.

## 0.3.6 — 2026-10-12

### Fixed
- **Files with long names broke the fan's memory card** (the real cause of the failed gallery
  videos, which have long names). A real P56 stores names of at most 24 characters; 25–27 are
  silently not stored, and 28 (the limit this app used) also breaks the card's file list, so
  every later upload fails until it is formatted. Names are now cut at 24 characters, and the
  simulator refuses longer names like a real fan.

## 0.3.5 — 2026-10-12

### Changed
- Media: **Upload & show** makes the frames of a picked video itself; "Make frames" is only
  needed to choose frames/s, a start or an end first.

## 0.3.4 — 2026-10-12

### Fixed
- **Uploads through Home Assistant (Linux) still lost large files** and left the fan's memory
  card refusing every upload. On Linux the last megabytes of a file can still be on their way
  when the app has written them, and the fan reports its status while it receives them; the app
  took that for "stored". It now waits for the proof: the playlist the fan sends by itself, with
  the new file in it. Without that proof it no longer switches the fan off and on. Tested on a
  real P56 through a relay that behaves like Linux.

## 0.3.3 — 2026-10-11

### Fixed
- **Long videos broke the fan's memory card.** After the last byte of a file the app switched
  the fan off and on at once, while a real P56 was still writing a large file to its card: the
  file was lost and the card refused every upload ("transfer failed", code 8) until it was
  formatted. Like Holoscope, the app now repeats "finished" every half second until the fan
  reports back, and stops and cancels at once when the fan reports an error. Tested on a real
  P56 with files of 9, 15 and 27 MB (a 44 s gallery video is about 14 MB).

## 0.3.2 — 2026-10-11

### Fixed
- **Casting with an empty playlist**: a P56 without files pauses itself and then showed nothing
  that was cast. Casting now resumes a paused fan (as it already switched on a fan that is off).
  Confirmed on a real P56, as are the playlist order and casting a window through Home Assistant.
- **The playlist pointed at the file next to the one that plays** on a real P56: the fan reports
  the playing file counting from 0, while selecting counts from 1 (as Holoscope does). The
  status is now read that way, and the simulator reports it like a real fan.
- Cast: "This screen" was missing when the app controls the fan through a P56 server (such as
  Home Assistant). The app now uses the system's screen sharing there (choose a screen or
  window in its own picker). The Home Assistant panel itself does not offer it.

## 0.3.1 — 2026-10-11

### Added
- **The dial shows what the fan plays**: a preview of the playing file (kept for every file
  this app uploads, also across restarts; the fan cannot send its files back), and the live
  picture while casting. Files put on the fan by another app still show their name.
- Cast on Windows and macOS: **one window** instead of the whole screen (Cast → This screen →
  choose the window).
- Cast: in Home Assistant, any **camera entity** can be cast (Cast → Home Assistant camera); the
  add-on fetches a new snapshot several times a second and keeps the fan in live mode.

### Changed
- Fan tab: the dial only turns the picture (outer ring); brightness is a plain slider (1–16)
  below it, which is easier to use than the inner arc.
- Cast: "Video (live)" is gone; a video is cast as media (make its frames on the Media tab).
  "Camera" is offered only where the browser provides a camera.

### Fixed
- App tab: the server address stays filled in when switching between "this app controls the
  fan" and "through a P56 server".
- Android: "Control through a P56 server" could not reach a server (such as the Home Assistant
  add-on) in release builds, which blocked plain http; only debug builds allowed it.

## 0.3.0 — 2026-10-11

### Fixed
- **Live cast works on a real P56.** A cast now starts the fan's live mode with `5A 26 26 F5`
  (the fan answers `5A 29 29 F5`), as Holoscope.exe does; without it the fan answered every
  frame but kept playing its playlist. Frames are written whole without waiting for an answer
  (15 fps on a real P56 instead of ~2), and no heartbeat or status poll goes out while casting.
  The simulator shows cast frames only in live mode and flags a heartbeat during a cast.
  Tested on a real P56: the test card, a webcam from the Windows app at 15 fps, and the camera
  of an Android phone.
- **Uploads on a real P56 were reported as failed and could not replace a file.** The fan lists
  its files without `.mp4`; the names are now read as `name.mp4`, so an uploaded file is found
  in the playlist and a file with the same name is replaced (the "already exists" error on a
  second text or ticker). The simulator lists names the same way. A file in the playlist counts
  as delivered: a real P56 does not acknowledge the data itself.
- **Photos from a phone are upright.** The EXIF orientation of a picture is applied (phones
  store portrait photos sideways with an orientation tag).
- Casting turns a fan on that is off (it shows nothing that is cast to it otherwise).
- While a text or ticker is being made, Upload waits for it instead of sending the previous
  picture. A typed text that was not made yet (or was changed since) is made by Upload itself,
  instead of sending the test card.
- The camera or video being cast is shown small on the Cast tab instead of in a `display:none`
  element (in which Android's WebView may decode no frames), and a camera or video that gives
  no picture for 5 s stops the cast with a message instead of silently casting nothing.
- The playlist shows names without `.mp4`; a colour mode the fan does not report (a real P56
  sends 0) is shown as "not reported" instead of a guess.
- **Animations (GIF, video) are uploaded the way Holoscope does it.** A file is a duration
  followed by one record per frame (`[length][1600 B audio][JPEG][padding]`); this project used to
  put all JPEGs of an animation in one record. Verified in the machine code of Holoscope.apk
  2.5.2 / 2.5.4 and Holoscope.exe 1.1.8. Still pictures were already right and are unchanged
  byte for byte. Tested on a real P56: a 40-frame animation plays at the right speed.
- Animations play at their own speed and in time with their sound: frames are picked on a
  50 ms timeline (the fan's fixed 20 frames per second).
- Packet meanings aligned with Holoscope: the Bluetooth report (state, volume), the group layout
  in the machine info (it is not an IP address), `0x23`, `0x22` no longer aborts an upload, text
  fields of at most 31 bytes, file names of at most 28 bytes + `.mp4`.
- A JPEG is never cut short any more (that could drop its end marker); the quality goes down
  instead. Files may use larger JPEGs (61440 B, Holoscope's limit) than cast frames (20476 B).
- Casting: a frame only counts when the fan acknowledges it, a fan that stops answering is
  reconnected, and a slow fan no longer holds up the others (one lock per fan).
- Uploads report whether the fan acknowledged and listed the file; a dropped connection is
  an error instead of a success.
- A timed cast from `/api/show` (Home Assistant) is stopped by `cast_stop` and replaced by the
  next cast instead of running on.

### Added
- **Video sound on the fan:** uploaded videos carry their sound (16-bit mono, 16 kHz, 1/20 s per
  frame) for a speaker on the fan, as Holoscope does; a switch in the Media tab, `--no-sound` in
  the CLI.
- Bluetooth state (on, connected, connecting) and the speaker volume as the fan reports them.
- Model names per fan type (P56, P50, P62, …); turn a picture in 90° steps.
- **Moving text:** a ticker ("lichtkrant") across the middle or text turning round the rim, at
  an adjustable speed and direction; in the Media tab and in `/api/show` (`motion`: `scroll` /
  `orbit`, `speed`, `reverse`), uploaded as an animation or cast live.
- **Several controllers for one fan:** P56 Studio can control the fans through a p56-server
  (App tab) instead of its own engine, so phones, computers and Home Assistant share one fan.

### Security
- p56-server refuses requests from other web sites (a page could otherwise make this computer's
  browser call the API on localhost); P56 Studio's own origin gets CORS headers.

### Changed
- New look, "Baan": the fan as a dial (brightness on the inner arc, picture angle on the outer
  ring), own icons and fonts, rail / bottom bar navigation, new app icon. See
  [docs/DESIGN.md](docs/DESIGN.md).
- Release builds also produce the Linux app (.deb, AppImage) and, when the signing key is set
  up as a repository secret, the Android APK; checksums cover every file.
- CI checks formatting, clippy (warnings are errors) and that the bundled UI is up to date.

## 0.2.5 — 2026-10-11
- Fixes from a code review: stale auto-delete timers, cast only to the selected fans, an
  optional API token for clients on the network, `path` limited to media folders, unknown fan
  types refused, size-limited downloads, colours per server, UI type check in CI, camera
  released on every exit path.

## 0.2.4 — 2026-10-11
- Text cards with styles (alert, warning, info, success, price, clock), Material Design
  icons, colour by value and animations (GIF, frames) for `show_text` / `show_image`.

## 0.2.3 — 2026-10-11
- Code and documentation in English; the app is bilingual (Dutch / English).

## 0.2.2 — 2026-10-11
- The add-on's virtual fan follows the saved option at once (read from the Supervisor API).
- `p56-sim --count N`: extra simulated fans.

## 0.2.1 — 2026-10-11
- Virtual fan on/off from the add-on option; Home Assistant follows add-on restarts.

## 0.2.0 — 2026-10-11
- First public version: protocol worked out from Holoscope, P56 Studio for Windows and
  Android, `p56` CLI, `p56-sim` simulator with a gauntlet of tests, `p56-server` and the Home
  Assistant add-on and integration.
