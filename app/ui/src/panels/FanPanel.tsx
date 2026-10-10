import { Power, Play, Pause, RefreshCw, Wifi, AlertTriangle, Repeat, Repeat1 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Confirm, Section, ValueSlider } from '@/components/bits';
import { invoke } from '@/lib/api';
import { useT } from '@/lib/i18n';
import { useP56 } from '@/state';

const control = (id: number, action: string) => invoke('control', { id, action });
const setValue = (id: number, what: string, value: number) => invoke('set_value', { id, what, value });

export function NoFan() {
  const { linkState, linkError, discovered, localIp } = useP56();
  const { t, rich } = useT();
  return (
    <div className="mx-auto max-w-xl space-y-4 py-6" data-testid="no-fan">
      <div className="flex items-center gap-4">
        <div className="relative h-16 w-16 shrink-0 rounded-full border border-border">
          <div className="absolute inset-0 animate-spin rounded-full border-t-2 border-primary [animation-duration:2.4s]" />
          <Wifi className="absolute left-1/2 top-1/2 h-6 w-6 -translate-x-1/2 -translate-y-1/2 text-primary" />
        </div>
        <div>
          <h2 className="text-lg font-semibold">{t('nofan.title')}</h2>
          <p className="text-sm text-muted-foreground">{t('nofan.subtitle')}</p>
        </div>
      </div>
      {linkState === 'error' && (
        <Alert variant="destructive"><AlertTriangle className="h-4 w-4" /><AlertTitle>{t('nofan.cannotStart')}</AlertTitle><AlertDescription>{linkError}</AlertDescription></Alert>
      )}
      <ol className="list-decimal space-y-2 pl-5 text-sm">
        <li>{rich('nofan.step1', { wifi: <b>{t('nofan.fanWifi')}</b>, ip: <code className="rounded bg-muted px-1">192.168.4.1</code> })}</li>
        <li>{rich('nofan.step2', { holoscope: <b>Holoscope</b> })}</li>
        <li>{t('nofan.step3')}</li>
      </ol>
      <p className="text-xs text-muted-foreground">
        {rich('nofan.broadcasting', { ip: <span className="font-mono">{localIp || '…'}</span> })}
        {discovered.length > 0 && rich('nofan.seenUdp', { list: <span className="font-mono">{discovered.join(', ')}</span> })}
      </p>
    </div>
  );
}

