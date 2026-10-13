import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Slider } from '@/components/ui/slider';
import { IconAlert, type Icon } from '@/components/icons';
import { cn } from '@/lib/utils';
import { useT } from '@/lib/i18n';

// ---- the orbit: a round view of the fan with its controls on the rings around it ----

// geometry in a 640×640 viewBox: disc, inner ring, angle bezel
const C = 320, R_DISC = 230, R_ARC = 270, R_BEZEL = 304;
const DISC_INSET = `${((C - R_DISC) / 640) * 100}%`;
const rad = (deg: number) => (deg * Math.PI) / 180;
const polar = (deg: number, r: number) => [C + r * Math.cos(rad(deg)), C + r * Math.sin(rad(deg))] as const;
const circ = (r: number) => 2 * Math.PI * r;

export type RingControl = {
  value: number; min: number; max: number; step?: number;
  onCommit: (v: number) => void;
  label: string;
  format: (v: number) => string;
  testId?: string;
  disabled?: boolean;
};

/**
 * One draggable ring around the disc (role="slider"). Drag along it, or focus it and use the arrow
 * keys, Page Up/Down, Home and End. A drag commits on release, keys commit after a short pause.
 */
function useRing(c: RingControl, from: number, sweep: number, onPreview?: (v: number | null) => void) {
  const { value, min, max, step = 1, onCommit } = c;
  const [local, setLocal] = useState<number | null>(null);
  const dragging = useRef(false);
  const timers = useRef<{ key?: number; hold?: number }>({});
  useEffect(() => () => { clearTimeout(timers.current.key); clearTimeout(timers.current.hold); }, []);
  const show = (v: number | null) => { setLocal(v); onPreview?.(v); };
  const clamp = (v: number) => Math.min(max, Math.max(min, Math.round(v / step) * step));
  const commit = (v: number) => {
    onCommit(v);
    // keep showing the new value until the fan's status has caught up
    clearTimeout(timers.current.hold);
    timers.current.hold = window.setTimeout(() => show(null), 1200);
  };
  const fromPointer = (e: PointerEvent<SVGElement>) => {
    const box = (e.currentTarget.ownerSVGElement ?? (e.currentTarget as unknown as SVGSVGElement)).getBoundingClientRect();
    const theta = (Math.atan2(e.clientY - (box.top + box.height / 2), e.clientX - (box.left + box.width / 2)) * 180) / Math.PI;
    let rel = (((theta - from) % 360) + 360) % 360;
    if (rel > sweep) rel = rel > sweep + (360 - sweep) / 2 ? 0 : sweep; // in the gap: snap to the nearest end
    return clamp(min + (rel / sweep) * (max - min));
  };
  const current = local ?? value;
  const handlers = {
    onPointerDown: (e: PointerEvent<SVGElement>) => {
      if (c.disabled) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      dragging.current = true;
      clearTimeout(timers.current.hold);
      show(fromPointer(e));
    },
    onPointerMove: (e: PointerEvent<SVGElement>) => { if (dragging.current) show(fromPointer(e)); },
    onPointerUp: (e: PointerEvent<SVGElement>) => {
      if (!dragging.current) return;
      dragging.current = false;
      commit(fromPointer(e));
    },
    onPointerCancel: () => { if (dragging.current) { dragging.current = false; commit(current); } },
  };
  const onKeyDown = (e: KeyboardEvent<SVGGElement>) => {
    if (c.disabled) return;
    const big = Math.max(step, Math.round((max - min) / 10));
    const d: Record<string, number> = { ArrowRight: step, ArrowUp: step, ArrowLeft: -step, ArrowDown: -step, PageUp: big, PageDown: -big };
    let v: number;
    if (e.key === 'Home') v = min;
    else if (e.key === 'End') v = max;
    else if (e.key in d) v = clamp(current + d[e.key]);
    else return;
    e.preventDefault();
    show(v);
    clearTimeout(timers.current.key);
    clearTimeout(timers.current.hold);
    timers.current.key = window.setTimeout(() => commit(v), 250);
  };
  const angle = from + ((current - min) / (max - min || 1)) * sweep;
  return { current, angle, handlers, onKeyDown };
}

