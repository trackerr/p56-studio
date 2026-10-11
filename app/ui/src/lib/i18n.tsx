// Tiny dependency-free i18n layer: English and Dutch dictionaries, `{name}` interpolation,
// simple one/other plurals (pass `count`), a React provider and a module-level `t()` for
// non-React code (API bridge, event handlers). The language is stored in localStorage `p56.lang`.
import { createContext, Fragment, useCallback, useContext, useMemo, useSyncExternalStore, type ReactNode } from 'react';

export type Lang = 'en' | 'nl';
export const LANGS: Lang[] = ['nl', 'en'];
type Plural = { one: string; other: string };
type Entry = string | Plural;

const en = {
  // header / tabs
  'link.connected': 'connected',
  'link.searching': 'searching for fan…',
  'link.starting': 'starting…',
  'link.error': 'error',
  'header.fans': { one: '{count} fan', other: '{count} fans' },
  'tab.fan': 'Fan',
  'tab.playlist': 'Playlist',
  'tab.media': 'Media',
  'tab.cast': 'Cast',
  'tab.network': 'Network',
  'tab.app': 'App',
  'tabShort.playlist': 'List',
  'tabShort.network': 'Net',
  'tabShort.cast': 'Cast',
  'nav.label': 'Main menu',
  'header.pickFan': 'Choose a fan',
  'header.viaServer': 'via server',
  'lang.label': 'Language',
  'lang.hint': 'Interface language. Messages from the fan and technical details are shown as they are received.',

  // common
  'common.cancel': 'Cancel',
  'common.close': 'Close',
  'common.save': 'Save',
  'common.stop': 'Stop',
  'common.refresh': 'Refresh',
  'common.reset': 'Reset',
  'common.noFan': 'No fan connected',
  'common.notifications': 'Notifications',
  'common.typeId': 'type {id}',
  'common.fps': 'Frames/s',
  'common.maxFrames': 'Max frames',
  'common.testCard': 'Test card',
  'common.source': 'Source',
  'common.current': 'Current: {value}',
  'common.noImage': 'no image',

  // state / events
  'log.linkStarted': 'link started, broadcasting as {ip}{extra}',
  'log.cannotStart': 'cannot start: {error}',
  'log.fanConnected': 'fan #{id} connected ({ip})',
  'log.fanGone': 'fan #{id} gone',
  'log.fanSeen': 'fan seen: {ip}',
  'toast.fanConnected': 'Fan connected',
  'toast.fanDisconnected': 'Lost connection to the fan',
  'toast.fanError': 'The fan reports an error',

  // api / demo
  'api.pickFilter': 'Image or video',
  'api.photoPrefix': 'photo',
  'api.videoUnreadable': 'the app cannot read this video (codec?)',
  'api.videoNoDuration': 'the video has no duration',
  'api.unauthorized': 'the server needs an API token',
  'auth.title': 'API token required',
  'auth.hint': 'This P56 server only accepts devices on the network with an API token. Enter the token from the server settings.',
  'auth.placeholder': 'API token',
  'api.uploadResult': { one: '{name} ({count} frame)', other: '{name} ({count} frames)' },
  'demo.file1': 'welcome.mp4',
  'demo.phone': 'Phone',
  'demo.textName': 'text',
  'demo.unknownCommand': 'demo: unknown command {cmd}',

  // fan panel
  'nofan.title': 'Looking for your fan…',
  'nofan.subtitle': "The fan connects by itself as soon as it sees this app's signal.",
  'nofan.cannotStart': 'Cannot start',
  'nofan.step1': 'Connect this device to the {wifi} (default IP {ip}) or to the same router.',
  'nofan.fanWifi': "fan's Wi-Fi",
  'nofan.step2': 'Close {holoscope} and other P56 apps; only one can listen at a time.',
  'nofan.step3': 'Windows: allow the app through the firewall (TCP 6666, UDP 8988) when asked.',
  'nofan.broadcasting': 'This app broadcasts as {ip}',
  'nofan.seenUdp': ' · seen via UDP: {list}',
  'fan.on': 'on',
  'fan.off': 'off',
  'fan.playing': 'playing',
  'fan.paused': 'paused',
  'fan.unsupported': 'unknown image format',
  'fan.legacy': 'old image format',
  'fan.lastError': 'Last message from the fan',
  'fan.power': 'Power',
  'fan.turnOff': 'Turn off',
  'fan.turnOn': 'Turn on',
  'fan.play': 'Play',
  'fan.pause': 'Pause',
  'fan.display': 'Display',
  'fan.brightness': 'Brightness',
  'fan.angle': 'Angle',
  'fan.color': 'Color',
  'fan.colorMode': 'Color mode',
  'fan.trueColor': 'True color',
  'fan.highBrightness': 'High brightness',
  'fan.colorNotReported': 'not reported',
  'fan.playback': 'Playback',
  'fan.behavior': 'Behavior',
  'fan.ringHint': 'Drag the outer ring to turn the picture.',
  'strip.title': 'Now on the fan',
  'strip.add': 'Add',
  'strip.show': 'Show {name}',
  'fan.repeat': 'Repeat',
  'fan.repeatAll': 'Whole list',
  'fan.repeatOne': 'Current',
  'fan.breath': 'Breathing light',
  'fan.autostart': 'Start automatically at power-on',
  'fan.master': 'Group master',
  'fan.volume': 'Volume',
  'fan.volumeHint': 'Volume only works on fans with a speaker.',
  'fan.maintenance': 'Maintenance',
  'fan.formatTitle': 'Format memory card?',
  'fan.format': 'Format',
  'fan.formatBody': 'All files on the fan will be erased. This cannot be undone.',
  'fan.resetTitle': 'Restore factory settings?',
  'fan.resetAction': 'Restore',
  'fan.resetBody': 'Name, brightness, angle and other settings return to their defaults.',
  'fan.factory': 'Factory settings',

  // playlist
  'playlist.title': { one: 'Playlist · {count} file', other: 'Playlist · {count} files' },
  'playlist.empty': 'The playlist is empty. Add something via {media}.',
  'playlist.playing': 'playing',
  'playlist.show': 'Show',
  'playlist.rename': 'Rename',
  'playlist.deleteTitle': 'Delete "{name}"?',
  'playlist.delete': 'Delete',
  'playlist.deleteBody': 'The file will be removed from the fan.',
  'playlist.renameTitle': 'Rename slot {slot}',

  // media
  'media.placement': 'Position & scale',
  'media.horizontal': 'Horizontal',
  'media.vertical': 'Vertical',
  'media.scale': 'Scale',
  'media.mirror': 'Mirror (fan viewed from behind)',
  'media.rotate': 'Rotation',
  'media.motion': 'Motion',
  'media.still': 'Still',
  'media.scroll': 'Ticker',
  'media.orbit': 'Around the rim',
  'media.speed': 'Speed',
  'media.withSound': 'with sound ({s} s)',
  'media.sendSound': 'Send the sound along (for a speaker on the fan)',
  'media.rotateBy90': 'Turn 90° clockwise',
  'media.textName': 'text',
  'media.logText': 'text: {text}',
  'media.textFailed': 'Could not create text',
  'media.openFailed': 'Cannot open file',
  'media.videoPicked': 'Video selected',
  'media.videoPickedHint': 'Choose frames/s and optionally a start and end, then click "{button}".',
  'media.reading': 'reading…',
  'media.logMedia': { one: 'media: {name} ({count} frame, {size})', other: 'media: {name} ({count} frames, {size})' },
  'media.makingFrames': 'making frames…',
  'media.stopped': 'Stopped',
  'media.logVideo': 'video: {name} → {count} frames at {fps} fps',
  'media.framesMade': { one: '{count} frame created', other: '{count} frames created' },
  'media.videoFailed': 'Could not read video',
  'media.preparing': 'preparing…',
  'media.makingText': 'making the text…',
  'media.uploaded': 'Uploaded',
  'media.logUpload': 'upload {result}',
  'media.uploadFailed': 'Upload failed',
  'media.logUploadError': 'upload error: {error}',
  'media.stageTranscoding': 'transcoding…',
  'media.stageSending': 'sending…',
  'media.pick': 'Choose photo, GIF or video…',
  'media.testCardName': 'testcard',
  'media.textPlaceholder': 'or type some text…',
  'media.textColor': 'Text color',
  'media.background': 'Background',
  'media.text': 'Text',
  'media.info': { one: '{name} · {count} frame', other: '{name} · {count} frames' },
  'media.infoVideo': '{name} · video, no frames yet',
  'media.infoTestCard': 'Test card (red, green and blue rings)',
  'media.convert': 'Convert video to frames',
  'media.start': 'Start (s)',
  'media.end': 'End (s, 0 = end)',
  'media.makeFrames': 'Make frames',
  'media.toFan': 'To the fan',
  'media.nameOnFan': 'Name on the fan',
  'media.fileName': 'file name',
  'media.showSlot': 'Show slot',
  'media.upload': 'Upload & show',
  'media.connectFirst': 'Connect a fan first.',
  'media.preview': 'as the fan sees it · {size} · JPEG {kb} kB · quality {q}',
  'media.previewLoading': 'loading preview…',
  'media.frame': 'frame',
  'media.layoutTitle': 'The JPEG image as the fan receives it (arms side by side)',

  // cast
  'cast.pickMediaFirst': 'Pick media on the Media tab first',
  'cast.pickVideoFirst': 'Pick a video on the Media tab first',
  'cast.logStarted': 'cast started: {src}',
  'cast.logStopped': 'cast stopped',
  'cast.noCamera': 'camera not available in this webview',
  'cast.failed': 'Casting failed',
  'cast.frameFailed': 'Frame not sent, casting stopped',
  'cast.noFrames': 'The camera or video gives no picture (5 s without a frame)',
  'cast.srcMedia': 'Selected media',
  'cast.srcVideo': 'Video (live)',
  'cast.srcCamera': 'Camera',
  'cast.srcScreen': 'This screen',
  'cast.usingMedia': { one: 'Using: {name} ({count} frame)', other: 'Using: {name} ({count} frames)' },
  'cast.pickMediaHint': 'Pick a photo/GIF/video on the Media tab first.',
  'cast.videoHint': 'Plays {name} live and sends every frame.',
  'cast.pickVideoHint': 'Pick a video on the Media tab first.',
  'cast.cameraHint': "Uses this device's camera (rear camera on phones).",
  'cast.screenHint': 'Mirrors your main screen to the fan.',
  'cast.testHint': "Red, green and blue rings with a white line to 12 o'clock.",
  'cast.start': 'Start casting',
  'cast.live': 'Casting live',
  'cast.toFans': { one: 'to {count} fan', other: 'to {count} fans' },
  'cast.stats': { one: '{fps} fps · {count} fan · JPEG {kb} kB · q{q}', other: '{fps} fps · {count} fans · JPEG {kb} kB · q{q}' },
  'cast.starting': 'starting…',
  'cast.stopped': 'stopped',

  // network
  'net.deviceName': 'Device name',
  'net.nameSaved': 'Name saved',
  'net.bluetooth': "Fan's Bluetooth",
  'net.btOn': 'Bluetooth on',
  'net.btOff': 'Bluetooth off',
  'net.on': 'On',
  'net.off': 'Off',
  'net.scan': 'Scan',
  'net.disconnect': 'Disconnect',
  'net.connected': 'connected',
  'net.connecting': 'connecting…',
  'net.notConnected': 'not connected',
  'net.connect': 'Connect',
  'net.noDevices': 'No devices yet. Turn Bluetooth on and click {scan}.',
  'net.hotspot': "Fan's own Wi-Fi (hotspot)",
  'net.hotspotWarning': 'After this, the default name and password no longer work. Write down the new details. Current: {current}',
  'net.wifiName': 'Wi-Fi name',
  'net.passwordMin': 'password (min. 8 characters)',
  'net.hotspotTitle': 'Change hotspot to "{ssid}"?',
  'net.change': 'Change',
  'net.hotspotBody': 'Afterwards, connect to Wi-Fi {ssid} with password {pw}. The current connection will drop.',
  'net.changeHotspot': 'Change hotspot',
  'net.router': 'Connect to your router',
  'net.routerHint': 'The fan will then find your router by itself. Put this device on the same network. Current: {current}',
  'net.none': 'none',
  'net.routerName': 'router name (SSID)',
  'net.password': 'password',
  'net.setRouter': 'Set router',
  'net.routerSet': 'Router set',

  // app panel
  'app.fanType': 'Fan type',
  'app.auto': 'Automatic (as reported by the fan)',
  'app.unsupported': 'unsupported',
  'app.legacy': 'old format',
  'app.fanTypeHint': 'Sets the image resolution and arm layout. The P56 is type 8.',
  'app.connection': 'Connection',
  'app.ipLabel': 'IP to broadcast (empty = automatic per network adapter)',
  'app.example': 'e.g. {value}',
  'app.targetsLabel': 'Fan IP, if you know it (separate several with commas)',
  'app.apply': 'Save and reconnect',
  'app.reconnected': 'Reconnected',
  'app.broadcastingAs': 'broadcasting as {ip}',
  'app.tcpPort': 'TCP port {port}',
  'app.adapter': 'network adapter {name}',
  'app.seenUdp': 'fans seen via UDP: {list}',
  'app.noneUdp': 'no fan seen via UDP yet',
  'app.log': 'Log',
  'app.serverTitle': 'Control through a P56 server',
  'app.modeLocal': 'This app controls the fan',
  'app.modeServer': 'Through a P56 server',
  'app.serverAddress': 'Server address',
  'app.serverTest': 'Test',
  'app.serverUse': 'Use this server',
  'app.serverOk': 'A P56 server answers at this address.',
  'app.serverMissing': 'No P56 server answers at this address. Is it running with --lan?',
  'app.serverActive': 'This app controls the fans through {url}.',
  'app.serverHint': 'A fan keeps one connection. Through a p56-server (or the Home Assistant add-on) several phones and computers can control it at the same time. Start the server with --lan and an API token to reach it from other devices.',
  'app.browserDemo': 'browser (demo fan)',
} satisfies Record<string, Entry>;

