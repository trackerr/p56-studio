import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { IconCamera, IconCast, IconImage, IconMonitor, IconRings, IconStop, type Icon } from '@/components/icons';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Choice, RoundView, Section, ValueSlider } from '@/components/bits';
import { grab, invoke, isTauri, onEvent } from '@/lib/api';
import { useT } from '@/lib/i18n';
import { useP56 } from '@/state';
import { PlacementBox } from './MediaPanel';

type Src = 'test' | 'media' | 'screen' | 'display' | 'camera' | 'ha';
type HaCamera = { entity_id: string; name: string };
type DesktopWindow = { id: number; title: string; app: string };

// this device's camera: only where the browser offers it (not on a plain-http page, such as
// Home Assistant opened without https)
// The camera of this device; not in the Home Assistant panel, where the cameras are Home
// Assistant's own (the device looking at the panel is not the point there).
const inHaPanel = !isTauri && location.pathname.includes('/hassio_ingress/');
const hasCamera = () => !inHaPanel && !!navigator.mediaDevices?.getUserMedia;
// Screen sharing through the webview's own picker (a screen or window): for the P56 Studio app
// when it controls the fan through a P56 server or Home Assistant, whose engine cannot capture
// this screen. Not in a plain browser, such as the Home Assistant panel.
const hasDisplay = () => isTauri && !!navigator.mediaDevices?.getDisplayMedia;

