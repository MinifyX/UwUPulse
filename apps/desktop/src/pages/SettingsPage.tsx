import {
  Button,
  Card,
  FONT_CHOICES,
  FONT_NAMES,
  FONT_STACKS,
  Hint,
  IconButton,
  ICONS,
  Nyu,
  Segmented,
  SettingRow,
  Switch,
  TEXT_SIZE_CHOICES,
  TEXT_SIZE_LABELS,
  TextInput,
  Wordmark,
  type ContrastSetting,
  type FontChoice,
  type MotionSetting,
  type TextSizeChoice,
  type ThemeSetting,
} from '@uwusuite/design';
import { useEffect, useState, type KeyboardEvent } from 'react';
import { api, errorText, type HostInfo, type Settings, type UpdateInfo } from '../api';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { Page } from '../components/Page';
import { copy } from '../lib/clipboard';
import { N_, t, useLanguage } from '../lib/i18n';
import {
  APP_VERSION,
  setPrefs,
  usePrefs,
  type LanguageSetting,
  type UpdateChannel,
} from '../lib/prefs';
import { patchState, usePulse } from '../lib/pulse';
import { toasts } from '../lib/toasts';
import { notesFor } from '../lib/updates';

const PORT_MIN = 1024;
const PORT_MAX = 65535;
const IDLE_MIN = 10;
const IDLE_MAX = 3600;

type UpdateProps = {
  /** A downloaded update waiting for the next start, or null. */
  update: UpdateInfo | null;
  onUpdateFound: (update: UpdateInfo) => void;
  onInstallUpdate: () => Promise<void>;
};

export function SettingsPage(props: UpdateProps) {
  useLanguage();
  // A host without its own updater (the OBS plugin) is updated by the
  // UwUPulse setup, which says so instead of offering an update button.
  const [host, setHost] = useState<HostInfo | null>(null);
  useEffect(() => {
    void api
      .getHostInfo()
      .then(setHost)
      .catch(() => undefined);
  }, []);
  const updates = !host || host.kind === 'desktop' || host.updater;
  return (
    <Page title={t('Einstellungen')}>
      <ServerCard />
      <RecordingCard />
      <AppearanceCard />
      <ApiCard />
      {updates ? <UpdatesCard {...props} /> : <SetupUpdatesCard />}
      <AboutCard />
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
  const lang = useLanguage();
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
      <SettingRow label={t('Kontrast')} description={t('„System“ folgt dem Betriebssystem.')}>
        <Segmented<ContrastSetting>
          label={t('Kontrast')}
          value={prefs.contrast}
          onChange={(contrast) => setPrefs({ contrast })}
          options={[
            { value: 'system', label: t('System') },
            { value: 'normal', label: t('Normal') },
            { value: 'high', label: t('Hoch') },
          ]}
        />
      </SettingRow>
      <SettingRow label={t('Animationen')} description={t('„System“ folgt dem Betriebssystem.')}>
        <Segmented<MotionSetting>
          label={t('Animationen')}
          value={prefs.motion}
          onChange={(motion) => setPrefs({ motion })}
          options={[
            { value: 'system', label: t('System') },
            { value: 'on', label: t('An') },
            { value: 'off', label: t('Aus') },
          ]}
        />
      </SettingRow>
      <SettingRow label={t('Textgröße')} description={t('Zusätzlich zur Textgröße des Systems.')}>
        <Segmented<TextSizeChoice>
          label={t('Textgröße')}
          value={prefs.textSize}
          onChange={(textSize) => setPrefs({ textSize })}
          options={TEXT_SIZE_CHOICES.map((value) => ({
            value,
            label: TEXT_SIZE_LABELS[lang][value],
          }))}
        />
      </SettingRow>
      <div className="setting-block">
        <div className="setting-block-head">
          <p className="setting-block-label">{t('Schrift')}</p>
          <p className="setting-block-description">
            {t(
              'Die Schrift der Oberfläche, nur auf diesem Gerät. UwU Sans ist die Schrift aller UwU-Apps.',
            )}
          </p>
        </div>
        <FontPicker value={prefs.font} onChange={(font) => setPrefs({ font })} />
      </div>
    </Card>
  );
}

/**
 * The package's fonts as a radio group, each in its own face. The overlays
 * keep their own font (Overlay → Widget).
 */