function useFocus() {
  const [focus, setFocus] = useState(false);
  return { focus, bind: { onFocus: () => setFocus(true), onBlur: () => setFocus(false) } };
}

function AngleBezel({ c, onPreview }: { c: RingControl; onPreview: (v: number | null) => void }) {
  // value 0 sits at 12 o'clock; -180 / +180 meet at 6 o'clock
  const { current, angle, handlers, onKeyDown } = useRing(c, -270, 360, onPreview);
  const { focus, bind } = useFocus();
  // Chrome on Android ignores touch-action inside an SVG: a drag along the ring then became a
  // page scroll after the first move (pointercancel). Hold on to a touch that starts on the ring.
  const grip = useRef<SVGCircleElement>(null);
  useEffect(() => {
    const el = grip.current;
    if (!el) return;
    const hold = (e: TouchEvent) => { if (e.cancelable) e.preventDefault(); };
    el.addEventListener('touchstart', hold, { passive: false });
    el.addEventListener('touchmove', hold, { passive: false });
    return () => { el.removeEventListener('touchstart', hold); el.removeEventListener('touchmove', hold); };
  }, []);
  const L = circ(R_BEZEL);
  // the label rides just inside the bezel
  const [tx, ty] = polar(angle, R_BEZEL - 22);
  return (
    <g data-testid={c.testId}>
      <g role="slider" tabIndex={c.disabled ? -1 : 0} aria-label={c.label} aria-valuemin={c.min} aria-valuemax={c.max} aria-valuenow={current}
        aria-valuetext={c.format(current)} aria-disabled={c.disabled || undefined} onKeyDown={onKeyDown} {...bind} style={{ outline: 'none' }}>
        {focus && <circle cx={C} cy={C} r={R_BEZEL} fill="none" stroke="hsl(var(--primary) / 0.12)" strokeWidth={26} />}
        <circle cx={C} cy={C} r={R_BEZEL} fill="none" stroke="hsl(var(--input))" strokeWidth={10} strokeDasharray={`1.2 ${L / 36 - 1.2}`} />
        <circle cx={C} cy={C} r={R_BEZEL} fill="none" stroke="hsl(var(--muted-foreground))" strokeWidth={16} strokeDasharray={`2 ${L / 4 - 2}`} />
        <path d={`M${C - 10} 2 L${C + 10} 2 L${C} 21 Z`} fill="hsl(var(--primary))" transform={`rotate(${angle + 90} ${C} ${C})`} />
        <circle ref={grip} cx={C} cy={C} r={R_BEZEL} fill="none" stroke="transparent" strokeWidth={40}
          style={{ pointerEvents: c.disabled ? 'none' : 'stroke', cursor: 'grab', touchAction: 'none' }} {...handlers} />
      </g>
      <text x={tx} y={ty} textAnchor="middle" dominantBaseline="middle" className="pointer-events-none fill-primary font-sans" fontSize={13} fontWeight={500}>{c.format(current)}</text>
    </g>
  );
}

/** The thin RGB fringe around the disc: the only colour in the interface, like the fan's LEDs. */
function Fringe() {
  return (
    <>
      <circle cx={C - 1.4} cy={C} r={R_DISC + 1} fill="none" stroke="var(--fringe-r)" strokeOpacity={0.5} />
      <circle cx={C} cy={C + 1.4} r={R_DISC + 1} fill="none" stroke="var(--fringe-g)" strokeOpacity={0.35} />
      <circle cx={C + 1.4} cy={C} r={R_DISC + 1} fill="none" stroke="var(--fringe-b)" strokeOpacity={0.55} />
    </>
  );
}

/**
 * The round view. `src` is an image of what the fan shows (else `children` are drawn in the disc).
 * Pass `angle` to make the outer ring the picture-angle control; without it the ring is decoration.
 */
