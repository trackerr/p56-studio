import { Fan, ListVideo, ImagePlus, Cast, Wifi, SlidersHorizontal } from 'lucide-react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Toaster } from '@/components/ui/sonner';
import { P56Provider, useP56 } from '@/state';
import { FanPanel } from '@/panels/FanPanel';
import { PlaylistPanel } from '@/panels/PlaylistPanel';
import { MediaPanel } from '@/panels/MediaPanel';
import { CastPanel } from '@/panels/CastPanel';
import { NetworkPanel } from '@/panels/NetworkPanel';
import { AppPanel } from '@/panels/AppPanel';
import { cn } from '@/lib/utils';

function Header() {
  const { devices, fan, selectFan, linkState } = useP56();
  const dot = { connected: 'bg-emerald-400 shadow-[0_0_8px_#34d399]', searching: 'bg-amber-400 animate-pulse', starting: 'bg-zinc-500', error: 'bg-red-500' }[linkState];
  const text = { connected: fan ? `${fan.name} · ${fan.ip}` : 'verbonden', searching: 'zoeken naar fan…', starting: 'starten…', error: 'fout' }[linkState];
  return (
    <header className="flex flex-wrap items-center gap-3 border-b border-border bg-card/80 px-4 py-2.5 pt-[max(0.625rem,env(safe-area-inset-top))] backdrop-blur">
      <div className="flex items-center gap-2">
        <div className="relative h-7 w-7 rounded-full border border-primary/50">
          <div className="absolute inset-0 animate-spin rounded-full border-t-2 border-primary [animation-duration:1.6s]" />
        </div>
        <span className="font-semibold tracking-tight">P56 Studio</span>
      </div>
      {devices.length > 1 && (
        <Select value={String(fan?.id ?? '')} onValueChange={(v) => selectFan(Number(v))}>
          <SelectTrigger className="h-8 w-auto min-w-40" data-testid="fan-select"><SelectValue /></SelectTrigger>
          <SelectContent>{devices.map((d) => <SelectItem key={d.id} value={String(d.id)}>{d.name} ({d.ip})</SelectItem>)}</SelectContent>
        </Select>
      )}
      <div className="ml-auto flex items-center gap-2 text-xs text-muted-foreground" data-testid="link-status">
        <span className={cn('h-2 w-2 rounded-full', dot)} />{text}
        {devices.length > 1 && <span>· {devices.length} fans</span>}
      </div>
    </header>
  );
}

const tabs = [
  ['fan', 'Fan', Fan, FanPanel],
  ['playlist', 'Afspeellijst', ListVideo, PlaylistPanel],
  ['media', 'Media', ImagePlus, MediaPanel],
  ['cast', 'Casten', Cast, CastPanel],
  ['network', 'Netwerk', Wifi, NetworkPanel],
  ['app', 'App', SlidersHorizontal, AppPanel],
] as const;

export default function App() {
  return (
    <P56Provider>
      <div className="flex h-full flex-col">
        <Header />
        <Tabs defaultValue="fan" className="flex min-h-0 flex-1 flex-col">
          <div className="border-b border-border bg-card/40">
            <TabsList className="h-auto w-full justify-start gap-1 overflow-x-auto rounded-none bg-transparent px-3 py-1.5 [scrollbar-width:none]">
              {tabs.map(([v, l, Icon]) => (
                <TabsTrigger key={v} value={v} data-testid={`tab-${v}`} className="gap-1.5 rounded-md px-3 py-1.5 data-[state=active]:bg-secondary">
                  <Icon className="h-4 w-4" /><span>{l}</span>
                </TabsTrigger>
              ))}
            </TabsList>
          </div>
          <main className="min-h-0 flex-1 overflow-auto px-4 py-4 pb-[max(1rem,env(safe-area-inset-bottom))]">
            <div className="mx-auto max-w-6xl">
              {tabs.map(([v, , , Panel]) => (
                <TabsContent key={v} value={v} className="mt-0"><Panel /></TabsContent>
              ))}
            </div>
          </main>
        </Tabs>
      </div>
      <Toaster position="bottom-center" theme="dark" richColors closeButton />
    </P56Provider>
  );
}
