import { Button, Dialog, Nyu } from '@uwusuite/design';
import { useEffect, useState } from 'react';
import { api, errorText, events, type PairRequest } from '../api';
import { formatCode } from '../lib/format.ts';
import { t, useLanguage } from '../lib/i18n';
import { useNow } from '../lib/pulse';
import { toasts } from '../lib/toasts';

/** The core drops an unanswered request after this long (docs/plan.md). */
const REQUEST_MS = 120_000;

/**
 * An iPhone asks to pair by code comparison: both screens show the same six
 * digits, the user accepts here only if they match. Shown over any page.
 */
export function PairRequestDialog() {
  useLanguage();
  const [request, setRequest] = useState<(PairRequest & { until: number }) | null>(null);
  const [busy, setBusy] = useState<'accept' | 'reject' | null>(null);
  const now = useNow(1000);

  useEffect(() => {
    const offRequest = events.onPairRequest((r) =>
      setRequest({ ...r, until: Date.now() + REQUEST_MS }),
    );
    const offDone = events.onPairDone((done) =>
      setRequest((r) => {
        if (!r || r.requestId !== done.requestId) return r;
        if (done.ok)
          toasts.show(t('{name} ist gekoppelt.', { name: r.deviceName }), { tone: 'success' });
        return null;
      }),
    );
    return () => {
      offRequest();
      offDone();
    };
  }, []);

  const left = request ? Math.max(0, Math.ceil((request.until - now) / 1000)) : 0;
  useEffect(() => {
    if (request && left === 0) setRequest(null);
  }, [request, left]);

  const answer = async (accept: boolean) => {
    if (!request) return;
    setBusy(accept ? 'accept' : 'reject');
    try {
      await api.answerPairRequest(request.requestId, accept);
      if (!accept) setRequest(null);
    } catch (error) {
      toasts.show(t('Antwort nicht gesendet: {reason}', { reason: errorText(error) }), {
        tone: 'error',
      });
      setRequest(null);
    } finally {
      setBusy(null);
    }
  };

  return (
    <Dialog
      open={!!request}
      onClose={() => void answer(false)}
      closeOnOutsideClick={false}
      title={t('Kopplungsanfrage')}
      width="sm"
      footer={
        <>
          <Button variant="ghost" busy={busy === 'reject'} onClick={() => void answer(false)}>
            {t('Ablehnen')}
          </Button>
          <Button variant="primary" busy={busy === 'accept'} onClick={() => void answer(true)}>
            {t('Code stimmt, koppeln')}
          </Button>
        </>
      }
    >
      {request && (
        <div className="pair-request">
          <Nyu shell="box" mood="puzzled" size={72} />
          <p className="dialog-lead">
            {t('„{name}“ möchte sich koppeln. Zeigt das iPhone denselben Code?', {
              name: request.deviceName,
            })}
          </p>
          <div className="pair-code" aria-label={t('Code')}>
            {formatCode(request.code)}
          </div>
          <p className="muted-line">{t('Läuft in {n} s ab.', { n: left })}</p>
        </div>
      )}
    </Dialog>
  );
}
