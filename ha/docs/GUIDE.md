# P56 POV Display in Home Assistant: user guide

For everyday use. Installation is in the [README](../README.md#-installation); every option and
the HTTP API are in the [add-on documentation](../p56_pov_display/DOCS.md).

- [1. How it fits together](#1-how-it-fits-together)
- [2. First time](#2-first-time)
- [3. The P56 POV panel](#3-the-p56-pov-panel)
- [4. On your dashboard](#4-on-your-dashboard)
- [5. Automations in a few clicks](#5-automations-in-a-few-clicks)
- [6. Your cameras on the fan](#6-your-cameras-on-the-fan)
- [7. Pictures made by AI](#7-pictures-made-by-ai)
- [8. Your phone and computer through Home Assistant](#8-your-phone-and-computer-through-home-assistant)
- [9. When something does not work](#9-when-something-does-not-work)

---

## 1. How it fits together

- The **add-on** is the part that talks to the fan. The fan connects to it over your Wi-Fi.
- The **integration** turns every fan into a device with entities (a light, a text field,
  a file picker …) and adds the actions `p56_pov.show_text`, `p56_pov.show_image`,
  `p56_pov.show_ai` (a picture made by AI), `p56_pov.play_file` and `p56_pov.delete_file`.
- The **panel** "P56 POV" in the sidebar is the full P56 Studio app: upload photos and videos,
  type text, cast live, change the fan's Wi-Fi.

A fan accepts **one connection**. With the add-on running, Home Assistant keeps that connection,
and your phone and computer can join in through Home Assistant ([section 8](#8-your-phone-and-computer-through-home-assistant)).

## 2. First time

1. Install the add-on and the integration as in the [README](../README.md#-installation), and
   start the add-on.
2. **Put the fan on your Wi-Fi** (once): a new fan makes its own Wi-Fi. Connect a phone to it,
   open P56 Studio (or the vendor app) and set your router under **Network → Connect to your
   router**. Then close that app. The fan needs a **2.4 GHz** network (a combined 2.4/5 GHz
   network is fine), and do not connect a Bluetooth speaker to it (see [section 9](#9-when-something-does-not-work)).
3. Within a minute the fan connects to Home Assistant. **Settings → Devices & services** shows
   "P56 POV Display discovered": press **Add**.
4. **Put one black picture on the fan** and leave it there (panel → **Media**: a black picture,
   named for example `black`, **Upload & show**). With an empty playlist a P56 blinks four blue
   LEDs; with the black picture it shows nothing, and messages return to it when they are over.

> No fan yet? Turn on **Virtual fan** (`simulate`) in the add-on configuration: a virtual fan
> appears and you can try everything.

## 3. The P56 POV panel

Open **P56 POV** in the sidebar (turn on *Show in sidebar* in the add-on if it is not there).
It works like the P56 Studio app; its [user guide](https://github.com/trackerr/p56-studio/blob/main/docs/GUIDE.md)
explains every tab. In short:

- **Fan**: the dial shows what the fan plays. Drag the outer ring to turn the picture; the slider
  below sets the brightness.
- **Media**: choose a photo, GIF or video, or type a text (still, ticker or around the rim), and
  press **Upload & show**.
- **Playlist**: show, rename or delete files.
- **Cast**: show something live without storing it, such as one of your cameras.
- **Network**: the fan's name and Wi-Fi.

## 4. On your dashboard

The integration gives every fan entities you can put on any dashboard:

| Card | Entity (for a fan called *P56*) |
| --- | --- |
| On/off and brightness | `light.p56` (a light card) |
| Type a text, the fan shows it | `text.p56_text` (an entities card) |
| Choose what plays | `select.p56_file` |
| What the fan shows | `image.p56_shown` (a picture entity card); faded once it is no longer on the fan |
| Play / pause | `switch.p56_playing` |

## 5. Automations in a few clicks

In **Settings → Automations & scenes → Create automation**, choose a trigger, then add the action
**P56 POV Display: Show text**, **Show picture** or **Show AI picture**. A few ideas:

**The washing machine is done** (a notification with a bell for 15 seconds, then the playlist
continues; a `title` becomes the first line)

```yaml
action: notify.send_message
target:
  entity_id: notify.p56_message
data:
  message: Washing done
```

**The energy price, for a minute, in a colour from green (cheap) to red (expensive)**

```yaml
action: p56_pov.show_text
data:
  text: "{{ states('sensor.energy_price') }}"
  style: price
  value: "{{ states('sensor.energy_price') }}"
  value_min: 0.10
  value_max: 0.40
  show_seconds: 60
```

**Someone at the door: a snapshot of the doorbell camera for 20 seconds**

```yaml
action: p56_pov.show_image
data:
  camera: camera.front_door
  show_seconds: 20
```

With `show_seconds` the message stays on until its time is up (the fan repeats only that one
meanwhile), then the fan goes back to what it played before. If the fan was off, it switches on
for the message and off again afterwards, so a fan that is normally off only lights up for
messages. It first spins up for 2.5 seconds on black, so the message is steady at once. This works for stored messages and for `mode: cast` (live, nothing stored). Using the
same `name` again replaces the earlier picture instead of adding a new one.

**Important messages first.** Give a message with `show_seconds` a `priority` (default 0). A
message that is less important than the one showing waits and comes afterwards; a more
important one interrupts, and the interrupted message comes back for the time it had left.
So the doorbell (`priority: 10`) interrupts the weather (`priority: 0`), and an alarm
(`priority: 20`) is never pushed away by the doorbell.

**It starts raining: the rain radar with a text over it** (with the Buienradar integration, which
offers `camera.buienradar` and a weather entity)

```yaml
triggers:
  - trigger: state
    entity_id: weather.home                     # your own weather entity
    to: [rainy, pouring, lightning-rainy]
actions:
  - action: p56_pov.show_image
    data:
      camera: camera.buienradar
      text: "Rain · {{ state_attr('weather.home', 'temperature') | round(0) | int }}°"
      mode: cast
      show_seconds: 30
```

**A meter ring.** With `gauge: true` and `value`, `value_min`, `value_max`, a ring along the
rim fills like a meter, clockwise from the top, in the colour of the value: a battery, the
solar power or the price at a glance.

```yaml
action: p56_pov.show_text
data:
  text: "{{ states('sensor.home_battery') }}%"
  value: "{{ states('sensor.home_battery') }}"
  value_min: 0
  value_max: 100
  invert_colors: true      # red when empty, green when full
  gauge: true
```

More examples: [examples/automations.yaml](../examples/automations.yaml).

## 6. Your cameras on the fan

**Live, from the panel:** open **P56 POV → Cast**, choose **Home Assistant camera**, pick the
camera from the list and press **Start casting**. Home Assistant sends a new picture several
times a second until you press stop; the fan then returns to its playlist.

**A snapshot, from an automation:** use `p56_pov.show_image` with `camera:` (see above). Add
`mode: cast` to show it live for `show_seconds` without storing anything on the fan.

## 7. Pictures made by AI

Type what you want to see, and an AI draws it for the fan, in any style: a photo, a painting,
a cartoon, a 3D render, neon and more. Show it still, or let the object turn round its own axis
(as on a turntable).

**What you need:** an AI that can make pictures. There are two ways, and you can have both:

**A. On your own computer, with ComfyUI (free, private).** A PC with a recent graphics card
(about 12 GB of video memory) makes a picture in 5–10 seconds, and nothing leaves your house.

1. Install [ComfyUI](https://www.comfy.org) on that PC.
2. In ComfyUI open **Templates** and install **Qwen Image 2.1: Text to Image**; let it download
   its model files (the *turbo* model is the fastest). The prompt enhancer it mentions is not
   needed.
3. Let ComfyUI accept the network: start it with `--listen` (for example
   `python main.py --listen 0.0.0.0 --port 8188`; in ComfyUI Desktop set the listen address to
   `0.0.0.0` in the server settings) and allow that port in the PC's firewall. Check that
   `http://<that PC>:8188` opens on your phone.
4. In Home Assistant: **Settings → Add-ons → P56 POV Display → Configuration**, fill in **ComfyUI
   address**, for example `http://192.168.1.30:8188` (plain `http://`), save and restart the
   add-on. Its log then says *AI pictures from ComfyUI at …*.

The first picture after ComfyUI starts takes a little longer (it loads the model). When the PC
is off, the AI of way B is used, if you set that up too.

**B. An AI in Home Assistant, for example Google Gemini (paid per picture).**

1. Make an API key in [Google AI Studio](https://aistudio.google.com) and turn on billing for
   it (pictures need it).
2. **Settings → Devices & services → Add integration → Google Generative AI**, and enter the key.
   It also adds an **AI Task** (an entity `ai_task.…`).
3. Nothing else: P56 finds it by itself. Nothing about your AI account is stored by P56.

Other AIs in Home Assistant that offer an AI Task with pictures work the same way.

**From the panel:** open **P56 POV → Media**. When an AI is available, an **AI picture** box
appears: describe the picture (for example *a jellyfish*), choose **Still**, **Turning** or **Real 3D** and a
**Style** (or leave it on *As described* and put the style in your words), and press **Make**. After 5–30 seconds (ComfyUI: 5–10) it is in the preview, ready for **Upload**
or the **Cast** tab like any other picture; a turning picture plays in the preview (pause it with
the button next to the frame slider). A black background and one bold subject in the
middle are asked for automatically, because the fan shows black as transparent; the style is
up to you.

**Real 3D** (only with ComfyUI) makes an actual 3D model of the object and shows it turning, from
the front, the sides and the back: a pumpkin for Halloween, a Christmas tree, a treasure chest.
It takes about 2 minutes the first time; the add-on keeps it, so the same description again is
there at once, also next year. In ComfyUI install the template **Pixal3D & TRELLIS.2: Image to
Model** with its files as well.

**From an automation:**

```yaml
action: p56_pov.show_ai
data:
  prompt: a neon jellyfish
  kind: spin          # still, spin (turning round its own axis) or 3d (a real 3D model, ComfyUI, ~2 min)
  art_style: photo    # or 3d, cartoon, anime, painting, watercolor, pixel, neon, line, comic, or your own words
  show_seconds: 60
```

**Text over an AI background**, for example the electricity price or a notification: add
`text`. The AI then draws a whole picture of the prompt (a washing machine for "the laundry is
done"), without writing, and the text is written over it on a darkened band, sharp and
readable. An icon is not shown over an AI picture. The same prompt reuses the picture made
before, so updating the price often does not cost a new picture each time; let the prompt
change only with the mood (cheap or expensive). `value`, `value_min` and `value_max` colour the
text from green to red, as with `show_text`.

```yaml
action: p56_pov.show_ai
data:
  prompt: >
    {{ 'sunny green energy' if states('sensor.electricity_price') | float < 0.25 else 'red warning, expensive power' }}
  text: "€ {{ states('sensor.electricity_price') }}"
  value: "{{ states('sensor.electricity_price') }}"
  value_min: 0.20
  value_max: 0.35
  show_seconds: 60
  mode: cast
```

**Every notification its own picture.** An automation with the trigger *Persistent notification*
(*added*) can let the AI draw what the notification is about. With `entity_id: comfyui` only your
own ComfyUI makes it, so the text of your notifications never leaves the house; when the PC is
off, `show_ai` fails and the second action shows the text with an icon instead (`ai` is
then not set).

```yaml
triggers:
  - trigger: persistent_notification
    update_type: [added]
actions:
  - action: p56_pov.show_ai
    continue_on_error: true
    response_variable: ai
    data:
      entity_id: comfyui
      prompt: "{{ trigger.notification.title }} {{ trigger.notification.message }}"
      text: "{{ trigger.notification.message }}"
      show_seconds: 15
      priority: 5
      mode: cast
  - if: "{{ ai is not defined }}"
    then:
      - action: p56_pov.show_text
        data:
          text: "{{ trigger.notification.message }}"
          style: info
          show_seconds: 15
          priority: 5
          mode: cast
```

The same box and action also appear in P56 Studio on a phone or computer that goes through
Home Assistant ([section 8](#8-your-phone-and-computer-through-home-assistant)).

## 8. Your phone and computer through Home Assistant

Because Home Assistant keeps the fan's one connection, a P56 Studio app on your phone or computer
cannot find the fan by itself while the add-on runs. Let the app go **through** Home Assistant
instead; then everything works at the same time.

**Once, in Home Assistant**

1. **Settings → Add-ons → P56 POV Display → Configuration**.
2. Turn on **API reachable on the network** (`lan_api`).
3. Fill in **API token** (`api_token`) with a password you make up (keep it: you need it on each
   device).
4. Save and **restart** the add-on.

**On each phone or computer (P56 Studio)**

1. Open the **App** tab and choose **Through a P56 server**.
2. Enter your Home Assistant address with port **8765**, for example `192.168.1.10:8765`
   (the same address you use for Home Assistant, but with `:8765`).
3. Press **Test**, then **Use this server**.
4. Enter the **API token** when the app asks for it.

The app now shows and controls the same fan as Home Assistant. The address stays filled in when
you switch back to **This app controls the fan** (for example on a trip, connecting to the fan's
own Wi-Fi) and later return to the server.

## 9. When something does not work

| What you see | What to do |
| --- | --- |
| No fan appears | Is the fan on the same network as Home Assistant? Close Holoscope and P56 Studio on other devices (or set them to go through Home Assistant, [section 8](#8-your-phone-and-computer-through-home-assistant)). If your network blocks broadcasts, fill in the fan's IP under **Fan IP addresses** (`fan_ip`) in the add-on. |
| The phone app finds the fan, Home Assistant does not | The app has the fan's connection. Close it (swipe it away), or set it to go through Home Assistant. |
| The app says "No P56 server answers at this address" | Check the address and `:8765`, and that **API reachable on the network** is on and the add-on was restarted. |
| The app asks for an API token | Enter the **API token** from the add-on configuration. |
| "Home Assistant camera" is missing on the Cast tab | Update the add-on to 2.1.1 or later (it needs access to Home Assistant's cameras). |
| The **AI picture** box is missing on the Media tab | Set up an AI that can make pictures ([section 7](#7-pictures-made-by-ai)) and update the add-on to 2.1.9 or later. |
| "the AI made no picture" | The message after it comes from the AI; often the key has no billing or hit its limit. |
| "ComfyUI not reachable" | The PC is off, ComfyUI does not listen on the network (`--listen`), or its firewall blocks the port. Check that `http://<that PC>:8188` opens on your phone. |
| "ComfyUI has no Qwen Image 2.1 file in models/…" | Install the **Qwen Image 2.1: Text to Image** template in ComfyUI, with its model files. |
| "ComfyUI took too long" | The PC is busy, or **ComfyUI picture size** is very large; the default 768 is plenty. |
| "no AI in Home Assistant can make pictures" | Set up ComfyUI or an AI in Home Assistant ([section 7](#7-pictures-made-by-ai)). |
| "the AI made a picture, but it could not be fetched from Home Assistant" | Home Assistant must answer on port 8123 with plain http on the same machine (not when SSL is set up directly in Home Assistant); ComfyUI does not have this limit. |
| The fan falls off Wi-Fi and keeps showing its own hotspot | Do not pair a Bluetooth receiver or speaker to the fan: Wi-Fi and Bluetooth share one radio, and the fan cannot rejoin Wi-Fi while Bluetooth is connected. Disconnect the speaker (with the vendor app, or switch the speaker off and restart the fan), then set the router again. P56 Studio does not use Bluetooth. |
| Four blue lights blink on the fan | Its playlist is empty. Keep one black picture on it ([section 2](#2-first-time), step 4): the fan then shows nothing, and messages return to it. |
| Something else | Open the add-on's **Log** tab; the last lines usually say what is going on. |
