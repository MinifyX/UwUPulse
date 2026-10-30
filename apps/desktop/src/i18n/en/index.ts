/**
 * The English catalogue: German string → English string, one file per area of
 * the app. See `lib/i18n.ts`.
 */

import app from './app.json';
import devices from './devices.json';
import live from './live.json';
import overlay from './overlay.json';
import sessions from './sessions.json';
import settings from './settings.json';

export const EN: Readonly<Record<string, string>> = {
  ...app,
  ...live,
  ...overlay,
  ...sessions,
  ...devices,
  ...settings,
};
