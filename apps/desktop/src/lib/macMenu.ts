/**
 * UwUPulse's macOS menu bar: @uwusuite/design's skeleton (`setMacMenu`, the
 * suite's menus in Apple's order) with the app's pages under Darstellung and
 * its project pages under Hilfe. "Einstellungen … ⌘," is the package's own
 * entry in the app menu.
 *
 * ⌘W and the red light hide the window; the server, the recording and the
 * overlays keep running, and the Dock icon brings the window back (lib.rs).
 *
 * Off macOS this does nothing: the window has its own title bar there.
 */

import { setMacMenu, type MacMenuEntry } from '@uwusuite/design/tauri';
import type { Language } from './i18n';
import { t } from './i18n';
import type { ProjectPage } from './updates';

export interface PulseMenuOptions<Page extends string> {
  lang: Language;
  pages: { id: Page; label: string }[];
  current: Page;
  go: (page: Page) => void;
  onSettings: () => void;
  onCheckForUpdates?: () => void;
  openProjectPage: (page: ProjectPage) => void;
}

/** ⌘1 … ⌘9 for the pages, in the sidebar's order. */
export function pageAccelerator(index: number): string | undefined {
  return index < 9 ? `CmdOrCtrl+${index + 1}` : undefined;
}

export function setPulseMacMenu<Page extends string>(options: PulseMenuOptions<Page>) {
  const { lang, pages, current, go, onSettings, onCheckForUpdates, openProjectPage } = options;
  const view: MacMenuEntry[] = pages.map((page, index) => ({
    text: t(page.label),
    accelerator: pageAccelerator(index),
    checked: page.id === current,
    action: () => go(page.id),
  }));
  return setMacMenu({
    appName: 'UwUPulse',
    lang,
    onSettings,
    app: onCheckForUpdates ? [{ text: t('Nach Updates suchen …'), action: onCheckForUpdates }] : [],
    view,
    help: [
      { text: t('UwUPulse auf GitHub'), action: () => openProjectPage('source') },
      { text: t('Versionen'), action: () => openProjectPage('releases') },
      { text: t('Problem melden'), action: () => openProjectPage('issues') },
    ],
  });
}
