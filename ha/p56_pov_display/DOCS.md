# P56 POV Display add-on

Runs **p56-server**: it talks to Holoscope / P56 POV fans on your network the way
the vendor app does, and gives Home Assistant (and anything else) a simple HTTP API.
Everything runs inside Home Assistant; no phone, PC or cloud is needed.

- **Panel** "P56 POV" in the sidebar: the full P56 Studio web app (playlist, upload of
  pictures/GIF/video, live cast (also of your Home Assistant cameras), brightness, angle,
  network settings …).
- **Integration** `p56_pov` is offered automatically (Settings → Devices & services):
  light, switches, selects, sensors, a text field, a notify entity, an image of what is
  shown, and the actions `p56_pov.show_text` / `p56_pov.show_image`.
- **HTTP API** on port 8765 for scripts and `rest_command` (the 1.x endpoints still work).

## How the fans connect

The fan firmware works backwards: the server broadcasts a discovery packet (UDP 8988),
and the fan then opens a TCP connection to the server on **port 6666**. That is why
the add-on uses the host network. The fan has to be on the same network as Home
Assistant (its own hotspot mode is for the vendor app, use the router mode). Put the
fan on your Wi-Fi once with the vendor app or with P56 Studio (Network tab).

## Options

| Option | Meaning |
| --- | --- |
| `fan_ip` | Extra fan IPs to send the discovery packet to directly (other subnet, broadcast blocked). Usually empty. |
| `advertise_ip` | IP the fans should connect back to. Empty: every network card announces itself. |
| `lan_api` | `false` (default): only Home Assistant itself and the ingress panel can use port 8765. `true`: the whole LAN (apps/scripts elsewhere); set `api_token` as well. |
| `api_token` | Optional. When set, clients on the network must send this token (see below). Home Assistant itself and the panel never need it. |
| `simulate` | Start a virtual P56 fan, to try everything without hardware. Takes effect right after saving; no restart needed. |
| `log_level` | `trace`, `debug`, `info`, `warn`, `error`. |

### Security

The API on port 8765 can do everything the panel can, including formatting the fan's
memory card, a factory reset and changing its Wi-Fi settings. Therefore:

- With `lan_api: false` (the default) only Home Assistant itself (the integration,
  `rest_command`, the panel) reaches it; other devices get `403 Forbidden`.
- With `lan_api: true`, set an `api_token`. Requests from other devices then need the
  token, or they get `401 Unauthorized`. Send it as a header, or (for browsers and
  `EventSource`, which cannot set headers) as a query parameter:

  ```text
  Authorization: Bearer <your token>
  http://<home-assistant>:8765/api/state?token=<your token>
  ```

  The page `/` and `/api/ping` answer without a token. Requests from Home Assistant
  itself and from the add-on network (172.30.32.0/23, the panel) never need it.
- Without a token, `lan_api: true` lets anyone on your network control the fans; the
  add-on log warns about this.
- `path` in `/api/show` (and `image` in the 1.x `/upload`) can only read pictures from
  `/media` and `/share` (after resolving `..` and symbolic links); anything else is
  refused with `403`. Send other pictures as `data` (base64) or `frames`.

## Integration

After the add-on starts, Home Assistant shows "P56 POV Display discovered".
Add it, and every fan that connects becomes a device:

| Entity | What |
| --- | --- |
| `light.<fan>` | Picture on/off and brightness (16 steps) |
| `switch.<fan>_playing` | Play / pause |
| `select.<fan>_file` | What plays (the playlist) |
| `select.<fan>_repeat`, `select.<fan>_colour_mode` | Repeat all/one, true colour/high brightness |
| `number.<fan>_picture_angle` | Rotate the picture (−180 … 180°) |
| `text.<fan>_text` | Type a text: the fan shows it (replaces its previous text) |
| `notify.<fan>_message` | Notifications, shown for a while (option, default 15 s), then the playlist resumes |
| `image.<fan>_shown` | Round preview of what Home Assistant last sent |
| `sensor.<fan>_playing_file`, `sensor.<fan>_files` | Current file; number of files (list in the attributes) |
| `sensor.p56_server_connected_fans` | 0 = no fan connected |

Options (gear icon): default mode (store on the fan or live), duration for the text
entity and for notifications, text and background colour.

### Actions

```yaml
action: p56_pov.show_text
data:
  text: "{{ states('sensor.energy_price') }} €/kWh"
  color: [255, 200, 0]       # or "#ffc800", "yellow", "255,200,0"
  background: [0, 0, 0]
  show_seconds: 60           # remove again after 60 s; 0 = keep
  name: energy               # same name replaces the earlier file
  mode: upload               # or cast: show live, store nothing
# device_id: …               # optional: only these fans
```

```yaml
action: p56_pov.show_image
data:
  camera: camera.front_door  # or path: /media/x.png, url: https://…, media: (media library)
  show_seconds: 20
```

#### Styles, icons and colours that follow a value