export function Orbit({ src, children, spinning, dim, angle, className, label }: {
  src?: string | null; children?: ReactNode; spinning?: boolean; dim?: boolean;
  angle?: RingControl; className?: string; label?: string;
}) {
  const [turn, setTurn] = useState<number | null>(null);
  const rotate = turn ?? angle?.value ?? 0;
  return (
    <div className={cn('relative aspect-square w-full select-none', className)}>
      <div className={cn('absolute overflow-hidden rounded-full bg-black transition-opacity', dim && 'opacity-30')}
        style={{ inset: DISC_INSET }} role={label ? 'img' : undefined} aria-label={label}>
        <div className="h-full w-full transition-transform duration-300" style={{ transform: `rotate(${rotate}deg)` }}>
          {src ? <img src={src} alt="" className="h-full w-full" draggable={false} /> : children}
        </div>
        {spinning && <div aria-hidden="true" className="orbit-sweep pointer-events-none absolute inset-0 rounded-full" />}
        <div aria-hidden="true" className="pointer-events-none absolute left-1/2 top-1/2 h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full border border-line bg-[#141414]" />
      </div>
      <svg viewBox="0 0 640 640" className="pointer-events-none absolute inset-0 h-full w-full overflow-visible">
        <Fringe />
        {angle ? <AngleBezel c={angle} onPreview={setTurn} /> : (
          <>
            <circle cx={C} cy={C} r={R_BEZEL} fill="none" stroke="hsl(var(--input))" strokeWidth={10} strokeDasharray={`1.2 ${circ(R_BEZEL) / 36 - 1.2}`} />
            <circle cx={C} cy={C} r={R_BEZEL} fill="none" stroke="hsl(var(--muted-foreground))" strokeWidth={16} strokeDasharray={`2 ${circ(R_BEZEL) / 4 - 2}`} />
          </>
        )}
        <circle cx={C} cy={C} r={R_ARC} fill="none" stroke="hsl(var(--secondary))" strokeWidth={2} />
      </svg>
    </div>
  );
}

/** Text drawn on the disc the way a hologram looks: warm white with a red/blue fringe. */
export function DiscText({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={cn('grid h-full w-full place-items-center p-[14%] text-center font-display leading-tight text-primary [overflow-wrap:anywhere]', className)}
      style={{ textShadow: '-1.5px 0 rgb(255 45 85 / 0.55), 1.5px 0 rgb(61 123 255 / 0.6)' }}>
      {children}
    </div>
  );
}

/** Round preview of what the fan shows (media, cast). */
export function RoundView({ src, label, spinning }: { src?: string | null; label?: ReactNode; spinning?: boolean }) {
  const { t } = useT();
  return (
    <div className="flex flex-col items-center gap-3">
      <Orbit src={src} spinning={spinning} className="max-w-[380px]">
        <DiscText className="text-[11px] text-muted-foreground [text-shadow:none]">{t('common.noImage')}</DiscText>
      </Orbit>
      {label && <div className="text-center text-xs text-muted-foreground">{label}</div>}
    </div>
  );
}

// ---- small parts ----

