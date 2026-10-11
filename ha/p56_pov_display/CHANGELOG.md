# Changelog

## 2.1.2

p56-server 0.3.2.

- **The file list pointed at the file next to the one that plays** on a real P56 (the fan
  counts the playing file from 0). The integration's file select and sensors are right now too.
- **Casting with an empty playlist** works: casting resumes a paused fan.
- The P56 Studio app on Windows can cast its screen or a window through Home Assistant.

## 2.1.1

p56-server 0.3.1. New: a [user guide](https://github.com/trackerr/home-assistant-p56/blob/main/docs/GUIDE.md),
also for using the P56 Studio app on your phone through Home Assistant.

- **Cast your Home Assistant cameras** to the fan: Cast → Home Assistant camera lists every
  camera entity. The add-on now asks for access to Home Assistant's API (`homeassistant_api`)
  for this.
- The dial on the Fan tab shows a preview of what the fan plays (files uploaded through the
  add-on, kept in `/data/thumbs`) and the live picture while casting.
- Fan tab: brightness is a slider under the dial; "Video (live)" is gone from Cast (a video is
  cast as media).

## 2.1.0

p56-server 0.3.0, tested on a real P56.

- **Live cast works on a real P56** (`mode: cast`, and casting from the panel): the server now
  switches the fan to live mode first, as Holoscope does. Before, the fan answered every frame
  but kept playing its playlist. Casting also turns a fan on that is off.
- **Uploads on a real P56**: the fan lists its files without `.mp4`, which made an upload look
  failed and kept a file with the same name from being replaced (the "already exists" error
  when a text entity updates). Fixed.
- Animations (GIF, video) are uploaded in Holoscope's layout (one record per frame) and play at
  the right speed; photos from a phone are turned upright (EXIF orientation).
- `lan_api` now maps to p56-server's `--lan` (the server answers only Home Assistant itself and
  the panel unless it is on); `api_token` works as before.

## 2.0.5

Security and reliability fixes.

- **API token**: new option `api_token`. When set, clients on the network must send it
  (`Authorization: Bearer <token>` or `?token=<token>`); Home Assistant itself and the panel
  do not need it. Without it they get `401 Unauthorized`.
- **`lan_api` is now off by default** for new installations: only Home Assistant itself and
  the panel reach port 8765. Existing installations keep their saved options (also
  `lan_api: true`); if you use the API from other machines, set an `api_token` as well.
- `path` in `/api/show` (and `image` in the 1.x `/upload`) only reads pictures from `/media`
  and `/share`; any other file is refused (`403`). Use `data` or `frames` for anything else.
- `show_seconds`: the removal timer of an earlier picture no longer deletes a newer file with
  the same name, or another file that took its slot after it was deleted by hand. Afterwards
  the fan returns to the file that played before, found by name (slot numbers shift).
- `mode: cast` with a `mac` or `id` now streams to that fan only, no longer to every fan.
- p56-server 0.2.5 (the `p56` command line tool also casts only to the fans chosen with
  `--fan`/`--all`, and refuses unknown or unsupported fan types instead of using another layout).

## 2.0.4

- `/api/show`: ready-made styles (alert, warning, info, success, price, clock) with a ring along the rim,
  Material Design Icons (`icon`), text colour from a value (`color_scale`), animated GIFs with all
  their frames, and `frames` for any series of pictures.
- p56-server 0.2.4.

## 2.0.3

- The panel (P56 Studio web app) is available in English and Dutch, with a switch in the header.
- Log and error messages in English.
- p56-server 0.2.3.

## 2.0.2

- The **Virtual fan** option now really takes effect right after saving: the add-on reads its
  saved options from the Supervisor (2.0.1 only saw them after a restart).
- p56-server 0.2.2.

## 2.0.1

- Virtual fan option read while running (from the options file).
- p56-server 0.2.1.

## 2.0.0

- Rewritten on **p56-server** (Rust, static binary, ~5 MB) instead of Python + Pillow.
- Full protocol of the vendor app: status of every fan (power, playing, brightness,
  angle, colour mode, repeat, breath light, autostart, master), playlist, device
  name, hotspot/router settings, angle, volume, delete/rename, format/reset.
- Several fans at once; reconnects by itself.
- **Panel** in the sidebar (ingress) with the complete P56 Studio web app.
- **Discovery**: the `p56_pov` integration is offered automatically.
- Live events (`/api/events`) so the integration updates at once.
- `/api/show`: text (word wrap, sized to fit the round display) and pictures
  (PNG/JPEG/GIF/WebP/BMP, base64 or path), stored on the fan or shown live (cast),
  optional removal after `show_seconds`, same name replaces the earlier file.
- `/status` and `/health` answer without a fan as well (so Home Assistant can start).
- Option `simulate`: a virtual fan for testing without hardware.
- Option `lan_api`: limit the API to Home Assistant itself.
- 1.x endpoints (`/text`, `/upload`, `/delete`, `/on`, `/off`) unchanged.

## 1.0.0

- First local add-on (Python).
