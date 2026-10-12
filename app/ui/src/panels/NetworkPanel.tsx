import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { IconRouter, IconTag, IconWifi } from '@/components/icons';
import { Confirm, Notice, Section } from '@/components/bits';
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
  return (
    <div className="grid gap-x-12 gap-y-8 lg:grid-cols-2">
      <Section title={<span className="inline-flex items-center gap-2"><IconTag className="h-3.5 w-3.5" />{t('net.deviceName')}</span>}>
        <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); if (devName.trim()) text(t('net.deviceName'), 'name', devName.trim()).then(() => toast.success(t('net.nameSaved'))); }}>
          <Input value={devName} maxLength={31} data-testid="dev-name" aria-label={t('net.deviceName')} onChange={(e) => setDevName(e.target.value)} />
          <Button type="submit" data-testid="dev-name-save">{t('common.save')}</Button>
        </form>
        <p className="mt-3 text-xs text-muted-foreground">{t('common.current', { value: fan.name })}</p>
      </Section>

      <Section title={<span className="inline-flex items-center gap-2"><IconWifi className="h-3.5 w-3.5" />{t('net.hotspot')}</span>}>
        <Notice className="mb-4">{rich('net.hotspotWarning', { current: <b className="font-medium text-primary">{fan.hotspot || '?'}</b> })}</Notice>
        <div className="grid gap-2 sm:grid-cols-2">
          <Input placeholder={t('net.wifiName')} aria-label={t('net.wifiName')} maxLength={31} value={hs.ssid} data-testid="hs-ssid" onChange={(e) => setHs({ ...hs, ssid: e.target.value })} />
          <Input placeholder={t('net.passwordMin')} aria-label={t('net.passwordMin')} type="password" maxLength={31} value={hs.pw} data-testid="hs-pw" onChange={(e) => setHs({ ...hs, pw: e.target.value })} />
        </div>
        <Confirm title={t('net.hotspotTitle', { ssid: hs.ssid })} action={t('net.change')}
          body={rich('net.hotspotBody', { ssid: <b className="font-medium text-primary">{hs.ssid}</b>, pw: <b className="font-medium text-primary">{hs.pw}</b> })}
          onConfirm={() => text(t('net.changeHotspot'), 'hotspot', hs.ssid, hs.pw)}
          trigger={(open) => <Button className="mt-3" variant="secondary" data-testid="hs-go" disabled={!hs.ssid || hs.pw.length < 8} onClick={open}>{t('net.changeHotspot')}</Button>} />
      </Section>

      <Section title={<span className="inline-flex items-center gap-2"><IconRouter className="h-3.5 w-3.5" />{t('net.router')}</span>}>
        <p className="mb-4 text-xs leading-relaxed text-muted-foreground">{rich('net.routerHint', { current: <b className="font-medium text-primary">{fan.router || t('net.none')}</b> })}</p>
        <div className="grid gap-2 sm:grid-cols-2">
          <Input placeholder={t('net.routerName')} aria-label={t('net.routerName')} maxLength={31} value={rt.ssid} data-testid="rt-ssid" onChange={(e) => setRt({ ...rt, ssid: e.target.value })} />
          <Input placeholder={t('net.password')} aria-label={t('net.password')} type="password" maxLength={31} value={rt.pw} data-testid="rt-pw" onChange={(e) => setRt({ ...rt, pw: e.target.value })} />
        </div>
        <Button className="mt-3" data-testid="rt-go" disabled={!rt.ssid} onClick={() => text(t('net.setRouter'), 'router', rt.ssid, rt.pw).then(() => toast.success(t('net.routerSet')))}>{t('net.setRouter')}</Button>
      </Section>
    </div>
  );
}
