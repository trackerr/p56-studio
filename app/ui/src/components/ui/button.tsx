import * as React from 'react';
import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '@/lib/utils';

const buttonVariants = cva(
  'inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-[2px] font-sans text-[13px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-40',
  {
    variants: {
      variant: {
        /** the light: the one main action of a panel */
        default: 'bg-primary text-primary-foreground hover:bg-primary/85',
        secondary: 'border border-line text-foreground hover:border-primary/70 hover:text-primary',
        outline: 'border border-line text-foreground hover:border-primary/70 hover:text-primary',
        ghost: 'text-muted-foreground hover:text-primary',
        destructive: 'border border-[#5a2b25] text-destructive hover:border-destructive',
        link: 'text-primary underline underline-offset-4',
      },
      size: {
        default: 'h-11 px-4',
        sm: 'h-9 px-3 text-xs',
        icon: 'h-11 w-11',
        /** round transport keys (power, play, cast) */
        round: 'h-16 w-16 rounded-full',
      },
    },
    defaultVariants: { variant: 'default', size: 'default' },
  },
);

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(({ className, variant, size, asChild = false, ...props }, ref) => {
  const Comp = asChild ? Slot : 'button';
  return <Comp className={cn(buttonVariants({ variant, size, className }))} ref={ref} {...props} />;
});
Button.displayName = 'Button';

export { Button, buttonVariants };
