/**
 * The app in German or English.
 *
 * German is the source language: every string is written in German where it
 * is used and wrapped in `t()`; the English catalogue in `src/i18n/en/` maps
 * each German string to its English one. `pnpm lint` runs
 * `scripts/check-i18n.mjs`, which lists every string without English.
 *
 * Placeholders are `{name}`, filled from `vars`. Strings kept in module-level
 * constants are marked with `N_()` and translated with `t()` where shown.
 * A component that shows text calls `useLanguage()` so it redraws on change.
 */

import { EN } from '../i18n/en';
import { getPrefs, usePrefs, type LanguageSetting } from './prefs';

export type Language = 'de' | 'en';
export type Vars = Record<string, string | number>;

export function resolveLanguage(setting: LanguageSetting): Language {
  if (setting !== 'system') return setting;
  const preferred = navigator.languages?.[0] ?? navigator.language ?? '';
  return preferred.toLowerCase().startsWith('de') ? 'de' : 'en';
}

export function language(): Language {
  return resolveLanguage(getPrefs().language);
}

function fill(template: string, vars?: Vars): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? String(vars[name]) : whole,
  );
}

/** `text` (German) in the current language, placeholders filled. */
export function t(text: string, vars?: Vars): string {
  return fill(language() === 'en' ? (EN[text] ?? text) : text, vars);
}

/** Marks a German string kept in a constant for translation where it is shown. */
export function N_(text: string): string {
  return text;
}

export function useLanguage(): Language {
  return resolveLanguage(usePrefs().language);
}

export function locale(lang: Language = language()): string {
  return lang === 'de' ? 'de-DE' : 'en-GB';
}

export function formatDateTime(ms: number): string {
  return new Intl.DateTimeFormat(locale(), { dateStyle: 'medium', timeStyle: 'short' }).format(ms);
}

export function formatTime(ms: number): string {
  return new Intl.DateTimeFormat(locale(), { timeStyle: 'medium' }).format(ms);
}

/** "vor 5 s", "vor 3 min", or a date for anything older than a day. */
export function formatAgo(ms: number, now = Date.now()): string {
  const seconds = Math.max(0, Math.round((now - ms) / 1000));
  if (seconds < 60) return t('vor {n} s', { n: seconds });
  if (seconds < 3600) return t('vor {n} min', { n: Math.floor(seconds / 60) });
  if (seconds < 86400) return t('vor {n} h', { n: Math.floor(seconds / 3600) });
  return formatDateTime(ms);
}
