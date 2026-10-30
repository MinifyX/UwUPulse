/**
 * The window's own preferences (language, look, updates), kept in
 * localStorage under `uwupulse.settings` — the key /boot.js reads before the
 * first paint for theme, contrast and motion (vite.config.ts); the font goes
 * on in main.tsx (lib/appearance.ts). Server settings live in the core
 * (api.getSettings).
 */

import type {
  ContrastSetting,
  FontChoice,
  MotionSetting,
  TextSizeChoice,
  ThemeSetting,
} from '@uwusuite/design';
import { useSyncExternalStore } from 'react';

export type LanguageSetting = 'system' | 'de' | 'en';
export type UpdateChannel = 'stable' | 'beta';

export interface Prefs {
  language: LanguageSetting;
  theme: ThemeSetting;
  contrast: ContrastSetting;
  motion: MotionSetting;
  /** The UI font, only on this computer. */
  font: FontChoice;
  /** On top of the system's text size. */
  textSize: TextSizeChoice;
  updateChannel: UpdateChannel;
  /** Look for updates in the background (shortly after start, then every six hours). */
  autoUpdate: boolean;
}

export const PREFS_KEY = 'uwupulse.settings';

// The same lists as in @uwusuite/design (FONT_CHOICES, TEXT_SIZE_CHOICES),
// written out so this module stays free of the package at run time and the
// tests can load it in plain Node.
const LANGUAGES = ['system', 'de', 'en'] as const;
const THEMES = ['system', 'light', 'dark'] as const;
const CONTRASTS = ['system', 'normal', 'high'] as const;
const MOTIONS = ['system', 'on', 'off'] as const;
const FONTS = ['uwu', 'manrope', 'rubik', 'dmsans', 'system'] as const;
const TEXT_SIZES = ['smaller', 'system', 'larger', 'largest'] as const;
const CHANNELS = ['stable', 'beta'] as const;

/** A beta build starts on the beta channel, like the Rust side (`Channel::default`). */
export function defaultChannel(version: string): UpdateChannel {
  return version.includes('-') ? 'beta' : 'stable';
}

export function defaults(version = ''): Prefs {
  return {
    language: 'system',
    theme: 'system',
    contrast: 'system',
    motion: 'system',
    font: 'uwu',
    textSize: 'system',
    updateChannel: defaultChannel(version),
    autoUpdate: true,
  };
}

function pick<T extends string>(list: readonly T[], value: unknown, fallback: T): T {
  return list.includes(value as T) ? (value as T) : fallback;
}

/** Stored JSON → preferences; anything unknown or missing falls back to the default. */
export function parsePrefs(raw: string | null, version = ''): Prefs {
  const base = defaults(version);
  let stored: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(raw ?? '{}');
    stored = parsed && typeof parsed === 'object' ? (parsed as Record<string, unknown>) : {};
  } catch {
    stored = {};
  }
  return {
    language: pick(LANGUAGES, stored.language, base.language),
    theme: pick(THEMES, stored.theme, base.theme),
    contrast: pick(CONTRASTS, stored.contrast, base.contrast),
    motion: pick(MOTIONS, stored.motion, base.motion),
    font: pick(FONTS, stored.font, base.font),
    textSize: pick(TEXT_SIZES, stored.textSize, base.textSize),
    updateChannel: pick(CHANNELS, stored.updateChannel, base.updateChannel),
    autoUpdate: typeof stored.autoUpdate === 'boolean' ? stored.autoUpdate : base.autoUpdate,
  };
}

/** The app's version, from Vite; empty in plain Node (tests). */
export const APP_VERSION: string =
  (import.meta as { env?: { APP_VERSION?: string } }).env?.APP_VERSION ?? '';

function read(): Prefs {
  if (typeof window === 'undefined') return defaults(APP_VERSION); // plain Node (tests)
  try {
    return parsePrefs(localStorage.getItem(PREFS_KEY), APP_VERSION);
  } catch {
    return defaults(APP_VERSION);
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
