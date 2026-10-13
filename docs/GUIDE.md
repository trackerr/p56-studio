# P56 Studio user guide

A short guide for everyday use. You do not need to know anything about networks or protocols;
for that, see [PROTOCOL.md](../PROTOCOL.md).

- [1. Getting connected](#1-getting-connected)
- [2. The Fan tab](#2-the-fan-tab)
- [3. Putting pictures, GIFs, videos and text on the fan](#3-putting-pictures-gifs-videos-and-text-on-the-fan)
- [4. The playlist](#4-the-playlist)
- [5. Casting live](#5-casting-live)
- [6. Network: Wi-Fi and name](#6-network-wi-fi-and-name)
- [7. Several phones and computers, or Home Assistant, together](#7-several-phones-and-computers-or-home-assistant-together)
- [8. When something does not work](#8-when-something-does-not-work)

---

## 1. Getting connected

A new fan makes its **own Wi-Fi network** (a hotspot), named like `3D-P56-…`.

1. Switch on the fan.
2. Connect your phone or computer to the fan's Wi-Fi (its password is in the fan's manual or on
   its label). It has no internet; that is normal. On Android, choose "stay connected" if the
   phone asks.
3. Open **P56 Studio**. After a few seconds the fan's name appears at the top of the screen.

**Put the fan on your home Wi-Fi (recommended, once).** Then your phone and computer keep their
normal internet while you use the fan: go to **Network → Connect to your router**, enter your Wi-Fi
name and password and press **Set router**. Put your phone or computer back on your home Wi-Fi;
the fan finds it by itself. The fan has a **2.4 GHz** radio only, so it needs a 2.4 GHz network
(a combined 2.4/5 GHz network is fine); name and password can be at most 31 characters.

**Keep one black picture on the fan.** With an empty playlist a P56 blinks four blue LEDs.
Upload a black picture (for example named `black`) and leave it there: the fan then shows
nothing while it has nothing else to play.

> Only **one app at a time** can control a fan: close Holoscope, and open P56 Studio on one
> device. Want to use several devices, or Home Assistant? See [section 7](#7-several-phones-and-computers-or-home-assistant-together).

## 2. The Fan tab

The round dial is the fan.

- **The picture in the dial** is what the fan shows right now: the file that plays (for files you
  uploaded with P56 Studio or Home Assistant) or the live picture while you cast. Files that
  another app put on the fan show their name instead.
- **Turn the picture**: drag the **outer ring**. The fan turns its picture as you let go.
- **Brightness**: the **slider** under the dial, from 1 to 16.
- **Power**, **pause / play** and **refresh** are the round buttons below.
- On the right (below on a phone): **repeat** the whole list or one file, the
  breathing light (a status light: all LEDs light up in one colour now and then, to show that the
  fan is on), start automatically at power-on,
  volume (only on fans with a speaker) and,
  under maintenance, **format** (erases all files) and **factory settings**.

## 3. Putting pictures, GIFs, videos and text on the fan

Go to the **Media** tab.

**A photo or GIF**

1. Press **Choose photo, GIF or video…** and pick a file.
2. Check the round preview: that is how it will look on the fan.
3. Adjust under **Position & scale** if needed: move it, zoom, turn it, or mirror it (when you
   look at the fan from behind).
4. Give it a **name on the fan** and press **Upload & show**.

Photos from a phone are turned upright automatically.

**A video**

1. Choose the video, set where it starts and stops and how smooth it should be.
2. Press **Make frames** and wait until it is done.
3. Press **Upload & show**. The sound of the video goes along (fans with a speaker play it).

**Text**

1. Type your text in **or type some text…** and choose the colours.
2. Choose how it moves: **Still**, **Ticker** (runs across the middle) or **Around the rim**, and
   the speed.
3. Press **Upload & show**. (Pressing **Text** first shows a preview; Upload makes it for you if
   you did not.)

Uploading the same name again replaces the old file.

## 4. The playlist

The **Playlist** tab lists the files on the fan. Per file:

- 👁 show it now,
- ✏ rename it,
- 🗑 delete it.

With repeat **Whole list** (Fan tab) the fan plays all files one after another; with **Current**
it keeps playing one.

## 5. Casting live

Casting sends pictures to the fan live, without storing anything on it. Go to the **Cast** tab,
choose a source and press the round **Start casting** button. Press it again to stop; the fan then goes
back to its playlist by itself.

| Source | What it sends |
| --- | --- |
| **Test card** | Coloured rings: handy to check that casting works |
| **Selected media** | What you chose on the Media tab (a photo, GIF, or a video after **Make frames**) |
| **Camera** | The camera of this device (on a phone the rear camera) |
| **This screen** (Windows) | Your whole screen, or choose **one window** in the list, for example a browser or video player |
| **Home Assistant camera** | In Home Assistant: any camera of your home, chosen from a list |

The dial on the Cast tab shows the picture as the fan receives it. Casting switches the fan on if
it is off.

## 6. Network: Wi-Fi and name

On the **Network** tab:

- **Device name**: the name of the fan in the apps.
- **Connect to your router**: put the fan on your home Wi-Fi (see [section 1](#1-getting-connected)).
- **Fan's own Wi-Fi (hotspot)**: change the name and password of the fan's own Wi-Fi. Write the
  new ones down: the old ones stop working.

P56 Studio does not use the fan's Bluetooth. Do not pair a Bluetooth receiver or speaker to the
fan: Wi-Fi and Bluetooth share one radio, and the fan cannot rejoin your Wi-Fi while Bluetooth is
connected.

## 7. Several phones and computers, or Home Assistant, together

A fan accepts **one connection**: the first app that finds it. To use it from several devices at
once, let **one server** keep the fan, and let every app go through that server. The easiest
server is the **Home Assistant add-on** (see
[home-assistant-p56](https://github.com/trackerr/home-assistant-p56)); on a computer that is always
on, `p56-server --lan` works too.

Through a server that can make AI pictures (the Home Assistant add-on with an AI set up, or
`p56-server --comfyui`), the **Media** tab also gets an **AI picture** box: describe a picture and
it is drawn for the fan (see the
[Home Assistant guide](https://github.com/trackerr/home-assistant-p56/blob/main/docs/GUIDE.md#7-pictures-made-by-ai)).

**With Home Assistant as the server**

1. In Home Assistant, open **Settings → Add-ons → P56 POV Display → Configuration**.
2. Turn on **lan_api** and fill in an **api_token**: a password you make up. Save, and restart
   the add-on.
3. On your phone or computer, open P56 Studio, go to the **App** tab and choose **Through a P56
   server**.
4. Enter the address of Home Assistant with port 8765, for example `192.168.1.20:8765`, press
   **Test** and then **Use this server**.
5. When the app asks for the **API token**, enter the token from step 2.

From now on the app, Home Assistant and every other device you set up this way control the same
fan. The address stays filled in: switch back to **This app controls the fan** and later to the
server again without typing it again.

Without a server, the app that finds the fan first gets it. Close that app (on a phone: swipe it
away) and the fan connects to the next one within a few seconds.

## 8. When something does not work

| What you see | What to do |
| --- | --- |
| "Searching for your fan…" stays | Is this device on the fan's Wi-Fi, or on the same router as the fan? Is Holoscope or another P56 Studio still open somewhere? On Windows, allow P56 Studio in the firewall when asked. |
| The fan connected to another device | Close the app there, or use one server for all ([section 7](#7-several-phones-and-computers-or-home-assistant-together)). |
| "No P56 server answers at this address" | Check the address and port 8765. In Home Assistant, turn on **API reachable on the network** in the add-on. |
| "API token required" | Enter the **API token** from the add-on configuration (or `--api-token` of p56-server). |
| Casting shows nothing | Press the test card first. If the rings do show, the source is the problem (camera permission, a closed window). |
| A picture is turned the wrong way | Drag the outer ring on the Fan tab, or use **Rotation** under Position & scale before uploading. |
| The fan stays on its own Wi-Fi after setting the router | Check the Wi-Fi name and password, and set them again from the fan's own Wi-Fi. |
| The fan falls off Wi-Fi and keeps showing its own hotspot | Do not pair a Bluetooth receiver or speaker to the fan: Wi-Fi and Bluetooth share one radio, and the fan cannot rejoin Wi-Fi while Bluetooth is connected. Disconnect the speaker (with the vendor app, or switch the speaker off and restart the fan), then set the router again. P56 Studio does not use Bluetooth. |
| Four blue lights blink on the fan | Its playlist is empty. Keep one black picture on it ([section 1](#1-getting-connected)). |
