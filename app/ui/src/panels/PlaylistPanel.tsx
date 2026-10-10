import { useState } from 'react';
import { Eye, Pencil, Trash2, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Confirm, Section } from '@/components/bits';
import { invoke } from '@/lib/api';
import { useP56 } from '@/state';
import { NoFan } from './FanPanel';

export function PlaylistPanel() {
  const { fan, run } = useP56();
  const [edit, setEdit] = useState<{ slot: number; name: string } | null>(null);
  if (!fan) return <NoFan />;
  const cur = fan.status?.play_index ?? 0;
  return (
    <Section title={`Afspeellijst · ${fan.files.length} bestand(en)`}
      aside={<Button size="sm" variant="ghost" onClick={() => run('Vernieuwen', (i) => invoke('control', { id: i, action: 'refresh' }))}><RefreshCw className="mr-1 h-3.5 w-3.5" />Vernieuwen</Button>}>
      {fan.files.length === 0 ? (
        <p className="py-8 text-center text-sm text-muted-foreground" data-testid="playlist-empty">De afspeellijst is leeg. Voeg iets toe via <b>Media</b>.</p>
      ) : (
        <ul className="divide-y divide-border" data-testid="playlist">
          {fan.files.map(([n, name]) => (
            <li key={n} className="flex items-center gap-3 py-2">
              <span className={`w-7 text-right font-mono text-sm ${n === cur ? 'text-primary' : 'text-muted-foreground'}`}>{n}</span>
              <span className={`min-w-0 flex-1 truncate ${n === cur ? 'font-medium text-primary' : ''}`}>{name}{n === cur && <span className="ml-2 text-xs">● speelt</span>}</span>
              <Button size="icon" variant="ghost" title="Tonen" data-testid={`show-${n}`} onClick={() => run('Tonen', (i) => invoke('set_value', { id: i, what: 'select', value: n }))}><Eye className="h-4 w-4" /></Button>
              <Button size="icon" variant="ghost" title="Hernoemen" data-testid={`rename-${n}`} onClick={() => setEdit({ slot: n, name: name.replace(/\.mp4$/i, '') })}><Pencil className="h-4 w-4" /></Button>
              <Confirm title={`"${name}" wissen?`} action="Wissen" body="Het bestand wordt van de fan verwijderd."
                onConfirm={() => run('Wissen', (i) => invoke('set_value', { id: i, what: 'delete', value: n }))}
                trigger={(open) => <Button size="icon" variant="ghost" title="Wissen" data-testid={`delete-${n}`} onClick={open}><Trash2 className="h-4 w-4 text-destructive" /></Button>} />
            </li>
          ))}
        </ul>
      )}
      <Dialog open={!!edit} onOpenChange={(o) => !o && setEdit(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>Slot {edit?.slot} hernoemen</DialogTitle></DialogHeader>
          <form onSubmit={(e) => { e.preventDefault(); if (edit) run('Hernoemen', (i) => invoke('set_text', { id: i, what: 'rename', a: edit.name, slot: edit.slot })); setEdit(null); }}>
            <div className="flex items-center gap-2">
              <Input autoFocus value={edit?.name ?? ''} maxLength={60} data-testid="rename-input" onChange={(e) => edit && setEdit({ ...edit, name: e.target.value })} />
              <span className="text-sm text-muted-foreground">.mp4</span>
            </div>
            <DialogFooter className="mt-4"><Button type="submit" data-testid="rename-ok">Opslaan</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </Section>
  );
}