/** Confirmation dialog for destructive actions. */
export function Confirm({ trigger, title, body, action, onConfirm, danger = true }: {
  trigger: (open: () => void) => ReactNode; title: string; body: ReactNode; action: string; onConfirm: () => void; danger?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const { t } = useT();
  return (
    <>
      {trigger(() => setOpen(true))}
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription asChild><div>{body}</div></DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="ghost" data-testid="confirm-cancel" onClick={() => setOpen(false)}>{t('common.cancel')}</Button>
            <Button variant={danger ? 'destructive' : 'default'} data-testid="confirm-ok" onClick={() => { setOpen(false); onConfirm(); }}>{action}</Button>
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
    <div className="grid grid-cols-[minmax(84px,auto)_1fr_56px] items-center gap-4" data-testid={id}>
      <span className="text-[13px]">{label}</span>
      <Slider value={[v]} min={min} max={max} step={step} disabled={disabled} aria-label={label}
        onValueChange={([n]) => setDrag(n)} onValueCommit={([n]) => { setDrag(null); onCommit(n); }} />
      <span className="orbit-value text-right text-[13px]">{format ? format(v) : v}</span>
    </div>
  );
}

/** A titled group: a hairline on top and a small wide-caps title. No boxes. */
export function Section({ title, children, aside, className }: { title: ReactNode; children: ReactNode; aside?: ReactNode; className?: string }) {
  return (
    <section className={cn('border-t border-border pt-4', className)}>
      <div className="mb-3 flex min-h-6 items-center justify-between gap-2">
        <h2 className="orbit-label flex items-center gap-2">{title}</h2>
        {aside}
      </div>
      {children}
    </section>
  );
}

/** One choice out of a few, drawn as orbits: the chosen one carries its planet. Arrow keys move. */
export function Choice<T extends string>({ label, value, options, onChange, disabled, className }: {
  label: string; value: T | ''; disabled?: boolean; className?: string;
  options: { value: T; label: string; testId?: string; icon?: Icon }[];
  onChange: (v: T) => void;
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const d = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!d || disabled) return;
    e.preventDefault();
    const i = Math.max(0, options.findIndex((o) => o.value === value));
    const n = (i + d + options.length) % options.length;
    refs.current[n]?.focus();
    onChange(options[n].value);
  };
  return (
    <div role="radiogroup" aria-label={label} className={cn('flex flex-wrap gap-x-5', className)} onKeyDown={onKey}>
      {options.map((o, i) => {
        const on = o.value === value;
        const I = o.icon;
        return (
          <button key={o.value} ref={(el) => { refs.current[i] = el; }} type="button" role="radio" aria-checked={on} disabled={disabled}
            tabIndex={on || (!value && i === 0) ? 0 : -1} data-testid={o.testId} data-state={on ? 'on' : 'off'} onClick={() => onChange(o.value)}
            className={cn('flex min-h-11 items-center gap-2.5 text-[13px] transition-colors disabled:opacity-40', on ? 'text-primary' : 'text-muted-foreground hover:text-foreground')}>
            <svg viewBox="0 0 16 16" aria-hidden="true" className="h-4 w-4 shrink-0">
              <circle cx="8" cy="8" r="7" fill="none" stroke="currentColor" strokeOpacity={on ? 1 : 0.7} />
              {on && <circle cx="8" cy="8" r="3.5" fill="currentColor" />}
            </svg>
            {I && <I className="h-4 w-4" />}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

/** A label above a value, e.g. "HOEK 0°". */
export function Readout({ label, children }: { label: string; children: ReactNode }) {
  return <span className="orbit-label">{label} <b className="ml-1 font-normal text-primary">{children}</b></span>;
}

/** A framed message (instead of a coloured alert box). */
export function Notice({ title, children, danger, className }: { title?: ReactNode; children: ReactNode; danger?: boolean; className?: string }) {
  return (
    <div role={danger ? 'alert' : 'note'} className={cn('flex gap-3 border p-3 text-[13px] leading-relaxed', danger ? 'border-[#5a2b25] text-destructive' : 'border-line text-foreground', className)}>
      <IconAlert className="mt-0.5" />
      <div className="min-w-0">
        {title && <div className="font-medium">{title}</div>}
        <div className={danger ? '' : 'text-muted-foreground'}>{children}</div>
      </div>
    </div>
  );
}

/** A status word with its own orbit dot (instead of coloured badges). */
export function Status({ on, children, testId }: { on?: boolean; children: ReactNode; testId?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-1.5', on ? 'text-primary' : 'text-muted-foreground')} data-testid={testId}>
      <span aria-hidden="true" className={cn('h-1.5 w-1.5 rounded-full', on ? 'bg-primary shadow-[0_0_8px_hsl(var(--primary))]' : 'border border-muted-foreground')} />
      {children}
    </span>
  );
}
