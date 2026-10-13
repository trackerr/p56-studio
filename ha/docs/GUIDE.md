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
  a file picker …) and adds the actions `p56_pov.show_text` and `p56_pov.show_image`.
- The **panel** "P56 POV" in the sidebar is the full P56 Studio app: upload photos and videos,
  type text, cast live, change the fan's Wi-Fi.

A fan accepts **one connection**. With the add-on running, Home Assistant keeps that connection,
and your phone and computer can join in through Home Assistant ([section 8](#8-your-phone-and-computer-through-home-assistant)).

## 2. First time

1. Install the add-on and the integration as in the [README](../README.md#-installation), and
   start the add-on.
2. **Put the fan on your Wi-Fi** (once): a new fan makes its own Wi-Fi. Connect a phone to it,
   open P56 Studio (or the vendor app) and set your router under **Network → Connect to your
   router**. Then close that app.
3. Within a minute the fan connects to Home Assistant. **Settings → Devices & services** shows
   "P56 POV Display discovered": press **Add**.

> No fan yet? Turn on **simulate** in the add-on configuration: a virtual fan appears and you
> can try everything.

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
**P56 POV Display: Show text** or **Show picture**. A few ideas:

**The washing machine is done** (a notification for 15 seconds, then the playlist continues)

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
messages. Using the same `name` again
replaces the earlier picture instead of adding a new one.

**Important messages first.** Give a message with `show_seconds` a `priority` (default 0). A
message that is less important than the one showing waits and comes afterwards; a more
important one interrupts, and the interrupted message comes back for the time it had left.
So the doorbell (`priority: 10`) interrupts the weather (`priority: 0`), and an alarm
(`priority: 20`) is never pushed away by the doorbell.

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

**What you need:** an AI in Home Assistant that can make pictures, for example **Google
Generative AI** (Gemini). Set it up under **Settings → Devices & services → Add integration**
with your own API key; it then also offers an **AI Task**. P56 uses whichever one you set up;
nothing about your AI account is stored by P56. Making pictures can cost money at the AI's
provider (with Google it needs billing turned on for the key).

**Or on your own computer:** a PC with a graphics card can make the pictures with
[ComfyUI](https://www.comfy.org) and the free **Qwen Image 2.1** model (install its *Text to
Image* template). Start ComfyUI with `--listen`, and in the add-on's **Configuration** fill in
**ComfyUI address**, for example `http://192.168.1.20:8188`. A picture then takes 5–10 seconds,
costs nothing and never leaves your house. When that PC is off, the AI of Home Assistant above is
used instead.

**From the panel:** open **P56 POV → Media**. When an AI is available, an **AI picture** box
appears: describe the picture (for example *a jellyfish*), choose **Still** or **Turning** and a
**Style** (or leave it on *As described* and put the style in your words), and press **Make**. After 5–30 seconds it is in the preview, ready for **Upload**
or the **Cast** tab like any other picture; a turning picture plays in the preview (pause it with
the button next to the frame slider). A black background and one bold subject in the
middle are asked for automatically, because the fan shows black as transparent; the style is
up to you.

**From an automation:**

```yaml
action: p56_pov.show_ai
data:
  prompt: a neon jellyfish
  kind: spin          # still, or spin (turning round its own axis)
  art_style: photo    # or 3d, cartoon, anime, painting, watercolor, pixel, neon, line, comic, or your own words
  show_seconds: 60
```

**Text over an AI background**, for example the electricity price: add `text`. The AI then draws
a background with a calm middle, and the price is written over it, sharp and readable. The
same prompt reuses the background made before, so updating the price often does not cost
a new picture each time; let the prompt change only with the mood (cheap or expensive).

```yaml
action: p56_pov.show_ai
data:
  prompt: >
    {{ 'sunny green energy' if states('sensor.electricity_price') | float < 0.25 else 'red warning, expensive power' }}
  text: "€ {{ states('sensor.electricity_price') }}"
```

The same box and action also appear in P56 Studio on a phone or computer that goes through
Home Assistant ([section 8](#8-your-phone-and-computer-through-home-assistant)).

## 8. Your phone and computer through Home Assistant

Because Home Assistant keeps the fan's one connection, a P56 Studio app on your phone or computer
cannot find the fan by itself while the add-on runs. Let the app go **through** Home Assistant
instead; then everything works at the same time.

**Once, in Home Assistant**

1. **Settings → Add-ons → P56 POV Display → Configuration**.
2. Turn on **lan_api**.
3. Fill in **api_token** with a password you make up (keep it: you need it on each device).
4. Save and **restart** the add-on.

**On each phone or computer (P56 Studio)**

1. Open the **App** tab and choose **Through a P56 server**.
2. Enter your Home Assistant address with port **8765**, for example `192.168.1.20:8765`
   (the same address you use for Home Assistant, but with `:8765`).
3. Press **Test**, then **Use this server**.
4. Enter the **api_token** when the app asks for it.

The app now shows and controls the same fan as Home Assistant. The address stays filled in when
you switch back to **This app controls the fan** (for example on a trip, connecting to the fan's
own Wi-Fi) and later return to the server.

## 9. When something does not work

| What you see | What to do |
| --- | --- |
| No fan appears | Is the fan on the same network as Home Assistant? Close Holoscope and P56 Studio on other devices (or set them to go through Home Assistant, [section 8](#8-your-phone-and-computer-through-home-assistant)). If your network blocks broadcasts, fill in the fan's IP under **fan_ip** in the add-on. |
| The phone app finds the fan, Home Assistant does not | The app has the fan's connection. Close it (swipe it away), or set it to go through Home Assistant. |
| The app says "No P56 server answers at this address" | Check the address and `:8765`, and that **lan_api** is on and the add-on was restarted. |
| The app asks for an API token | Enter the **api_token** from the add-on configuration. |
| "Home Assistant camera" is missing on the Cast tab | Update the add-on to 2.1.1 or later (it needs access to Home Assistant's cameras). |
| The **AI picture** box is missing on the Media tab | Set up an AI that can make pictures ([section 7](#7-pictures-made-by-ai)) and update the add-on to 2.1.9 or later. |
| "the AI made no picture" | The message after it comes from the AI; often the key has no billing or hit its limit. |
| The fan falls off Wi-Fi and keeps showing its own hotspot | Do not pair a Bluetooth receiver or speaker to the fan: Wi-Fi and Bluetooth share one radio, and the fan cannot rejoin Wi-Fi while Bluetooth is connected. Disconnect the speaker or switch the fan's Bluetooth off. P56 Studio does not use Bluetooth. |
| Four blue lights blink on the fan | Its card is empty. Keep one black picture on it (upload a black image named for example `zwart`): the fan then shows nothing, and messages return to it. |
| Something else | Open the add-on's **Log** tab; the last lines usually say what is going on. |
