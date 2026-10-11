import { IconPause, IconPlay, IconPlus, IconPower, IconRefresh, IconRepeat, IconRepeatOne } from '@/components/icons';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';
import { Choice, Confirm, DiscText, Notice, Orbit, Readout, Section, Status, ValueSlider } from '@/components/bits';
import { invoke, type Device } from '@/lib/api';
import { useT } from '@/lib/i18n';
import { useNav } from '@/lib/nav';
import { cn } from '@/lib/utils';
import { useP56 } from '@/state';

const control = (id: number, action: string) => invoke('control', { id, action });
const setValue = (id: number, what: string, value: number) => invoke('set_value', { id, what, value });
const stem = (name: string) => name.replace(/\.mp4$/i, '');
const pad = (n: number) => String(n).padStart(2, '0');

export function NoFan() {
  const { linkState, linkError, discovered, localIp } = useP56();
  const { t, rich } = useT();
  return (
    <div className="mx-auto max-w-xl space-y-6 py-6" data-testid="no-fan">
      <div className="flex items-center gap-5">
        <svg viewBox="0 0 80 80" aria-hidden="true" className="h-20 w-20 shrink-0 text-primary">
          <circle cx="40" cy="40" r="30" fill="none" stroke="hsl(var(--input))" />
          <circle cx="40" cy="40" r="16" fill="none" stroke="hsl(var(--input))" strokeDasharray="2 4" />
          <g className="orbit-seek"><circle cx="40" cy="10" r="3.5" fill="currentColor" /></g>
          <circle cx="40" cy="40" r="2.5" fill="currentColor" />
        </svg>
        <div className="space-y-1.5">
          <h1 className="font-display text-sm uppercase tracking-[0.12em] text-primary">{t('nofan.title')}</h1>
          <p className="text-[13px] text-muted-foreground">{t('nofan.subtitle')}</p>
        </div>
      </div>
      {linkState === 'error' && <Notice danger title={t('nofan.cannotStart')}>{linkError}</Notice>}
      <ol className="space-y-3 text-[13px] leading-relaxed">
        {[
          rich('nofan.step1', { wifi: <b className="font-medium text-primary">{t('nofan.fanWifi')}</b>, ip: <span className="text-primary">192.168.4.1</span> }),
          rich('nofan.step2', { holoscope: <b className="font-medium text-primary">Holoscope</b> }),
          t('nofan.step3'),
        ].map((step, i) => (
          <li key={i} className="flex gap-4 border-t border-border pt-3">
            <span className="orbit-label pt-1">{pad(i + 1)}</span><span>{step}</span>
          </li>
        ))}
      </ol>
      <p className="text-xs text-muted-foreground">
        {rich('nofan.broadcasting', { ip: <span className="text-foreground">{localIp || '…'}</span> })}
        {discovered.length > 0 && rich('nofan.seenUdp', { list: <span className="text-foreground">{discovered.join(', ')}</span> })}
      </p>
    </div>
  );
}

/** The fan's playlist as orbits on a line; the current one is lit. Tap one to show it. */
function PlaylistStrip({ fan }: { fan: Device }) {
  const { run } = useP56();
  const { t } = useT();
  const go = useNav();
  const cur = fan.status?.play_index ?? 0;
  return (
    <section aria-label={t('strip.title')} className="space-y-4">
      <h2 className="orbit-label">{t('strip.title')}</h2>
      <div className="relative">
        <div aria-hidden="true" className="absolute left-0 right-0 top-[42px] h-px bg-border" />
        <ol className="relative flex gap-6 overflow-x-auto pb-2 [scrollbar-width:none] lg:gap-10">
          {fan.files.map(([n, name]) => {
            const on = n === cur;
            return (
              <li key={n} className="w-[104px] shrink-0">
                <button type="button" data-testid={`strip-${n}`} aria-current={on || undefined} aria-label={t('strip.show', { name: stem(name) })}
                  onClick={() => run(t('playlist.show'), (i) => setValue(i, 'select', n))}
                  className={cn('group flex w-full flex-col items-center gap-3 text-xs transition-colors', on ? 'text-primary' : 'pt-3.5 text-muted-foreground hover:text-foreground')}>
                  <span className={cn('grid place-items-center rounded-full bg-black font-display',
                    on ? 'h-[84px] w-[84px] border-2 border-primary text-sm shadow-[0_0_0_6px_hsl(var(--background)),0_0_0_7px_hsl(var(--input))]'
                      : 'h-14 w-14 border border-line text-[11px] group-hover:border-muted-foreground')}>
                    {pad(n)}
                  </span>
                  <span className="w-full truncate text-center">{stem(name)}</span>
                </button>
              </li>
            );
          })}
          <li className="w-[104px] shrink-0 pt-3.5">
            <button type="button" onClick={() => go('media')} data-testid="strip-add"
              className="flex w-full flex-col items-center gap-3 text-xs text-muted-foreground transition-colors hover:text-foreground">
              <span className="grid h-14 w-14 place-items-center rounded-full border border-dashed border-muted-foreground/70 bg-background"><IconPlus /></span>
              {t('strip.add')}
            </button>
          </li>
        </ol>
      </div>
    </section>
  );
}

