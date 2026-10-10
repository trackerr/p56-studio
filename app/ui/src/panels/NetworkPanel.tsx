import { useEffect, useState } from 'react';
import { Bluetooth, Router, Wifi, Tag } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Confirm, Section } from '@/components/bits';
import { invoke } from '@/lib/api';
import { useP56 } from '@/state';
import { NoFan } from './FanPanel';

export function NetworkPanel() {
  const { fan, run } = useP56();
  const [devName, setDevName] = useState('');
  const [hs, setHs] = useState({ ssid: '', pw: '' });
  const [rt, setRt] = useState({ ssid: '', pw: '' });
  useEffect(() => { if (fan) setDevName((n) => n || fan.name); }, [fan?.id]);
  if (!fan) return <NoFan />;
  const text = (what: string, a: string, b?: string) => run(what, (i) => invoke('set_text', { id: i, what, a, b: b ?? null }));
  const ble = fan.ble;
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <Section title={<span className="inline-flex items-center gap-1.5"><Tag className="h-3.5 w-3.5" />Apparaatnaam</span>}>
        <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); if (devName.trim()) text('name', devName.trim()).then(() => toast.success('Naam opgeslagen')); }}>
          <Input value={devName} maxLength={32} data-testid="dev-name" onChange={(e) => setDevName(e.target.value)} />
          <Button type="submit" data-testid="dev-name-save">Opslaan</Button>
        </form>
        <p className="mt-2 text-xs text-muted-foreground">Nu: {fan.name}</p>
      </Section>

      <Section title={<span className="inline-flex items-center gap-1.5"><Bluetooth className="h-3.5 w-3.5" />Bluetooth van de fan</span>}>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" onClick={() => run('Bluetooth aan', (i) => invoke('control', { id: i, action: 'ble_on' }))} data-testid="ble-on">Aan</Button>
          <Button variant="secondary" onClick={() => run('Bluetooth uit', (i) => invoke('control', { id: i, action: 'ble_off' }))}>Uit</Button>
          <Button onClick={() => run('Zoeken', (i) => invoke('control', { id: i, action: 'ble_scan' }))} data-testid="ble-scan">Zoeken</Button>
          <Button variant="ghost" onClick={() => run('Loskoppelen', (i) => invoke('control', { id: i, action: 'ble_disconnect' }))}>Loskoppelen</Button>
        </div>
        <ul className="mt-3 divide-y divide-border" data-testid="ble-list">
          {ble && ble[1].length ? ble[1].map((n, i) => (
            <li key={n + i} className="flex items-center justify-between py-1.5 text-sm">
              <span>{n}{ble[0] === i + 1 && <span className="ml-2 text-xs text-primary">● verbonden</span>}</span>
              <Button size="sm" variant="ghost" onClick={() => run('Verbinden', (id) => invoke('set_value', { id, what: 'ble_connect', value: i }))}>Verbinden</Button>
            </li>
          )) : <li className="py-2 text-xs text-muted-foreground">Nog geen apparaten. Zet bluetooth aan en klik op Zoeken.</li>}
        </ul>
      </Section>

      <Section title={<span className="inline-flex items-center gap-1.5"><Wifi className="h-3.5 w-3.5" />Eigen wifi van de fan (hotspot)</span>}>
        <Alert className="mb-3 border-amber-500/40 text-amber-200"><AlertDescription>
          Daarna werken de standaardnaam en het standaardwachtwoord niet meer. Noteer de nieuwe gegevens. Nu: <b>{fan.hotspot || '?'}</b>
        </AlertDescription></Alert>
        <div className="grid gap-2 sm:grid-cols-2">
          <Input placeholder="wifi-naam" maxLength={32} value={hs.ssid} data-testid="hs-ssid" onChange={(e) => setHs({ ...hs, ssid: e.target.value })} />
          <Input placeholder="wachtwoord (min. 8 tekens)" type="password" maxLength={32} value={hs.pw} data-testid="hs-pw" onChange={(e) => setHs({ ...hs, pw: e.target.value })} />
        </div>
        <Confirm title={`Hotspot wijzigen naar "${hs.ssid}"?`} action="Wijzigen"
          body={<>Daarna verbind je met wifi <b>{hs.ssid}</b> en wachtwoord <b>{hs.pw}</b>. De huidige verbinding valt weg.</>}
          onConfirm={() => text('hotspot', hs.ssid, hs.pw)}
          trigger={(open) => <Button className="mt-2" variant="outline" data-testid="hs-go" disabled={!hs.ssid || hs.pw.length < 8} onClick={open}>Hotspot wijzigen</Button>} />
      </Section>

      <Section title={<span className="inline-flex items-center gap-1.5"><Router className="h-3.5 w-3.5" />Verbinden met je router</span>}>
        <p className="mb-2 text-xs text-muted-foreground">De fan zoekt daarna zelf je router. Zet dit apparaat op hetzelfde netwerk. Nu: <b>{fan.router || 'geen'}</b></p>
        <div className="grid gap-2 sm:grid-cols-2">
          <Input placeholder="router-naam (SSID)" maxLength={32} value={rt.ssid} data-testid="rt-ssid" onChange={(e) => setRt({ ...rt, ssid: e.target.value })} />
          <Input placeholder="wachtwoord" type="password" maxLength={32} value={rt.pw} data-testid="rt-pw" onChange={(e) => setRt({ ...rt, pw: e.target.value })} />
        </div>
        <Button className="mt-2" data-testid="rt-go" disabled={!rt.ssid} onClick={() => text('router', rt.ssid, rt.pw).then(() => toast.success('Router ingesteld'))}>Router instellen</Button>
      </Section>
    </div>
  );
}
