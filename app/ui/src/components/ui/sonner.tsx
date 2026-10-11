import { Toaster as Sonner } from 'sonner';
import { useT } from '@/lib/i18n';

type ToasterProps = React.ComponentProps<typeof Sonner>;

/** Notifications: square, hairline border, mono text; colour only on the leading glyph. */
const Toaster = ({ toastOptions, ...props }: ToasterProps) => {
  const { t } = useT();
  return (
    <Sonner
      theme="dark"
      className="toaster group"
      containerAriaLabel={t('common.notifications')}
      toastOptions={{
        closeButtonAriaLabel: t('common.close'),
        classNames: {
          toast: 'group toast !rounded-[2px] !border !border-line !bg-card !font-sans !text-[13px] !text-foreground !shadow-none',
          title: '!font-medium',
          description: '!text-muted-foreground',
          icon: '[&_svg]:!text-primary',
          error: '[&_[data-icon]_svg]:!text-destructive',
          closeButton: '!border-line !bg-card !text-muted-foreground hover:!text-primary',
          actionButton: '!bg-primary !text-primary-foreground',
          cancelButton: '!bg-secondary !text-muted-foreground',
        },
        ...toastOptions,
      }}
      {...props}
    />
  );
};

export { Toaster };
