import {
  Button,
  Card,
  Dialog,
  EmptyState,
  Hint,
  Icon,
  ICONS,
  Nyu,
  Spinner,
  StatusDot,
  Switch,
} from '@uwusuite/design';
import { Bluetooth, BluetoothSearching } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import {
  api,
  errorText,
  events,
  type BleDevice,
  type PairedDevice,
  type PairingOffer,
} from '../api';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { Page } from '../components/Page';
import { secondsLeft } from '../lib/format.ts';
import { formatAgo, formatDateTime, t, useLanguage } from '../lib/i18n';
import { useNow } from '../lib/pulse';
import { toasts } from '../lib/toasts';

export function DevicesPage() {
  useLanguage();
  return (
    <Page title={t('Geräte')} subtitle={t('iPhones mit Apple Watch und Bluetooth-Brustgurte.')}>
      <PairedCard />
      <BleCard />
    </Page>
  );
}

// ── Paired iPhones ───────────────────────────────────────

function PairedCard() {
  const [devices, setDevices] = useState<PairedDevice[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pairing, setPairing] = useState(false);
  const [removing, setRemoving] = useState<PairedDevice | null>(null);
  const now = useNow(5000);

  useEffect(() => {
    api
      .listPaired()
      .then(setDevices)
      .catch((e) => setError(errorText(e)));
    return events.onPaired(setDevices);
  }, []);

  return (
    <Card
      title={t('iPhones')}
      subtitle={t('Die iPhone-App leitet die Werte der Apple Watch weiter.')}
      aside={
        <Button variant="primary" icon={ICONS.qrCode} onClick={() => setPairing(true)}>
          {t('iPhone koppeln')}
        </Button>
      }
    >
      {error && <Hint tone="danger">{t('Geräte nicht geladen: {reason}', { reason: error })}</Hint>}
      {devices && devices.length === 0 && (
        <EmptyState
          compact
          art={<Nyu shell="box" mood="uwu" size={96} />}
          title={t('Noch kein iPhone gekoppelt')}
          body={t(
            'Öffne UwUPulse auf dem iPhone und scanne den QR-Code. Ohne Kamera geht es auch per Code-Vergleich.',
          )}
        />
      )}
      {devices && devices.length > 0 && (
        <ul className="rows">
          {devices.map((d) => (
            <li key={d.id} className="row">
              <span className="row-icon">
                <Icon icon={ICONS.device} size="md" />
              </span>
              <div className="row-main">
                <div className="row-title">
                  <StatusDot state={d.connected ? 'online' : 'offline'} />
                  {d.name}
                </div>
                <div className="row-sub">
                  {[
                    d.connected
                      ? t('Verbunden')
                      : d.lastSeen
                        ? t('Zuletzt {ago}', { ago: formatAgo(d.lastSeen, now) })
                        : t('Noch nie verbunden'),
                    t('gekoppelt am {date}', { date: formatDateTime(d.pairedAt) }),
                  ].join(' · ')}
                </div>
              </div>
              <div className="row-actions">
                <Button
                  size="sm"
                  variant="ghost"
                  icon={ICONS.unlink}
                  onClick={() => setRemoving(d)}
                >
                  {t('Entfernen')}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}

      <PairingDialog open={pairing} onClose={() => setPairing(false)} />

      <ConfirmDialog
        open={!!removing}
        title={t('iPhone entfernen?')}
        action={t('Entfernen')}
        onClose={() => setRemoving(null)}
        onConfirm={async () => {
          if (!removing) return;
          try {
            await api.removePaired(removing.id);
            setDevices((list) => list?.filter((d) => d.id !== removing.id) ?? null);
          } catch (e) {
            toasts.show(t('Nicht entfernt: {reason}', { reason: errorText(e) }), { tone: 'error' });
          }
        }}
      >
        {t('„{name}“ muss danach neu gekoppelt werden, bevor es wieder Werte schicken kann.', {
          name: removing?.name ?? '',
        })}
      </ConfirmDialog>
    </Card>
  );
}

/**
 * A one-time QR offer. It counts down to its expiry, can be renewed, and is
 * cancelled when the dialog closes. A newly paired device closes it.
 */
function PairingDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [offer, setOffer] = useState<PairingOffer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const now = useNow(500);
  const known = useRef<Set<string> | null>(null);

  const create = () => {
    setOffer(null);
    setError(null);
    api
      .createPairing()
      .then(setOffer)
      .catch((e) => setError(errorText(e)));
  };

  useEffect(() => {
    if (!open) return;
    known.current = null;
    api
      .listPaired()
      .then((list) => (known.current = new Set(list.map((d) => d.id))))
      .catch(() => {});
    create();
    const off = events.onPaired((list) => {
      const fresh = known.current && list.find((d) => !known.current!.has(d.id));
      if (fresh) {
        toasts.show(t('{name} ist gekoppelt.', { name: fresh.name }), { tone: 'success' });
        onClose();
      }
    });
    return () => {
      off();
      void api.cancelPairing().catch(() => {});
    };
    // onClose is stable enough; re-running would issue a new offer.
  }, [open]);

  const left = offer ? secondsLeft(offer.expiresAt, now) : 0;
  const expired = !!offer && left <= 0;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={t('iPhone koppeln')}
      width="md"
      footer={
        <>
          <Button variant="ghost" icon={ICONS.refresh} onClick={create}>
            {t('Neuer Code')}
          </Button>
          <Button onClick={onClose}>{t('Schließen')}</Button>
        </>
      }
    >
      <div className="pairing">
        <div className="qr-box" data-expired={expired}>
          {offer && !expired && (
            // The core renders the SVG itself from data it generated.
            <div
              className="qr"
              role="img"
              aria-label={t('QR-Code zum Koppeln')}
              dangerouslySetInnerHTML={{ __html: offer.qrSvg }}
            />
          )}
          {!offer && !error && <Spinner />}
          {expired && (
            <Button icon={ICONS.refresh} onClick={create}>
              {t('Abgelaufen, neu erzeugen')}
            </Button>
          )}
        </div>
        <div className="pairing-steps">
          <ol>
            <li>{t('UwUPulse auf dem iPhone öffnen (gleiches WLAN).')}</li>
            <li>{t('„Mit Computer koppeln“ wählen und den Code scannen.')}</li>
            <li>{t('Fertig: Das iPhone erscheint in der Liste.')}</li>
          </ol>
          {offer && !expired && (
            <p className="muted-line">
              {t('Gültig noch {time}, nur einmal verwendbar.', {
                time: `${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}`,
              })}
            </p>
          )}
          {error && <Hint tone="danger">{t('Kein Code: {reason}', { reason: error })}</Hint>}
          <Hint tone="info">
            {t(
              'Ohne Kamera: Auf dem iPhone den Computer in der Liste antippen. Hier erscheint dann ein Code zum Vergleichen.',
            )}
          </Hint>
        </div>
      </div>
    </Dialog>
  );
}

// ── BLE heart rate straps ────────────────────────────────

function BleCard() {
  const [devices, setDevices] = useState<BleDevice[]>([]);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .bleList()
      .then(setDevices)
      .catch((e) => setError(errorText(e)));
    const off = events.onBle(setDevices);
    return () => {
      off();
      // Scanning costs power: it stops when the page is left.
      void api.bleSetScanning(false).catch(() => {});
    };
  }, []);

  const toggleScan = async (on: boolean) => {
    setScanning(on);
    try {
      await api.bleSetScanning(on);
      setError(null);
    } catch (e) {
      setScanning(false);
      setError(errorText(e));
    }
  };

  const act = async (device: BleDevice) => {
    try {
      if (device.connected) await api.bleDisconnect(device.id);
      else await api.bleConnect(device.id);
    } catch (e) {
      toasts.show(
        t('{name}: {reason}', { name: device.name || t('Unbekannt'), reason: errorText(e) }),
        { tone: 'error' },
      );
    }
  };

  return (
    <Card
      title={t('Bluetooth-Gurte')}
      subtitle={t('Brustgurte mit dem Standard-Herzfrequenzprofil, direkt an diesem Computer.')}
      aside={
        <Switch checked={scanning} onChange={(on) => void toggleScan(on)} label={t('Suchen')} />
      }
    >
      {error && <Hint tone="danger">{t('Bluetooth: {reason}', { reason: error })}</Hint>}
      {devices.length === 0 ? (
        <div className="ble-empty">
          <Icon icon={scanning ? BluetoothSearching : Bluetooth} size="lg" />
          <span>
            {scanning
              ? t('Suche … Gurt anlegen, damit er sendet.')
              : t('Schalte „Suchen“ ein, um Gurte in der Nähe zu finden.')}
          </span>
        </div>
      ) : (
        <ul className="rows">
          {devices.map((d) => (
            <li key={d.id} className="row">
              <span className="row-icon">
                <Icon icon={Bluetooth} size="md" />
              </span>
              <div className="row-main">
                <div className="row-title">
                  <StatusDot
                    state={d.connected ? 'online' : d.connecting ? 'connecting' : 'offline'}
                  />
                  {d.name || t('Unbekannt')}
                </div>
                <div className="row-sub">
                  {[
                    d.connected
                      ? t('Verbunden')
                      : d.connecting
                        ? t('Verbindet …')
                        : t('Nicht verbunden'),
                    d.rssi != null ? `${d.rssi} dBm` : null,
                  ]
                    .filter(Boolean)
                    .join(' · ')}
                </div>
              </div>
              <div className="row-actions">
                <Button
                  size="sm"
                  variant={d.connected ? 'ghost' : 'primary'}
                  icon={d.connected ? ICONS.disconnect : ICONS.connect}
                  busy={d.connecting}
                  onClick={() => void act(d)}
                >
                  {d.connected ? t('Trennen') : t('Verbinden')}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}
