# P56 POV Display add-on

Runs **p56-server**: it talks to Holoscope / P56 POV fans on your network the way
the vendor app does, and gives Home Assistant (and anything else) a simple HTTP API.
Everything runs inside Home Assistant; no phone, PC or cloud is needed.

- **Panel** "P56 POV" in the sidebar: the full P56 Studio web app (playlist, upload of
  pictures/GIF/video, live cast, brightness, angle, network settings …).
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
| `lan_api` | `true`: port 8765 answers the whole LAN (apps/scripts elsewhere). `false`: only Home Assistant itself and the ingress panel. |
| `simulate` | Start a virtual P56 fan, to try everything without hardware. Takes effect right after saving; no restart needed. |
| `log_level` | `trace`, `debug`, `info`, `warn`, `error`. |

The port 8765 API has no login. Turn `lan_api` off if other devices on your
network should not be able to use it.

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
  color: [255, 200, 0]       # or "#ffc800", "geel", "255,200,0"
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

`p56_pov.play_file` and `p56_pov.delete_file` take a device and a `slot` or `name`.
`show_text` and `show_image` return the result (slot, file name) as a response.

## HTTP API (port 8765)

| Method + path | Body / answer |
| --- | --- |
| `GET /api/state` | `{connected, fan_ip, playlist:[{slot,name}], devices:[…], last_activity, version}` |
| `POST /api/show` | `{text \| data (base64) \| path, color, background, font_size, show_seconds, mode, name, mac \| id, placement:{x,y,scale,mirror}}` |
| `POST /api/call/<cmd>` | every command of the web app, e.g. `control {id, action:"on"\|"off"\|"play"\|"pause"\|…}`, `set_value {id, what:"brightness"\|"angle"\|"volume"\|"select"\|"delete", value}` → `{ok, result \| error}` |
| `GET /api/events` | live events (server-sent events) |
| `GET /api/shown/<mac>` | PNG of what was last sent to that fan |
| `GET /health`, `GET /status`, `POST /text`, `POST /upload`, `POST /delete`, `POST /on`, `POST /off` | the 1.x API, unchanged |

Example `rest_command` (works from Home Assistant itself, also with `lan_api: false`):

```yaml
rest_command:
  p56_text:
    url: http://127.0.0.1:8765/api/show
    method: POST
    content_type: application/json
    payload: '{"text": "{{ text }}", "show_seconds": {{ seconds | default(0) }}}'
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
