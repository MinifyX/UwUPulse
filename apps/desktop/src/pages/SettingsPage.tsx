import {
  Button,
  Card,
  Hint,
  IconButton,
  ICONS,
  Segmented,
  SettingRow,
  Switch,
  TextInput,
  type ThemeSetting,
} from '@uwusuite/design';
import { useEffect, useState } from 'react';
import { api, errorText, type Settings } from '../api';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { Page } from '../components/Page';
import { copy } from '../lib/clipboard';
import { N_, t, useLanguage } from '../lib/i18n';
import { setPrefs, usePrefs, type LanguageSetting } from '../lib/prefs';
import { patchState, usePulse } from '../lib/pulse';
import { toasts } from '../lib/toasts';

const PORT_MIN = 1024;
const PORT_MAX = 65535;
const IDLE_MIN = 10;
const IDLE_MAX = 3600;

export function SettingsPage() {
  useLanguage();
  return (
    <Page title={t('Einstellungen')}>
      <ServerCard />
      <RecordingCard />
      <AppearanceCard />
      <ApiCard />
    </Page>
  );
}

/** Loads the settings once; `save` writes a patch and keeps what the core returns. */
function useSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api
      .getSettings()
      .then(setSettings)
      .catch((e) => setError(errorText(e)));
  }, []);
  const save = async (patch: Partial<Settings>) => {
    if (!settings) return false;
    try {
      const next = await api.setSettings({ ...settings, ...patch });
      setSettings(next);
      return true;
    } catch (e) {
      toasts.show(t('Nicht gespeichert: {reason}', { reason: errorText(e) }), { tone: 'error' });
      return false;
    }
  };
  return { settings, error, save };
}

function ServerCard() {
  const { settings, error, save } = useSettings();
  const server = usePulse().state?.server ?? null;
  const [port, setPort] = useState('');
  const [showToken, setShowToken] = useState(false);
  const [confirm, setConfirm] = useState(false);

  useEffect(() => {
    if (settings) setPort(String(settings.port));
  }, [settings?.port]);

  const portNumber = Number(port);
  const portValid =
    Number.isInteger(portNumber) && portNumber >= PORT_MIN && portNumber <= PORT_MAX;
  const portDirty = !!settings && portValid && portNumber !== settings.port;

  const applyPort = async () => {
    if (!portDirty) return;
    if (await save({ port: portNumber }))
      toasts.show(t('Der Server läuft jetzt auf Port {port}.', { port: portNumber }), {
        tone: 'success',
      });
  };

  return (
    <Card title={t('Lokaler Server')} subtitle={t('Für OBS-Overlays, iPhones und eigene Skripte.')}>
      {error && (
        <Hint tone="danger">{t('Einstellungen nicht geladen: {reason}', { reason: error })}</Hint>
      )}
      {server?.error && (
        <Hint tone="danger">{t('Der Server läuft nicht: {reason}', { reason: server.error })}</Hint>
      )}
      <SettingRow
        label={t('Port')}
        description={t(
          'Ändert alle Overlay-URLs. Browserquellen müssen danach neu eingefügt werden.',
        )}
        htmlFor="port"
      >
        <div className="inline-form">
          <TextInput
            id="port"
            className="port-input"
            inputMode="numeric"
            value={port}
            aria-invalid={!portValid}
            disabled={!settings}
            onChange={(e) => setPort(e.target.value.replace(/\D/g, '').slice(0, 5))}
            onKeyDown={(e) => e.key === 'Enter' && void applyPort()}
          />
          <Button size="sm" disabled={!portDirty} onClick={() => void applyPort()}>
            {t('Übernehmen')}
          </Button>
        </div>
      </SettingRow>
      {!portValid && (
        <Hint tone="warning">
          {t('Erlaubt sind {min} bis {max}.', { min: PORT_MIN, max: PORT_MAX })}
        </Hint>
      )}
      <SettingRow
        label={t('Im LAN erreichbar')}
        description={t(
          'Nötig für iPhones und für OBS auf einem anderen Computer. Aus: nur dieser Computer.',
        )}
      >
        <Switch
          checked={settings?.lanEnabled ?? false}
          disabled={!settings}
          label={t('Im LAN erreichbar')}
          onChange={(on) => void save({ lanEnabled: on })}
        />
      </SettingRow>
      <SettingRow
        label={t('Token')}
        description={t('Schützt Overlays und API. Wer es kennt, sieht deine Herzfrequenz.')}
      >
        <div className="inline-form">
          <code className="token" data-hidden={!showToken}>
            {server ? (showToken ? server.token : '••••••••••••••••') : '–'}
          </code>
          <IconButton
            icon={showToken ? ICONS.hide : ICONS.show}
            label={showToken ? t('Token verbergen') : t('Token anzeigen')}
            onClick={() => setShowToken((s) => !s)}
          />
          <IconButton
            icon={ICONS.copy}
            label={t('Token kopieren')}
            disabled={!server}
            onClick={() => server && void copy(server.token, t('Token kopiert.'))}
          />
        </div>
      </SettingRow>
      <div className="card-actions">
        <Button
          variant="ghost"
          icon={ICONS.refresh}
          disabled={!server}
          onClick={() => setConfirm(true)}
        >
          {t('Neues Token erzeugen')}
        </Button>
      </div>

      <ConfirmDialog
        open={confirm}
        title={t('Neues Token erzeugen?')}
        action={t('Neu erzeugen')}
        onClose={() => setConfirm(false)}
        onConfirm={async () => {
          try {
            const info = await api.regenerateToken();
            patchState({ server: info });
            toasts.show(t('Neues Token aktiv.'), { tone: 'success' });
          } catch (e) {
            toasts.show(t('Kein neues Token: {reason}', { reason: errorText(e) }), {
              tone: 'error',
            });
          }
        }}
      >
        {t(
          'Alle Overlay-URLs und API-Zugänge mit dem alten Token hören sofort auf zu funktionieren.',
        )}
      </ConfirmDialog>
    </Card>
  );
}

