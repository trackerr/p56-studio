import { useState, type ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Slider } from '@/components/ui/slider';
import { cn } from '@/lib/utils';

/** Round "blade" view of what the fan shows. */
export function RoundView({ src, label, spinning }: { src?: string | null; label?: ReactNode; spinning?: boolean }) {
  return (
    <div className="flex flex-col items-center gap-2">
      <div className={cn('relative aspect-square w-[min(320px,78vw)] overflow-hidden rounded-full border border-border bg-black shadow-[inset_0_0_60px_rgba(57,208,255,0.10)]')}>
        {src ? <img src={src} alt="" className="h-full w-full" /> : (
          <div className="absolute inset-0 grid place-items-center text-xs text-muted-foreground">geen beeld</div>
        )}
        <div className={cn('pointer-events-none absolute inset-0 rounded-full', spinning && 'blade-sweep')} />
        <div className="pointer-events-none absolute left-1/2 top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-zinc-800 ring-1 ring-zinc-600" />
      </div>
      {label && <div className="text-center text-xs text-muted-foreground">{label}</div>}
    </div>
  );
}

/** Confirmation dialog for destructive actions. */
export function Confirm({ trigger, title, body, action, onConfirm, danger = true }: {
  trigger: (open: () => void) => ReactNode; title: string; body: ReactNode; action: string; onConfirm: () => void; danger?: boolean;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      {trigger(() => setOpen(true))}
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription asChild><div>{body}</div></DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2">
            <Button variant="ghost" onClick={() => setOpen(false)}>Annuleren</Button>
            <Button variant={danger ? 'destructive' : 'default'} onClick={() => { setOpen(false); onConfirm(); }}>{action}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

/** Slider that shows its value and only commits when released (one command per drag). */
export function ValueSlider({ label, value, min, max, step = 1, format, onCommit, disabled, id }: {
  label: string; value: number; min: number; max: number; step?: number; format?: (v: number) => string;
  onCommit: (v: number) => void; disabled?: boolean; id?: string;
}) {
  const [drag, setDrag] = useState<number | null>(null);
  const v = drag ?? value;
  return (
    <div className="grid grid-cols-[88px_1fr_52px] items-center gap-3 py-2" data-testid={id}>
      <span className="text-xs text-muted-foreground">{label}</span>
      <Slider value={[v]} min={min} max={max} step={step} disabled={disabled}
        onValueChange={([n]) => setDrag(n)} onValueCommit={([n]) => { setDrag(null); onCommit(n); }} />
      <span className="text-right font-mono text-sm tabular-nums">{format ? format(v) : v}</span>
    </div>
  );
}

export function Section({ title, children, aside, className }: { title: ReactNode; children: ReactNode; aside?: ReactNode; className?: string }) {
  return (
    <section className={cn('rounded-lg border border-border bg-card/60 p-4', className)}>
      <div className="mb-2 flex items-center justify-between gap-2">
        <h3 className="text-[11px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">{title}</h3>
        {aside}
      </div>
      {children}
    </section>
  );
}