```yaml
action: p56_pov.show_text
data:
  text: "{{ states('sensor.energy_price') }} €/kWh"
  style: price                         # alert, warning, info, success, price, clock
  value: "{{ states('sensor.energy_price') }}"
  value_min: 0.20                      # green at or below
  value_max: 0.35                      # red at or above (yellow halfway)
```

```yaml
action: p56_pov.show_text
data:
  text: Washing machine done
  style: success
  icon: mdi:washing-machine            # any Material Design Icon, as in Home Assistant
  show_seconds: 300
```

| Style | Look |
| --- | --- |
| `alert` | red ring, warning icon, dark red background |
| `warning` | orange ring, outlined warning icon, yellow text |
| `info` | blue ring, info icon |
| `success` | green ring, check icon |
| `price` | lightning icon; ring and text take the colour of `value` |
| `clock` | ring with clock ticks; without `text` it shows the current time |

Everything a style sets can be overridden: `color`, `background`, `icon`, `icon_color`,
`ring` (on/off). An icon alone (no text) is drawn large.

#### Animations: GIF and video

`show_image` keeps every frame of an animated GIF, and turns a video (MP4, WebM, MOV, AVI)
into frames with the ffmpeg that comes with Home Assistant (`fps`, default 10; `max_frames`,
default 100). The fan plays it in a loop; with `mode: cast` it plays live for `show_seconds`.

#### AI pictures

`p56_pov.show_ai` has an AI Task of Home Assistant draw a picture from `prompt` (the add-on
adds a black background and asks for one bold subject in the middle). `kind: still` (default)
shows it as it is, `spin` turns the object round its own upright axis (as on a turntable, one
turn in 2.4 s), and `anim` asks the AI for a 4×4 sheet of 16 frames in which the subject moves
on the spot, and plays them as an animation. White margins the AI sometimes adds become black,
and every frame is centred so the subject stays put. `entity_id` picks the AI Task; by default the
first one that can make pictures is used. The panel's Media tab has the same as an
**AI picture** box. It needs `homeassistant_api` (on by default) and Home Assistant on port 8123
with plain http on the same machine, where the add-on fetches the finished picture.

`p56_pov.play_file` and `p56_pov.delete_file` take a device and a `slot` or `name`.
`show_text` and `show_image` return the result (slot, file name) as a response.

## HTTP API (port 8765)

| Method + path | Body / answer |
| --- | --- |
| `GET /api/state` | `{connected, fan_ip, playlist:[{slot,name}], devices:[…], last_activity, version}` |
| `POST /api/show` | `{text \| icon \| data (base64; GIF keeps all frames) \| path (only under /media or /share) \| frames:[base64,…], style, icon, icon_color, color_scale:{value,min,max,colors?,invert?}, ring, color, background, font_size, show_seconds, mode, name, max_frames, frame_ms, mac \| id, placement:{x,y,scale,mirror}}` |
| `POST /api/call/<cmd>` | every command of the web app, e.g. `control {id, action:"on"\|"off"\|"play"\|"pause"\|…}`, `set_value {id, what:"brightness"\|"angle"\|"volume"\|"select"\|"delete", value}` → `{ok, result \| error}` |
| `GET /api/events` | live events (server-sent events) |
| `GET /api/shown/<mac>` | PNG of what was last sent to that fan |
| `GET /health`, `GET /status`, `POST /text`, `POST /upload`, `POST /delete`, `POST /on`, `POST /off` | the 1.x API, unchanged |

`mode: cast` with `mac` or `id` streams to that fan only; without them, to every fan.
With `show_seconds`, the file is removed again after that time and the fan returns to the
file that played before; showing the same `name` again cancels the earlier removal.

Example `rest_command` (works from Home Assistant itself, also with `lan_api: false`,
and needs no token):

```yaml
rest_command:
  p56_text:
    url: http://127.0.0.1:8765/api/show
    method: POST
    content_type: application/json
    payload: '{"text": "{{ text }}", "show_seconds": {{ seconds | default(0) }}}'
```

From another machine (with `lan_api: true` and an `api_token`):

```sh
curl -H "Authorization: Bearer $P56_TOKEN" -H "Content-Type: application/json" \
  -d '{"text": "Hello"}' http://homeassistant.local:8765/api/show
```

## Upgrading from 1.x (the local add-on)

1. Stop and uninstall the old local add-on "P56 POV Display" (`local_p56_pov_display`).
2. Install this add-on from the repository and start it.
3. Remove the old `p56_pov` integration entry, update the integration (HACS) and add
   the discovered one. Old `rest_command`s that post to `/text` keep working.

## Troubleshooting

- *No fan appears*: check the log. The fan must be in router mode on the same network.
  Fill in `fan_ip` with the fan's IP if broadcast does not reach it.
- *Port 6666 in use*: another program (the old add-on, a P56 Studio on the same
  host) is already listening. Only one server can serve a fan.
- Try `simulate: true`: a virtual fan connects and everything can be tested.
