# P56 POV Display for Home Assistant

Control **Holoscope / P56 POV fans** (the spinning LED "hologram" fans) from Home
Assistant, locally, without the vendor app or a cloud. This repository has two parts
that work together:

| Part | What it does |
| --- | --- |
| **Add-on** [`p56_pov_display`](p56_pov_display) | Runs *p56-server*: the fans connect to it (TCP 6666), it speaks their protocol, serves the P56 Studio web app as a sidebar panel and offers an HTTP API. |
| **Integration** [`custom_components/p56_pov`](custom_components/p56_pov) | Turns every fan into a Home Assistant device with entities and actions, and updates live from the add-on. |

The add-on is built on [p56-studio](https://github.com/trackerr/p56-studio): the same
Rust core powers the Windows and Android apps that replace the vendor app.

```
 fan ──TCP 6666──▶ add-on (p56-server) ◀──HTTP + live events── integration p56_pov
  ▲  UDP 8988 discovery      │  port 8765: API + web UI (panel)
  └──────────────────────────┘
```

## Install

1. **Add-on**: Settings → Add-ons → Add-on store → ⋮ → Repositories → add
   `https://github.com/trackerr/home-assistant-p56`. Install **P56 POV Display**, start it.
   Turn on *simulate* to try it without a fan.
2. **Integration**: HACS → ⋮ → Custom repositories → add this repository as type
   *Integration* → install **P56 POV Display** → restart Home Assistant.
   (Or copy `custom_components/p56_pov` to `/config/custom_components/`.)
3. Settings → Devices & services: "P56 POV Display discovered" → add.

Requires Home Assistant 2026.8 or newer; amd64 or aarch64 for the add-on.

## Use it

- **Dashboard**: the light entity (on/off, brightness), the *File* select (playlist),
  *Playing* switch, the *Text* field and the *Shown* image.
- **Notifications**: `notify.send_message` to `notify.<fan>_message` shows a message for
  15 s (configurable), then the playlist resumes.
- **Actions** `p56_pov.show_text`, `p56_pov.show_image` (file, URL, camera snapshot or
  media library), `p56_pov.play_file`, `p56_pov.delete_file`.

```yaml
# Energy price on the fan every hour, removed again after a minute
triggers:
  - trigger: time_pattern
    minutes: 0
actions:
  - action: p56_pov.show_text
    data:
      text: "{{ states('sensor.energy_price') | round(2) }} €/kWh"
      color: "#ffc800"
      name: energy
      show_seconds: 60
```

More in [examples/automations.yaml](examples/automations.yaml) and in the
[add-on documentation](p56_pov_display/DOCS.md) (options, HTTP API, `rest_command`).

## Upgrading from the 1.x local add-on

Uninstall the old local add-on, install this one, remove the old integration entry
and add the discovered one. `rest_command`s that post to `/text` keep working.

## Development

Integration tests run Home Assistant against a real p56-server with its virtual fan:

```bash
pip install pytest-homeassistant-custom-component
cargo build -p p56-server          # in a p56-studio checkout
P56_SERVER=…/target/debug/p56-server pytest
```

## License

MIT. Not affiliated with the maker of the fans; the protocol was worked out for
interoperability with hardware you own.
