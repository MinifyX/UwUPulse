import { Icon, ICONS, matchesAccelerator, Toaster, UwuLabels } from '@uwusuite/design';
import { hideWindowOnClose, onMacQuit } from '@uwusuite/design/tauri';
import { Activity, Heart, MonitorPlay, type LucideIcon } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { api, events, inTauri, type UpdateInfo } from './api';
import { PairRequestDialog } from './components/PairRequestDialog';
import { platform, SETTINGS_SHORTCUT, TitleBar } from './components/TitleBar';
import { UpdateHint } from './components/UpdateHint';
import { useAppAppearance } from './lib/appearance';
import { N_, t, useLanguage } from './lib/i18n';
import { pageAccelerator, setPulseMacMenu } from './lib/macMenu';
import { usePrefs } from './lib/prefs';
import { isLive, startPulse, useNow, usePulse } from './lib/pulse';
import { toasts } from './lib/toasts';
import { formatBpm } from './lib/format.ts';
import { DevicesPage } from './pages/DevicesPage';
import { LivePage } from './pages/LivePage';
import { OverlayPage } from './pages/OverlayPage';
import { SessionsPage } from './pages/SessionsPage';
import { SettingsPage } from './pages/SettingsPage';

export type PageId = 'live' | 'overlay' | 'sessions' | 'devices' | 'settings';

const NAV: { id: PageId; label: string; icon: LucideIcon }[] = [
  { id: 'live', label: N_('Live'), icon: Activity },
  { id: 'overlay', label: N_('Overlay'), icon: MonitorPlay },
  { id: 'sessions', label: N_('Sessions'), icon: ICONS.history },
  { id: 'devices', label: N_('Geräte'), icon: ICONS.device },
  { id: 'settings', label: N_('Einstellungen'), icon: ICONS.settings },
];

const apple = platform === 'mac';

export function App() {
  const lang = useLanguage();
  const prefs = usePrefs();
  useAppAppearance(prefs);
  const [page, setPage] = useState<PageId>('live');
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [hintDismissed, setHintDismissed] = useState(false);
  const openSettings = useCallback(() => setPage('settings'), []);

  useEffect(() => startPulse(), []);

  // ── Updates (src-tauri/src/updates.rs) ──────────────────
  useEffect(() => {
    void api.setUpdateChannel(prefs.updateChannel).catch(() => undefined);
  }, [prefs.updateChannel]);
  useEffect(() => {
    void api.setAutoUpdate(prefs.autoUpdate).catch(() => undefined);
  }, [prefs.autoUpdate]);
  useEffect(() => {
    void api
      .updateStatus()
      .then((found) => found && setUpdate(found))
      .catch(() => undefined);
    return events.onUpdateReady((found) => {
      setUpdate(found);
      setHintDismissed(false);
    });
  }, []);
  const installUpdate = useCallback(() => api.installUpdate(), []);
  const checkFromMenu = useCallback(() => {
    setPage('settings');
    void api
      .checkForUpdates()
      .then((found) => {
        if (found) setUpdate(found);
        else toasts.show(t('UwUPulse ist auf dem neuesten Stand. ✧'), { tone: 'success' });
      })
      .catch(() => undefined);
  }, []);

  // ── Keyboard: ⌘, / Ctrl+, and ⌘1 … ⌘5 (the menu bar has them on a Mac) ──
  useEffect(() => {
    if (apple && inTauri) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (matchesAccelerator(event, SETTINGS_SHORTCUT, apple)) {
        event.preventDefault();
        setPage('settings');
        return;
      }
      NAV.forEach((item, index) => {
        const accelerator = pageAccelerator(index);
        if (accelerator && matchesAccelerator(event, accelerator, apple)) {
          event.preventDefault();
          setPage(item.id);
        }
      });
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  // ── macOS: menu bar, closing hides, ⌘Q quits ────────────
  useEffect(() => {
    if (!apple || !inTauri) return;
    void setPulseMacMenu({
      lang,
      pages: NAV.map(({ id, label }) => ({ id, label })),
      current: page,
      go: setPage,
      onSettings: openSettings,
      onCheckForUpdates: checkFromMenu,
      openProjectPage: (target) => void api.openProjectPage(target).catch(() => undefined),
    }).catch(() => undefined);
  }, [lang, page, openSettings, checkFromMenu]);

  useEffect(() => {
    if (!apple || !inTauri) return;
    const stops: Array<() => void> = [];
    let stopped = false;
    const keep = (stop: () => void) => {
      if (stopped) stop();
      else stops.push(stop);
    };
    // ⌘W and the red light hide the window; the server, the recording and
    // the overlays keep running, the Dock icon brings it back (lib.rs).
    void hideWindowOnClose(platform)
      .then(keep)
      .catch(() => undefined);
    // ⌘Q, the Dock, logging out: nothing to save, so it just ends.
    void onMacQuit(async () => true, { platform })
      .then(keep)
      .catch(() => undefined);
    return () => {
      stopped = true;
      for (const stop of stops) stop();
    };
  }, []);

  return (
    <UwuLabels labels={lang}>
      <div className="shell">
        <TitleBar onSettings={openSettings} />
        <div className="shell-body">
          <Sidebar page={page} go={setPage} />
          <main className="shell-main" key={page}>
            {page === 'live' && <LivePage go={setPage} />}
            {page === 'overlay' && <OverlayPage />}
            {page === 'sessions' && <SessionsPage />}
            {page === 'devices' && <DevicesPage />}
            {page === 'settings' && (
              <SettingsPage
                update={update}
                onUpdateFound={setUpdate}
                onInstallUpdate={installUpdate}
              />
            )}
          </main>
        </div>
      </div>
      {update && !hintDismissed && page !== 'settings' && (
        <UpdateHint
          update={update}
          onLater={() => setHintDismissed(true)}
          onRestart={installUpdate}
        />
      )}
      <PairRequestDialog />
      <Toaster store={toasts} />
    </UwuLabels>
  );
}

function Sidebar({ page, go }: { page: PageId; go: (page: PageId) => void }) {
  useLanguage();
  const pulse = usePulse();
  const now = useNow();
  const live = isLive(pulse, now);
  return (
    <nav className="sidebar" aria-label={t('Bereiche')}>
      <ul className="nav-list">
        {NAV.map((item) => (
          <li key={item.id}>
            <button
              type="button"
              className="nav-item"
              aria-current={item.id === page ? 'page' : undefined}
              onClick={() => go(item.id)}
            >
              <Icon icon={item.icon} size="md" />
              <span className="nav-label">{t(item.label)}</span>
            </button>
          </li>
        ))}
      </ul>
      <span className="spacer" />
      <div className="sidebar-pulse" data-live={live} title={t('Aktuelle Herzfrequenz')}>
        <Icon icon={Heart} size="sm" />
        <span>{live ? formatBpm(pulse.state?.current?.bpm) : '--'}</span>
        <span className="sidebar-unit">BPM</span>
      </div>
    </nav>
  );
}
