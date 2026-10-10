import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Section } from '@/components/bits';
import { invoke, isTauri } from '@/lib/api';
import { useP56 } from '@/state';

const ls = (k: string) => { try { return localStorage.getItem(k) || ''; } catch { return ''; } };
const lsSet = (k: string, v: string) => { try { localStorage.setItem(k, v); } catch { /* private mode */ } };

export function AppPanel() {
  const { fanTypes, log, restartLink, platform, localIp } = useP56();
  const [type, setType] = useState(ls('p56.type') || '0');
  const [ip, setIp] = useState(ls('p56.ip'));
  const [targets, setTargets] = useState(ls('p56.targets'));
  const [info, setInfo] = useState<{ ip: string; port: number; interfaces: string[]; discovered: string[] } | null>(null);
  const logRef = useRef<HTMLPreElement>(null);

  useEffect(() => { invoke('set_fan_type', { fanType: Number(type) }).catch(() => {}); lsSet('p56.type', type); }, [type]);
  useEffect(() => {
    const f = () => invoke<typeof info>('link_info').then(setInfo).catch(() => {});
    f(); const t = setInterval(f, 2000); return () => clearInterval(t);
  }, []);
  useEffect(() => { logRef.current?.scrollTo(0, logRef.current.scrollHeight); }, [log]);

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <Section title="Fantype">
        <Select value={type} onValueChange={setType}>
          <SelectTrigger data-testid="fan-type"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="0">Automatisch (wat de fan meldt)</SelectItem>
            {fanTypes.map((t) => (
              <SelectItem key={t.id} value={String(t.id)} disabled={t.support === 'unsupported'}>
                type {t.id} · {t.angles}×{t.leds}{t.jpeg ? ` · JPEG ${t.jpeg}` : ''}{t.id === 8 ? ' · P56' : ''}{t.support === 'unsupported' ? ' · niet ondersteund' : t.support === 'legacy' ? ' · oud formaat' : ''}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="mt-2 text-xs text-muted-foreground">Bepaalt resolutie en armindeling van het beeld. De P56 is type 8.</p>
      </Section>

      <Section title="Verbinding">
        <div className="space-y-3">
          <div className="space-y-1"><Label className="text-xs text-muted-foreground" htmlFor="set-ip">IP dat wordt uitgezonden (leeg = automatisch per netwerkkaart)</Label>
            <Input id="set-ip" value={ip} placeholder="bv. 192.168.4.2" onChange={(e) => setIp(e.target.value)} /></div>
          <div className="space-y-1"><Label className="text-xs text-muted-foreground" htmlFor="set-targets">IP van de fan, als je het weet (komma's tussen meerdere)</Label>
            <Input id="set-targets" value={targets} placeholder="bv. 192.168.1.50" onChange={(e) => setTargets(e.target.value)} /></div>
          <Button data-testid="apply-net" onClick={async () => { lsSet('p56.ip', ip.trim()); lsSet('p56.targets', targets.trim()); await restartLink(); toast.success('Opnieuw verbonden'); }}>Opslaan en opnieuw verbinden</Button>
        </div>
        <pre className="mt-3 overflow-auto rounded-md bg-black/40 p-2 font-mono text-[11px] leading-relaxed text-muted-foreground">
{info ? `uitzenden als ${info.ip} · TCP-poort ${info.port}\n${info.interfaces.map((i) => `netwerkkaart ${i}`).join('\n')}\n${info.discovered.length ? `fans gezien via UDP: ${info.discovered.join(', ')}` : 'nog geen fan gezien via UDP'}` : `uitzenden als ${localIp || '…'}`}
        </pre>
      </Section>

      <Section title="Logboek" className="lg:col-span-2">
        <pre ref={logRef} data-testid="log" className="h-56 overflow-auto whitespace-pre-wrap rounded-md bg-black/40 p-2 font-mono text-[11px] text-muted-foreground">{log.join('\n') || '—'}</pre>
        <p className="mt-2 text-xs text-muted-foreground">P56 Studio {platform.version} · {platform.os || (isTauri ? '' : 'browser (demo-fan)')}</p>
      </Section>
    </div>
  );
}
