/**
 * How the window looks, from the preferences: theme, contrast and motion
 * through @uwusuite/design (useAppearance puts them on <html>; /boot.js does
 * it before the first paint), the font through applyUiFont, the text size
 * through useTypeScale, the language on <html lang>.
 */

import { applyUiFont, useAppearance, useTypeScale } from '@uwusuite/design';
import { useEffect } from 'react';
import { resolveLanguage } from './i18n';
import { getPrefs, type Prefs } from './prefs';

/** Before the first render: font and language (theme, contrast and motion are /boot.js's). */
export function prepareDocument() {
  const prefs = getPrefs();
  applyUiFont(prefs.font);
  document.documentElement.lang = resolveLanguage(prefs.language);
}

/** Keeps <html> in step with the preferences and the system. */
export function useAppAppearance(prefs: Prefs) {
  useAppearance({ theme: prefs.theme, contrast: prefs.contrast, motion: prefs.motion });
  useTypeScale({ font: prefs.font, textSize: prefs.textSize });
  const lang = resolveLanguage(prefs.language);

  useEffect(() => {
    applyUiFont(prefs.font);
  }, [prefs.font]);

  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);
}