export function FanPanel() {
  const { fan, run } = useP56();
  const { t } = useT();
  if (!fan) return <NoFan />;
  const s = fan.status;
  const current = fan.files.find(([n]) => n === s?.play_index);
  const toggles = [
    ['breath', t('fan.breath'), s?.breath, 'breath_on', 'breath_off'],
    ['autostart', t('fan.autostart'), s?.autostart, 'autostart_on', 'autostart_off'],
    ['master', t('fan.master'), s?.master, 'master_on', 'master_off'],
  ] as const;
  return (
    <div className="space-y-10">
      <div className="grid gap-10 lg:grid-cols-[minmax(0,1fr)_340px] lg:gap-12">
        <div className="flex min-w-0 flex-col items-center gap-5">
          <div className="flex flex-wrap justify-center gap-x-8 gap-y-2">
            <Readout label={t('fan.angle')}>{s?.angle ?? 0}°</Readout>
            <Readout label={t('fan.color')}>{s?.color_mode === 2 ? t('fan.highBrightness') : t('fan.trueColor')}</Readout>
            <Readout label={t('fan.repeat')}>{s?.repeat_all === false ? t('fan.repeatOne') : t('fan.repeatAll')}</Readout>
          </div>

          <Orbit className="max-w-[600px]" spinning={!!s?.power && !!s?.playing} dim={!s?.power}
            label={current ? stem(current[1]) : undefined}
            brightness={{
              value: s?.brightness ?? 16, min: 1, max: 16, label: t('fan.brightness'), format: (v) => `${v}/16`, testId: 'sl-brightness', disabled: !s,
              onCommit: (v) => run(t('fan.brightness'), (i) => setValue(i, 'brightness', v)),
            }}
            angle={{
              value: s?.angle ?? 0, min: -180, max: 180, label: t('fan.angle'), format: (v) => `${v}°`, testId: 'sl-angle', disabled: !s,
              onCommit: (v) => run(t('fan.angle'), (i) => setValue(i, 'angle', v)),
            }}>
            <DiscText className="text-[clamp(13px,4.2vw,30px)]">
              {s && !s.power ? <span className="text-muted-foreground [text-shadow:none]">{t('fan.off')}</span>
                : current ? <span>{stem(current[1])}<span className="mt-3 block font-sans text-[11px] text-muted-foreground [text-shadow:none]">{pad(current[0])} / {pad(fan.files.length)}</span></span>
                : <span className="text-muted-foreground [text-shadow:none]">{fan.name}</span>}
            </DiscText>
          </Orbit>
          <p className="max-w-sm text-center text-xs text-muted-foreground">{t('fan.ringHint')}</p>

          <div className="flex items-start gap-8">
            <div className="flex flex-col items-center gap-2.5">
              <Button size="round" data-testid="btn-power" variant={s?.power ? 'default' : 'secondary'} aria-label={s?.power ? t('fan.turnOff') : t('fan.turnOn')}
                onClick={() => run(t('fan.power'), (i) => control(i, s?.power ? 'off' : 'on'))}>
                <IconPower className="h-6 w-6" />
              </Button>
              <span className="orbit-label">{s?.power ? t('fan.turnOff') : t('fan.turnOn')}</span>
            </div>
            <div className="flex flex-col items-center gap-2.5">
              <Button size="round" variant="secondary" data-testid="btn-play" aria-label={s?.playing ? t('fan.pause') : t('fan.play')}
                onClick={() => run(t('fan.play'), (i) => control(i, s?.playing ? 'pause' : 'play'))}>
                {s?.playing ? <IconPause className="h-6 w-6" /> : <IconPlay className="h-6 w-6" />}
              </Button>
              <span className="orbit-label">{s?.playing ? t('fan.pause') : t('fan.play')}</span>
            </div>
            <div className="flex flex-col items-center gap-2.5">
              <Button size="round" variant="secondary" aria-label={t('common.refresh')} onClick={() => run(t('common.refresh'), (i) => control(i, 'refresh'))}>
                <IconRefresh className="h-6 w-6" />
              </Button>
              <span className="orbit-label">{t('common.refresh')}</span>
            </div>
          </div>
        </div>

        <aside className="min-w-0 space-y-7">
          <div className="space-y-2" data-testid="fan-card">
            <h1 className="break-words font-display text-xl uppercase tracking-[0.06em] text-primary" data-testid="fan-name">{fan.name}</h1>
            <div className="text-xs text-muted-foreground">{fan.ip} · {fan.mac || '—'} · {s ? t('common.typeId', { id: s.fan_type }) : '…'}{fan.model ? ` · ${fan.model}` : ''}</div>
            <div className="flex flex-wrap gap-x-4 gap-y-1 pt-1 text-xs">
              <Status on={!!s?.power} testId="badge-power">{s ? (s.power ? t('fan.on') : t('fan.off')) : '…'}</Status>
              {s?.power && <Status on={s.playing}>{s.playing ? t('fan.playing') : t('fan.paused')}</Status>}
              {fan.support === 'unsupported' && <span className="text-destructive">{t('fan.unsupported')}</span>}
              {fan.support === 'legacy' && <span className="text-muted-foreground">{t('fan.legacy')}</span>}
            </div>
            {fan.last_error && <Notice danger title={t('fan.lastError')} className="mt-3">{fan.last_error}</Notice>}
          </div>

          <Section title={t('fan.playback')}>
            <div className="space-y-1">
              <div className="orbit-label pt-1">{t('fan.repeat')}</div>
              <Choice label={t('fan.repeat')} value={s ? (s.repeat_all ? 'all' : 'one') : ''}
                onChange={(v) => run(t('fan.repeat'), (i) => control(i, v === 'all' ? 'repeat_all' : 'repeat_one'))}
                options={[
                  { value: 'all', label: t('fan.repeatAll'), testId: 'repeat-all', icon: IconRepeat },
                  { value: 'one', label: t('fan.repeatOne'), testId: 'repeat-one', icon: IconRepeatOne },
                ]} />
              <div className="orbit-label pt-3">{t('fan.color')}</div>
              <Choice label={t('fan.colorMode')} value={s?.color_mode === 1 ? 'true' : s?.color_mode === 2 ? 'bright' : ''}
                onChange={(v) => run(t('fan.colorMode'), (i) => control(i, v === 'true' ? 'color_true' : 'color_bright'))}
                options={[
                  { value: 'true', label: t('fan.trueColor'), testId: 'color-true' },
                  { value: 'bright', label: t('fan.highBrightness'), testId: 'color-bright' },
                ]} />
            </div>
          </Section>

          <Section title={t('fan.behavior')}>
            {toggles.map(([key, label, on, a, b]) => (
              <div key={key} className="flex items-center justify-between gap-3">
                <Label htmlFor={`sw-${key}`}>{label}</Label>
                <Switch id={`sw-${key}`} data-testid={`sw-${key}`} checked={!!on} onCheckedChange={(c) => run(label, (i) => control(i, c ? a : b))} />
              </div>
            ))}
            <ValueSlider id="sl-volume" label={t('fan.volume')} value={fan.ble?.volume ?? 10} min={0} max={10} onCommit={(v) => run(t('fan.volume'), (i) => setValue(i, 'volume', v))} />
            <p className="mt-1 text-xs text-muted-foreground">{t('fan.volumeHint')}</p>
          </Section>

          <Section title={t('fan.maintenance')}>
            <div className="flex flex-wrap gap-2">
              <Confirm title={t('fan.formatTitle')} action={t('fan.format')} body={t('fan.formatBody')}
                onConfirm={() => run(t('fan.format'), (i) => control(i, 'format'))}
                trigger={(open) => <Button variant="destructive" onClick={open} data-testid="btn-format">{t('fan.format')}</Button>} />
              <Confirm title={t('fan.resetTitle')} action={t('fan.resetAction')} body={t('fan.resetBody')}
                onConfirm={() => run(t('fan.factory'), (i) => control(i, 'reset'))}
                trigger={(open) => <Button variant="destructive" onClick={open} data-testid="btn-reset">{t('fan.factory')}</Button>} />
            </div>
          </Section>
        </aside>
      </div>

      <PlaylistStrip fan={fan} />
    </div>
  );
}
