import { useEffect, useRef, useState } from 'react';
import { Camera, Monitor, Image as ImageIcon, FlaskConical, Film, Square, Radio } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { RoundView, Section, ValueSlider } from '@/components/bits';
import { grab, invoke, onEvent } from '@/lib/api';
import { useP56 } from '@/state';
import { PlacementBox } from './MediaPanel';

type Src = 'test' | 'media' | 'screen' | 'camera' | 'video';

export function CastPanel() {
  const { devices, media, picked, placement, platform, addLog } = useP56();
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
    if (!devices.length) { toast.error('Geen fan verbonden'); return; }
    try {
      if (src === 'test' || src === 'screen' || src === 'media') {
        if (src === 'media' && !media) { toast.error('Kies eerst media op het tabblad Media'); return; }
        await invoke('cast_start', { args: { source: src === 'media' ? `media:${media!.id}` : src, fps, placement } });
        setRunning(true);
        addLog(`cast gestart: ${src}`);
        return;
      }
      // frontend-driven sources
      const v = vidRef.current!;
      if (src === 'camera') {
        if (!navigator.mediaDevices?.getUserMedia) throw new Error('camera niet beschikbaar in deze webview');
        v.srcObject = await navigator.mediaDevices.getUserMedia({ video: { facingMode: 'environment', width: { ideal: 1280 } }, audio: false });
      } else {
        const p = picked;
        if (!p?.video) { toast.error('Kies eerst een video op het tabblad Media'); return; }
        v.srcObject = null;
        v.src = URL.createObjectURL(new Blob([p.bytes as BlobPart], { type: p.mime }));
        v.loop = true;
      }
      await v.play();
      live.current = true;
      setRunning(true);
      addLog(`cast gestart: ${src}`);
      loop(v);
    } catch (e) {
      toast.error('Casten lukt niet', { description: String(e) });
      stop();
    }
  }

  async function loop(v: HTMLVideoElement) {
    while (live.current) {
      const t = performance.now();
      const b64 = grab(v, canvas.current);
      if (b64) {
        try { await invoke('cast_frame', { data: b64, placement: placementRef.current }); }
        catch (e) { toast.error('Frame niet verstuurd', { description: String(e) }); break; }
      }
      const wait = 1000 / fps - (performance.now() - t);
      if (wait > 0) await new Promise((r) => setTimeout(r, wait));
    }
  }

  function stop() {
    live.current = false;
    invoke('cast_stop').catch(() => {});
    const v = vidRef.current;
    if (v) {
      (v.srcObject as MediaStream | null)?.getTracks().forEach((t) => t.stop());
      v.pause(); v.srcObject = null;
      if (v.src.startsWith('blob:')) { URL.revokeObjectURL(v.src); v.removeAttribute('src'); }
    }
    setRunning(false);
    addLog('cast gestopt');
  }

  const items: [Src, string, React.ComponentType<{ className?: string }>, boolean][] = [
    ['test', 'Testkaart', FlaskConical, true],
    ['media', 'Gekozen media', ImageIcon, true],
    ['video', 'Video (live)', Film, true],
    ['camera', 'Camera', Camera, true],
    ['screen', 'Dit scherm', Monitor, platform.screen],
  ];
  return (
    <div className="grid gap-4 lg:grid-cols-[1fr_360px]">
      <div className="space-y-4">
        <Section title="Bron">
          <ToggleGroup type="single" variant="outline" value={src} onValueChange={(v) => v && !running && setSrc(v as Src)} className="flex-wrap justify-start">
            {items.filter(([, , , ok]) => ok).map(([v, l, Icon]) => (
              <ToggleGroupItem key={v} value={v} data-testid={`src-${v}`} disabled={running}><Icon className="mr-1.5 h-4 w-4" />{l}</ToggleGroupItem>
            ))}
          </ToggleGroup>
          <p className="mt-2 text-xs text-muted-foreground">
            {src === 'media' && (media ? `Gebruikt: ${media.name} (${media.frames} frame(s))` : 'Kies eerst een foto/GIF/video op het tabblad Media.')}
            {src === 'video' && (picked?.video ? `Speelt ${picked.name} live af en stuurt elk frame.` : 'Kies eerst een video op het tabblad Media.')}
            {src === 'camera' && 'Gebruikt de camera van dit apparaat (achtercamera op telefoons).'}
            {src === 'screen' && 'Spiegelt je hoofdscherm naar de fan.'}
            {src === 'test' && 'Rode, groene en blauwe ring met een witte lijn naar 12 uur.'}
          </p>
          <ValueSlider label="Frames/s" value={fps} min={1} max={25} onCommit={setFps} disabled={running} />
          <div className="mt-2 flex items-center gap-2">
            {!running
              ? <Button onClick={start} data-testid="cast-start"><Radio className="mr-1.5 h-4 w-4" />Start casten</Button>
              : <Button variant="destructive" onClick={stop} data-testid="cast-stop"><Square className="mr-1.5 h-4 w-4" />Stop</Button>}
            <span className="text-xs text-muted-foreground">naar {devices.length} fan(s)</span>
          </div>
        </Section>
        <PlacementBox />
        <video ref={vidRef} muted playsInline className="hidden" />
      </div>
      <div className="lg:sticky lg:top-4 lg:self-start">
        <RoundView src={pv} spinning={running}
          label={<span data-testid="cast-stats">{stats ? `${stats.fps} fps · ${stats.fans} fan(s) · JPEG ${(stats.jpeg / 1024).toFixed(1)} kB · q${stats.q}` : running ? 'starten…' : 'gestopt'}</span>} />
      </div>
    </div>
  );
}
