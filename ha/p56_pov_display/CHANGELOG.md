# Changelog

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