export function CastPanel() {
  const { devices, media, placement, platform, addLog } = useP56();
  const { t } = useT();
  const [src, setSrc] = useState<Src>('test');
  // 25 looked smoother than 15 or 20 on a real P56, and the network keeps up (measured)
  const [fps, setFps] = useState(25);
  const [running, setRunning] = useState(false);
  const [stats, setStats] = useState<{ fps: number; fans: number; jpeg: number; q: number } | null>(null);
  const [pv, setPv] = useState<string | null>(null);
  const live = useRef(false);
  const vidRef = useRef<HTMLVideoElement>(null);
  const canvas = useRef(document.createElement('canvas'));
  const placementRef = useRef(placement);
  placementRef.current = placement;
  // camera entities of Home Assistant (the add-on); null where there is no Home Assistant
  const [haCams, setHaCams] = useState<HaCamera[] | null>(null);
  const [haCam, setHaCam] = useState('');
  // "screen" = the whole screen, else the id of one window
  const [wins, setWins] = useState<DesktopWindow[]>([]);
  const [win, setWin] = useState('screen');
  const loadWins = () => invoke<DesktopWindow[]>('windows').then((w) => setWins(w ?? [])).catch(() => setWins([]));
  useEffect(() => { if (src === 'screen') loadWins(); }, [src]);
  useEffect(() => {
    // in the Home Assistant panel ask again while Home Assistant is still starting (up to 30 s)
    let tries = inHaPanel ? 6 : 1;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const load = () => invoke<HaCamera[]>('ha_cameras')
      .then((c) => { setHaCams(c); setHaCam((cur) => cur || c[0]?.entity_id || ''); })
      .catch(() => { setHaCams(null); if (--tries > 0) timer = setTimeout(load, 5000); });
    load();
    return () => clearTimeout(timer);
  }, []);

  useEffect(() => onEvent((e) => {
    if (e.type !== 'cast') return;
    if (e.data.stopped) { if (!live.current) setRunning(false); return; }
    setStats(e.data);
    if (e.data.preview) setPv(e.data.preview);
  }), []);

  // The camera stream lives in a ref so every exit path (stop, unmount, frame error, failed
  // start) releases it. `session` invalidates pending async work.
  const streamRef = useRef<MediaStream | null>(null);
  const videoEl = useRef<HTMLVideoElement | null>(null);
  const session = useRef(0);

  /** Stop the camera tracks and detach them from the <video>. */
  function releaseMedia() {
    streamRef.current?.getTracks().forEach((track) => track.stop());
    streamRef.current = null;
    const v = videoEl.current;
    if (v) {
      v.pause();
      v.srcObject = null;
    }
  }

  /** End the current cast, whatever its source: frame loop, media, backend. Safe to call twice. */
  function cleanup() {
    live.current = false;
    session.current++;
    releaseMedia();
    invoke('cast_stop').catch(() => {});
  }
  const cleanupRef = useRef(cleanup);
  cleanupRef.current = cleanup;
  useEffect(() => () => cleanupRef.current(), []);

  function fail(title: string, e: unknown) {
    cleanup();
    setRunning(false);
    toast.error(title, { description: String(e) });
  }

  async function start() {
    if (!devices.length) { toast.error(t('common.noFan')); return; }
    if (src === 'test' || src === 'screen' || src === 'media' || src === 'ha') {
      if (src === 'media' && !media) { toast.error(t('cast.pickMediaFirst')); return; }
      if (src === 'ha' && !haCam) { toast.error(t('cast.pickHaCamera')); return; }
      const source = src === 'media' ? `media:${media!.id}` : src === 'ha' ? `ha_camera:${haCam}` : src === 'screen' && win !== 'screen' ? `window:${win}` : src;
      try {
        await invoke('cast_start', { args: { source, fps, placement } });
        setRunning(true);
        addLog(t('cast.logStarted', { src }));
      } catch (e) { fail(t('cast.failed'), e); }
      return;
    }
    // frontend-driven: this device's camera
    releaseMedia();
    const id = ++session.current;
    const v = vidRef.current!;
    videoEl.current = v;
    try {
      let stream: MediaStream;
      if (src === 'display') {
        if (!hasDisplay()) throw new Error(t('cast.noDisplay'));
        stream = await navigator.mediaDevices.getDisplayMedia({ video: { frameRate: 15 }, audio: false });
      } else {
        if (!hasCamera()) throw new Error(t('cast.noCamera'));
        stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: 'environment', width: { ideal: 1280 } }, audio: false });
      }
      // Stopped, restarted or unmounted while the permission prompt was open: drop this stream.
      if (id !== session.current) { stream.getTracks().forEach((track) => track.stop()); return; }
      streamRef.current = stream;
      // sharing stopped from the browser's own bar: stop casting as well
      stream.getVideoTracks()[0]?.addEventListener('ended', () => { if (id === session.current) stop(); });
      v.srcObject = stream;
      await v.play();
      if (id !== session.current) return;
      live.current = true;
      setRunning(true);
      addLog(t('cast.logStarted', { src }));
      loop(v, id);
    } catch (e) {
      if (id === session.current) fail(t('cast.failed'), e);
    }
  }

  async function loop(v: HTMLVideoElement, id: number) {
    let lastFrame = performance.now();
    while (live.current && id === session.current) {
      const t0 = performance.now();
      const b64 = grab(v, canvas.current);
      // a camera that gives no picture would otherwise cast nothing without a word
      if (!b64 && t0 - lastFrame > 5000) { fail(t('cast.failed'), t('cast.noFrames')); return; }
      if (b64) {
        lastFrame = t0;
        try { await invoke('cast_frame', { data: b64, placement: placementRef.current }); }
        catch (e) {
          if (id === session.current) fail(t('cast.frameFailed'), e);
          return;
        }
      }
      const wait = 1000 / fps - (performance.now() - t0);
      if (wait > 0) await new Promise((r) => setTimeout(r, wait));
    }
  }

  function stop() {
    cleanup();
    setRunning(false);
    addLog(t('cast.logStopped'));
  }

  const items: [Src, string, Icon, boolean][] = [
    ['test', t('common.testCard'), IconRings, true],
    ['media', t('cast.srcMedia'), IconImage, true],
    ['camera', t('cast.srcCamera'), IconCamera, hasCamera()],
    ['ha', t('cast.srcHa'), IconCamera, haCams !== null],
    ['screen', t('cast.srcScreen'), IconMonitor, platform.screen],
    ['display', t('cast.srcScreen'), IconMonitor, !platform.screen && hasDisplay()],
  ];
  return (
    <div className="grid gap-10 lg:grid-cols-[minmax(0,1fr)_380px] lg:gap-12">
      <div className="min-w-0 space-y-8">
        <Section title={t('common.source')} className="border-t-0 pt-0">
          <Choice label={t('common.source')} value={src} disabled={running} className="flex-col sm:flex-row"
            onChange={(v) => !running && setSrc(v)}
            options={items.filter(([, , , ok]) => ok).map(([v, l, I]) => ({ value: v, label: l, icon: I, testId: `src-${v}` }))} />
          <p className="mt-2 text-xs text-muted-foreground">
            {src === 'media' && (media ? t('cast.usingMedia', { name: media.name, count: media.frames }) : t('cast.pickMediaHint'))}
            {src === 'ha' && (haCams?.length ? t('cast.haHint') : t('cast.noHaCameras'))}
            {src === 'camera' && t('cast.cameraHint')}
            {src === 'screen' && t('cast.screenHint')}
            {src === 'display' && t('cast.displayHint')}
            {src === 'test' && t('cast.testHint')}
          </p>
          {src === 'screen' && (
            <div className="mt-3 max-w-sm">
              <Select value={win} onValueChange={setWin} onOpenChange={(o) => o && loadWins()} disabled={running}>
                <SelectTrigger data-testid="cast-window" aria-label={t('cast.window')}><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="screen">{t('cast.wholeScreen')}</SelectItem>
                  {wins.map((w) => <SelectItem key={w.id} value={String(w.id)}>{w.app && !w.title.includes(w.app) ? `${w.title} · ${w.app}` : w.title}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
          )}
          {src === 'ha' && !!haCams?.length && (
            <div className="mt-3 max-w-sm">
              <Select value={haCam} onValueChange={setHaCam} disabled={running}>
                <SelectTrigger data-testid="ha-camera" aria-label={t('cast.srcHa')}><SelectValue /></SelectTrigger>
                <SelectContent>
                  {haCams.map((c) => <SelectItem key={c.entity_id} value={c.entity_id}>{c.name}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
          )}
          <div className="mt-4"><ValueSlider label={t('common.fps')} value={fps} min={1} max={25} onCommit={setFps} disabled={running} /></div>
          <div className="mt-6 flex items-center gap-5">
            {!running
              ? <Button size="round" onClick={start} data-testid="cast-start" aria-label={t('cast.start')}><IconCast className="h-6 w-6" /></Button>
              : <Button size="round" variant="destructive" onClick={stop} data-testid="cast-stop" aria-label={t('common.stop')}><IconStop className="h-6 w-6" /></Button>}
            <div className="space-y-1">
              <div className="orbit-label text-primary">{running ? t('cast.live') : t('cast.start')}</div>
              <div className="text-xs text-muted-foreground">{t('cast.toFans', { count: devices.length })}</div>
            </div>
          </div>
        </Section>
        <PlacementBox />
        {/* Never display:none: Android's WebView then decodes no frames (the camera turns on, but
            the picture stays empty). Shown small while a camera or video is cast. */}
        <video ref={vidRef} muted playsInline data-testid="cast-video"
          className={running && (src === 'camera' || src === 'display') ? 'aspect-square w-32 rounded-full border border-border object-cover' : 'pointer-events-none fixed left-0 top-0 h-px w-px opacity-[0.01]'} />
      </div>
      <div className="min-w-0 lg:sticky lg:top-0 lg:self-start">
        <RoundView src={pv} spinning={running}
          label={<span data-testid="cast-stats" data-fps={stats?.fps ?? ''}>{stats ? t('cast.stats', { fps: stats.fps, count: stats.fans, kb: (stats.jpeg / 1024).toFixed(1), q: stats.q }) : running ? t('cast.starting') : t('cast.stopped')}</span>} />
      </div>
    </div>
  );
}
