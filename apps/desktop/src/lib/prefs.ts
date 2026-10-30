/**
 * The window's own preferences (language, theme), kept in localStorage under
 * `uwupulse.settings` — the key /boot.js reads before the first paint
 * (vite.config.ts). Server settings live in the core (api.getSettings).
 */

import type { ThemeSetting } from '@uwusuite/design';
import { useSyncExternalStore } from 'react';

export type LanguageSetting = 'system' | 'de' | 'en';

export interface Prefs {
  language: LanguageSetting;
  theme: ThemeSetting;
}

export const PREFS_KEY = 'uwupulse.settings';
const DEFAULTS: Prefs = { language: 'system', theme: 'system' };

function read(): Prefs {
  try {
    const raw = JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}') as Partial<Prefs>;
    return {
      language: ['system', 'de', 'en'].includes(raw.language as string)
        ? (raw.language as LanguageSetting)
        : DEFAULTS.language,
      theme: ['system', 'light', 'dark'].includes(raw.theme as string)
        ? (raw.theme as ThemeSetting)
        : DEFAULTS.theme,
    };
  } catch {
    return DEFAULTS;
  }
}

let current = read();
const listeners = new Set<() => void>();

export function getPrefs(): Prefs {
  return current;
}

export function setPrefs(patch: Partial<Prefs>): void {
  current = { ...current, ...patch };
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(current));
  } catch {
    // Private mode or full storage: the choice holds for this run.
  }
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function usePrefs(): Prefs {
  return useSyncExternalStore(subscribe, getPrefs);
}
