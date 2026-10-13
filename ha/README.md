<div align="center">

<img src="custom_components/p56_pov/brand/icon.png" width="96" alt="">

# P56 POV Display for Home Assistant

**Put text, prices, notifications, camera snapshots and pictures on your Holoscope / P56
hologram fan, straight from Home Assistant. Local, no vendor app, no cloud.**

[![Validate](https://github.com/trackerr/home-assistant-p56/actions/workflows/validate.yml/badge.svg)](https://github.com/trackerr/home-assistant-p56/actions/workflows/validate.yml)
[![HACS Custom](https://img.shields.io/badge/HACS-Custom-41BDF5.svg)](https://hacs.xyz/docs/faq/custom_repositories)
![Home Assistant 2026.8+](https://img.shields.io/badge/Home%20Assistant-2026.8%2B-18BCF2?logo=homeassistant&logoColor=white)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

<img src="docs/fan-preview.png" width="220" alt="Energy price as shown on the fan">

</div>

---

## ✨ What you get

| | |
| --- | --- |
| 💡 **Control** | On/off and brightness as a light, play/pause, playlist, repeat, picture angle |
| 📝 **Text** | A text field on your dashboard, or the `p56_pov.show_text` action, sized automatically to fit the round display |
| 🔔 **Notifications** | `notify.send_message` shows a message with a bell for 15 seconds, then the playlist resumes; a fan that was off goes off again |
| 🎨 **Styles & icons** | Ready-made looks (alert, warning, info, success, price, clock) with a ring along the rim, any Home Assistant icon, and a text colour that follows a value (green → red) |
| 🖼️ **Pictures & animations** | `p56_pov.show_image` from a file, a URL, the media library or a camera snapshot; animated GIFs and videos play as animations |
| ✨ **AI pictures** | `p56_pov.show_ai` or the panel's Media tab: describe a picture and an AI draws it in any style (photo, painting, cartoon, 3D, neon…), still or turning round its own axis. Either free and private with ComfyUI on a PC in your house, or with the AI you set up in Home Assistant (AI Task, e.g. Google Gemini). With a text, the AI draws a fitting background for a notification or a price |
| 👀 **Preview** | An image entity that shows what was last sent to the fan, faded once it is no longer on it |
| 🖥️ **Web app** | The full P56 Studio app as a sidebar panel: upload photos, GIFs and video, cast live, change Wi-Fi |
| 🔌 **Self-contained** | Everything runs inside Home Assistant; the fans connect to it on your own network |

<p align="center"><img src="docs/panel.jpg" width="820" alt="The P56 Studio panel in Home Assistant"></p>

---

## 🚀 Installation

> 📖 After installing: the **[user guide](docs/GUIDE.md)** explains the panel, dashboards,
> automations, your cameras, and using your phone through Home Assistant.

You need **two parts**: the **add-on** (talks to the fan) and the **integration** (gives you the
entities and actions). Both are in this repository.

### Step 1 — Add-on

[![Add the add-on repository to your Home Assistant](https://my.home-assistant.io/badges/supervisor_add_addon_repository.svg)](https://my.home-assistant.io/redirect/supervisor_add_addon_repository/?repository_url=https%3A%2F%2Fgithub.com%2Ftrackerr%2Fhome-assistant-p56)

1. Click the button above (or go to **Settings → Add-ons → Add-on store → ⋮ → Repositories**
   and add `https://github.com/trackerr/home-assistant-p56`).
2. Install **P56 POV Display** and press **Start**.
3. Optional: turn on **Show in sidebar** to get the **P56 POV** panel.

> 💡 No fan at hand? Turn on the **Virtual fan** option: a simulated fan connects and you can
> try everything.

### Step 2 — Integration

[![Open this repository in HACS](https://my.home-assistant.io/badges/hacs_repository.svg)](https://my.home-assistant.io/redirect/hacs_repository/?owner=trackerr&repository=home-assistant-p56&category=integration)

1. Click the button above (or in **HACS → ⋮ → Custom repositories** add this repository with type
   **Integration**).
2. Download **P56 POV Display** and **restart Home Assistant**.

<details>
<summary>Without HACS</summary>

Copy the folder `custom_components/p56_pov` to `/config/custom_components/` and restart Home Assistant.
</details>

### Step 3 — Put the fan on your Wi-Fi (once)

A new fan starts as its own Wi-Fi hotspot, which Home Assistant can't reach. Connect a phone or
PC to that hotspot once, open [P56 Studio](https://github.com/trackerr/p56-studio/releases)
(or the vendor app), go to **Network → Connect to your router**, enter your Wi-Fi name and
password and press **Set router**. From then on the fan joins your Wi-Fi and finds Home Assistant
by itself.

- The fan has a **2.4 GHz** radio only: use a 2.4 GHz network (a combined 2.4/5 GHz one is fine).
- Name and password can be at most 31 characters.
- Keep Bluetooth speakers away from the fan's Bluetooth: Wi-Fi and Bluetooth share one radio,
  and a connected Bluetooth receiver can stop the fan from joining your Wi-Fi.

### Step 4 — Connect

Go to **Settings → Devices & services**. Home Assistant shows **"P56 POV Display discovered"**:
click **Add**. Every fan that connects appears as a device (give it a minute after step 3).

### Step 5 — Keep one black picture on the fan

With an empty playlist a P56 blinks four blue LEDs. Upload one black picture (for example named
`black`, in the panel's **Media** tab) and leave it there: the fan then shows nothing, and
messages return to it when they are over.

### Optional — AI pictures

For `p56_pov.show_ai` and the panel's **AI picture** box you need an AI that can draw:
**ComfyUI** with the free Qwen Image 2.1 model on a PC with a graphics card in your house
(add-on option **ComfyUI address**), or an AI in Home Assistant such as **Google Generative AI**.
The [user guide](docs/GUIDE.md#7-pictures-made-by-ai) walks you through both.

> 🔄 **Updating:** update the add-on and the integration together (both are in HACS / the
> add-on store), then restart Home Assistant.

---

## 🧩 Entities

For each fan (example names for a fan called *P56*):

| Entity | What it does |
| --- | --- |
| `light.p56` | Picture on/off and brightness |
| `switch.p56_playing` | Play / pause |
| `switch.p56_breathing_light`, `switch.p56_start_on_power`, `switch.p56_master` | Breathing light, start by itself on power, group master |
| `select.p56_file` | Choose what plays |
| `select.p56_repeat` | Repeat all files or one |
| `text.p56_text` | Type something, the fan shows it |
| `notify.p56_message` | Notifications with a bell, shown for a while |
| `image.p56_shown` | What Home Assistant last sent to the fan; faded once it is no longer on it (attribute `live`) |
| `sensor.p56_playing_file`, `sensor.p56_files` | Current file, number of files |
| `sensor.p56_ip_address`, `sensor.p56_last_error` | The fan's IP, the last error it reported |
| `number.p56_picture_angle` | Turn the picture (−180 … 180°) |
| `number.p56_volume` | Volume of the fan's speaker (sound of videos) |
| `button.p56_refresh`, `button.p56_delete_playing_file` | Ask the fan for its state; delete the file that plays |

Plus `sensor.p56_server_connected_fans` (0 means no fan is connected) and
`sensor.p56_server_last_activity`.

---

## 🛠️ Actions & examples

### Show text

```yaml
action: p56_pov.show_text
data:
  text: "{{ states('sensor.energy_price') }} €/kWh"
  color: "#ffc800"          # or [255, 200, 0], or a name: yellow, red, …
  show_seconds: 60          # remove again after a minute (0 = keep)
  name: energy              # the same name replaces the previous one
```

### Styles, icons and colours that follow a value

<p align="center"><img src="docs/styles.png" width="820" alt="The alert, price, success and clock styles on the fan"></p>

```yaml
action: p56_pov.show_text
data:
  text: "{{ states('sensor.energy_price') }} €/kWh"
  style: price                       # alert · warning · info · success · price · clock
  value: "{{ states('sensor.energy_price') }}"
  value_min: 0.20                    # green
  value_max: 0.35                    # red (yellow in between)
```

```yaml
action: p56_pov.show_text
data:
  text: Washing machine done
  style: success
  icon: mdi:washing-machine          # any icon from Home Assistant's icon picker
```

### Show a picture or an animation

```yaml
action: p56_pov.show_image
data:
  camera: camera.front_door   # or: path: /media/logo.gif  ·  url: https://…/clip.mp4  ·  media: (picker)
  show_seconds: 20
```

### An AI picture behind a text

```yaml
action: p56_pov.show_ai
data:
  prompt: "{{ 'cheap green energy' if states('sensor.energy_price') | float < 0.25 else 'expensive power' }}"
  text: "{{ states('sensor.energy_price') }} €/kWh"
  value: "{{ states('sensor.energy_price') }}"
  value_min: 0.20
  value_max: 0.35
  show_seconds: 60
  mode: cast
```

### Send a notification

```yaml
action: notify.send_message
target:
  entity_id: notify.p56_message
data:
  message: The washing machine is done
```

| Action | Fields |
| --- | --- |
| `p56_pov.show_text` | `text`, `style`, `icon`, `value` + `value_min` + `value_max`, `gauge`, `color`, `background`, `icon_color`, `ring`, `font_size`, `show_seconds`, `priority`, `mode`, `name`, `device_id` |
| `p56_pov.show_image` | one of `path` / `url` / `camera` / `media` (pictures, GIFs, videos), plus `text` (over every frame, with `color`, `font_size`), `show_seconds`, `priority`, `mode`, `name`, `fps`, `max_frames`, `scale`, `x_offset`, `y_offset`, `mirror`, `device_id` |
| `p56_pov.show_ai` | `prompt`, plus `kind` (`still` / `spin`), `art_style` (`photo`, `3d`, `cartoon`, `painting`, … or your own words), `entity_id` (`comfyui` or an AI Task; default ComfyUI when set up, otherwise the first AI Task that makes pictures), `text` (written over a whole AI background, with `color`, `font_size`, `value` + `value_min` + `value_max`, `invert_colors`, `gauge`), `icon` (only without text), `fresh`, `show_seconds`, `priority`, `mode`, `name`, `device_id` |
| `p56_pov.play_file` | `device_id` and `slot` or `name` |
| `p56_pov.delete_file` | `device_id` and `slot` or `name` |

`mode: upload` (default) stores the picture on the fan; `mode: cast` shows it live without
storing anything. Leave out `device_id` to use all fans. With `show_seconds` it is a message: the
fan shows only that, then returns to what it played, and a fan that was off goes off again;
`priority` decides whether a message waits for or interrupts the one showing.

More ready-to-use automations: [examples/automations.yaml](examples/automations.yaml).

---

## ❓ Questions & troubleshooting

<details>
<summary><b>No fan appears</b></summary>

- The fan must be on the **same network** as Home Assistant (router mode, not its own hotspot).
- Check the add-on log: it shows every fan that answers.
- Broadcast blocked or another subnet? Enter the fan's IP under the add-on option **Fan IP addresses**.
- Only one program can serve a fan at a time: close the vendor app and P56 Studio on other devices.
</details>

<details>
<summary><b>Four blue lights blink on the fan</b></summary>

Its playlist is empty. Keep one black picture on the fan (step 5).
</details>

<details>
<summary><b>The fan keeps falling back to its own hotspot</b></summary>

- The network must be 2.4 GHz, and name and password at most 31 characters.
- A Bluetooth receiver connected to the fan can keep it off Wi-Fi (they share one radio):
  disconnect it, then set the router again.
</details>

<details>
<summary><b>I used the old 1.x add-on</b></summary>

Uninstall the old local add-on and remove the old integration entry, then follow the steps above.
Old `rest_command`s that post to `http://localhost:8765/text` keep working.
</details>

<details>
<summary><b>Can other devices use the API?</b></summary>

Yes: port **8765** offers an HTTP API (`/api/show`, `/api/state`, live events) and the web app.
Turn off the add-on option **API reachable on the network** to allow only Home Assistant itself.
See the [add-on documentation](p56_pov_display/DOCS.md) for the full API.
</details>

<details>
<summary><b>Which fans work?</b></summary>

Fans that use the Holoscope app (P56 and relatives). The add-on runs on **amd64** and **aarch64**
(Raspberry Pi 4/5, most mini PCs).
</details>

---

## 🏗️ How it works

```
 fan ──TCP 6666──▶ add-on (p56-server) ◀── HTTP + live events ── integration p56_pov
  ▲   UDP 8988 discovery    │  port 8765: API + web app (sidebar panel)
  └─────────────────────────┘
```

The add-on is built on [**p56-studio**](https://github.com/trackerr/p56-studio), the same Rust core
as the Windows and Android apps that replace the vendor app.

<details>
<summary>Development</summary>

The integration tests start Home Assistant against a real p56-server with a virtual fan:

```bash
pip install pytest-homeassistant-custom-component
P56_SERVER=/path/to/p56-server pytest
```
</details>

## 📄 License

MIT. Not affiliated with the maker of the fans or the Holoscope app.
