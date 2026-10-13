# P56 POV Display add-on

Runs **p56-server**: it talks to Holoscope / P56 POV fans on your network the way
the vendor app does, and gives Home Assistant (and anything else) a simple HTTP API.
Everything runs inside Home Assistant; no phone, PC or cloud is needed.

- **Panel** "P56 POV" in the sidebar: the full P56 Studio web app (playlist, upload of
  pictures/GIF/video, live cast (also of your Home Assistant cameras), brightness, angle,
  network settings …).
- **Integration** `p56_pov` is offered automatically (Settings → Devices & services):
  light, switches, selects, sensors, a text field, a notify entity, an image of what is
  shown, and the actions `p56_pov.show_text`, `p56_pov.show_image`, `p56_pov.show_ai` (AI
  pictures), `p56_pov.play_file` and `p56_pov.delete_file`.
- **HTTP API** on port 8765 for scripts and `rest_command` (the 1.x endpoints still work).

## How the fans connect

The fan firmware works backwards: the server broadcasts a discovery packet (UDP 8988),
and the fan then opens a TCP connection to the server on **port 6666**. That is why
the add-on uses the host network. The fan has to be on the same network as Home
Assistant (its own hotspot mode is for the vendor app, use the router mode). Put the
fan on your Wi-Fi once with the vendor app or with P56 Studio (Network tab). The fan has a
2.4 GHz radio only, so it needs a 2.4 GHz network (a combined 2.4/5 GHz network is fine).

**Tip: keep one black picture on the fan.** With an empty playlist a P56 blinks four blue
LEDs. Upload a black picture (for example named `black`) and leave it there: the fan then shows
nothing, and messages return to it when they are over.

## Options

