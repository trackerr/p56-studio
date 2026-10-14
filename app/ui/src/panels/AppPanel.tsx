import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Choice, Section } from '@/components/bits';
import { appVersion, invoke, isTauri, lastServer, pingServer, rememberServer, remoteServer, serverUrl, setRemoteServer } from '@/lib/api';
import { LangSwitch, useT } from '@/lib/i18n';
import { useP56 } from '@/state';

const ls = (k: string) => { try { return localStorage.getItem(k) || ''; } catch { return ''; } };
const lsSet = (k: string, v: string) => { try { localStorage.setItem(k, v); } catch { /* private mode */ } };

export function AppPanel() {
  const { fanTypes, log, restartLink, platform, localIp, viaServer } = useP56();
  const { t } = useT();
  // the app's own version: through a p56-server, `platform` is the server's
  const [own, setOwn] = useState<string | null>(null);
  useEffect(() => { appVersion.then(setOwn); }, []);
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
    <div className="grid gap-x-12 gap-y-8 lg:grid-cols-2">
      <Section title={t('app.fanType')}>
        <Select value={type} onValueChange={setType}>
          <SelectTrigger data-testid="fan-type"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="0">{t('app.auto')}</SelectItem>
            {fanTypes.map((ft) => (
              <SelectItem key={ft.id} value={String(ft.id)} disabled={ft.support === 'unsupported'}>
                {t('common.typeId', { id: ft.id })} · {ft.angles}×{ft.leds}{ft.jpeg ? ` · JPEG ${ft.jpeg}` : ''}{ft.model ? ` · ${ft.model}` : ''}{ft.support === 'unsupported' ? ` · ${t('app.unsupported')}` : ft.support === 'legacy' ? ` · ${t('app.legacy')}` : ''}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="mt-3 text-xs leading-relaxed text-muted-foreground">{t('app.fanTypeHint')}</p>
      </Section>

      {isTauri && <ServerMode />}

      {!remoteServer && <Section title={t('app.connection')}>
        {/* the server's link is shared by every device: its IP / fan addresses come from the server's options */}
        {viaServer ? <p data-testid="net-server" className="text-xs leading-relaxed text-muted-foreground">{t('app.connectionServer')}</p> : <div className="space-y-4">
          <div className="space-y-1.5"><Label className="text-xs text-muted-foreground" htmlFor="set-ip">{t('app.ipLabel')}</Label>
            <Input id="set-ip" value={ip} placeholder={t('app.example', { value: '192.168.4.2' })} onChange={(e) => setIp(e.target.value)} /></div>
          <div className="space-y-1.5"><Label className="text-xs text-muted-foreground" htmlFor="set-targets">{t('app.targetsLabel')}</Label>
            <Input id="set-targets" value={targets} placeholder={t('app.example', { value: '192.168.1.50' })} onChange={(e) => setTargets(e.target.value)} /></div>
          <Button data-testid="apply-net" onClick={async () => { lsSet('p56.ip', ip.trim()); lsSet('p56.targets', targets.trim()); await restartLink(); toast.success(t('app.reconnected')); }}>{t('app.apply')}</Button>
        </div>}
        <pre className="mt-4 overflow-auto font-mono text-[11px] leading-relaxed text-muted-foreground">{linkInfo}</pre>
      </Section>}

      <Section title={t('lang.label')} aside={<LangSwitch testId="app-lang" />} className="lg:col-span-2">
        <p className="text-xs leading-relaxed text-muted-foreground">{t('lang.hint')}</p>
      </Section>

      <Section title={t('app.log')} className="lg:col-span-2">
        <pre ref={logRef} data-testid="log" className="h-60 overflow-auto whitespace-pre-wrap border border-border bg-black p-3 font-mono text-[11px] leading-relaxed text-muted-foreground">{log.join('\n') || '—'}</pre>
        <p className="mt-3 text-xs text-muted-foreground" data-testid="app-version">
          {own && remoteServer
            ? <>P56 Studio {own} · {t('app.serverVersion', { version: platform.version, os: platform.os })}</>
            : <>P56 Studio {platform.version} · {platform.os || (isTauri ? '' : t('app.browserDemo'))}</>}
        </p>
      </Section>
    </div>
  );
}

/** Control the fan through a p56-server instead of this app's own engine. */
function ServerMode() {
  const { t } = useT();
  const [mode, setMode] = useState<'local' | 'server'>(remoteServer ? 'server' : 'local');
  // the address stays filled in when switching back and forth
  const [url, setUrl] = useState(remoteServer || lastServer);
  const [found, setFound] = useState<boolean | null>(null);
  const test = async () => { rememberServer(url); setFound(await pingServer(url)); };
  return (
    <Section title={t('app.serverTitle')} className="lg:col-span-2">
      <Choice label={t('app.serverTitle')} value={mode} onChange={(m) => { setMode(m); if (m === 'local' && remoteServer) setRemoteServer(''); }}
        options={[{ value: 'local', label: t('app.modeLocal'), testId: 'mode-local' }, { value: 'server', label: t('app.modeServer'), testId: 'mode-server' }]} />
      {mode === 'server' && (
        <form className="mt-3 flex flex-wrap items-center gap-2" onSubmit={(e) => { e.preventDefault(); setRemoteServer(url); }}>
          <Input className="min-w-48 flex-1" value={url} placeholder={t('app.example', { value: '192.168.1.10:8765' })} aria-label={t('app.serverAddress')}
            data-testid="server-url" onChange={(e) => { setUrl(e.target.value); setFound(null); }} />
          <Button type="button" variant="secondary" disabled={!url.trim()} onClick={test}>{t('app.serverTest')}</Button>
          <Button type="submit" data-testid="server-use" disabled={!url.trim() || serverUrl(url) === remoteServer}>{t('app.serverUse')}</Button>
        </form>
      )}
      <p className="mt-3 text-xs leading-relaxed text-muted-foreground">
        {found === true ? t('app.serverOk') : found === false ? t('app.serverMissing') : remoteServer ? t('app.serverActive', { url: remoteServer }) : t('app.serverHint')}
      </p>
    </Section>
  );
}