export type Key = keyof typeof en;

const nl: Record<Key, Entry> = {
  'link.connected': 'verbonden',
  'link.searching': 'zoeken naar fan…',
  'link.starting': 'starten…',
  'link.error': 'fout',
  'header.fans': { one: '{count} fan', other: '{count} fans' },
  'tab.fan': 'Fan',
  'tab.playlist': 'Afspeellijst',
  'tab.media': 'Media',
  'tab.cast': 'Casten',
  'tab.network': 'Netwerk',
  'tab.app': 'App',
  'tabShort.playlist': 'Lijst',
  'tabShort.network': 'Net',
  'tabShort.cast': 'Cast',
  'nav.label': 'Hoofdmenu',
  'header.pickFan': 'Kies een fan',
  'header.viaServer': 'via server',
  'lang.label': 'Taal',
  'lang.hint': 'Taal van de app. Meldingen van de fan en technische details worden getoond zoals ze binnenkomen.',

  'common.cancel': 'Annuleren',
  'common.close': 'Sluiten',
  'common.save': 'Opslaan',
  'common.stop': 'Stop',
  'common.refresh': 'Vernieuwen',
  'common.reset': 'Reset',
  'common.noFan': 'Geen fan verbonden',
  'common.notifications': 'Meldingen',
  'common.typeId': 'type {id}',
  'common.fps': 'Frames/s',
  'common.maxFrames': 'Max. frames',
  'common.testCard': 'Testkaart',
  'common.source': 'Bron',
  'common.current': 'Nu: {value}',
  'common.noImage': 'geen beeld',

  'log.linkStarted': 'verbinding gestart, uitzenden als {ip}{extra}',
  'log.cannotStart': 'kan niet starten: {error}',
  'log.fanConnected': 'fan #{id} verbonden ({ip})',
  'log.fanGone': 'fan #{id} weg',
  'log.fanSeen': 'fan gezien: {ip}',
  'toast.fanConnected': 'Fan verbonden',
  'toast.fanDisconnected': 'Verbinding met fan verbroken',
  'toast.fanError': 'De fan meldt een fout',

  'api.pickFilter': 'Beeld of video',
  'api.photoPrefix': 'foto',
  'api.videoUnreadable': 'deze video kan de app niet lezen (codec?)',
  'api.videoNoDuration': 'video heeft geen lengte',
  'api.unauthorized': 'de server vraagt een API-token',
  'auth.title': 'API-token nodig',
  'auth.hint': 'Deze P56-server accepteert apparaten in het netwerk alleen met een API-token. Vul het token uit de serverinstellingen in.',
  'auth.placeholder': 'API-token',
  'api.uploadResult': { one: '{name} ({count} frame)', other: '{name} ({count} frames)' },
  'demo.file1': 'welkom.mp4',
  'demo.phone': 'Telefoon',
  'demo.textName': 'tekst',
  'demo.unknownCommand': 'demo: onbekend commando {cmd}',

  'nofan.title': 'Zoeken naar je fan…',
  'nofan.subtitle': 'De fan maakt zelf verbinding zodra hij het signaal van deze app ziet.',
  'nofan.cannotStart': 'Kan niet starten',
  'nofan.step1': 'Verbind dit apparaat met de {wifi} (standaard IP {ip}) of met dezelfde router.',
  'nofan.fanWifi': 'wifi van de fan',
  'nofan.step2': 'Sluit {holoscope} en andere P56-apps; er mag er maar één tegelijk luisteren.',
  'nofan.step3': 'Windows: sta de app toe in de firewall (TCP 6666, UDP 8988) als daarom gevraagd wordt.',
  'nofan.broadcasting': 'Deze app zendt uit als {ip}',
  'nofan.seenUdp': ' · gezien via UDP: {list}',
  'fan.on': 'aan',
  'fan.off': 'uit',
  'fan.playing': 'speelt',
  'fan.paused': 'pauze',
  'fan.unsupported': 'beeldformaat onbekend',
  'fan.legacy': 'oud beeldformaat',
  'fan.lastError': 'Laatste melding van de fan',
  'fan.power': 'Aan/uit',
  'fan.turnOff': 'Uitzetten',
  'fan.turnOn': 'Aanzetten',
  'fan.play': 'Afspelen',
  'fan.pause': 'Pauze',
  'fan.display': 'Beeld',
  'fan.brightness': 'Helderheid',
  'fan.angle': 'Hoek',
  'fan.color': 'Kleur',
  'fan.colorMode': 'Kleurmodus',
  'fan.trueColor': 'True color',
  'fan.highBrightness': 'Hoge helderheid',
  'fan.colorNotReported': 'niet gemeld',
  'fan.playback': 'Afspelen',
  'fan.behavior': 'Gedrag',
  'fan.ringHint': 'Sleep de buitenrand om het beeld te draaien.',
  'strip.title': 'Nu op de fan',
  'strip.add': 'Toevoegen',
  'strip.show': '{name} tonen',
  'fan.repeat': 'Herhalen',
  'fan.repeatAll': 'Hele lijst',
  'fan.repeatOne': 'Huidig',
  'fan.breath': 'Ademlicht',
  'fan.autostart': 'Automatisch starten bij aanzetten',
  'fan.master': 'Groeps-master',
  'fan.volume': 'Volume',
  'fan.volumeHint': 'Volume werkt alleen op fans met een speaker.',
  'fan.maintenance': 'Onderhoud',
  'fan.formatTitle': 'Geheugenkaart formatteren?',
  'fan.format': 'Formatteren',
  'fan.formatBody': 'Alle bestanden op de fan worden gewist. Dit kan niet ongedaan worden gemaakt.',
  'fan.resetTitle': 'Fabrieksinstellingen herstellen?',
  'fan.resetAction': 'Herstellen',
  'fan.resetBody': 'Naam, helderheid, hoek en andere instellingen gaan terug naar de standaard.',
  'fan.factory': 'Fabrieksinstellingen',

  'playlist.title': { one: 'Afspeellijst · {count} bestand', other: 'Afspeellijst · {count} bestanden' },
  'playlist.empty': 'De afspeellijst is leeg. Voeg iets toe via {media}.',
  'playlist.playing': 'speelt',
  'playlist.show': 'Tonen',
  'playlist.rename': 'Hernoemen',
  'playlist.deleteTitle': '"{name}" wissen?',
  'playlist.delete': 'Wissen',
  'playlist.deleteBody': 'Het bestand wordt van de fan verwijderd.',
  'playlist.renameTitle': 'Slot {slot} hernoemen',

  'media.placement': 'Positie & schaal',
  'media.horizontal': 'Horizontaal',
  'media.vertical': 'Verticaal',
  'media.scale': 'Schaal',
  'media.mirror': 'Spiegelen (fan van achteren bekeken)',
  'media.rotate': 'Draaiing',
  'media.motion': 'Beweging',
  'media.still': 'Stilstaand',
  'media.scroll': 'Lichtkrant',
  'media.orbit': 'Rondgaand',
  'media.speed': 'Snelheid',
  'media.withSound': 'met geluid ({s} s)',
  'media.sendSound': 'Geluid meesturen (voor een speaker op de fan)',
  'media.rotateBy90': '90° rechtsom draaien',
  'media.textName': 'tekst',
  'media.logText': 'tekst: {text}',
  'media.textFailed': 'Tekst maken mislukt',
  'media.openFailed': 'Kan bestand niet openen',
  'media.videoPicked': 'Video gekozen',
  'media.videoPickedHint': 'Kies frames/s en eventueel begin en einde, en klik op "{button}".',
  'media.reading': 'inlezen…',
  'media.logMedia': { one: 'media: {name} ({count} frame, {size})', other: 'media: {name} ({count} frames, {size})' },
  'media.makingFrames': 'frames maken…',
  'media.stopped': 'Gestopt',
  'media.logVideo': 'video: {name} → {count} frames bij {fps} fps',
  'media.framesMade': { one: '{count} frame gemaakt', other: '{count} frames gemaakt' },
  'media.videoFailed': 'Video lezen mislukt',
  'media.preparing': 'voorbereiden…',
  'media.makingText': 'tekst maken…',
  'media.uploaded': 'Geüpload',
  'media.logUpload': 'upload {result}',
  'media.uploadFailed': 'Upload mislukt',
  'media.logUploadError': 'upload fout: {error}',
  'media.stageTranscoding': 'transcoderen…',
  'media.stageSending': 'versturen…',
  'media.pick': 'Foto, GIF of video kiezen…',
  'media.testCardName': 'testkaart',
  'media.textPlaceholder': 'of typ een tekst…',
  'media.textColor': 'Tekstkleur',
  'media.background': 'Achtergrond',
  'media.text': 'Tekst',
  'media.info': { one: '{name} · {count} frame', other: '{name} · {count} frames' },
  'media.infoVideo': '{name} · video, nog geen frames',
  'media.infoTestCard': 'Testkaart (rode, groene en blauwe ring)',
  'media.convert': 'Video omzetten naar frames',
  'media.start': 'Begin (s)',
  'media.end': 'Einde (s, 0 = eind)',
  'media.makeFrames': 'Frames maken',
  'media.toFan': 'Naar de fan',
  'media.nameOnFan': 'Naam op de fan',
  'media.fileName': 'bestandsnaam',
  'media.showSlot': 'Toon slot',
  'media.upload': 'Uploaden & tonen',
  'media.connectFirst': 'Verbind eerst een fan.',
  'media.preview': 'zo ziet de fan het · {size} · JPEG {kb} kB · kwaliteit {q}',
  'media.previewLoading': 'voorbeeld laden…',
  'media.frame': 'frame',
  'media.layoutTitle': 'Het JPEG-beeld zoals de fan het ontvangt (armen naast elkaar)',

  'cast.pickMediaFirst': 'Kies eerst media op het tabblad Media',
  'cast.pickVideoFirst': 'Kies eerst een video op het tabblad Media',
  'cast.logStarted': 'cast gestart: {src}',
  'cast.logStopped': 'cast gestopt',
  'cast.noCamera': 'camera niet beschikbaar in deze webview',
  'cast.failed': 'Casten lukt niet',
  'cast.frameFailed': 'Frame niet verstuurd, casten gestopt',
  'cast.noFrames': 'De camera of video geeft geen beeld (5 s zonder frame)',
  'cast.srcMedia': 'Gekozen media',
  'cast.srcVideo': 'Video (live)',
  'cast.srcCamera': 'Camera',
  'cast.srcScreen': 'Dit scherm',
  'cast.usingMedia': { one: 'Gebruikt: {name} ({count} frame)', other: 'Gebruikt: {name} ({count} frames)' },
  'cast.pickMediaHint': 'Kies eerst een foto/GIF/video op het tabblad Media.',
  'cast.videoHint': 'Speelt {name} live af en stuurt elk frame.',
  'cast.pickVideoHint': 'Kies eerst een video op het tabblad Media.',
  'cast.cameraHint': 'Gebruikt de camera van dit apparaat (achtercamera op telefoons).',
  'cast.screenHint': 'Spiegelt je hoofdscherm naar de fan.',
  'cast.testHint': 'Rode, groene en blauwe ring met een witte lijn naar 12 uur.',
  'cast.start': 'Start casten',
  'cast.live': 'Live aan het casten',
  'cast.toFans': { one: 'naar {count} fan', other: 'naar {count} fans' },
  'cast.stats': { one: '{fps} fps · {count} fan · JPEG {kb} kB · q{q}', other: '{fps} fps · {count} fans · JPEG {kb} kB · q{q}' },
  'cast.starting': 'starten…',
  'cast.stopped': 'gestopt',

  'net.deviceName': 'Apparaatnaam',
  'net.nameSaved': 'Naam opgeslagen',
  'net.bluetooth': 'Bluetooth van de fan',
  'net.btOn': 'Bluetooth aan',
  'net.btOff': 'Bluetooth uit',
  'net.on': 'Aan',
  'net.off': 'Uit',
  'net.scan': 'Zoeken',
  'net.disconnect': 'Loskoppelen',
  'net.connected': 'verbonden',
  'net.connecting': 'verbinden…',
  'net.notConnected': 'niet verbonden',
  'net.connect': 'Verbinden',
  'net.noDevices': 'Nog geen apparaten. Zet bluetooth aan en klik op {scan}.',
  'net.hotspot': 'Eigen wifi van de fan (hotspot)',
  'net.hotspotWarning': 'Daarna werken de standaardnaam en het standaardwachtwoord niet meer. Noteer de nieuwe gegevens. Nu: {current}',
  'net.wifiName': 'wifi-naam',
  'net.passwordMin': 'wachtwoord (min. 8 tekens)',
  'net.hotspotTitle': 'Hotspot wijzigen naar "{ssid}"?',
  'net.change': 'Wijzigen',
  'net.hotspotBody': 'Daarna verbind je met wifi {ssid} en wachtwoord {pw}. De huidige verbinding valt weg.',
  'net.changeHotspot': 'Hotspot wijzigen',
  'net.router': 'Verbinden met je router',
  'net.routerHint': 'De fan zoekt daarna zelf je router. Zet dit apparaat op hetzelfde netwerk. Nu: {current}',
  'net.none': 'geen',
  'net.routerName': 'router-naam (SSID)',
  'net.password': 'wachtwoord',
  'net.setRouter': 'Router instellen',
  'net.routerSet': 'Router ingesteld',

  'app.fanType': 'Fantype',
  'app.auto': 'Automatisch (wat de fan meldt)',
  'app.unsupported': 'niet ondersteund',
  'app.legacy': 'oud formaat',
  'app.fanTypeHint': 'Bepaalt resolutie en armindeling van het beeld. De P56 is type 8.',
  'app.connection': 'Verbinding',
  'app.ipLabel': 'IP dat wordt uitgezonden (leeg = automatisch per netwerkkaart)',
  'app.example': 'bv. {value}',
  'app.targetsLabel': "IP van de fan, als je het weet (komma's tussen meerdere)",
  'app.apply': 'Opslaan en opnieuw verbinden',
  'app.reconnected': 'Opnieuw verbonden',
  'app.broadcastingAs': 'uitzenden als {ip}',
  'app.tcpPort': 'TCP-poort {port}',
  'app.adapter': 'netwerkkaart {name}',
  'app.seenUdp': 'fans gezien via UDP: {list}',
  'app.noneUdp': 'nog geen fan gezien via UDP',
  'app.log': 'Logboek',
  'app.serverTitle': 'Bedienen via een P56-server',
  'app.modeLocal': 'Deze app bedient de fan',
  'app.modeServer': 'Via een P56-server',
  'app.serverAddress': 'Adres van de server',
  'app.serverTest': 'Testen',
  'app.serverUse': 'Deze server gebruiken',
  'app.serverOk': 'Op dit adres antwoordt een P56-server.',
  'app.serverMissing': 'Op dit adres antwoordt geen P56-server. Draait hij met --lan?',
  'app.serverActive': 'Deze app bedient de fans via {url}.',
  'app.serverHint': 'Een fan houdt één verbinding. Via een p56-server (of de Home Assistant-add-on) kunnen meerdere telefoons en computers hem tegelijk bedienen. Start de server met --lan en een API-token om hem vanaf andere apparaten te bereiken.',
  'app.browserDemo': 'browser (demo-fan)',
};

