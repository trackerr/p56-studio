import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { toast } from 'sonner';
import { t } from '@/lib/i18n';
import { DEFAULT_PLACEMENT, invoke, remoteServer, onEvent, type Device, type FanTypeInfo, type MediaInfo, type Picked, type Placement } from '@/lib/api';

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
  if (!c) throw new Error('useP56 used outside P56Provider');
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
  const [placement, setPlacement] = useState<Placement>(DEFAULT_PLACEMENT);
  const [media, setMedia] = useState<CurrentMedia | null>(null);
  const [picked, setPicked] = useState<Picked | null>(null);
  const started = useRef(false);

  const addLog = useCallback((m: string) => {
    const time = new Date().toLocaleTimeString();
    setLog((l) => [...l.slice(-300), `[${time}] ${m}`]);
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
      addLog(t('log.linkStarted', { ip, extra: s.targets.length ? ` + ${s.targets.join(', ')}` : '' }));
    } catch (e) {
      setLinkState('error');
      setLinkError(String(e));
      addLog(t('log.cannotStart', { error: String(e) }));
    }
  }, [addLog]);

  const restartLink = useCallback(async () => {
    // through a p56-server: never stop its link (other devices use it too)
    if (remoteServer) { await refresh(); return; }
    await invoke('stop_link').catch(() => {});
    setDevices([]);
    await startLink();
  }, [startLink, refresh]);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    invoke<Platform>('platform').then(setPlatform).catch(() => {});
    invoke<FanTypeInfo[]>('fan_types').then(setFanTypes).catch(() => {});
    startLink().then(refresh);
  }, [startLink, refresh]);

  useEffect(() => {
    const timer = setInterval(() => {
      refresh();
      invoke<{ discovered: string[] }>('link_info').then((i) => setDiscovered(i.discovered)).catch(() => {});
    }, 1500);
    return () => clearInterval(timer);
  }, [refresh]);

  useEffect(() => onEvent((e) => {
    const d = e.data ?? {};
    if (e.type === 'log') addLog(d.msg);
    else if (e.type === 'connected') { addLog(t('log.fanConnected', { id: d.id, ip: d.ip })); toast.success(t('toast.fanConnected'), { description: d.ip }); refresh(); }
    else if (e.type === 'disconnected') { addLog(t('log.fanGone', { id: d.id })); toast.warning(t('toast.fanDisconnected')); refresh(); }
    else if (e.type === 'discovered') addLog(t('log.fanSeen', { ip: d.ip }));
    else if (e.type === 'packet') {
      if (d.kind === 'error') toast.error(t('toast.fanError'), { description: d.msg });
      refresh();
    }
  }), [addLog, refresh]);

  const fan = useMemo(() => devices.find((d) => d.id === sel) ?? null, [devices, sel]);

  const run = useCallback(async <R,>(label: string, f: (id: number) => Promise<R>) => {
    if (!fan) { toast.error(t('common.noFan')); return undefined; }
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
