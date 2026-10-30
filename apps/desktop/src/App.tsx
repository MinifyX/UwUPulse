import {
  detectPlatform,
  Icon,
  ICONS,
  TitleBar,
  Toaster,
  useAppearance,
  UwuLabels,
  Wordmark,
} from '@uwusuite/design';
import { useTauriWindow } from '@uwusuite/design/tauri';
import { Activity, Heart, MonitorPlay, type LucideIcon } from 'lucide-react';
import { useEffect, useState, type MouseEvent } from 'react';
import { PairRequestDialog } from './components/PairRequestDialog';
import { N_, t, useLanguage } from './lib/i18n';
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

const platform = detectPlatform();

export function App() {
  const lang = useLanguage();
  const prefs = usePrefs();
  useAppearance({ theme: prefs.theme, contrast: 'system', motion: 'system' });
  const [page, setPage] = useState<PageId>('live');

  useEffect(() => startPulse(), []);
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  return (
    <UwuLabels labels={lang}>
      <div className="shell">
        <WindowBar />
        <div className="shell-body">
          <Sidebar page={page} go={setPage} />
          <main className="shell-main" key={page}>
            {page === 'live' && <LivePage go={setPage} />}
            {page === 'overlay' && <OverlayPage />}
            {page === 'sessions' && <SessionsPage />}
            {page === 'devices' && <DevicesPage />}
            {page === 'settings' && <SettingsPage />}
          </main>
        </div>
      </div>
      <PairRequestDialog />
      <Toaster store={toasts} />
    </UwuLabels>
  );
}

/**
 * Tauri maximizes on a double-click of a drag region itself; the package's
 * title bar would do it again and undo it. Capture keeps the second one away.
 */
function leaveDoubleClickToTauri(event: MouseEvent) {
  if ((event.target as HTMLElement).hasAttribute('data-tauri-drag-region')) event.stopPropagation();
}

/** Own title bar on Windows and Linux; macOS draws its native one. */
function WindowBar() {
  const controls = useTauriWindow();
  if (platform === 'mac') return null;
  return (
    <div className="titlebar-frame" onDoubleClickCapture={leaveDoubleClickToTauri}>
      <TitleBar
        platform={platform}
        controls={controls}
        brand={<Wordmark product="Pulse" shell="box" className="text-body" />}
      />
    </div>
  );
}

function Sidebar({ page, go }: { page: PageId; go: (page: PageId) => void }) {
  useLanguage();
  const pulse = usePulse();
  const now = useNow();
  const live = isLive(pulse, now);
  return (
    <nav className="sidebar" aria-label={t('Bereiche')}>
      {platform === 'mac' && <Wordmark product="Pulse" shell="box" className="sidebar-brand" />}
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
              <span>{t(item.label)}</span>
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
