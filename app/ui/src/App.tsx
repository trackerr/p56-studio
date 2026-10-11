import { useState } from 'react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Toaster } from '@/components/ui/sonner';
import { TokenPrompt } from '@/components/TokenPrompt';
import { Mark, OrbitGlyph } from '@/components/icons';
import { P56Provider, useP56 } from '@/state';
import { FanPanel } from '@/panels/FanPanel';
import { PlaylistPanel } from '@/panels/PlaylistPanel';
import { MediaPanel } from '@/panels/MediaPanel';
import { CastPanel } from '@/panels/CastPanel';
import { NetworkPanel } from '@/panels/NetworkPanel';
import { AppPanel } from '@/panels/AppPanel';
import { cn } from '@/lib/utils';
import { I18nProvider, LangSwitch, useT } from '@/lib/i18n';
import { NavProvider, type TabId } from '@/lib/nav';

function Header() {
  const { devices, fan, selectFan, linkState } = useP56();
  const { t } = useT();
  const dot = {
    connected: 'bg-primary shadow-[0_0_10px_hsl(var(--primary))]',
    searching: 'border border-muted-foreground animate-pulse',
    starting: 'bg-muted-foreground',
    error: 'bg-destructive',
  }[linkState];
  const text = linkState === 'connected' && fan ? `${fan.name} · ${fan.ip}` : t(`link.${linkState}`);
  return (
    <header className="flex items-center gap-4 border-b border-border px-4 pb-1.5 pt-[max(0.375rem,env(safe-area-inset-top))] lg:gap-8 lg:px-8">
      <div className="flex shrink-0 items-center gap-3 text-primary">
        <Mark />
        <span className="hidden font-display text-[11px] tracking-[0.26em] text-foreground sm:inline">P56 STUDIO</span>
      </div>
      {devices.length > 1 && (
        <div role="radiogroup" aria-label={t('header.pickFan')} data-testid="fan-select" className="flex min-w-0 gap-1 overflow-x-auto [scrollbar-width:none]">
          {devices.map((d) => {
            const on = d.id === fan?.id;
            return (
              <button key={d.id} type="button" role="radio" aria-checked={on} title={d.ip} onClick={() => selectFan(d.id)}
                className={cn('flex min-h-11 shrink-0 items-center gap-2.5 px-2.5 font-display text-[10px] uppercase tracking-[0.16em] transition-colors',
                  on ? 'text-primary' : 'text-muted-foreground hover:text-foreground')}>
                <svg viewBox="0 0 16 16" aria-hidden="true" className="h-4 w-4">
                  <circle cx="8" cy="8" r="7" fill="none" stroke="currentColor" />
                  {on && <circle cx="8" cy="8" r="3.5" fill="currentColor" />}
                </svg>
                {d.name}
              </button>
            );
          })}
        </div>
      )}
      <div className="ml-auto flex min-w-0 items-center gap-3 lg:gap-5">
        <div className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground" data-testid="link-status">
          <span className={cn('h-[7px] w-[7px] shrink-0 rounded-full', dot)} />
          <span className="truncate">{text}</span>
          {devices.length > 1 && <span className="hidden shrink-0 md:inline">· {t('header.fans', { count: devices.length })}</span>}
        </div>
        <LangSwitch className="shrink-0" />
      </div>
    </header>
  );
}

const tabs = [
  ['fan', 'tab.fan', 'tab.fan', FanPanel],
  ['playlist', 'tab.playlist', 'tabShort.playlist', PlaylistPanel],
  ['media', 'tab.media', 'tab.media', MediaPanel],
  ['cast', 'tab.cast', 'tabShort.cast', CastPanel],
  ['network', 'tab.network', 'tabShort.network', NetworkPanel],
  ['app', 'tab.app', 'tab.app', AppPanel],
] as const;

function Shell() {
  const { t } = useT();
  const [tab, setTab] = useState<TabId>('fan');
  return (
    <P56Provider>
      <NavProvider value={setTab}>
        <div className="flex h-full flex-col">
          <Header />
          <Tabs value={tab} onValueChange={(v) => setTab(v as TabId)} className="flex min-h-0 flex-1 flex-col lg:flex-row">
            {/* phone: a bar at the bottom; desktop: a rail on the left */}
            <TabsList aria-label={t('nav.label')}
              className="order-last grid grid-cols-6 border-t border-border px-1 pb-[max(0.375rem,env(safe-area-inset-bottom))] pt-1 lg:order-first lg:flex lg:w-[208px] lg:shrink-0 lg:flex-col lg:gap-0.5 lg:border-t-0 lg:px-8 lg:pb-8 lg:pt-7">
              {tabs.map(([v, label, short]) => (
                <TabsTrigger key={v} value={v} data-testid={`tab-${v}`}
                  className="flex min-h-[52px] min-w-0 flex-col items-center justify-center gap-1.5 font-display text-[8px] uppercase tracking-[0.12em] text-muted-foreground hover:text-foreground data-[state=active]:text-primary lg:min-h-12 lg:flex-row lg:justify-start lg:gap-3.5 lg:text-[10px] lg:tracking-[0.16em]">
                  <OrbitGlyph active={tab === v} />
                  <span className="hidden lg:inline">{t(label)}</span>
                  <span className="w-full truncate px-0.5 text-center lg:hidden">{t(short)}</span>
                </TabsTrigger>
              ))}
            </TabsList>
            <main className="min-h-0 flex-1 overflow-auto px-4 py-5 lg:px-8 lg:py-7">
              <div className="mx-auto max-w-[1180px]">
                {tabs.map(([v, , , Panel]) => (
                  <TabsContent key={v} value={v}><Panel /></TabsContent>
                ))}
              </div>
            </main>
          </Tabs>
        </div>
        <Toaster position="bottom-center" closeButton mobileOffset={{ bottom: 'calc(80px + env(safe-area-inset-bottom))' }} />
        <TokenPrompt />
      </NavProvider>
    </P56Provider>
  );
}

export default function App() {
  return (
    <I18nProvider>
      <Shell />
    </I18nProvider>
  );
}