function FontPicker({
  value,
  onChange,
}: {
  value: FontChoice;
  onChange: (font: FontChoice) => void;
}) {
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[event.key];
    if (!step) return;
    event.preventDefault();
    const index = FONT_CHOICES.indexOf(value);
    const next = (index + step + FONT_CHOICES.length) % FONT_CHOICES.length;
    onChange(FONT_CHOICES[next]!);
    event.currentTarget.querySelectorAll<HTMLButtonElement>('[role=radio]')[next]?.focus();
  };
  return (
    <div className="font-picker" role="radiogroup" aria-label={t('Schrift')} onKeyDown={onKeyDown}>
      {FONT_CHOICES.map((choice) => (
        <button
          key={choice}
          type="button"
          role="radio"
          aria-checked={value === choice}
          tabIndex={value === choice ? 0 : -1}
          onClick={() => onChange(choice)}
          style={{ fontFamily: FONT_STACKS[choice] }}
        >
          <span className="font-picker-name">
            {choice === 'system' ? t('System') : FONT_NAMES[choice]}
          </span>
          <span className="font-picker-sample">{t('Herzfrequenz 0123 Il1 O0')}</span>
        </button>
      ))}
    </div>
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

type CheckResult = { tone: 'success' | 'danger'; text: string } | null;

/** Updates for the OBS plugin: they come with the UwUPulse setup. */
function SetupUpdatesCard() {
  return (
    <Card title={t('Updates')}>
      <SettingRow
        label={t('Version {version}', { version: APP_VERSION || '–' })}
        description={t(
          'Updates über den UwUPulse-Setup: Lade die neue Version herunter und starte ihn, er bringt das OBS-Plugin mit auf den neuesten Stand.',
        )}
      >
        <Button
          size="sm"
          variant="ghost"
          icon={ICONS.openExternal}
          onClick={() => void api.openProjectPage('releases')}
        >
          {t('Versionen')}
        </Button>
      </SettingRow>
    </Card>
  );
}

function UpdatesCard({ update, onUpdateFound, onInstallUpdate }: UpdateProps) {
  const lang = useLanguage();
  const prefs = usePrefs();
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [result, setResult] = useState<CheckResult>(null);
  const [showNotes, setShowNotes] = useState(false);
  const notes = notesFor(update?.notes, lang);

  const check = async () => {
    setChecking(true);
    setResult(null);
    try {
      const found = await api.checkForUpdates();
      if (found) onUpdateFound(found);
      else setResult({ tone: 'success', text: t('UwUPulse ist auf dem neuesten Stand. ✧') });
    } catch (e) {
      setResult({
        tone: 'danger',
        text: t('Suche fehlgeschlagen: {error}', { error: errorText(e) }),
      });
    } finally {
      setChecking(false);
    }
  };

  const install = async () => {
    setInstalling(true);
    setResult(null);
    try {
      await onInstallUpdate();
    } catch (e) {
      setResult({ tone: 'danger', text: errorText(e) });
      setInstalling(false);
    }
  };

  return (
    <Card title={t('Updates')}>
      <SettingRow
        label={t('Version {version}', { version: APP_VERSION || '–' })}
        description={t(
          'UwUPulse lädt neue Versionen still herunter und installiert sie beim nächsten Start. Jedes Update ist signiert und wird vor dem Start geprüft.',
        )}
      >
        {update ? (
          <Button
            size="sm"
            variant="primary"
            icon={ICONS.download}
            busy={installing}
            onClick={() => void install()}
          >
            {t('{version} installieren', { version: update.version })}
          </Button>
        ) : (
          <Button size="sm" icon={ICONS.refresh} busy={checking} onClick={() => void check()}>
            {checking ? t('Sucht …') : t('Nach Updates suchen')}
          </Button>
        )}
      </SettingRow>
      {update && notes && (
        <div className="setting-notes">
          <button
            type="button"
            className="link-button"
            aria-expanded={showNotes}
            onClick={() => setShowNotes(!showNotes)}
          >
            {t('Was ist neu?')}
          </button>
          {showNotes && <p className="update-hint-notes">{notes}</p>}
        </div>
      )}
      {result && (
        <p className="setting-result" data-tone={result.tone} role="status">
          {result.text}
        </p>
      )}
      <SettingRow
        label={t('Automatisch nach Updates suchen')}
        description={t('Kurz nach dem Start und danach alle sechs Stunden.')}
      >
        <Switch
          checked={prefs.autoUpdate}
          label={t('Automatisch nach Updates suchen')}
          onChange={(autoUpdate) => setPrefs({ autoUpdate })}
        />
      </SettingRow>
      <SettingRow
        label={t('Update-Kanal')}
        description={
          prefs.updateChannel === 'beta'
            ? t('Beta bekommt neue Versionen früher. Es kann mal etwas wackeln.')
            : t('Stabil bekommt nur fertige Versionen.')
        }
      >
        <Segmented<UpdateChannel>
          label={t('Update-Kanal')}
          value={prefs.updateChannel}
          onChange={(updateChannel) => setPrefs({ updateChannel })}
          options={[
            { value: 'stable', label: t('Stabil') },
            { value: 'beta', label: t('Beta') },
          ]}
        />
      </SettingRow>
    </Card>
  );
}

function AboutCard() {
  useLanguage();
  return (
    <Card title={t('Über UwUPulse')}>
      <div className="about">
        <Nyu shell="box" size={88} mood="happy" title="Nyu" />
        <Wordmark product="Pulse" className="mt-1.5 text-title" />
        <p className="about-version">{t('Version {version}', { version: APP_VERSION || '–' })}</p>
        <p className="about-text">
          {t(
            'Freie Software unter der MIT-Lizenz. Kein Tracking, kein Konto, deine Herzfrequenz bleibt lokal.',
          )}
        </p>
        <div className="about-actions">
          <Button
            size="sm"
            variant="ghost"
            icon={ICONS.openExternal}
            onClick={() => void api.openProjectPage('source')}
          >
            {t('Quellcode auf GitHub')}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            icon={ICONS.openExternal}
            onClick={() => void api.openProjectPage('releases')}
          >
            {t('Versionen')}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            icon={ICONS.openExternal}
            onClick={() => void api.openProjectPage('license')}
          >
            {t('Lizenz')}
          </Button>
        </div>
      </div>
    </Card>
  );
}
