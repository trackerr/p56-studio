import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Section } from '@/components/bits';
import { invoke, isTauri } from '@/lib/api';
import { LangSwitch, useT } from '@/lib/i18n';
import { useP56 } from '@/state';

const ls = (k: string) => { try { return localStorage.getItem(k) || ''; } catch { return ''; } };
const lsSet = (k: string, v: string) => { try { localStorage.setItem(k, v); } catch { /* private mode */ } };

export function AppPanel() {
  const { fanTypes, log, restartLink, platform, localIp } = useP56();
  const { t } = useT();
  const [type, setType] = useState(ls('p56.type') || '0');
  const [ip, setIp] = useState(ls('p56.ip'));
  const [targets, setTargets] = useState(ls('p56.targets'));
  const [info, setInfo] = useState<{ ip: string; port: number; interfaces: string[]; discovered: string[] } | null>(null);
  const logRef = useRef<HTMLPreElement>(null);

  useEffect(() => { invoke('set_fan_type', { fanType: Number(type) }).catch(() => {}); lsSet('p56.type', type); }, [type]);
  useEffect(() => {
    const f = () => invoke<typeof info>('link_info').then(setInfo).catch(() => {});
    f(); const timer = setInterval(f, 2000); return () => clearInterval(timer);
  }, []);
  useEffect(() => { logRef.current?.scrollTo(0, logRef.current.scrollHeight); }, [log]);

  const linkInfo = info
    ? [
      `${t('app.broadcastingAs', { ip: info.ip })} · ${t('app.tcpPort', { port: info.port })}`,
      ...info.interfaces.map((i) => t('app.adapter', { name: i })),
      info.discovered.length ? t('app.seenUdp', { list: info.discovered.join(', ') }) : t('app.noneUdp'),
    ].join('\n')
    : t('app.broadcastingAs', { ip: localIp || '…' });

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <Section title={t('app.fanType')}>
        <Select value={type} onValueChange={setType}>
          <SelectTrigger data-testid="fan-type"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="0">{t('app.auto')}</SelectItem>
            {fanTypes.map((ft) => (
              <SelectItem key={ft.id} value={String(ft.id)} disabled={ft.support === 'unsupported'}>
                {t('common.typeId', { id: ft.id })} · {ft.angles}×{ft.leds}{ft.jpeg ? ` · JPEG ${ft.jpeg}` : ''}{ft.id === 8 ? ' · P56' : ''}{ft.support === 'unsupported' ? ` · ${t('app.unsupported')}` : ft.support === 'legacy' ? ` · ${t('app.legacy')}` : ''}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="mt-2 text-xs text-muted-foreground">{t('app.fanTypeHint')}</p>
      </Section>

      <Section title={t('app.connection')}>
        <div className="space-y-3">
          <div className="space-y-1"><Label className="text-xs text-muted-foreground" htmlFor="set-ip">{t('app.ipLabel')}</Label>
            <Input id="set-ip" value={ip} placeholder={t('app.example', { value: '192.168.4.2' })} onChange={(e) => setIp(e.target.value)} /></div>
          <div className="space-y-1"><Label className="text-xs text-muted-foreground" htmlFor="set-targets">{t('app.targetsLabel')}</Label>
            <Input id="set-targets" value={targets} placeholder={t('app.example', { value: '192.168.1.50' })} onChange={(e) => setTargets(e.target.value)} /></div>
          <Button data-testid="apply-net" onClick={async () => { lsSet('p56.ip', ip.trim()); lsSet('p56.targets', targets.trim()); await restartLink(); toast.success(t('app.reconnected')); }}>{t('app.apply')}</Button>
        </div>
        <pre className="mt-3 overflow-auto rounded-md bg-black/40 p-2 font-mono text-[11px] leading-relaxed text-muted-foreground">{linkInfo}</pre>
      </Section>

      <Section title={t('lang.label')} aside={<LangSwitch testId="app-lang" />} className="lg:col-span-2">
        <p className="text-xs text-muted-foreground">{t('lang.hint')}</p>
      </Section>

      <Section title={t('app.log')} className="lg:col-span-2">
        <pre ref={logRef} data-testid="log" className="h-56 overflow-auto whitespace-pre-wrap rounded-md bg-black/40 p-2 font-mono text-[11px] text-muted-foreground">{log.join('\n') || '—'}</pre>
        <p className="mt-2 text-xs text-muted-foreground">P56 Studio {platform.version} · {platform.os || (isTauri ? '' : t('app.browserDemo'))}</p>
      </Section>
    </div>
  );
}
