# Changelog

## 2.1.25

p56-server 0.3.25, with integration 2.27.0.

- **Real 3D:** `p56_pov.show_ai` with `kind: 3d` (and **Real 3D** in the panel's AI box) makes a
  real 3D model of the object with ComfyUI and shows it turning from all sides. About 2 minutes
  the first time; the add-on keeps it in `/data/ai`, so the same prompt is there at once
  afterwards, also after a restart. Needs ComfyUI with the Trellis 2 files.

## 2.1.24

p56-server 0.3.24, with integration 2.26.0.

- The panel's **Cast** tab shows only your Home Assistant cameras, no longer the camera of the
  phone or computer you look at the panel with.
- Right after Home Assistant starts, the Cast tab now finds your cameras by itself (it asks
  again for up to 30 seconds) instead of only after reopening the panel.

## 2.1.23

p56-server 0.3.23, with integration 2.25.0.

- **A steady start:** a fan that was off first spins up for 2.5 seconds on black (its black
  picture, or black sent live) before a message appears, so the message is steady at once.

## 2.1.22

p56-server 0.3.22, with integration 2.24.0.

- **Text over a picture or animation:** `p56_pov.show_image` takes `text` (and `color`,
  `font_size`): written over every frame, e.g. "Rain in 15 minutes" over the Buienradar radar.

## 2.1.21

p56-server 0.3.21, with integration 2.23.0 (needed for `entity_id: comfyui`).

- **AI pictures from your own computer:** option `comfyui_url` (and `comfyui_size`) makes
  `p56_pov.show_ai` and the panel's AI picture use ComfyUI with Qwen Image 2.1 in your house;
  Home Assistant's AI Task (e.g. Google) is the fallback. `entity_id: comfyui` picks it.
- The AI is no longer told about the fan itself, so it does not paint a fan disc or a ring
  around the picture.
- A background behind text is now a whole picture of its prompt (a washing machine for "the
  laundry is done"), bright on a darker background, instead of a dark middle with shapes around
  the edge; so every notification can get its own fitting picture.

## 2.1.20

p56-server 0.3.20, with integration 2.22.0.

- **Removed: pages.** The pages that took turns on the fan (sub-entries of the server, the
  switch *Pages* and the sensor *Page*) are gone; use automations with `p56_pov.show_text` or
  `p56_pov.show_ai` instead. Pages you still have are removed, with that switch and sensor.
- The picture entity (`image.<fan>_shown`) keeps the last picture, also after a restart, and
  shows it faded once it is no longer on the fan. Attribute `live`.
- Switching the fan off is checked: right after a cast a fan may ignore it, so it is sent again.

## 2.1.19

p56-server 0.3.19.

- A fan that was off and showed a message as a cast (`mode: cast`, e.g. `show_ai`) went black
  afterwards but stayed on. It now goes off again, like after a stored message.

## 2.1.18

p56-server 0.3.18.

- A notification (`notify`) shows a bell above its text.
- Over an AI background (`p56_pov.show_ai` with `text`) only the text is shown, no icon.

## 2.1.17

p56-server 0.3.17.

- **A fan that was off goes off again after a message** (doorbell, alarm, price…): it switches
  on for the message and off again afterwards. A fan that was on stays on.
- A message from an automation is stored also while something is cast live, and the panel's
  power button works while pages are shown (pages leave a switched-off fan alone).

## 2.1.16

p56-server 0.3.16.

- **Messages and texts no longer flash by**: they were stored as a 0.1 s animation, so with
  several files the fan raced through the playlist and a message could not stay on. A text or
  picture is now a still that the fan shows 5 s, and a message really stays until its time is up.

## 2.1.15

p56-server 0.3.15.

- **Messages stay on for real**: a message (e.g. the electricity price) could flash by while a
  picture from the playlist stayed on. The fan now really switches to the message before it
  repeats only that one.

- **A fan that joins the network later is picked up** without restarting the add-on (it could
  stay unconnected when the add-on started while the fan was away). The server looks up the
  address it advertises again while running, also calls fans it has seen before directly, and
  restarts its discovery by itself if that ever stops.
- The log shows when a fan answers the discovery (`… answered the discovery`) and when the
  advertised address changes, which helps when a fan does not connect.

## 2.1.14

p56-server 0.3.14.

### Added
- **Pages that take turns**: add pages to the P56 server on the integration page (text with
  templates, icon, style, colour by value, meter ring, a condition and a duration) and turn on
  the switch **Pages**. They take turns on the fans, live and without storing anything;
  messages always go first. The sensor **Page** shows which page is on.

### Removed
- **Bluetooth**: the P56 POV panel no longer has a Bluetooth section on the Network tab, and the
  server no longer accepts the Bluetooth commands. On the P56, Wi-Fi and Bluetooth share one
  radio: with a Bluetooth receiver connected, the fan could no longer rejoin the home Wi-Fi and
  kept showing its own hotspot. Do not pair a Bluetooth speaker to the fan. The volume still works.
- **Colour mode**: the `select.<fan>_colour_mode` entity is gone, and the P56 POV panel no longer
  shows the colour readout or the colour choice. On a real P56, switching between true colour
  and high brightness made no visible difference, and the fan never reports which mode it is in.
  Remove the entity from dashboards or automations that still use it.

## 2.1.13

p56-server 0.3.13.

- **Important messages first**: `priority` for `show_text`, `show_image` and `show_ai` with a
  duration. A less important message waits, a more important one interrupts, and the
  interrupted message comes back afterwards.
- **Meter ring**: `gauge: true` with `value`, `value_min`, `value_max` fills the ring like a
  meter (battery, solar power, price).
- **Messages stay on**: a message with `show_seconds` (energy price, notifications) no longer
  makes way for the next file of the playlist; the fan repeats only the message until its time
  is up, then returns to what it played, with the repeat mode from before.
- After two messages in a row the fan returns to what it played before.

## 2.1.12

p56-server 0.3.12.

- **AI pictures in any style**: photo, 3D render, cartoon, anime, painting, watercolour, pixel art,
  neon, line drawing, comic, or your own words (`art_style` in `p56_pov.show_ai`, **Style** in the
  panel). The pictures are no longer pushed towards a bright neon look.
- Backgrounds behind text (notifications, prices) keep the simple look: a few bold shapes, not
  busy.
- Drawn animations are no longer offered (the AI could not draw them consistently); Turning stays.

## 2.1.11

p56-server 0.3.11.

- **Text over an AI background**: `p56_pov.show_ai` with `text` (for example the electricity
  price) has the AI draw a background and writes the text over it, sharp and readable, with the
  colour-by-value fields of `show_text`. The same prompt reuses the background made before, so
  frequent updates do not cost a new picture each time.

## 2.1.10

p56-server 0.3.10.

- AI pictures: **Turning** turns the object round its own axis (as on a turntable) instead of
  turning the whole picture; animations no longer get a white edge or jump about, and the AI is
  asked for frames that really move.
- The panel's preview plays animations.

## 2.1.9

p56-server 0.3.9.

- **AI pictures**: describe a picture in the panel's Media tab (or use the new action
  `p56_pov.show_ai`) and the AI you set up in Home Assistant draws a still, a turning picture or
  a short animation for the fan. Needs an AI Task that makes pictures, e.g. Google Generative AI.

## 2.1.8

p56-server 0.3.8.

- Live cast from the panel runs at 25 frames per second by default (smoother on a P56).

## 2.1.7

p56-server 0.3.7.

- Panel: when the fan already has a file with that name, it asks whether to overwrite it.
- Panel: a long file name in "Delete …?" wraps inside the dialog on a phone.

## 2.1.6

p56-server 0.3.6.

- **Files with long names broke the fan's memory card**, the real cause of the failed gallery
  videos (their file names are long). A real P56 stores names of at most 24 characters; longer
  names are now cut to 24. If your fan answers every upload with "not confirmed", format its card
  once (panel → Fan → Format).
- New screenshot of the panel in the README.

## 2.1.5

p56-server 0.3.5.

- Panel, Media tab: **Upload & show** makes the frames of a picked video itself; "Make frames"
  is only needed to choose frames/s, a start or an end first.

## 2.1.4

p56-server 0.3.4.

- **Large uploads through Home Assistant are stored safely.** 2.1.3 could still lose a long
  video and leave the fan's memory card refusing uploads: on Home Assistant's Linux the server
  thought the file was stored too early. It now waits until the fan lists the new file. If your
  fan answers every upload with "transfer failed", format its card once (panel → Fan → Format).

## 2.1.3

p56-server 0.3.3.

- **Long videos and big pictures arrive safely.** Uploads of large files could get lost and leave
  the fan's memory card refusing every upload until it was formatted. The server now waits until
  the fan has stored the file, as the official app does. Tested with files up to 27 MB.

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
