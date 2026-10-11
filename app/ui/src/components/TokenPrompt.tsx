import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { onAuthNeeded, setApiToken } from '@/lib/api';
import { useT } from '@/lib/i18n';

/** Asks for the p56-server API token after the server answered 401 (LAN client without a valid token). */
export function TokenPrompt() {
  const { t } = useT();
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState('');
  useEffect(() => onAuthNeeded(setOpen), []);

  function save(e: React.FormEvent) {
    e.preventDefault();
    if (!value.trim()) return;
    setApiToken(value);
    // Reload so the initial data and the event stream are fetched with the new token.
    window.location.reload();
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent data-testid="token-prompt">
        <DialogHeader>
          <DialogTitle>{t('auth.title')}</DialogTitle>
          <DialogDescription>{t('auth.hint')}</DialogDescription>
        </DialogHeader>
        <form onSubmit={save}>
          <Input autoFocus type="password" autoComplete="off" value={value} placeholder={t('auth.placeholder')}
            data-testid="token-input" onChange={(e) => setValue(e.target.value)} />
          <DialogFooter className="mt-4">
            <Button type="submit" disabled={!value.trim()} data-testid="token-save">{t('common.save')}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