| Option | Meaning |
| --- | --- |
| `fan_ip` | Extra fan IPs to send the discovery packet to directly (other subnet, broadcast blocked). Usually empty. |
| `advertise_ip` | IP the fans should connect back to. Empty: every network card announces itself. |
| `lan_api` | `false` (default): only Home Assistant itself and the ingress panel can use port 8765. `true`: the whole LAN (apps/scripts elsewhere); set `api_token` as well. |
| `api_token` | Optional. When set, clients on the network must send this token (see below). Home Assistant itself and the panel never need it. |
| `simulate` | Start a virtual P56 fan, to try everything without hardware. Takes effect right after saving; no restart needed. |
| `comfyui_url` | Optional. Address of [ComfyUI](https://www.comfy.org) on a computer in your house, e.g. `http://192.168.1.30:8188` (plain http): AI pictures are then made there, see [AI pictures](#ai-pictures). Restart the add-on after changing it. |
| `comfyui_size` | Optional. Size of ComfyUI's pictures in pixels, 256–1536 (rounded to a multiple of 64), default 768. A P56 shows about 344 pixels across, so more only costs time. |
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
| `switch.<fan>_breathing_light`, `switch.<fan>_start_on_power`, `switch.<fan>_master` | The fan's status light (all LEDs light up in one colour now and then, to show that the fan is on), starting by itself when it gets power, and group master |
| `select.<fan>_file` | What plays (the playlist) |
| `select.<fan>_repeat` | Repeat all files or one |
| `number.<fan>_picture_angle` | Rotate the picture (−180 … 180°) |
| `number.<fan>_volume` | Volume of the fan's speaker (sound of videos) |
| `button.<fan>_refresh`, `button.<fan>_delete_playing_file` | Ask the fan for its state; delete the file that plays |
| `text.<fan>_text` | Type a text: the fan shows it (replaces its previous text) |
| `notify.<fan>_message` | Notifications with a bell above the text (a `title` becomes the first line), shown for a while (option, default 15 s), then the playlist resumes |
| `image.<fan>_shown` | Round preview of what Home Assistant last sent; faded (grey, see-through) once it is no longer on the fan (another file plays, the cast is over, the fan is off). Attribute `live` |
| `sensor.<fan>_playing_file`, `sensor.<fan>_files` | Current file; number of files (list in the attributes) |
| `sensor.<fan>_ip_address`, `sensor.<fan>_last_error` | The fan's IP; the last error it reported |
| `sensor.p56_server_connected_fans`, `sensor.p56_server_last_activity` | 0 = no fan connected; when a fan last sent something |

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

`gauge: true` (with `value`, `value_min`, `value_max`) turns the ring into a meter: filled
clockwise from the top as far as the value is between min and max, the rest a dim track.

#### Priority of messages

A message with `show_seconds` can have a `priority` (default 0, from −100 to 100). While a
message shows on a fan, a less important one for that fan waits (at most 10 minutes) and comes
afterwards, the most important and then the oldest first; an equally important one replaces it;
a more important one interrupts it, and the interrupted message comes back afterwards for the
time it had left. At the end the fan returns to the file it played before the first message.
`show_text`, `show_image` and `show_ai` all take `priority`; a waiting message answers with
`queued: true`.

While a message shows, the fan repeats only that one. A fan that was off is switched on for the
message and off again afterwards (also with `mode: cast`), so a fan that is normally off only
lights up for messages. It first spins up for 2.5 seconds on black, so the message is steady at
once: on its black picture (recognised by its preview, or by the name `black`, `zwart`, `blank`
or `leeg`), otherwise on black sent live.

#### Animations: GIF and video

`show_image` keeps every frame of an animated GIF, and turns a video (MP4, WebM, MOV, AVI)
into frames with the ffmpeg that comes with Home Assistant (`fps`, default 10; `max_frames`,
default 100). The fan plays it in a loop; with `mode: cast` it plays live for `show_seconds`.

With `text` (and optionally `color`, `font_size`) the text is written over the picture, over
every frame of an animation, on a darkened band so it stays readable:

```yaml
action: p56_pov.show_image
data:
  camera: camera.buienradar          # the animated rain radar of the Buienradar integration
  text: "Rain · {{ state_attr('weather.home', 'temperature') | round(0) | int }}°"
  mode: cast
  show_seconds: 30
```

#### AI pictures

`p56_pov.show_ai` has an AI draw a picture from `prompt`: ComfyUI on a computer in your house
(option `comfyui_url`, see below) or an AI Task of Home Assistant such as Google Generative AI.
The add-on adds only what the fan needs: a black background and the subject in the middle. `kind: still` (default)
shows it as it is, and `spin` turns the object round its own upright axis (as on a turntable,
one turn in 2.4 s). `kind: 3d` makes a **real 3D model** of the object with ComfyUI (Trellis 2)
and shows it turning, seen from all sides (60 views, one turn in 3.6 s). It takes about 2
minutes on a laptop graphics card; the add-on keeps the model (in its own `/data/ai`), so the same
prompt again is there at once, also after a restart (`fresh: true` makes a new one). Only with
ComfyUI; it needs the files of ComfyUI's template *Pixal3D & TRELLIS.2: Image to Model*
(`trellis_2…` in `diffusion_models`, the Trellis 2 shape and texture VAEs, `dino…` in
`clip_vision` and `birefnet` in `background_removal`). `art_style` sets how it is drawn: `photo`, `3d`, `cartoon`, `anime`,
`painting`, `watercolor`, `pixel`, `neon`, `line`, `comic`, or any words of your own; without it
the AI follows the prompt. White margins the AI sometimes adds become black. `entity_id` picks the AI Task, or
`comfyui` for ComfyUI (below); by default ComfyUI when it is set up, otherwise the first AI Task
that can make pictures; when ComfyUI cannot be reached, the AI Task takes over. The panel's
Media tab has the same as an **AI picture** box. The AI Task route needs Home Assistant on port
8123 with plain http on the same machine, where the add-on fetches the finished picture (with
SSL set up directly in Home Assistant this fails; ComfyUI does not need it).

With `text`, the AI makes a **background** instead: a whole, simple picture of the prompt
(bright things on a darker background, little detail, no writing), and the add-on writes the
text over it, sharp, on a darkened band, with `color`, `font_size`, `value` / `value_min` /
`value_max`, `invert_colors` and `gauge` as in `show_text`. Over an AI background only the text
is shown; an `icon` is left out (with an icon and no text, the icon is drawn over it). `kind`
does not matter for a background. The same prompt again reuses the picture made before (the last 8 are kept
while the add-on runs), so an automation that updates a price every quarter of an hour pays
for a background only when its prompt changes; `fresh: true` always makes a new one.

**Local AI with ComfyUI:** with the option `comfyui_url` (e.g. `http://192.168.1.30:8188`) the
add-on makes the pictures with [ComfyUI](https://www.comfy.org) on a computer in your house, so
nothing leaves your network; a picture takes 5–10 seconds on a recent graphics card. Set up:

1. Install ComfyUI and its **Qwen Image 2.1: Text to Image** template (Templates in ComfyUI).
   It puts three files in place, which the add-on finds by name:
   `models/diffusion_models/qwen_image_2.1…` (the *turbo* model is preferred: 8 steps),
   `models/text_encoders/qwen3vl…` and `models/vae/qwen_image_2.1…`. The prompt enhancer of
   the template is not needed.
2. Let ComfyUI listen on the network: start it with `--listen` (for example
   `python main.py --listen 0.0.0.0 --port 8188`; in ComfyUI Desktop set the listen address to
   `0.0.0.0` in the server settings) and allow that port in the computer's firewall. Check that
   `http://<computer>:8188` opens from another device.
3. Fill in `comfyui_url` (plain `http://`, no https) and restart the add-on. Its log then says
   `AI pictures from ComfyUI at …`.

`comfyui_size` sets the size (default 768). The picture is a preview in ComfyUI, not a saved
file. The first picture after ComfyUI starts takes longer (the model loads); a picture may take
at most 2 minutes. When ComfyUI cannot be reached, the AI Task of Home Assistant is used instead,
unless `entity_id: comfyui` asked for ComfyUI only (then nothing goes out of the house: use that
when the prompt contains private text, such as a notification).

```yaml
action: p56_pov.show_ai
data:
  prompt: >
    {{ 'sunny green energy' if states('sensor.electricity_price') | float < 0.25 else 'red warning, expensive power' }}
  text: "€ {{ states('sensor.electricity_price') }}"
  value: "{{ states('sensor.electricity_price') }}"
  value_min: 0.1
  value_max: 0.4
  name: energy
  show_seconds: 60
  mode: cast
```

A notification with its own picture, made only by ComfyUI (the text stays in the house):

```yaml
action: p56_pov.show_ai
data:
  entity_id: comfyui
  prompt: "{{ trigger.notification.title }} {{ trigger.notification.message }}"
  text: "{{ trigger.notification.message }}"
  show_seconds: 15
  priority: 5
  mode: cast
```

`p56_pov.play_file` and `p56_pov.delete_file` take a device and a `slot` or `name`.
`show_text`, `show_image` and `show_ai` return the result (slot, file name) as a response.

## HTTP API (port 8765)

| Method + path | Body / answer |
| --- | --- |
| `GET /api/state` | `{connected, fan_ip, playlist:[{slot,name}], devices:[…], last_activity, version}` |
| `POST /api/show` | `{text \| icon \| data (base64; GIF keeps all frames) \| path (only under /media or /share) \| frames:[base64,…], style, icon, icon_color, color_scale:{value,min,max,colors?,invert?}, gauge, ring, color, background, backdrop (base64 picture behind the text), font_size, motion ("scroll" \| "orbit"), speed, reverse, show_seconds, priority, mode, name, max_frames, frame_ms, mac \| id, placement:{x,y,scale,mirror}}` |
| `POST /api/call/show_ai` | `{prompt, kind ("still" \| "spin"), art_style, entity_id ("comfyui" \| an AI Task), fresh, …}` plus the fields of `/api/show`: an AI picture on the fan. `ai_status` lists the AIs that can make pictures, `ai_image` makes one for the Media tab |
| `POST /api/call/<cmd>` | every command of the web app, e.g. `control {id, action:"on"\|"off"\|"play"\|"pause"\|…}`, `set_value {id, what:"brightness"\|"angle"\|"volume"\|"select"\|"delete", value}` → `{ok, result \| error}` |
| `GET /api/events` | live events (server-sent events) |
| `GET /api/shown/<mac>` | PNG of what was last sent to that fan, faded when it is no longer on it (header `X-P56-Live: 1` or `0`; `/api/state` has `shown: {at, live}` per fan) |
| `GET /health`, `GET /status`, `POST /text`, `POST /upload`, `POST /delete`, `POST /on`, `POST /off` | the 1.x API, unchanged |

`mode: cast` streams live for `show_seconds` (default 10 s) and stores nothing; with `mac` or
`id` to that fan only, without them to every fan.
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
- *Four blue LEDs blink on the fan*: its playlist is empty; keep one black picture on it.
- *AI: "ComfyUI not reachable"*: ComfyUI is off, does not listen on the network (`--listen`) or
  the firewall blocks its port. *"has no Qwen Image 2.1 file in models/…"*: install the
  template's files (see above). *"took too long"*: the computer is busy or the size is too big.
- *AI: "no AI in Home Assistant can make pictures"*: set up an AI Task (e.g. Google Generative
  AI) or `comfyui_url`. *"the AI made a picture, but it could not be fetched from Home
  Assistant"*: Home Assistant must answer on port 8123 with plain http on the same machine.