function RecordingCard() {
  const { settings, save } = useSettings();
  const [idle, setIdle] = useState('');

  useEffect(() => {
    if (settings) setIdle(String(settings.idleTimeoutSecs));
  }, [settings?.idleTimeoutSecs]);

  const idleNumber = Number(idle);
  const idleValid =
    Number.isInteger(idleNumber) && idleNumber >= IDLE_MIN && idleNumber <= IDLE_MAX;

  const applyIdle = () => {
    if (settings && idleValid && idleNumber !== settings.idleTimeoutSecs)
      void save({ idleTimeoutSecs: idleNumber });
    else if (settings && !idleValid) setIdle(String(settings.idleTimeoutSecs));
  };

  return (
    <Card title={t('Aufzeichnung')} subtitle={t('Sessions bleiben auf diesem Computer.')}>
      <SettingRow
        label={t('Sessions aufzeichnen')}
        description={t('Beginnt mit dem ersten Wert und endet nach der Leerlaufzeit.')}
      >
        <Switch
          checked={settings?.recordSessions ?? false}
          disabled={!settings}
          label={t('Sessions aufzeichnen')}
          onChange={(on) => void save({ recordSessions: on })}
        />
      </SettingRow>
      <SettingRow
        label={t('Leerlaufzeit')}
        description={t('Sekunden ohne Wert, bis eine Session endet ({min} bis {max}).', {
          min: IDLE_MIN,
          max: IDLE_MAX,
        })}
        htmlFor="idle"
      >
        <div className="inline-form">
          <TextInput
            id="idle"
            className="port-input"
            inputMode="numeric"
            value={idle}
            aria-invalid={!idleValid}
            disabled={!settings}
            onChange={(e) => setIdle(e.target.value.replace(/\D/g, '').slice(0, 4))}
            onBlur={applyIdle}
            onKeyDown={(e) => e.key === 'Enter' && applyIdle()}
          />
          <span className="unit">s</span>
        </div>
      </SettingRow>
    </Card>
  );
}

function AppearanceCard() {
  const prefs = usePrefs();
  return (
    <Card title={t('Darstellung')}>
      <SettingRow label={t('Sprache')}>
        <Segmented<LanguageSetting>
          label={t('Sprache')}
          value={prefs.language}
          onChange={(language) => setPrefs({ language })}
          options={[
            { value: 'system', label: t('System') },
            { value: 'de', label: 'Deutsch' },
            { value: 'en', label: 'English' },
          ]}
        />
      </SettingRow>
      <SettingRow label={t('Erscheinungsbild')}>
        <Segmented<ThemeSetting>
          label={t('Erscheinungsbild')}
          value={prefs.theme}
          onChange={(theme) => setPrefs({ theme })}
          options={[
            { value: 'system', label: t('System') },
            { value: 'light', label: t('Hell') },
            { value: 'dark', label: t('Dunkel') },
          ]}
        />
      </SettingRow>
    </Card>
  );
}

const ENDPOINTS: { path: string; what: string }[] = [
  { path: 'GET /api/v1/current', what: N_('Aktueller Wert: bpm, ts, src, dev, live') },
  { path: 'GET /api/v1/ws', what: N_('WebSocket: hello, hr, live, widget') },
  { path: 'GET /api/v1/widgets/{id}', what: N_('Konfiguration eines Widgets') },
  { path: 'GET /api/v1/sessions', what: N_('Alle Sessions') },
  { path: 'GET /api/v1/sessions/{id}', what: N_('Eine Session mit allen Werten') },
  { path: 'GET /api/v1/sessions/{id}/export?format=csv', what: N_('Export als CSV oder JSON') },
];

function ApiCard() {
  const server = usePulse().state?.server ?? null;
  const base = server?.urls[0] ?? 'http://127.0.0.1:8746';
  const example = `curl -H "Authorization: Bearer $TOKEN" ${base}/api/v1/current`;
  return (
    <Card title={t('API')} subtitle={t('Für eigene Skripte, Bots und Widgets.')}>
      <p className="muted-line">
        {t(
          'Jede Anfrage braucht das Token: als ?token=… oder im Header „Authorization: Bearer …“.',
        )}
      </p>
      <ul className="api-list">
        {ENDPOINTS.map((e) => (
          <li key={e.path}>
            <code>{e.path}</code>
            <span>{t(e.what)}</span>
          </li>
        ))}
      </ul>
      <div className="url-row">
        <code className="url-text">{example}</code>
        <Button
          size="sm"
          icon={ICONS.copy}
          onClick={() => void copy(example, t('Beispiel kopiert.'))}
        >
          {t('Kopieren')}
        </Button>
      </div>
    </Card>
  );
}
