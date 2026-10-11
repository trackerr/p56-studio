import * as React from 'react';
import * as SwitchPrimitives from '@radix-ui/react-switch';
import { cn } from '@/lib/utils';

/** Ring switch: an empty orbit when off, a lit planet in it when on. 44 px touch target. */
const Switch = React.forwardRef<
  React.ElementRef<typeof SwitchPrimitives.Root>,
  React.ComponentPropsWithoutRef<typeof SwitchPrimitives.Root>
>(({ className, ...props }, ref) => (
  <SwitchPrimitives.Root
    ref={ref}
    className={cn('group relative grid h-11 w-11 shrink-0 cursor-pointer place-items-center rounded-full disabled:cursor-not-allowed disabled:opacity-40', className)}
    {...props}
  >
    <span aria-hidden="true" className="h-[22px] w-[22px] rounded-full border border-muted-foreground/70 transition-colors group-hover:border-primary/70 group-data-[state=checked]:border-primary" />
    <SwitchPrimitives.Thumb className="pointer-events-none absolute left-1/2 top-1/2 h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-primary transition-transform data-[state=checked]:scale-100 data-[state=unchecked]:scale-0 data-[state=checked]:shadow-[0_0_8px_hsl(var(--primary))]" />
  </SwitchPrimitives.Root>
));
Switch.displayName = SwitchPrimitives.Root.displayName;

export { Switch };
