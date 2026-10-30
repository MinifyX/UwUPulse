import { Button, Dialog } from '@uwusuite/design';
import { useState, type ReactNode } from 'react';
import { t, useLanguage } from '../lib/i18n';

/** A question that throws something away: Cancel and a red action. */
export function ConfirmDialog({
  open,
  title,
  children,
  action,
  onConfirm,
  onClose,
}: {
  open: boolean;
  title: ReactNode;
  children: ReactNode;
  action: string;
  onConfirm: () => Promise<unknown> | void;
  onClose: () => void;
}) {
  useLanguage();
  const [busy, setBusy] = useState(false);
  const confirm = async () => {
    setBusy(true);
    try {
      await onConfirm();
      onClose();
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={title}
      width="sm"
      tone="warning"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t('Abbrechen')}
          </Button>
          <Button variant="danger" busy={busy} onClick={() => void confirm()}>
            {action}
          </Button>
        </>
      }
    >
      <p className="dialog-lead">{children}</p>
    </Dialog>
  );
}
