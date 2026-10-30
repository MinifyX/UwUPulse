/**
 * The page's side of the updater (src-tauri/src/updates.rs): a downloaded
 * update, the channel and whether the background check runs. Release notes
 * come from the feed as JSON with `de` and `en`, or as plain text.
 */

import type { Language } from './i18n';

export interface UpdateInfo {
  version: string;
  notes: string | null;
}

/** The pages the About card links to; Rust maps them to addresses. */
export type ProjectPage = 'source' | 'releases' | 'issues' | 'license';

/** Release notes in `lang`: JSON `{ de, en }` written for UwUPulse, otherwise the plain text. */
export function notesFor(notes: string | null | undefined, lang: Language): string {
  if (!notes) return '';
  try {
    const parsed = JSON.parse(notes) as unknown;
    if (parsed && typeof parsed === 'object') {
      const { de, en } = parsed as Record<string, unknown>;
      const text = lang === 'en' ? (en ?? de) : (de ?? en);
      if (typeof text === 'string') return text;
    }
  } catch {
    // Plain text notes.
  }
  return notes;
}
