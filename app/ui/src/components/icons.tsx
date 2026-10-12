// The app's own line icons (24×24, 1.6 stroke), drawn for the "Baan" look instead of a stock set.
import type { ReactNode, SVGProps } from 'react';
import { cn } from '@/lib/utils';

export type IconProps = SVGProps<SVGSVGElement> & { className?: string };
export type Icon = (p: IconProps) => ReactNode;

const icon = (body: ReactNode): Icon => function I({ className, ...rest }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round"
      aria-hidden="true" focusable="false" className={cn('h-4 w-4 shrink-0', className)} {...rest}>{body}</svg>
  );
};

export const IconPower = icon(<><path d="M12 3v8" /><path d="M6.3 6.3a8 8 0 1 0 11.4 0" /></>);
export const IconPlay = icon(<path d="M7 4.5 19 12 7 19.5z" />);
export const IconPause = icon(<><path d="M8 5v14" /><path d="M16 5v14" /></>);
export const IconNext = icon(<><path d="M6 5l9 7-9 7z" /><path d="M18 5v14" /></>);
export const IconStop = icon(<rect x="6" y="6" width="12" height="12" />);
/** Clockwise turn (the picture rotates 90° to the right). */
export const IconRotate = icon(<><path d="M20 12a8 8 0 1 1-2.34-5.66" /><path d="M20 4v5h-5" /><path d="M12 9v3l2 1.5" /></>);
export const IconRefresh = icon(<><path d="M20 12a8 8 0 1 1-2.34-5.66" /><path d="M20 4v5h-5" /></>);
export const IconEye = icon(<><path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z" /><circle cx="12" cy="12" r="3" /></>);
export const IconPencil = icon(<><path d="M4 20l4-1L19 8l-3-3L5 16z" /><path d="M14 7l3 3" /></>);
export const IconTrash = icon(<><path d="M4 7h16" /><path d="M9 7V4h6v3" /><path d="M6 7l1 13h10l1-13" /></>);
export const IconUpload = icon(<><path d="M12 16V4" /><path d="M7 9l5-5 5 5" /><path d="M4 20h16" /></>);
export const IconImage = icon(<><rect x="3" y="4" width="18" height="16" /><circle cx="8.5" cy="9.5" r="1.5" /><path d="M21 16l-5-5-9 9" /></>);
/** Concentric rings: the test card. */
export const IconRings = icon(<><circle cx="12" cy="12" r="9" /><circle cx="12" cy="12" r="5.5" /><circle cx="12" cy="12" r="2" /></>);
export const IconFilm = icon(<><rect x="3" y="4" width="18" height="16" /><path d="M7 4v16M17 4v16M3 9h4M3 15h4M17 9h4M17 15h4" /></>);
export const IconX = icon(<><path d="M6 6l12 12" /><path d="M18 6 6 18" /></>);
export const IconType = icon(<><path d="M5 7V4h14v3" /><path d="M12 4v16" /><path d="M9 20h6" /></>);
export const IconCamera = icon(<><path d="M3 8h4l2-3h6l2 3h4v11H3z" /><circle cx="12" cy="13.5" r="3.5" /></>);
export const IconMonitor = icon(<><rect x="3" y="4" width="18" height="12" /><path d="M8 20h8M12 16v4" /></>);
export const IconCast = icon(<><circle cx="12" cy="12" r="1.5" /><path d="M8.5 15.5a5 5 0 0 1 0-7M15.5 8.5a5 5 0 0 1 0 7M5.6 18.4a9 9 0 0 1 0-12.8M18.4 5.6a9 9 0 0 1 0 12.8" /></>);
export const IconRouter = icon(<><rect x="3" y="13" width="18" height="7" /><path d="M7 16.5h.01M11 16.5h.01" /><path d="M8.5 9.5a5 5 0 0 1 7 0M6 7a8.5 8.5 0 0 1 12 0" /></>);
export const IconWifi = icon(<><path d="M2 9a14.5 14.5 0 0 1 20 0" /><path d="M5 12.5a10 10 0 0 1 14 0" /><path d="M8.5 16a5 5 0 0 1 7 0" /><path d="M12 19.5h.01" /></>);
export const IconTag = icon(<><path d="M3 12V3h9l9 9-9 9z" /><circle cx="7.5" cy="7.5" r="1.5" /></>);
export const IconRepeat = icon(<><path d="M17 2l4 4-4 4" /><path d="M3 11V9a3 3 0 0 1 3-3h15" /><path d="M7 22l-4-4 4-4" /><path d="M21 13v2a3 3 0 0 1-3 3H3" /></>);
export const IconRepeatOne = icon(<><path d="M17 2l4 4-4 4" /><path d="M3 11V9a3 3 0 0 1 3-3h15" /><path d="M7 22l-4-4 4-4" /><path d="M21 13v2a3 3 0 0 1-3 3H3" /><path d="M11 10h1v4" /></>);
export const IconAlert = icon(<><path d="M12 3l10 18H2z" /><path d="M12 10v5M12 18h.01" /></>);
export const IconSparkle = icon(<><path d="M11 3l1.8 5.2L18 10l-5.2 1.8L11 17l-1.8-5.2L4 10l5.2-1.8z" /><path d="M19 15v5M16.5 17.5h5" /></>);
export const IconPlus = icon(<><path d="M12 5v14" /><path d="M5 12h14" /></>);
export const IconCheck = icon(<path d="M5 12l5 5 9-10" />);
export const IconChevron = icon(<path d="M6 9l6 6 6-6" />);

/** The P56 mark: a ring with one blade pointing to 12 o'clock. */
export function Mark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 30 30" aria-hidden="true" focusable="false" className={cn('h-7 w-7 shrink-0', className)}>
      <circle cx="15" cy="15" r="13" fill="none" stroke="currentColor" strokeWidth="1.2" />
      <line x1="15" y1="15" x2="15" y2="3" stroke="currentColor" strokeWidth="2" />
      <circle cx="15" cy="15" r="2.4" fill="currentColor" />
    </svg>
  );
}

/** Navigation glyph: a small orbit; the active one carries its planet. */
export function OrbitGlyph({ active, className }: { active?: boolean; className?: string }) {
  return (
    <svg viewBox="0 0 22 22" aria-hidden="true" focusable="false" className={cn('h-[22px] w-[22px] shrink-0', className)}>
      {active
        ? <><circle cx="11" cy="11" r="9" fill="none" stroke="currentColor" /><circle cx="11" cy="2" r="2.5" fill="currentColor" /></>
        : <circle cx="11" cy="11" r="4" fill="none" stroke="currentColor" />}
    </svg>
  );
}
