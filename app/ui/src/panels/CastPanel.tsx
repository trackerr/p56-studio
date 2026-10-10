import { useEffect, useRef, useState } from 'react';
import { Camera, Monitor, Image as ImageIcon, FlaskConical, Film, Square, Radio } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { RoundView, Section, ValueSlider } from '@/components/bits';
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

  useEffect(() => () => { live.current = false; invoke('cast_stop').catch(() => {}); }, []);

  async function start() {
    if (!devices.length) { toast.error(t('common.noFan')); return; }
    try {
      if (src === 'test' || src === 'screen' || src === 'media') {
        if (src === 'media' && !media) { toast.error(t('cast.pickMediaFirst')); return; }
        await invoke('cast_start', { args: { source: src === 'media' ? `media:${media!.id}` : src, fps, placement } });
        setRunning(true);
        addLog(t('cast.logStarted', { src }));
        return;
      }
      // frontend-driven sources
      const v = vidRef.current!;
      if (src === 'camera') {
        if (!navigator.mediaDevices?.getUserMedia) throw new Error(t('cast.noCamera'));
        v.srcObject = await navigator.mediaDevices.getUserMedia({ video: { facingMode: 'environment', width: { ideal: 1280 } }, audio: false });
      } else {
        const p = picked;
        if (!p?.video) { toast.error(t('cast.pickVideoFirst')); return; }
        v.srcObject = null;
        v.src = URL.createObjectURL(new Blob([p.bytes as BlobPart], { type: p.mime }));
        v.loop = true;
      }
      await v.play();
      live.current = true;
      setRunning(true);
      addLog(t('cast.logStarted', { src }));
      loop(v);
    } catch (e) {
      toast.error(t('cast.failed'), { description: String(e) });
      stop();
    }
  }

  async function loop(v: HTMLVideoElement) {
    while (live.current) {
      const t0 = performance.now();
      const b64 = grab(v, canvas.current);
      if (b64) {
        try { await invoke('cast_frame', { data: b64, placement: placementRef.current }); }
        catch (e) { toast.error(t('cast.frameFailed'), { description: String(e) }); break; }
      }
      const wait = 1000 / fps - (performance.now() - t0);
      if (wait > 0) await new Promise((r) => setTimeout(r, wait));
    }
  }

  function stop() {
    live.current = false;
    invoke('cast_stop').catch(() => {});
    const v = vidRef.current;
    if (v) {
      (v.srcObject as MediaStream | null)?.getTracks().forEach((track) => track.stop());
      v.pause(); v.srcObject = null;
      if (v.src.startsWith('blob:')) { URL.revokeObjectURL(v.src); v.removeAttribute('src'); }
    }
    setRunning(false);
    addLog(t('cast.logStopped'));
  }

  const items: [Src, string, React.ComponentType<{ className?: string }>, boolean][] = [
    ['test', t('common.testCard'), FlaskConical, true],
    ['media', t('cast.srcMedia'), ImageIcon, true],
    ['video', t('cast.srcVideo'), Film, true],
    ['camera', t('cast.srcCamera'), Camera, true],
    ['screen', t('cast.srcScreen'), Monitor, platform.screen],
  ];
  return (
    <div className="grid gap-4 lg:grid-cols-[1fr_360px]">
      <div className="space-y-4">
        <Section title={t('common.source')}>
          <ToggleGroup type="single" variant="outline" value={src} onValueChange={(v) => v && !running && setSrc(v as Src)} className="flex-wrap justify-start">
            {items.filter(([, , , ok]) => ok).map(([v, l, Icon]) => (
              <ToggleGroupItem key={v} value={v} data-testid={`src-${v}`} disabled={running}><Icon className="mr-1.5 h-4 w-4" />{l}</ToggleGroupItem>
            ))}
          </ToggleGroup>
          <p className="mt-2 text-xs text-muted-foreground">
            {src === 'media' && (media ? t('cast.usingMedia', { name: media.name, count: media.frames }) : t('cast.pickMediaHint'))}
            {src === 'video' && (picked?.video ? t('cast.videoHint', { name: picked.name }) : t('cast.pickVideoHint'))}
            {src === 'camera' && t('cast.cameraHint')}
            {src === 'screen' && t('cast.screenHint')}
            {src === 'test' && t('cast.testHint')}
          </p>
          <ValueSlider label={t('common.fps')} value={fps} min={1} max={25} onCommit={setFps} disabled={running} />
          <div className="mt-2 flex items-center gap-2">
            {!running
              ? <Button onClick={start} data-testid="cast-start"><Radio className="mr-1.5 h-4 w-4" />{t('cast.start')}</Button>
              : <Button variant="destructive" onClick={stop} data-testid="cast-stop"><Square className="mr-1.5 h-4 w-4" />{t('common.stop')}</Button>}
            <span className="text-xs text-muted-foreground">{t('cast.toFans', { count: devices.length })}</span>
          </div>
        </Section>
        <PlacementBox />
        <video ref={vidRef} muted playsInline className="hidden" />
      </div>
      <div className="lg:sticky lg:top-4 lg:self-start">
        <RoundView src={pv} spinning={running}
          label={<span data-testid="cast-stats" data-fps={stats?.fps ?? ''}>{stats ? t('cast.stats', { fps: stats.fps, count: stats.fans, kb: (stats.jpeg / 1024).toFixed(1), q: stats.q }) : running ? t('cast.starting') : t('cast.stopped')}</span>} />
      </div>
    </div>
  );
}
