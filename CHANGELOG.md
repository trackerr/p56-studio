# Changelog

P56 Studio (Windows, Linux, Android), `p56` (CLI), `p56-sim` and `p56-server`.
The Home Assistant add-on and integration have their own changelog in
[`ha/p56_pov_display/CHANGELOG.md`](ha/p56_pov_display/CHANGELOG.md).

## Unreleased

### Fixed
- **Uploads on a real P56 were reported as failed and could not replace a file.** The fan lists
  its files without `.mp4`; the names are now read as `name.mp4`, so an uploaded file is found
  in the playlist and a file with the same name is replaced (the "already exists" error on a
  second text or ticker). The simulator lists names the same way. A file in the playlist counts
  as delivered: a real P56 does not acknowledge the data itself.
- **Photos from a phone are upright.** The EXIF orientation of a picture is applied (phones
  store portrait photos sideways with an orientation tag).
- Casting turns a fan on that is off (it shows nothing that is cast to it otherwise).
- While a text or ticker is being made, Upload waits for it instead of sending the previous
  picture.
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
