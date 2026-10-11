import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { IconFilm, IconImage, IconRings, IconRotate, IconType, IconUpload, IconX } from '@/components/icons';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Progress } from '@/components/ui/progress';
import { Switch } from '@/components/ui/switch';
import { Slider } from '@/components/ui/slider';
import { Choice, RoundView, Section, ValueSlider } from '@/components/bits';

/** Text that moves: a ticker across the middle or round along the rim ('' = still). */
type Motion = '' | 'scroll' | 'orbit';
import { DEFAULT_PLACEMENT, decodeAudio, decodeVideo, invoke, onEvent, pickMedia, toBase64, type MediaInfo, type Picked, type Preview } from '@/lib/api';
import { useT, type Key, type TFunc } from '@/lib/i18n';
import { useP56 } from '@/state';

export function PlacementBox() {
  const { placement: p, setPlacement } = useP56();
  const { t } = useT();
  return (
    <Section title={t('media.placement')} aside={<Button size="sm" variant="ghost" onClick={() => setPlacement(DEFAULT_PLACEMENT)}>{t('common.reset')}</Button>}>
      <ValueSlider label={t('media.horizontal')} value={p.x} min={-0.5} max={0.5} step={0.01} format={(v) => v.toFixed(2)} onCommit={(x) => setPlacement({ ...p, x })} />
      <ValueSlider label={t('media.vertical')} value={p.y} min={-0.5} max={0.5} step={0.01} format={(v) => v.toFixed(2)} onCommit={(y) => setPlacement({ ...p, y })} />
      <ValueSlider label={t('media.scale')} value={p.scale} min={0.3} max={2} step={0.01} format={(v) => `${v.toFixed(2)}×`} onCommit={(scale) => setPlacement({ ...p, scale })} />
      <div className="flex items-center justify-between gap-3">
        <span>{t('media.rotate')}</span>
        <Button size="sm" variant="secondary" data-testid="rotate" aria-label={t('media.rotateBy90')} onClick={() => setPlacement({ ...p, rotate: ((p.rotate ?? 0) + 90) % 360 })}>
          <IconRotate />{p.rotate ?? 0}°
        </Button>
      </div>
      <div className="flex items-center justify-between gap-3">
        <Label htmlFor="sw-mirror">{t('media.mirror')}</Label>
        <Switch id="sw-mirror" checked={p.mirror} onCheckedChange={(mirror) => setPlacement({ ...p, mirror })} />
      </div>
    </Section>
  );
}

/** What the panel is busy with: a local step, or an upload stage reported by the backend. */
type Step = 'reading' | 'frames' | 'preparing' | 'text';
type Busy = null | { step: Step } | { stage: string };

// Upload stages as emitted by the backend; both the old Dutch and English ids are accepted.
const STAGES: Record<string, Key> = {
  transcoderen: 'media.stageTranscoding', transcoding: 'media.stageTranscoding', transcode: 'media.stageTranscoding', encoding: 'media.stageTranscoding',
  versturen: 'media.stageSending', sending: 'media.stageSending', send: 'media.stageSending', uploading: 'media.stageSending', upload: 'media.stageSending',
};
const STEPS: Record<Step, Key> = { reading: 'media.reading', frames: 'media.makingFrames', preparing: 'media.preparing', text: 'media.makingText' };

function busyLabel(b: Busy, t: TFunc): string {
  if (!b) return '';
  if ('step' in b) return t(STEPS[b.step]);
  const k = STAGES[b.stage.toLowerCase()];
  return k ? t(k) : `${b.stage}…`;
}

