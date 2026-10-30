import { Button, Card, EmptyState, Hint, ICONS, Nyu, Spinner, Tag } from '@uwusuite/design';
import { useEffect, useMemo, useState } from 'react';
import {
  api,
  errorText,
  events,
  inTauri,
  type ExportFormat,
  type SessionDetail,
  type SessionSummary,
} from '../api';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { HrGraph } from '../components/HrGraph';
import { Page } from '../components/Page';
import { formatBpm, formatDuration, formatPercent } from '../lib/format.ts';
import { formatDateTime, formatTime, t, useLanguage } from '../lib/i18n';
import { useNow } from '../lib/pulse';
import { toasts } from '../lib/toasts';
import { DEFAULT_ZONES, normalizeWidget, type Zone } from '../lib/widget.ts';
import { zoneShares } from '../lib/zones.ts';

export function SessionsPage() {
  useLanguage();
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [zones, setZones] = useState<Zone[]>(DEFAULT_ZONES);
  const now = useNow(5000);

  useEffect(() => {
    const load = () =>
      api
        .listSessions()
        .then((list) => {
          const sorted = [...list].sort((a, b) => b.startedAt - a.startedAt);
          setSessions(sorted);
          setSelected((s) => (s && sorted.some((x) => x.id === s) ? s : (sorted[0]?.id ?? null)));
          setError(null);
        })
        .catch((e) => setError(errorText(e)));
    void load();
    // The zones of the first widget that uses them, so both agree.
    api
      .listWidgets()
      .then((list) => {
        const withZones = list
          .map(normalizeWidget)
          .find((w) => w.zones.enabled && w.zones.items.length);
        if (withZones) setZones(withZones.zones.items);
      })
      .catch(() => {});
    return events.onSessions(() => void load());
  }, []);

  const summary = sessions?.find((s) => s.id === selected) ?? null;

  return (
    <Page wide title={t('Sessions')} subtitle={t('Aufzeichnungen, nur auf diesem Computer.')}>
      {error && (
        <Hint tone="danger">{t('Sessions nicht geladen: {reason}', { reason: error })}</Hint>
      )}
      {sessions && sessions.length === 0 && (
        <EmptyState
          art={<Nyu shell="box" mood="sleepy" size={110} />}
          title={t('Noch keine Session')}
          body={t(
            'Sobald Werte kommen, nimmt UwUPulse eine Session auf (in den Einstellungen abschaltbar).',
          )}
        />
      )}
      {sessions && sessions.length > 0 && (
        <div className="sessions-layout">
          <ul className="rows session-list" aria-label={t('Sessions')}>
            {sessions.map((s) => (
              <li key={s.id}>
                <button
                  type="button"
                  className="row row-button"
                  aria-current={s.id === selected}
                  onClick={() => setSelected(s.id)}
                >
                  <div className="row-main">
                    <div className="row-title">
                      {formatDateTime(s.startedAt)}
                      {s.endedAt == null && <Tag tone="danger">{t('läuft')}</Tag>}
                    </div>
                    <div className="row-sub">
                      {formatDuration((s.endedAt ?? now) - s.startedAt)} ·{' '}
                      {t('Ø {bpm} BPM', { bpm: formatBpm(s.avgBpm) })}
                    </div>
                  </div>
                </button>
              </li>
            ))}
          </ul>
          {summary && (
            <SessionView
              key={summary.id}
              summary={summary}
              zones={zones}
              onDeleted={() =>
                setSessions((list) => list?.filter((x) => x.id !== summary.id) ?? null)
              }
            />
          )}
        </div>
      )}
    </Page>
  );
}

