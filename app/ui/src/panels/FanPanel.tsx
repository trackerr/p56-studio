import { Power, Play, Pause, RefreshCw, Wifi, AlertTriangle, Repeat, Repeat1 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Confirm, Section, ValueSlider } from '@/components/bits';
import { invoke } from '@/lib/api';
import { useP56 } from '@/state';

const control = (id: number, action: string) => invoke('control', { id, action });
const setValue = (id: number, what: string, value: number) => invoke('set_value', { id, what, value });

export function NoFan() {
  const { linkState, linkError, discovered, localIp } = useP56();
  return (
    <div className="mx-auto max-w-xl space-y-4 py-6" data-testid="no-fan">
      <div className="flex items-center gap-4">
        <div className="relative h-16 w-16 shrink-0 rounded-full border border-border">
          <div className="absolute inset-0 animate-spin rounded-full border-t-2 border-primary [animation-duration:2.4s]" />
          <Wifi className="absolute left-1/2 top-1/2 h-6 w-6 -translate-x-1/2 -translate-y-1/2 text-primary" />
        </div>
        <div>
          <h2 className="text-lg font-semibold">Zoeken naar je fan…</h2>
          <p className="text-sm text-muted-foreground">De fan maakt zelf verbinding zodra hij het signaal van deze app ziet.</p>
        </div>
      </div>
      {linkState === 'error' && (
        <Alert variant="destructive"><AlertTriangle className="h-4 w-4" /><AlertTitle>Kan niet starten</AlertTitle><AlertDescription>{linkError}</AlertDescription></Alert>
      )}
      <ol className="list-decimal space-y-2 pl-5 text-sm">
        <li>Verbind dit apparaat met de <b>wifi van de fan</b> (standaard IP <code className="rounded bg-muted px-1">192.168.4.1</code>) of met dezelfde router.</li>
        <li>Sluit <b>Holoscope</b> en andere P56-apps; er mag er maar één tegelijk luisteren.</li>
        <li>Windows: sta de app toe in de firewall (TCP 6666, UDP 8988) als daarom gevraagd wordt.</li>
      </ol>
      <p className="text-xs text-muted-foreground">
        Deze app zendt uit als <span className="font-mono">{localIp || '…'}</span>
        {discovered.length > 0 && <> · gezien via UDP: <span className="font-mono">{discovered.join(', ')}</span></>}
      </p>
    </div>
  );
}