export function FanPanel() {
  const { fan, run } = useP56();
  const { t } = useT();
  if (!fan) return <NoFan />;
  const s = fan.status;
  const id = fan.id;
  const toggles = [
    ['breath', t('fan.breath'), s?.breath, 'breath_on', 'breath_off'],
    ['autostart', t('fan.autostart'), s?.autostart, 'autostart_on', 'autostart_off'],
    ['master', t('fan.master'), s?.master, 'master_on', 'master_off'],
  ] as const;
  return (
    <div className="grid gap-4 lg:grid-cols-[1.1fr_1fr]">
      <div className="space-y-4">
        <section className="rounded-lg border border-border bg-card p-4" data-testid="fan-card">
          <div className="flex flex-wrap items-start justify-between gap-2">
            <div>
              <div className="text-lg font-semibold" data-testid="fan-name">{fan.name}</div>
              <div className="font-mono text-xs text-muted-foreground">{fan.ip} · {fan.mac || '—'}</div>
            </div>
            <div className="flex flex-wrap gap-1.5">
              <Badge variant={s?.power ? 'default' : 'secondary'} data-testid="badge-power">{s ? (s.power ? t('fan.on') : t('fan.off')) : '…'}</Badge>
              {s?.power && <Badge variant="outline">{s.playing ? t('fan.playing') : t('fan.paused')}</Badge>}
              {s && <Badge variant="outline" className="font-mono">{t('common.typeId', { id: s.fan_type })}{s.fan_type === 8 ? ' · P56' : ''}</Badge>}
              {fan.support === 'unsupported' && <Badge variant="destructive">{t('fan.unsupported')}</Badge>}
              {fan.support === 'legacy' && <Badge variant="secondary">{t('fan.legacy')}</Badge>}
            </div>
          </div>
          {fan.last_error && (
            <Alert variant="destructive" className="mt-3"><AlertTriangle className="h-4 w-4" /><AlertTitle>{t('fan.lastError')}</AlertTitle><AlertDescription>{fan.last_error}</AlertDescription></Alert>
          )}
          <div className="mt-4 flex flex-wrap gap-2">
            <Button data-testid="btn-power" variant={s?.power ? 'secondary' : 'default'} onClick={() => run(t('fan.power'), (i) => control(i, s?.power ? 'off' : 'on'))}>
              <Power className="mr-1.5 h-4 w-4" />{s?.power ? t('fan.turnOff') : t('fan.turnOn')}
            </Button>
            <Button data-testid="btn-play" variant="secondary" onClick={() => run(t('fan.play'), (i) => control(i, s?.playing ? 'pause' : 'play'))}>
              {s?.playing ? <><Pause className="mr-1.5 h-4 w-4" />{t('fan.pause')}</> : <><Play className="mr-1.5 h-4 w-4" />{t('fan.play')}</>}
            </Button>
            <Button variant="ghost" size="icon" title={t('common.refresh')} aria-label={t('common.refresh')} onClick={() => run(t('common.refresh'), (i) => control(i, 'refresh'))}><RefreshCw className="h-4 w-4" /></Button>
          </div>
        </section>

        <Section title={t('fan.display')}>
          <ValueSlider id="sl-brightness" label={t('fan.brightness')} value={s?.brightness ?? 16} min={1} max={16} format={(v) => `${v}/16`}
            onCommit={(v) => run(t('fan.brightness'), (i) => setValue(i, 'brightness', v))} />
          <ValueSlider id="sl-angle" label={t('fan.angle')} value={s?.angle ?? 0} min={-180} max={180} format={(v) => `${v}°`}
            onCommit={(v) => run(t('fan.angle'), (i) => setValue(i, 'angle', v))} />
          <div className="mt-2 grid grid-cols-[88px_1fr] items-center gap-3">
            <span className="text-xs text-muted-foreground">{t('fan.color')}</span>
            <ToggleGroup type="single" variant="outline" className="justify-start" value={s?.color_mode === 1 ? 'true' : s?.color_mode === 2 ? 'bright' : ''}
              onValueChange={(v) => v && run(t('fan.colorMode'), (i) => control(i, v === 'true' ? 'color_true' : 'color_bright'))}>
              <ToggleGroupItem value="true" data-testid="color-true">{t('fan.trueColor')}</ToggleGroupItem>
              <ToggleGroupItem value="bright" data-testid="color-bright">{t('fan.highBrightness')}</ToggleGroupItem>
            </ToggleGroup>
          </div>
        </Section>
      </div>

      <div className="space-y-4">
        <Section title={t('fan.playback')}>
          <div className="grid grid-cols-[88px_1fr] items-center gap-3 py-1">
            <span className="text-xs text-muted-foreground">{t('fan.repeat')}</span>
            <ToggleGroup type="single" variant="outline" className="justify-start" value={s ? (s.repeat_all ? 'all' : 'one') : ''}
              onValueChange={(v) => v && run(t('fan.repeat'), (i) => control(i, v === 'all' ? 'repeat_all' : 'repeat_one'))}>
              <ToggleGroupItem value="all" data-testid="repeat-all"><Repeat className="mr-1.5 h-4 w-4" />{t('fan.repeatAll')}</ToggleGroupItem>
              <ToggleGroupItem value="one" data-testid="repeat-one"><Repeat1 className="mr-1.5 h-4 w-4" />{t('fan.repeatOne')}</ToggleGroupItem>
            </ToggleGroup>
          </div>
          {toggles.map(([key, label, on, a, b]) => (
            <div key={key} className="flex items-center justify-between py-2">
              <Label htmlFor={`sw-${key}`} className="font-normal">{label}</Label>
              <Switch id={`sw-${key}`} data-testid={`sw-${key}`} checked={!!on} onCheckedChange={(c) => run(label, (i) => control(i, c ? a : b))} />
            </div>
          ))}
          <ValueSlider id="sl-volume" label={t('fan.volume')} value={10} min={0} max={10} onCommit={(v) => run(t('fan.volume'), (i) => setValue(i, 'volume', v))} />
          <p className="text-xs text-muted-foreground">{t('fan.volumeHint')}</p>
        </Section>

        <Section title={t('fan.maintenance')}>
          <div className="flex flex-wrap gap-2">
            <Confirm title={t('fan.formatTitle')} action={t('fan.format')}
              body={t('fan.formatBody')}
              onConfirm={() => run(t('fan.format'), (i) => control(i, 'format'))}
              trigger={(open) => <Button variant="outline" className="border-destructive/60 text-destructive" onClick={open} data-testid="btn-format">{t('fan.format')}</Button>} />
            <Confirm title={t('fan.resetTitle')} action={t('fan.resetAction')}
              body={t('fan.resetBody')}
              onConfirm={() => run(t('fan.factory'), (i) => control(i, 'reset'))}
              trigger={(open) => <Button variant="outline" className="border-destructive/60 text-destructive" onClick={open} data-testid="btn-reset">{t('fan.factory')}</Button>} />
          </div>
        </Section>
      </div>
      <p className="sr-only">fan {id}</p>
    </div>
  );
}