export function MediaPanel() {
  const { fan, media, setMedia, placement, addLog, picked, setPicked } = useP56();
  const { t } = useT();
  const [busy, setBusy] = useState<Busy>(null);
  const [prog, setProg] = useState<number | null>(null);
  const [pv, setPv] = useState<Preview | null>(null);
  const [pvErr, setPvErr] = useState('');
  const [frame, setFrame] = useState(0);
  const [vid, setVid] = useState({ fps: 12, start: 0, end: 0, max: 300 });
  const [name, setName] = useState('');
  const [slot, setSlot] = useState(1);
  const [maxFrames, setMaxFrames] = useState(0);
  const [sound, setSound] = useState(true);
  const cancel = useRef(false);
  const [txt, setTxt] = useState({ text: '', color: '#ffffff', bg: '#000000', motion: '' as Motion, speed: 1 });

  // the text settings the current text media was made from, so Upload can tell whether it is stale
  const madeText = useRef<{ id: number; key: string } | null>(null);
  const textKey = JSON.stringify(txt);

  async function makeText(): Promise<{ id: number; name: string } | null> {
    // a ticker takes a moment to render: uploading meanwhile would send the previous picture
    setBusy({ step: 'text' });
    try {
      const m = await invoke<MediaInfo>('media_text', { text: txt.text, color: txt.color, background: txt.bg, name: txt.text.slice(0, 24), motion: txt.motion, speed: txt.speed });
      const n = txt.text.slice(0, 24).replace(/[^\w -]/g, '').trim() || t('media.textName');
      setPicked(null); setMedia(m); setFrame(0); setName(n);
      madeText.current = { id: m.id, key: textKey };
      addLog(t('media.logText', { text: txt.text }));
      return { id: m.id, name: n };
    } catch (e) { toast.error(t('media.textFailed'), { description: String(e) }); return null; }
    finally { setBusy(null); }
  }

  // preview on every change
  useEffect(() => {
    let alive = true;
    const timer = setTimeout(async () => {
      try {
        const p = await invoke<Preview>('preview', { media: media?.id ?? 0, frame, placement, id: fan?.id ?? null });
        if (alive) { setPv(p); setPvErr(''); }
      } catch (e) { if (alive) setPvErr(String(e)); }
    }, 120);
    return () => { alive = false; clearTimeout(timer); };
  }, [media?.id, media?.frames, frame, placement, fan?.id, fan?.status?.fan_type]);

  useEffect(() => onEvent((e) => {
    if (e.type === 'progress') {
      const { stage, done, total } = e.data;
      // The final event has done == total; this does not depend on the stage name's language.
      const finished = total > 0 && done >= total;
      setBusy(finished ? null : { stage: String(stage ?? '') });
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
      toast.error(t('media.openFailed'), { description: String(e), testId: 'toast-open-failed' });
    }
  }

  async function open(p: Picked) {
    try {
      setPicked(p);
      setFrame(0);
      setName(p.name.replace(/\.[^.]+$/, ''));
      if (p.video) {
        setMedia(null);
        toast.info(t('media.videoPicked'), { description: t('media.videoPickedHint', { button: t('media.makeFrames') }) });
        return;
      }
      setBusy({ step: 'reading' });
      const m = await invoke<MediaInfo>('media_load', { name: p.name, data: await toBase64(p.bytes) });
      setMedia({ ...m, picked: p });
      addLog(t('media.logMedia', { name: p.name, count: m.frames, size: `${m.width}×${m.height}` }));
    } catch (e) {
      toast.error(t('media.openFailed'), { description: String(e), testId: 'toast-open-failed' });
    } finally { setBusy(null); }
  }

  /** Turn the picked video into frames; returns the new media id, or null (stopped or failed). */
  async function makeFrames(): Promise<number | null> {
    if (!picked) return null;
    cancel.current = false;
    setBusy({ step: 'frames' }); setProg(0);
    try {
      const id = await invoke<number>('media_begin', { name: picked.name });
      const delay = Math.round(1000 / vid.fps);
      const total = await decodeVideo(picked, vid, async (b64, i, n) => {
        await invoke('media_push', { media: id, data: b64, delay });
        setProg(Math.round(((i + 1) / n) * 100));
      }, () => cancel.current);
      if (cancel.current) { await invoke('media_drop', { media: id }); toast.message(t('media.stopped')); return null; }
      // the sound of the same stretch of the video, for the fan's speaker
      const audio = await decodeAudio(picked, vid.start, total / vid.fps);
      const audioMs = audio ? await invoke<number>('media_audio', { media: id, data: audio }).catch(() => 0) : 0;
      setMedia({ id, name: picked.name, frames: total, width: 0, height: 0, picked, audioMs });
      addLog(t('media.logVideo', { name: picked.name, count: total, fps: vid.fps }));
      toast.success(t('media.framesMade', { count: total }));
      return id;
    } catch (e) {
      toast.error(t('media.videoFailed'), { description: String(e) });
      return null;
    } finally { setBusy(null); setProg(null); }
  }

  async function doUpload() {
    if (!fan) { toast.error(t('common.noFan')); return; }
    let mediaId = media?.id ?? 0, upName = name;
    // A typed text that was not made yet (or changed since): make it first, instead of sending
    // the test card or the previous text.
    const isText = !media || media.id === madeText.current?.id;
    if (!picked && isText && txt.text.trim() && madeText.current?.key !== textKey) {
      const made = await makeText();
      if (!made) return;
      mediaId = made.id; upName = made.name;
    }
    // A picked video whose frames were not made yet: make them first, with the settings of the
    // "convert" box (no need to press Make frames by hand).
    if (picked?.video && !media) {
      const id = await makeFrames();
      if (id === null) return;
      mediaId = id;
    }
    setBusy({ step: 'preparing' }); setProg(0);
    try {
      const r = await invoke<string>('upload', { args: { id: fan.id, media: mediaId, name: upName || null, slot, placement, max_frames: maxFrames, sound } });
      toast.success(t('media.uploaded'), { description: r });
      addLog(t('media.logUpload', { result: r }));
    } catch (e) {
      toast.error(t('media.uploadFailed'), { description: String(e) });
      addLog(t('media.logUploadError', { error: String(e) }));
    } finally { setBusy(null); setProg(null); }
  }

  const n = media?.frames ?? 1;
  const busyText = busyLabel(busy, t);
  const fields = [
    ['fps', t('common.fps'), 1, 25, 1],
    ['start', t('media.start'), 0, 3600, 0.1],
    ['end', t('media.end'), 0, 3600, 0.1],
    ['max', t('common.maxFrames'), 1, 3000, 1],
  ] as const;
  return (
    <div className="grid gap-10 lg:grid-cols-[minmax(0,1fr)_380px] lg:gap-12">
      <div className="min-w-0 space-y-8">
        <Section title={t('common.source')} className="border-t-0 pt-0">
          <div className="flex flex-wrap gap-2">
            <Button onClick={pick} disabled={!!busy} data-testid="pick"><IconImage />{t('media.pick')}</Button>
            <Button variant="secondary" disabled={!!busy} data-testid="testcard" onClick={() => { setPicked(null); setMedia(null); setName(t('media.testCardName')); setFrame(0); }}>
              <IconRings />{t('common.testCard')}
            </Button>
          </div>
          <form className="mt-4 flex flex-wrap items-center gap-2" onSubmit={(e) => { e.preventDefault(); makeText(); }}>
            <Input className="min-w-48 flex-1" placeholder={t('media.textPlaceholder')} aria-label={t('media.text')} value={txt.text} data-testid="text-input" onChange={(e) => setTxt({ ...txt, text: e.target.value })} />
            <input type="color" aria-label={t('media.textColor')} title={t('media.textColor')} value={txt.color} onChange={(e) => setTxt({ ...txt, color: e.target.value })} className="h-11 w-11 cursor-pointer rounded-full border border-line bg-transparent" />
            <input type="color" aria-label={t('media.background')} title={t('media.background')} value={txt.bg} onChange={(e) => setTxt({ ...txt, bg: e.target.value })} className="h-11 w-11 cursor-pointer rounded-full border border-line bg-transparent" />
            <Button type="submit" variant="secondary" disabled={!txt.text.trim() || !!busy} data-testid="text-make"><IconType />{t('media.text')}</Button>
          </form>
          <div className="mt-2 flex flex-wrap items-center gap-x-6">
            <Choice label={t('media.motion')} value={txt.motion} onChange={(motion) => setTxt({ ...txt, motion })}
              options={[
                { value: '', label: t('media.still'), testId: 'motion-none' },
                { value: 'scroll', label: t('media.scroll'), testId: 'motion-scroll' },
                { value: 'orbit', label: t('media.orbit'), testId: 'motion-orbit' },
              ]} />
            {txt.motion && (
              <div className="min-w-56 flex-1">
                <ValueSlider label={t('media.speed')} value={txt.speed} min={0.25} max={3} step={0.25} format={(v) => `${v}×`} onCommit={(speed) => setTxt({ ...txt, speed })} />
              </div>
            )}
          </div>
          {/* data-frames / data-size let tests check the result without depending on the UI language */}
          <p className="mt-3 text-xs text-muted-foreground" data-testid="media-info" data-frames={media?.frames ?? 0} data-size={media?.width ? `${media.width}x${media.height}` : ''}>
            {media ? <>{t('media.info', { name: media.name, count: media.frames })}{media.width ? ` · ${media.width}×${media.height}` : ''}{media.audioMs ? ` · ${t('media.withSound', { s: (media.audioMs / 1000).toFixed(1) })}` : ''}</>
              : picked?.video ? t('media.infoVideo', { name: picked.name }) : t('media.infoTestCard')}
          </p>
          {picked?.video && (
            <div className="mt-4 border border-dashed border-line p-4">
              <div className="mb-4 flex items-center gap-2 text-[13px] text-primary"><IconFilm />{t('media.convert')}</div>
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                {fields.map(([k, l, mn, mx, st]) => (
                  <div key={k} className="space-y-1.5"><Label className="orbit-label" htmlFor={`vid-${k}`}>{l}</Label>
                    <Input id={`vid-${k}`} type="number" min={mn} max={mx} step={st} value={vid[k]} onChange={(e) => setVid({ ...vid, [k]: Number(e.target.value) })} /></div>
                ))}
              </div>
              <div className="mt-4 flex gap-2">
                <Button onClick={() => { makeFrames(); }} disabled={!!busy} data-testid="make-frames">{t('media.makeFrames')}</Button>
                {busy && 'step' in busy && busy.step === 'frames' && <Button variant="ghost" onClick={() => (cancel.current = true)}><IconX />{t('common.stop')}</Button>}
              </div>
            </div>
          )}
        </Section>

        <Section title={t('media.toFan')}>
          <div className="grid gap-3 sm:grid-cols-[1fr_110px_130px]">
            <div className="space-y-1.5"><Label className="orbit-label" htmlFor="up-name">{t('media.nameOnFan')}</Label>
              <Input id="up-name" data-testid="up-name" maxLength={28} value={name} placeholder={t('media.fileName')} onChange={(e) => setName(e.target.value)} /></div>
            <div className="space-y-1.5"><Label className="orbit-label" htmlFor="up-slot">{t('media.showSlot')}</Label>
              <Input id="up-slot" type="number" min={1} max={255} value={slot} onChange={(e) => setSlot(Number(e.target.value) || 1)} /></div>
            <div className="space-y-1.5"><Label className="orbit-label" htmlFor="up-max">{t('common.maxFrames')}</Label>
              <Input id="up-max" type="number" min={0} value={maxFrames} onChange={(e) => setMaxFrames(Number(e.target.value) || 0)} /></div>
          </div>
          {!!media?.audioMs && (
            <div className="mt-3 flex items-center justify-between gap-3">
              <Label htmlFor="sw-sound">{t('media.sendSound')}</Label>
              <Switch id="sw-sound" data-testid="sw-sound" checked={sound} onCheckedChange={setSound} />
            </div>
          )}
          <div className="mt-4 flex flex-wrap items-center gap-3">
            <Button onClick={doUpload} disabled={!!busy || !fan} data-testid="upload"><IconUpload />{t('media.upload')}</Button>
            {!fan && <span className="text-xs text-muted-foreground">{t('media.connectFirst')}</span>}
            {busy && <span className="text-xs text-muted-foreground" data-testid="busy">{busyText}</span>}
          </div>
          {prog != null && <Progress value={prog} className="mt-4" />}
        </Section>

        <PlacementBox />
      </div>

      <div className="min-w-0 space-y-5 lg:sticky lg:top-0 lg:self-start">
        <RoundView src={pv?.round} label={pvErr ? <span className="text-destructive">{pvErr}</span> :
          pv ? t('media.preview', { size: `${pv.width}×${pv.height}`, kb: (pv.jpeg_bytes / 1024).toFixed(1), q: pv.quality }) : t('media.previewLoading')} />
        {n > 1 && (
          <div className="flex items-center gap-4">
            <span className="orbit-label">{t('media.frame')}</span>
            <Slider value={[frame]} min={0} max={n - 1} step={1} aria-label={t('media.frame')} onValueChange={([f]) => setFrame(f)} />
            <span className="orbit-value w-16 text-right text-xs">{frame + 1}/{n}</span>
          </div>
        )}
        {pv?.layout && (
          <figure className="space-y-2">
            <img src={pv.layout} alt="" className="w-full border border-line" />
            <figcaption className="text-xs text-muted-foreground">{t('media.layoutTitle')}</figcaption>
          </figure>
        )}
      </div>
    </div>
  );
}
