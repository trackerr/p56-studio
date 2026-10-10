import { useEffect, useRef, useState } from 'react';
import { FileImage, FlaskConical, Upload, Film, X, Type } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Progress } from '@/components/ui/progress';
import { Switch } from '@/components/ui/switch';
import { Slider } from '@/components/ui/slider';
import { RoundView, Section, ValueSlider } from '@/components/bits';
import { decodeVideo, invoke, onEvent, pickMedia, toBase64, type MediaInfo, type Picked, type Preview } from '@/lib/api';
import { useP56 } from '@/state';

export function PlacementBox() {
  const { placement: p, setPlacement } = useP56();
  return (
    <Section title="Positie & schaal" aside={<Button size="sm" variant="ghost" onClick={() => setPlacement({ x: 0, y: 0, scale: 1, mirror: false })}>Reset</Button>}>
      <ValueSlider label="Horizontaal" value={p.x} min={-0.5} max={0.5} step={0.01} format={(v) => v.toFixed(2)} onCommit={(x) => setPlacement({ ...p, x })} />
      <ValueSlider label="Verticaal" value={p.y} min={-0.5} max={0.5} step={0.01} format={(v) => v.toFixed(2)} onCommit={(y) => setPlacement({ ...p, y })} />
      <ValueSlider label="Schaal" value={p.scale} min={0.3} max={2} step={0.01} format={(v) => `${v.toFixed(2)}×`} onCommit={(scale) => setPlacement({ ...p, scale })} />
      <div className="flex items-center justify-between py-2">
        <Label htmlFor="sw-mirror" className="font-normal">Spiegelen (fan van achteren bekeken)</Label>
        <Switch id="sw-mirror" checked={p.mirror} onCheckedChange={(mirror) => setPlacement({ ...p, mirror })} />
      </div>
    </Section>
  );
}