const dicts: Record<Lang, Record<Key, Entry>> = { en, nl };
export const LANG_NAMES: Record<Lang, string> = { en: 'English', nl: 'Nederlands' };

type Vars = Record<string, string | number>;

function pick(lang: Lang, key: Key, count: unknown): string {
  const e = dicts[lang][key] ?? en[key];
  if (typeof e === 'string') return e;
  return count === 1 ? e.one : e.other;
}

/** Translate `key` into `lang`, filling `{name}` placeholders. Pass `count` for plural entries. */
export function translate(lang: Lang, key: Key, vars?: Vars): string {
  const s = pick(lang, key, vars?.count);
  return vars ? s.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m)) : s;
}

// ---- current language (module level, so non-React code can translate too) ----

const STORAGE_KEY = 'p56.lang';
const isLang = (v: unknown): v is Lang => v === 'en' || v === 'nl';

function initialLang(): Lang {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (isLang(v)) return v;
  } catch { /* storage unavailable */ }
  try {
    return (navigator.language || '').toLowerCase().startsWith('nl') ? 'nl' : 'en';
  } catch { return 'en'; }
}

let current: Lang = initialLang();
const subscribers = new Set<() => void>();
const applyHtmlLang = () => { try { document.documentElement.lang = current; } catch { /* no DOM */ } };
applyHtmlLang();

