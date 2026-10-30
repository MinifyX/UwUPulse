import {
  detectPlatform,
  Icon,
  ICONS,
  TitleBar as SuiteTitleBar,
  TitleBarAction,
  withShortcut,
  Wordmark,
  type WindowControls,
} from '@uwusuite/design';
import { useTauriWindow } from '@uwusuite/design/tauri';
import type { MouseEvent, ReactNode } from 'react';
import { inTauri } from '../api';
import { t, useLanguage } from '../lib/i18n';

export const platform = detectPlatform();

/** Settings, on every platform: ⌘, on a Mac (the menu bar), Ctrl+, elsewhere (App.tsx). */
export const SETTINGS_SHORTCUT = 'CmdOrCtrl+,';

type Props = {
  onSettings: () => void;
  /** More actions before the gear. */
  children?: ReactNode;
};

/**
 * Tauri maximises on a double-click of a drag region by itself (its drag
 * script), and the package's title bar does it again in React — the two would
 * cancel out. The capture phase runs first and keeps the second one away; the
 * window buttons are not drag regions, so they are never touched by this.
 */
function leaveDoubleClickToTauri(event: MouseEvent) {
  if ((event.target as HTMLElement).hasAttribute('data-tauri-drag-region')) {
    event.stopPropagation();
  }
}

/**
 * The window's own title bar on Windows and Linux (@uwusuite/design's
 * TitleBar): the window has no system frame there (tauri.conf.json), so
 * moving, minimizing, maximizing and closing all happen here.
 *
 * macOS draws the window's title bar itself (tauri.macos.conf.json), with the
 * traffic lights, and the gear lives in the menu bar (lib/macMenu.ts), so
 * there is nothing to draw there.
 */
export function TitleBar(props: Props) {
  if (platform === 'mac') return null;
  return inTauri ? <TauriTitleBar {...props} /> : <Bar {...props} controls={BROWSER_CONTROLS} />;
}

function TauriTitleBar(props: Props) {
  return <Bar {...props} controls={useTauriWindow()} />;
}

/** `pnpm dev` in a plain browser (dev/mock.ts): no window to drive. */
const BROWSER_CONTROLS: WindowControls = {
  maximized: false,
  minimize: () => undefined,
  toggleMaximize: () => undefined,
  close: () => undefined,
};

function Bar({ onSettings, children, controls }: Props & { controls: WindowControls }) {
  const lang = useLanguage();
  return (
    <div className="titlebar-frame" onDoubleClickCapture={leaveDoubleClickToTauri}>
      <SuiteTitleBar
        platform={platform}
        controls={controls}
        brand={<Wordmark product="Pulse" shell="box" className="text-body" />}
        actions={
          <>
            {children}
            <TitleBarAction
              label={withShortcut(t('Einstellungen'), SETTINGS_SHORTCUT, platform, lang)}
              onClick={onSettings}
            >
              <Icon icon={ICONS.settings} size="md" />
            </TitleBarAction>
          </>
        }
      />
    </div>
  );
}
