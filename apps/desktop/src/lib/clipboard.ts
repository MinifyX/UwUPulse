import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { errorText, inTauri } from '../api';
import { t } from './i18n';
import { toasts } from './toasts';

/** Copies `text` and says so in a toast. */
export async function copy(text: string, what = t('Kopiert.')): Promise<void> {
  try {
    if (inTauri) await writeText(text);
    else await navigator.clipboard.writeText(text);
    toasts.show(what, { tone: 'success' });
  } catch (error) {
    toasts.show(t('Kopieren fehlgeschlagen: {reason}', { reason: errorText(error) }), {
      tone: 'error',
    });
  }
}