export function FanPanel() {
  const { fan, run } = useP56();
  if (!fan) return <NoFan />;
  const s = fan.status;
  const id = fan.id;
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
              <Badge variant={s?.power ? 'default' : 'secondary'} data-testid="badge-power">{s ? (s.power ? 'aan' : 'uit') : '…'}</Badge>
              {s?.power && <Badge variant="outline">{s.playing ? 'speelt' : 'pauze'}</Badge>}
              {s && <Badge variant="outline" className="font-mono">type {s.fan_type}{s.fan_type === 8 ? ' · P56' : ''}</Badge>}
              {fan.support === 'unsupported' && <Badge variant="destructive">beeldformaat onbekend</Badge>}
              {fan.support === 'legacy' && <Badge variant="secondary">oud beeldformaat</Badge>}
            </div>
          </div>
          {fan.last_error && (
            <Alert variant="destructive" className="mt-3"><AlertTriangle className="h-4 w-4" /><AlertTitle>Laatste melding van de fan</AlertTitle><AlertDescription>{fan.last_error}</AlertDescription></Alert>
          )}
          <div className="mt-4 flex flex-wrap gap-2">
            <Button data-testid="btn-power" variant={s?.power ? 'secondary' : 'default'} onClick={() => run('Aan/uit', (i) => control(i, s?.power ? 'off' : 'on'))}>
              <Power className="mr-1.5 h-4 w-4" />{s?.power ? 'Uitzetten' : 'Aanzetten'}
            </Button>
            <Button data-testid="btn-play" variant="secondary" onClick={() => run('Afspelen', (i) => control(i, s?.playing ? 'pause' : 'play'))}>
              {s?.playing ? <><Pause className="mr-1.5 h-4 w-4" />Pauze</> : <><Play className="mr-1.5 h-4 w-4" />Afspelen</>}
            </Button>
            <Button variant="ghost" size="icon" title="Vernieuwen" onClick={() => run('Vernieuwen', (i) => control(i, 'refresh'))}><RefreshCw className="h-4 w-4" /></Button>
          </div>
        </section>

        <Section title="Beeld">
          <ValueSlider id="sl-brightness" label="Helderheid" value={s?.brightness ?? 16} min={1} max={16} format={(v) => `${v}/16`}
            onCommit={(v) => run('Helderheid', (i) => setValue(i, 'brightness', v))} />
          <ValueSlider id="sl-angle" label="Hoek" value={s?.angle ?? 0} min={-180} max={180} format={(v) => `${v}°`}
            onCommit={(v) => run('Hoek', (i) => setValue(i, 'angle', v))} />
          <div className="mt-2 grid grid-cols-[88px_1fr] items-center gap-3">
            <span className="text-xs text-muted-foreground">Kleur</span>
            <ToggleGroup type="single" variant="outline" className="justify-start" value={s?.color_mode === 1 ? 'true' : s?.color_mode === 2 ? 'bright' : ''}
              onValueChange={(v) => v && run('Kleurmodus', (i) => control(i, v === 'true' ? 'color_true' : 'color_bright'))}>
              <ToggleGroupItem value="true" data-testid="color-true">True color</ToggleGroupItem>
              <ToggleGroupItem value="bright" data-testid="color-bright">Hoge helderheid</ToggleGroupItem>
            </ToggleGroup>
          </div>
        </Section>
      </div>

      <div className="space-y-4">
        <Section title="Afspelen & gedrag">
          <div className="grid grid-cols-[88px_1fr] items-center gap-3 py-1">
            <span className="text-xs text-muted-foreground">Herhalen</span>
            <ToggleGroup type="single" variant="outline" className="justify-start" value={s ? (s.repeat_all ? 'all' : 'one') : ''}
              onValueChange={(v) => v && run('Herhalen', (i) => control(i, v === 'all' ? 'repeat_all' : 'repeat_one'))}>
              <ToggleGroupItem value="all" data-testid="repeat-all"><Repeat className="mr-1.5 h-4 w-4" />Hele lijst</ToggleGroupItem>
              <ToggleGroupItem value="one" data-testid="repeat-one"><Repeat1 className="mr-1.5 h-4 w-4" />Huidig</ToggleGroupItem>
            </ToggleGroup>
          </div>
          {([
            ['breath', 'Ademlicht', s?.breath, 'breath_on', 'breath_off'],
            ['autostart', 'Automatisch starten bij aanzetten', s?.autostart, 'autostart_on', 'autostart_off'],
            ['master', 'Groeps-master', s?.master, 'master_on', 'master_off'],
          ] as const).map(([key, label, on, a, b]) => (
            <div key={key} className="flex items-center justify-between py-2">
              <Label htmlFor={`sw-${key}`} className="font-normal">{label}</Label>
              <Switch id={`sw-${key}`} data-testid={`sw-${key}`} checked={!!on} onCheckedChange={(c) => run(label, (i) => control(i, c ? a : b))} />
            </div>
          ))}
          <ValueSlider id="sl-volume" label="Volume" value={10} min={0} max={10} onCommit={(v) => run('Volume', (i) => setValue(i, 'volume', v))} />
          <p className="text-xs text-muted-foreground">Volume werkt alleen op fans met een speaker.</p>
        </Section>

        <Section title="Onderhoud">
          <div className="flex flex-wrap gap-2">
            <Confirm title="Geheugenkaart formatteren?" action="Formatteren"
              body={<>Alle bestanden op de fan worden gewist. Dit kan niet ongedaan worden gemaakt.</>}
              onConfirm={() => run('Formatteren', (i) => control(i, 'format'))}
              trigger={(open) => <Button variant="outline" className="border-destructive/60 text-destructive" onClick={open} data-testid="btn-format">Formatteren</Button>} />
            <Confirm title="Fabrieksinstellingen herstellen?" action="Herstellen"
              body={<>Naam, helderheid, hoek en andere instellingen gaan terug naar de standaard.</>}
              onConfirm={() => run('Fabrieksinstellingen', (i) => control(i, 'reset'))}
              trigger={(open) => <Button variant="outline" className="border-destructive/60 text-destructive" onClick={open} data-testid="btn-reset">Fabrieksinstellingen</Button>} />
          </div>
        </Section>
      </div>
      <p className="sr-only">fan {id}</p>
    </div>
  );
}
