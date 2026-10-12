import { useState } from 'react';
import { IconEye, IconPencil, IconPlus, IconRefresh, IconTrash } from '@/components/icons';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Confirm, Section } from '@/components/bits';
import { invoke } from '@/lib/api';
import { useT } from '@/lib/i18n';
import { useNav } from '@/lib/nav';
import { cn } from '@/lib/utils';
import { useP56 } from '@/state';
import { NoFan, stem } from './FanPanel';

export function PlaylistPanel() {
  const { fan, run } = useP56();
  const { t, rich } = useT();
  const go = useNav();
  const [edit, setEdit] = useState<{ slot: number; name: string } | null>(null);
  if (!fan) return <NoFan />;
  const cur = fan.status?.play_index ?? 0;
  return (
    <Section title={t('playlist.title', { count: fan.files.length })} className="mx-auto max-w-3xl border-t-0 pt-0"
      aside={<Button size="sm" variant="ghost" onClick={() => run(t('common.refresh'), (i) => invoke('control', { id: i, action: 'refresh' }))}><IconRefresh />{t('common.refresh')}</Button>}>
      {fan.files.length === 0 ? (
        <div className="flex flex-col items-center gap-4 py-14 text-center" data-testid="playlist-empty">
          <span aria-hidden="true" className="h-16 w-16 rounded-full border border-dashed border-muted-foreground/70" />
          <p className="max-w-xs text-[13px] text-muted-foreground">
            {rich('playlist.empty', { media: <button type="button" className="text-primary underline underline-offset-4" onClick={() => go('media')}>{t('tab.media')}</button> })}
          </p>
        </div>
      ) : (
        <ol className="relative" data-testid="playlist">
          {/* the orbits hang on one vertical line */}
          <div aria-hidden="true" className="absolute bottom-6 left-[22px] top-6 w-px bg-border" />
          {fan.files.map(([n, name]) => {
            const on = n === cur;
            return (
              <li key={n} className="relative flex min-h-16 items-center gap-4 py-1.5">
                <span className={cn('grid h-11 w-11 shrink-0 place-items-center rounded-full bg-black font-display text-[10px]',
                  on ? 'border-2 border-primary text-primary shadow-[0_0_0_4px_hsl(var(--background)),0_0_0_5px_hsl(var(--input))]' : 'border border-line text-muted-foreground')}>
                  {String(n).padStart(2, '0')}
                </span>
                <span className={cn('min-w-0 flex-1 truncate text-[13px]', on && 'text-primary')}>
                  {stem(name)}
                  {on && <span className="orbit-label ml-3 text-primary">{t('playlist.playing')}</span>}
                </span>
                <div className="flex shrink-0">
                  <Button size="icon" variant="ghost" title={t('playlist.show')} aria-label={t('playlist.show')} data-testid={`show-${n}`}
                    onClick={() => run(t('playlist.show'), (i) => invoke('set_value', { id: i, what: 'select', value: n }))}><IconEye /></Button>
                  <Button size="icon" variant="ghost" title={t('playlist.rename')} aria-label={t('playlist.rename')} data-testid={`rename-${n}`}
                    onClick={() => setEdit({ slot: n, name: name.replace(/\.mp4$/i, '') })}><IconPencil /></Button>
                  <Confirm title={t('playlist.deleteTitle', { name: stem(name) })} action={t('playlist.delete')} body={t('playlist.deleteBody')}
                    onConfirm={() => run(t('playlist.delete'), (i) => invoke('set_value', { id: i, what: 'delete', value: n }))}
                    trigger={(open) => <Button size="icon" variant="ghost" className="hover:text-destructive" title={t('playlist.delete')} aria-label={t('playlist.delete')} data-testid={`delete-${n}`} onClick={open}><IconTrash /></Button>} />
                </div>
              </li>
            );
          })}
          <li className="relative flex min-h-16 items-center py-1.5">
            <button type="button" onClick={() => go('media')} className="flex items-center gap-4 text-[13px] text-muted-foreground transition-colors hover:text-primary">
              <span className="grid h-11 w-11 place-items-center rounded-full border border-dashed border-muted-foreground/70 bg-background"><IconPlus /></span>
              {t('strip.add')}
            </button>
          </li>
        </ol>
      )}
      <Dialog open={!!edit} onOpenChange={(o) => !o && setEdit(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t('playlist.renameTitle', { slot: edit?.slot ?? '' })}</DialogTitle></DialogHeader>
          <form onSubmit={(e) => { e.preventDefault(); if (edit) run(t('playlist.rename'), (i) => invoke('set_text', { id: i, what: 'rename', a: edit.name, slot: edit.slot })); setEdit(null); }}>
            <div className="flex items-center gap-2">
              <Input autoFocus value={edit?.name ?? ''} maxLength={24} data-testid="rename-input" aria-label={t('playlist.rename')}
                onChange={(e) => edit && setEdit({ ...edit, name: e.target.value })} />
              <span className="text-[13px] text-muted-foreground">.mp4</span>
            </div>
            <DialogFooter className="mt-5"><Button type="submit" data-testid="rename-ok">{t('common.save')}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </Section>
  );
}
