import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { toast } from 'sonner';
import { invoke, onEvent, type Device, type FanTypeInfo, type MediaInfo, type Picked, type Placement } from '@/lib/api';

export type CurrentMedia = MediaInfo & { picked?: Picked };
type Platform = { os: string; screen: boolean; version: string };

type Ctx = {
  devices: Device[];
  fan: Device | null;
  selectFan: (id: number) => void;
  linkState: 'starting' | 'searching' | 'connected' | 'error';
  linkError: string;
  localIp: string;
  discovered: string[];
  log: string[];
  addLog: (m: string) => void;
  platform: Platform;
  fanTypes: FanTypeInfo[];
  placement: Placement;
  setPlacement: (p: Placement) => void;
  media: CurrentMedia | null;
  setMedia: (m: CurrentMedia | null) => void;
  /** Last picked file (also a video before frames are made). */
  picked: Picked | null;
  setPicked: (p: Picked | null) => void;
  refresh: () => Promise<void>;
  /** Run a backend call for the selected fan with error toast. */
  run: <R>(label: string, f: (id: number) => Promise<R>) => Promise<R | undefined>;
  restartLink: () => Promise<void>;
};

const C = createContext<Ctx | null>(null);
export const useP56 = () => {
  const c = useContext(C);
  if (!c) throw new Error('useP56 buiten provider');
  return c;
};

const settings = () => {
  try {
    return { ip: localStorage.getItem('p56.ip') || '', targets: (localStorage.getItem('p56.targets') || '').split(',').map((s) => s.trim()).filter(Boolean) };
  } catch { return { ip: '', targets: [] as string[] }; }
};

export function P56Provider({ children }: { children: ReactNode }) {
  const [devices, setDevices] = useState<Device[]>([]);
  const [sel, setSel] = useState<number | null>(null);
  const [linkState, setLinkState] = useState<Ctx['linkState']>('starting');
  const [linkError, setLinkError] = useState('');
  const [localIp, setLocalIp] = useState('');
  const [discovered, setDiscovered] = useState<string[]>([]);
  const [log, setLog] = useState<string[]>([]);
  const [platform, setPlatform] = useState<Platform>({ os: '', screen: false, version: '' });
  const [fanTypes, setFanTypes] = useState<FanTypeInfo[]>([]);
  const [placement, setPlacement] = useState<Placement>({ x: 0, y: 0, scale: 1, mirror: false });
  const [media, setMedia] = useState<CurrentMedia | null>(null);
  const [picked, setPicked] = useState<Picked | null>(null);
  const started = useRef(false);

  const addLog = useCallback((m: string) => {
    const t = new Date().toLocaleTimeString();
    setLog((l) => [...l.slice(-300), `[${t}] ${m}`]);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const d = await invoke<Device[]>('devices');
      setDevices(d);
      setLinkState(d.length ? 'connected' : 'searching');
      setSel((s) => (s != null && d.some((x) => x.id === s) ? s : d[0]?.id ?? null));
    } catch { /* link not started yet */ }
  }, []);

  const startLink = useCallback(async () => {
    const s = settings();
    try {
      const ip = await invoke<string>('start_link', { ip: s.ip || null, targets: s.targets });
      setLocalIp(ip);
      setLinkState('searching');
      setLinkError('');
      addLog(`verbinding gestart, uitzenden als ${ip}${s.targets.length ? ` + ${s.targets.join(', ')}` : ''}`);
    } catch (e) {
      setLinkState('error');
      setLinkError(String(e));
      addLog(`kan niet starten: ${e}`);
    }
  }, [addLog]);

  const restartLink = useCallback(async () => {
    await invoke('stop_link').catch(() => {});
    setDevices([]);
    await startLink();
  }, [startLink]);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    invoke<Platform>('platform').then(setPlatform).catch(() => {});
    invoke<FanTypeInfo[]>('fan_types').then(setFanTypes).catch(() => {});
    startLink().then(refresh);
  }, [startLink, refresh]);

  useEffect(() => {
    const t = setInterval(() => {
      refresh();
      invoke<{ discovered: string[] }>('link_info').then((i) => setDiscovered(i.discovered)).catch(() => {});
    }, 1500);
    return () => clearInterval(t);
  }, [refresh]);

  useEffect(() => onEvent((e) => {
    const d = e.data ?? {};
    if (e.type === 'log') addLog(d.msg);
    else if (e.type === 'connected') { addLog(`fan #${d.id} verbonden (${d.ip})`); toast.success('Fan verbonden', { description: d.ip }); refresh(); }
    else if (e.type === 'disconnected') { addLog(`fan #${d.id} weg`); toast.warning('Verbinding met fan verbroken'); refresh(); }
    else if (e.type === 'discovered') addLog(`fan gezien: ${d.ip}`);
    else if (e.type === 'packet') {
      if (d.kind === 'error') toast.error('De fan meldt een fout', { description: d.msg });
      refresh();
    }
  }), [addLog, refresh]);

  const fan = useMemo(() => devices.find((d) => d.id === sel) ?? null, [devices, sel]);

  const run = useCallback(async <R,>(label: string, f: (id: number) => Promise<R>) => {
    if (!fan) { toast.error('Geen fan verbonden'); return undefined; }
    try {
      const r = await f(fan.id);
      setTimeout(refresh, 150);
      return r;
    } catch (e) {
      toast.error(label, { description: String(e) });
      addLog(`${label}: ${e}`);
      return undefined;
    }
  }, [fan, refresh, addLog]);

  const value: Ctx = {
    devices, fan, selectFan: setSel, linkState, linkError, localIp, discovered, log, addLog, platform, fanTypes,
    placement, setPlacement, media, setMedia, picked, setPicked, refresh, run, restartLink,
  };
  return <C.Provider value={value}>{children}</C.Provider>;
}
