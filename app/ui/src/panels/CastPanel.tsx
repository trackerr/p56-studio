import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { IconCamera, IconCast, IconFilm, IconImage, IconMonitor, IconRings, IconStop, type Icon } from '@/components/icons';
import { Choice, RoundView, Section, ValueSlider } from '@/components/bits';
import { grab, invoke, onEvent } from '@/lib/api';
import { useT } from '@/lib/i18n';
import { useP56 } from '@/state';
import { PlacementBox } from './MediaPanel';

type Src = 'test' | 'media' | 'screen' | 'camera' | 'video';

export function CastPanel() {
  const { devices, media, picked, placement, platform, addLog } = useP56();
  const { t } = useT();
  const [src, setSrc] = useState<Src>('test');
  const [fps, setFps] = useState(15);
  const [running, setRunning] = useState(false);
  const [stats, setStats] = useState<{ fps: number; fans: number; jpeg: number; q: number } | null>(null);
  const [pv, setPv] = useState<string | null>(null);
  const live = useRef(false);
  const vidRef = useRef<HTMLVideoElement>(null);
  const canvas = useRef(document.createElement('canvas'));
  const placementRef = useRef(placement);
  placementRef.current = placement;

  useEffect(() => onEvent((e) => {
    if (e.type !== 'cast') return;
    if (e.data.stopped) { if (!live.current) setRunning(false); return; }
    setStats(e.data);
    if (e.data.preview) setPv(e.data.preview);
  }), []);

  // Frontend-driven sources own a camera stream or a blob URL; both live in refs so every exit path
  // (stop, unmount, frame error, failed start) releases them. `session` invalidates pending async work.
  const streamRef = useRef<MediaStream | null>(null);
  const urlRef = useRef<string | null>(null);
  const videoEl = useRef<HTMLVideoElement | null>(null);
  const session = useRef(0);

  /** Stop the camera tracks, revoke the video URL and detach both from the <video>. */
  function releaseMedia() {
    streamRef.current?.getTracks().forEach((track) => track.stop());
    streamRef.current = null;
    const v = videoEl.current;
    if (v) {
      v.pause();
      v.srcObject = null;
      if (urlRef.current) v.removeAttribute('src');
    }
    if (urlRef.current) { URL.revokeObjectURL(urlRef.current); urlRef.current = null; }
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
    if (src === 'test' || src === 'screen' || src === 'media') {
      if (src === 'media' && !media) { toast.error(t('cast.pickMediaFirst')); return; }
      try {
        await invoke('cast_start', { args: { source: src === 'media' ? `media:${media!.id}` : src, fps, placement } });
        setRunning(true);
        addLog(t('cast.logStarted', { src }));
      } catch (e) { fail(t('cast.failed'), e); }
      return;
    }
    // frontend-driven sources
    if (src === 'video' && !picked?.video) { toast.error(t('cast.pickVideoFirst')); return; }
    releaseMedia();
    const id = ++session.current;
    const v = vidRef.current!;
    videoEl.current = v;
    try {
      if (src === 'camera') {
        if (!navigator.mediaDevices?.getUserMedia) throw new Error(t('cast.noCamera'));
        const stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: 'environment', width: { ideal: 1280 } }, audio: false });
        // Stopped, restarted or unmounted while the permission prompt was open: drop this stream.
        if (id !== session.current) { stream.getTracks().forEach((track) => track.stop()); return; }
        streamRef.current = stream;
        v.srcObject = stream;
      } else {
        const p = picked!;
        const url = URL.createObjectURL(new Blob([p.bytes as BlobPart], { type: p.mime }));
        urlRef.current = url;
        v.srcObject = null;
        v.src = url;
        v.loop = true;
      }
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
    while (live.current && id === session.current) {
      const t0 = performance.now();
      const b64 = grab(v, canvas.current);
      if (b64) {
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
    ['video', t('cast.srcVideo'), IconFilm, true],
    ['camera', t('cast.srcCamera'), IconCamera, true],
    ['screen', t('cast.srcScreen'), IconMonitor, platform.screen],
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
            {src === 'video' && (picked?.video ? t('cast.videoHint', { name: picked.name }) : t('cast.pickVideoHint'))}
            {src === 'camera' && t('cast.cameraHint')}
            {src === 'screen' && t('cast.screenHint')}
            {src === 'test' && t('cast.testHint')}
          </p>
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
        <video ref={vidRef} muted playsInline className="hidden" />
      </div>
      <div className="min-w-0 lg:sticky lg:top-0 lg:self-start">
        <RoundView src={pv} spinning={running}
          label={<span data-testid="cast-stats" data-fps={stats?.fps ?? ''}>{stats ? t('cast.stats', { fps: stats.fps, count: stats.fans, kb: (stats.jpeg / 1024).toFixed(1), q: stats.q }) : running ? t('cast.starting') : t('cast.stopped')}</span>} />
      </div>
    </div>
  );
}