export const getLang = (): Lang => current;

export function setLang(lang: Lang) {
  if (!isLang(lang) || lang === current) return;
  current = lang;
  try { localStorage.setItem(STORAGE_KEY, lang); } catch { /* private mode */ }
  applyHtmlLang();
  subscribers.forEach((f) => f());
}

function subscribe(f: () => void) {
  subscribers.add(f);
  return () => { subscribers.delete(f); };
}

/** Translate into the current language (for code outside React render, e.g. event handlers). */
export const t = (key: Key, vars?: Vars): string => translate(current, key, vars);

// ---- React ----

export type TFunc = (key: Key, vars?: Vars) => string;
type I18n = { lang: Lang; setLang: (l: Lang) => void; t: TFunc; rich: (key: Key, vars: Record<string, ReactNode>) => ReactNode };

function make(lang: Lang): I18n {
  const tt: TFunc = (key, vars) => translate(lang, key, vars);
  /** Like `t`, but placeholder values may be React nodes (bold text, code, …). */
  const rich = (key: Key, vars: Record<string, ReactNode>): ReactNode => {
    const parts = pick(lang, key, vars.count).split(/\{(\w+)\}/g);
    return parts.map((p, i) => <Fragment key={i}>{i % 2 ? (p in vars ? vars[p] : `{${p}}`) : p}</Fragment>);
  };
  return { lang, setLang, t: tt, rich };
}

