import { useEffect, useState } from 'react';
import { Bluetooth, Router, Wifi, Tag } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Confirm, Section } from '@/components/bits';
import { invoke } from '@/lib/api';
import { useT } from '@/lib/i18n';
import { useP56 } from '@/state';
import { NoFan } from './FanPanel';

export function NetworkPanel() {
  const { fan, run } = useP56();
  const { t, rich } = useT();
  const [devName, setDevName] = useState('');
  const [hs, setHs] = useState({ ssid: '', pw: '' });
  const [rt, setRt] = useState({ ssid: '', pw: '' });
  useEffect(() => { if (fan) setDevName((n) => n || fan.name); }, [fan?.id]);
  if (!fan) return <NoFan />;
  const text = (label: string, what: string, a: string, b?: string) => run(label, (i) => invoke('set_text', { id: i, what, a, b: b ?? null }));
  const ble = fan.ble;
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <Section title={<span className="inline-flex items-center gap-1.5"><Tag className="h-3.5 w-3.5" />{t('net.deviceName')}</span>}>
        <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); if (devName.trim()) text(t('net.deviceName'), 'name', devName.trim()).then(() => toast.success(t('net.nameSaved'))); }}>
          <Input value={devName} maxLength={32} data-testid="dev-name" aria-label={t('net.deviceName')} onChange={(e) => setDevName(e.target.value)} />
          <Button type="submit" data-testid="dev-name-save">{t('common.save')}</Button>
        </form>
        <p className="mt-2 text-xs text-muted-foreground">{t('common.current', { value: fan.name })}</p>
      </Section>

      <Section title={<span className="inline-flex items-center gap-1.5"><Bluetooth className="h-3.5 w-3.5" />{t('net.bluetooth')}</span>}>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" onClick={() => run(t('net.btOn'), (i) => invoke('control', { id: i, action: 'ble_on' }))} data-testid="ble-on">{t('net.on')}</Button>
          <Button variant="secondary" onClick={() => run(t('net.btOff'), (i) => invoke('control', { id: i, action: 'ble_off' }))}>{t('net.off')}</Button>
          <Button onClick={() => run(t('net.scan'), (i) => invoke('control', { id: i, action: 'ble_scan' }))} data-testid="ble-scan">{t('net.scan')}</Button>
          <Button variant="ghost" onClick={() => run(t('net.disconnect'), (i) => invoke('control', { id: i, action: 'ble_disconnect' }))}>{t('net.disconnect')}</Button>
        </div>
        <ul className="mt-3 divide-y divide-border" data-testid="ble-list">
          {ble && ble[1].length ? ble[1].map((n, i) => (
            <li key={n + i} className="flex items-center justify-between py-1.5 text-sm">
              <span>{n}{ble[0] === i + 1 && <span className="ml-2 text-xs text-primary">{t('net.connected')}</span>}</span>
              <Button size="sm" variant="ghost" onClick={() => run(t('net.connect'), (id) => invoke('set_value', { id, what: 'ble_connect', value: i }))}>{t('net.connect')}</Button>
            </li>
          )) : <li className="py-2 text-xs text-muted-foreground">{t('net.noDevices', { scan: t('net.scan') })}</li>}
        </ul>
      </Section>

      <Section title={<span className="inline-flex items-center gap-1.5"><Wifi className="h-3.5 w-3.5" />{t('net.hotspot')}</span>}>
        <Alert className="mb-3 border-amber-500/40 text-amber-200"><AlertDescription>
          {rich('net.hotspotWarning', { current: <b>{fan.hotspot || '?'}</b> })}
        </AlertDescription></Alert>
        <div className="grid gap-2 sm:grid-cols-2">
          <Input placeholder={t('net.wifiName')} aria-label={t('net.wifiName')} maxLength={32} value={hs.ssid} data-testid="hs-ssid" onChange={(e) => setHs({ ...hs, ssid: e.target.value })} />
          <Input placeholder={t('net.passwordMin')} aria-label={t('net.passwordMin')} type="password" maxLength={32} value={hs.pw} data-testid="hs-pw" onChange={(e) => setHs({ ...hs, pw: e.target.value })} />
        </div>
        <Confirm title={t('net.hotspotTitle', { ssid: hs.ssid })} action={t('net.change')}
          body={rich('net.hotspotBody', { ssid: <b>{hs.ssid}</b>, pw: <b>{hs.pw}</b> })}
          onConfirm={() => text(t('net.changeHotspot'), 'hotspot', hs.ssid, hs.pw)}
          trigger={(open) => <Button className="mt-2" variant="outline" data-testid="hs-go" disabled={!hs.ssid || hs.pw.length < 8} onClick={open}>{t('net.changeHotspot')}</Button>} />
      </Section>

      <Section title={<span className="inline-flex items-center gap-1.5"><Router className="h-3.5 w-3.5" />{t('net.router')}</span>}>
        <p className="mb-2 text-xs text-muted-foreground">{rich('net.routerHint', { current: <b>{fan.router || t('net.none')}</b> })}</p>
        <div className="grid gap-2 sm:grid-cols-2">
          <Input placeholder={t('net.routerName')} aria-label={t('net.routerName')} maxLength={32} value={rt.ssid} data-testid="rt-ssid" onChange={(e) => setRt({ ...rt, ssid: e.target.value })} />
          <Input placeholder={t('net.password')} aria-label={t('net.password')} type="password" maxLength={32} value={rt.pw} data-testid="rt-pw" onChange={(e) => setRt({ ...rt, pw: e.target.value })} />
        </div>
        <Button className="mt-2" data-testid="rt-go" disabled={!rt.ssid} onClick={() => text(t('net.setRouter'), 'router', rt.ssid, rt.pw).then(() => toast.success(t('net.routerSet')))}>{t('net.setRouter')}</Button>
      </Section>
    </div>
  );
}
