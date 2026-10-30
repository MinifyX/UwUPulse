import { Button, Hint, Nyu } from '@uwusuite/design';
import { useState } from 'react';
import { errorText } from '../api';
import { t, useLanguage } from '../lib/i18n';
import { notesFor, type UpdateInfo } from '../lib/updates';

type Props = {
  update: UpdateInfo;
  onLater: () => void;
  onRestart: () => Promise<void>;
};

/**
 * Nyu's quiet note that a new version is downloaded and ready. "Später"
 * leaves it for the next start, when it is installed before the server comes
 * up; "Jetzt neu starten" hands over to the setup right away.
 */
export function UpdateHint({ update, onLater, onRestart }: Props) {
  const lang = useLanguage();
  const [showNotes, setShowNotes] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const notes = notesFor(update.notes, lang);

  return (
    <aside className="update-hint" aria-live="polite">
      <div className="update-hint-head">
        <Nyu shell="box" size={40} mood="sparkle" title="Nyu" />
        <div>
          <p className="update-hint-title">{t('Ein Update ist bereit ✧')}</p>
          <p className="update-hint-meta">
            {t('Version {version}', { version: update.version })}
            {notes && (
              <>
                {' · '}
                <button
                  type="button"
                  className="link-button"
                  onClick={() => setShowNotes(!showNotes)}
                  aria-expanded={showNotes}
                >
                  {t('Was ist neu?')}
                </button>
              </>
            )}
          </p>
        </div>
      </div>
      {showNotes && <p className="update-hint-notes">{notes}</p>}
      <p className="update-hint-meta">
        {t('Overlays und Aufzeichnung pausieren kurz, solange UwUPulse neu startet.')}
      </p>
      {error && <Hint tone="danger">{error}</Hint>}
      <div className="update-hint-actions">
        <Button size="sm" variant="ghost" onClick={onLater}>
          {t('Später')}
        </Button>
        <Button
          size="sm"
          variant="primary"
          busy={restarting}
          onClick={async () => {
            setRestarting(true);
            setError(null);
            try {
              await onRestart();
            } catch (e) {
              setError(errorText(e));
              setRestarting(false);
            }
          }}
        >
          {restarting ? t('Startet neu …') : t('Jetzt neu starten')}
        </Button>
      </div>
    </aside>
  );
}