export function MediaPanel() {
  const { fan, media, setMedia, placement, addLog, picked, setPicked } = useP56();
  const [busy, setBusy] = useState('');
  const [prog, setProg] = useState<number | null>(null);
  const [pv, setPv] = useState<Preview | null>(null);
  const [pvErr, setPvErr] = useState('');
  const [frame, setFrame] = useState(0);
  const [vid, setVid] = useState({ fps: 12, start: 0, end: 0, max: 300 });
  const [name, setName] = useState('');
  const [slot, setSlot] = useState(1);
  const [maxFrames, setMaxFrames] = useState(0);
  const cancel = useRef(false);
  const [txt, setTxt] = useState({ text: '', color: '#ffffff', bg: '#000000' });

  async function makeText() {
    try {
      const m = await invoke<MediaInfo>('media_text', { text: txt.text, color: txt.color, background: txt.bg, name: txt.text.slice(0, 24) });
      setPicked(null); setMedia(m); setFrame(0); setName(txt.text.slice(0, 24).replace(/[^\w -]/g, '').trim() || 'tekst');
      addLog(`tekst: ${txt.text}`);
    } catch (e) { toast.error('Tekst maken mislukt', { description: String(e) }); }
  }

  // preview on every change
  useEffect(() => {
    let alive = true;
    const t = setTimeout(async () => {
      try {
        const p = await invoke<Preview>('preview', { media: media?.id ?? 0, frame, placement, id: fan?.id ?? null });
        if (alive) { setPv(p); setPvErr(''); }
      } catch (e) { if (alive) setPvErr(String(e)); }
    }, 120);
    return () => { alive = false; clearTimeout(t); };
  }, [media?.id, media?.frames, frame, placement, fan?.id, fan?.status?.fan_type]);

  useEffect(() => onEvent((e) => {
    if (e.type === 'progress') {
      const { stage, done, total } = e.data;
      setBusy(stage === 'klaar' ? '' : `${stage}…`);
      setProg(total ? Math.round((done / total) * 100) : null);
    }
  }), []);

  // Test hook: lets the end-to-end test "pick" a file without the native dialog.
  useEffect(() => {
    (window as any).__p56pick = async (name: string, b64: string, mime: string) => {
      const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      await open({ name, bytes, mime, video: mime.startsWith('video') });
    };
  });

  async function pick() {
    try {
      const p = await pickMedia();
      if (p) await open(p);
    } catch (e) {
      toast.error('Kan bestand niet openen', { description: String(e) });
    }
  }

  async function open(p: Picked) {
    try {
      setPicked(p);
      setFrame(0);
      setName(p.name.replace(/\.[^.]+$/, ''));
      if (p.video) {
        setMedia(null);
        toast.info('Video gekozen', { description: 'Kies frames/s en eventueel begin en einde, en klik op "Frames maken".' });
        return;
      }
      setBusy('inlezen…');
      const m = await invoke<MediaInfo>('media_load', { name: p.name, data: await toBase64(p.bytes) });
      setMedia({ ...m, picked: p });
      addLog(`media: ${p.name} (${m.frames} frame(s), ${m.width}×${m.height})`);
    } catch (e) {
      toast.error('Kan bestand niet openen', { description: String(e) });
    } finally { setBusy(''); }
  }

  async function makeFrames() {
    if (!picked) return;
    cancel.current = false;
    setBusy('frames maken…'); setProg(0);
    try {
      const id = await invoke<number>('media_begin', { name: picked.name });
      const delay = Math.round(1000 / vid.fps);
      const total = await decodeVideo(picked, vid, async (b64, i, n) => {
        await invoke('media_push', { media: id, data: b64, delay });
        setProg(Math.round(((i + 1) / n) * 100));
      }, () => cancel.current);
      if (cancel.current) { await invoke('media_drop', { media: id }); toast.message('Gestopt'); return; }
      setMedia({ id, name: picked.name, frames: total, width: 0, height: 0, picked });
      addLog(`video: ${picked.name} → ${total} frames bij ${vid.fps} fps`);
      toast.success(`${total} frames gemaakt`);
    } catch (e) {
      toast.error('Video lezen mislukt', { description: String(e) });
    } finally { setBusy(''); setProg(null); }
  }

  async function doUpload() {
    if (!fan) { toast.error('Geen fan verbonden'); return; }
    setBusy('voorbereiden…'); setProg(0);
    try {
      const r = await invoke<string>('upload', { args: { id: fan.id, media: media?.id ?? 0, name: name || null, slot, placement, max_frames: maxFrames } });
      toast.success('Geüpload', { description: r });
      addLog(`upload ${r}`);
    } catch (e) {
      toast.error('Upload mislukt', { description: String(e) });
      addLog(`upload fout: ${e}`);
    } finally { setBusy(''); setProg(null); }
  }

  const n = media?.frames ?? 1;
  return (
    <div className="grid gap-4 lg:grid-cols-[1fr_360px]">
      <div className="space-y-4">
        <Section title="Bron">
          <div className="flex flex-wrap gap-2">
            <Button onClick={pick} disabled={!!busy} data-testid="pick"><FileImage className="mr-1.5 h-4 w-4" />Foto, GIF of video kiezen…</Button>
            <Button variant="secondary" disabled={!!busy} data-testid="testcard" onClick={() => { setPicked(null); setMedia(null); setName('testkaart'); setFrame(0); }}>
              <FlaskConical className="mr-1.5 h-4 w-4" />Testkaart
            </Button>
          </div>
          <form className="mt-3 flex flex-wrap items-center gap-2" onSubmit={(e) => { e.preventDefault(); makeText(); }}>
            <Input className="min-w-48 flex-1" placeholder="of typ een tekst…" value={txt.text} data-testid="text-input" onChange={(e) => setTxt({ ...txt, text: e.target.value })} />
            <input type="color" aria-label="Tekstkleur" value={txt.color} onChange={(e) => setTxt({ ...txt, color: e.target.value })} className="h-9 w-10 cursor-pointer rounded border border-input bg-transparent" />
            <input type="color" aria-label="Achtergrond" value={txt.bg} onChange={(e) => setTxt({ ...txt, bg: e.target.value })} className="h-9 w-10 cursor-pointer rounded border border-input bg-transparent" />
            <Button type="submit" variant="secondary" disabled={!txt.text.trim() || !!busy} data-testid="text-make"><Type className="mr-1.5 h-4 w-4" />Tekst</Button>
          </form>
          <p className="mt-2 text-sm text-muted-foreground" data-testid="media-info">
            {media ? <>{media.name} · {media.frames} frame(s){media.width ? ` · ${media.width}×${media.height}` : ''}</>
              : picked?.video ? <>{picked.name} · video, nog geen frames</> : 'Testkaart (rode, groene en blauwe ring)'}
          </p>
          {picked?.video && (
            <div className="mt-3 rounded-md border border-dashed border-border p-3">
              <div className="mb-2 flex items-center gap-2 text-sm"><Film className="h-4 w-4" />Video omzetten naar frames</div>
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                {([['fps', 'Frames/s', 1, 25, 1], ['start', 'Begin (s)', 0, 3600, 0.1], ['end', 'Einde (s, 0 = eind)', 0, 3600, 0.1], ['max', 'Max. frames', 1, 3000, 1]] as const).map(([k, l, mn, mx, st]) => (
                  <div key={k} className="space-y-1"><Label className="text-xs text-muted-foreground">{l}</Label>
                    <Input type="number" min={mn} max={mx} step={st} value={vid[k]} onChange={(e) => setVid({ ...vid, [k]: Number(e.target.value) })} /></div>
                ))}
              </div>
              <div className="mt-3 flex gap-2">
                <Button onClick={makeFrames} disabled={!!busy} data-testid="make-frames">Frames maken</Button>
                {busy === 'frames maken…' && <Button variant="ghost" onClick={() => (cancel.current = true)}><X className="mr-1 h-4 w-4" />Stop</Button>}
              </div>
            </div>
          )}
        </Section>

        <Section title="Naar de fan">
          <div className="grid gap-3 sm:grid-cols-[1fr_110px_130px]">
            <div className="space-y-1"><Label className="text-xs text-muted-foreground" htmlFor="up-name">Naam op de fan</Label>
              <Input id="up-name" data-testid="up-name" value={name} placeholder="bestandsnaam" onChange={(e) => setName(e.target.value)} /></div>
            <div className="space-y-1"><Label className="text-xs text-muted-foreground" htmlFor="up-slot">Toon slot</Label>
              <Input id="up-slot" type="number" min={1} max={255} value={slot} onChange={(e) => setSlot(Number(e.target.value) || 1)} /></div>
            <div className="space-y-1"><Label className="text-xs text-muted-foreground" htmlFor="up-max">Max. frames</Label>
              <Input id="up-max" type="number" min={0} value={maxFrames} onChange={(e) => setMaxFrames(Number(e.target.value) || 0)} /></div>
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <Button onClick={doUpload} disabled={!!busy || !fan || (picked?.video && !media)} data-testid="upload"><Upload className="mr-1.5 h-4 w-4" />Uploaden &amp; tonen</Button>
            {!fan && <span className="text-xs text-muted-foreground">Verbind eerst een fan.</span>}
            {busy && <span className="text-xs text-muted-foreground" data-testid="busy">{busy}</span>}
          </div>
          {prog != null && <Progress value={prog} className="mt-3 h-1.5" />}
        </Section>

        <PlacementBox />
      </div>

      <div className="space-y-3 lg:sticky lg:top-4 lg:self-start">
        <RoundView src={pv?.round} label={pvErr ? <span className="text-destructive">{pvErr}</span> :
          pv ? <>zo ziet de fan het · {pv.width}×{pv.height} · JPEG {(pv.jpeg_bytes / 1024).toFixed(1)} kB · kwaliteit {pv.quality}</> : 'voorbeeld laden…'} />
        {n > 1 && (
          <div className="flex items-center gap-3 px-2">
            <span className="text-xs text-muted-foreground">frame</span>
            <Slider value={[frame]} min={0} max={n - 1} step={1} onValueChange={([f]) => setFrame(f)} />
            <span className="w-16 text-right font-mono text-xs">{frame + 1}/{n}</span>
          </div>
        )}
        {pv?.layout && <img src={pv.layout} alt="" className="w-full rounded border border-border" title="Het JPEG-beeld zoals de fan het ontvangt (armen naast elkaar)" />}
      </div>
    </div>
  );
}