function SessionView({
  summary,
  zones,
  onDeleted,
}: {
  summary: SessionSummary;
  zones: Zone[];
  onDeleted: () => void;
}) {
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState<ExportFormat | null>(null);
  const [confirm, setConfirm] = useState(false);
  const running = summary.endedAt == null;

  useEffect(() => {
    let cancelled = false;
    api
      .getSession(summary.id)
      .then((d) => !cancelled && setDetail(d))
      .catch((e) => !cancelled && setError(errorText(e)));
    return () => {
      cancelled = true;
    };
    // A running session grows: reload with each new summary.
  }, [summary.id, summary.sampleCount]);

  const shares = useMemo(() => (detail ? zoneShares(detail.samples, zones) : []), [detail, zones]);
  const first = detail?.samples[0]?.[0] ?? summary.startedAt;
  const last = detail?.samples[detail.samples.length - 1]?.[0] ?? summary.endedAt ?? first;

  const exportAs = async (format: ExportFormat) => {
    if (!inTauri) {
      toasts.show(t('Export geht nur in der App.'));
      return;
    }
    setExporting(format);
    try {
      const path = await api.exportSession(summary.id, format);
      if (!path) return;
      toasts.show(t('Exportiert nach {path}', { path }), { tone: 'success' });
    } catch (e) {
      toasts.show(t('Export fehlgeschlagen: {reason}', { reason: errorText(e) }), {
        tone: 'error',
      });
    } finally {
      setExporting(null);
    }
  };

  return (
    <div className="session-view">
      <Card
        title={formatDateTime(summary.startedAt)}
        subtitle={
          running
            ? t('Läuft seit {time}', { time: formatTime(summary.startedAt) })
            : t('{start} bis {end} · {duration}', {
                start: formatTime(summary.startedAt),
                end: formatTime(summary.endedAt!),
                duration: formatDuration(summary.endedAt! - summary.startedAt),
              })
        }
        aside={
          <div className="row-actions">
            <Button
              size="sm"
              icon={ICONS.export}
              busy={exporting === 'csv'}
              onClick={() => void exportAs('csv')}
            >
              CSV
            </Button>
            <Button
              size="sm"
              icon={ICONS.export}
              busy={exporting === 'json'}
              onClick={() => void exportAs('json')}
            >
              JSON
            </Button>
            <Button size="sm" variant="danger" icon={ICONS.delete} onClick={() => setConfirm(true)}>
              {t('Löschen')}
            </Button>
          </div>
        }
      >
        <div className="stat-grid">
          <Stat label={t('Minimum')} value={formatBpm(summary.minBpm)} unit="BPM" />
          <Stat label={t('Durchschnitt')} value={formatBpm(summary.avgBpm)} unit="BPM" />
          <Stat label={t('Maximum')} value={formatBpm(summary.maxBpm)} unit="BPM" />
          <Stat label={t('Messwerte')} value={String(summary.sampleCount)} />
        </div>
        {error && <Hint tone="danger">{error}</Hint>}
        {!detail && !error && (
          <div className="graph-loading">
            <Spinner />
          </div>
        )}
        {detail && detail.samples.length > 0 && (
          <HrGraph
            points={detail.samples}
            xMin={first}
            xMax={Math.max(last, first + 1)}
            height={240}
            zones={zones}
            label={t('Herzfrequenz der Session')}
            timeLabels={[formatTime(first), formatTime(last)]}
          />
        )}
      </Card>

      {detail && detail.samples.length > 0 && (
        <Card title={t('Zeit in Zonen')}>
          <div className="zone-bar" aria-hidden="true">
            {shares.map((s) =>
              s.share > 0 ? (
                <span key={s.zone.min} style={{ flexGrow: s.share, background: s.zone.color }} />
              ) : null,
            )}
          </div>
          <ul className="zone-legend">
            {shares.map((s) => (
              <li key={s.zone.min}>
                <span className="zone-swatch" style={{ background: s.zone.color }} />
                <span className="zone-range">
                  {s.max == null
                    ? t('ab {min} BPM', { min: s.zone.min })
                    : t('{min}–{max} BPM', { min: s.zone.min, max: s.max - 1 })}
                </span>
                <span className="zone-time">{formatDuration(s.ms)}</span>
                <span className="zone-share">{formatPercent(s.share)}</span>
              </li>
            ))}
          </ul>
        </Card>
      )}

      <ConfirmDialog
        open={confirm}
        title={t('Session löschen?')}
        action={t('Löschen')}
        onClose={() => setConfirm(false)}
        onConfirm={async () => {
          try {
            await api.deleteSession(summary.id);
            onDeleted();
            toasts.show(t('Session gelöscht.'));
          } catch (e) {
            toasts.show(t('Nicht gelöscht: {reason}', { reason: errorText(e) }), { tone: 'error' });
          }
        }}
      >
        {t('Die Session vom {date} wird endgültig gelöscht.', {
          date: formatDateTime(summary.startedAt),
        })}
      </ConfirmDialog>
    </div>
  );
}

function Stat({ label, value, unit }: { label: string; value: string; unit?: string }) {
  return (
    <div className="stat">
      <div className="stat-label">{label}</div>
      <div className="stat-value">
        {value}
        {unit && <span className="stat-unit"> {unit}</span>}
      </div>
    </div>
  );
}