const I18nContext = createContext<I18n | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const lang = useSyncExternalStore(subscribe, getLang, getLang);
  const value = useMemo(() => make(lang), [lang]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

/** `{ t, rich, lang, setLang }` for the current language; re-renders when it changes. */
export function useT(): I18n {
  const ctx = useContext(I18nContext);
  const lang = useSyncExternalStore(subscribe, getLang, getLang);
  const fallback = useMemo(() => make(lang), [lang]);
  return ctx ?? fallback;
}

/** Compact NL / EN switch: two words, the active one lit. */
export function LangSwitch({ className, testId = 'lang' }: { className?: string; testId?: string }) {
  const { lang, setLang: set, t: tt } = useT();
  const onKey = useCallback((l: Lang) => () => set(l), [set]);
  return (
    <div role="group" aria-label={tt('lang.label')} data-testid={`${testId}-switch`}
      className={'inline-flex font-display text-[10px] tracking-[0.14em] ' + (className ?? '')}>
      {LANGS.map((l) => (
        <button key={l} type="button" lang={l} title={LANG_NAMES[l]} aria-pressed={lang === l} data-testid={`${testId}-${l}`} onClick={onKey(l)}
          className={'min-h-11 min-w-10 uppercase transition-colors ' + (lang === l ? 'text-primary' : 'text-muted-foreground hover:text-foreground')}>
          {l}
        </button>
      ))}
    </div>
  );
}
