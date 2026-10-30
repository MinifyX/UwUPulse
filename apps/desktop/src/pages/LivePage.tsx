import {
  Button,
  Card,
  EmptyState,
  Hint,
  Icon,
  ICONS,
  Nyu,
  Segmented,
  StatusDot,
  StatusLine,
  Tag,
} from '@uwusuite/design';
import { Bluetooth } from 'lucide-react';
import { useState } from 'react';
import { api, errorText, type Source, type WatchAction } from '../api';
import { BeatHeart } from '../components/BeatHeart';
import { HrGraph } from '../components/HrGraph';
import { Page } from '../components/Page';
import { formatBpm, formatDuration } from '../lib/format.ts';
import { formatAgo, t, useLanguage } from '../lib/i18n';
import { isLive, useNow, usePulse } from '../lib/pulse';
import { toasts } from '../lib/toasts';
import type { PageId } from '../App';

type Range = '1' | '5' | '10';

export function LivePage({ go }: { go: (page: PageId) => void }) {
  useLanguage();
  const pulse = usePulse();
  const now = useNow();
  const [range, setRange] = useState<Range>('5');
  const state = pulse.state;
  const live = isLive(pulse, now);
  const current = state?.current ?? null;
  const rangeMs = Number(range) * 60_000;
  const lastTs = pulse.recent[pulse.recent.length - 1]?.[0] ?? now;
  const xMax = Math.max(now, lastTs);
  const points = pulse.recent.filter(([ts]) => ts >= xMax - rangeMs);

  return (
    <Page
      title={t('Live')}
      subtitle={
        <StatusLine
          state={
            live ? 'online' : state?.sources.some((s) => s.connected) ? 'connecting' : 'offline'
          }
        >
          {live
            ? t('Empfängt Werte')
            : state?.sources.some((s) => s.connected)
              ? t('Verbunden, wartet auf Werte')
              : t('Keine Quelle verbunden')}
        </StatusLine>
      }
    >
      {pulse.error && (
        <Hint tone="danger">
          {t('Der Kern antwortet nicht: {reason}', { reason: pulse.error })}
        </Hint>
      )}

      <section className="live-hero" data-live={live}>
        <BeatHeart bpm={current?.bpm ?? null} live={live} size={84} />
        <div className="live-value">
          <span className="live-bpm" aria-live="polite">
            {live || current ? formatBpm(current?.bpm) : '--'}
          </span>
          <span className="live-unit">BPM</span>
        </div>
        <div className="live-meta">
          {current ? (
            <span>
              {current.dev || (current.src === 'watch' ? 'Apple Watch' : t('Brustgurt'))} ·{' '}
              {pulse.receivedAt ? formatAgo(pulse.receivedAt, now) : formatAgo(current.ts, now)}
            </span>
          ) : (
            <span>{t('Noch kein Wert empfangen')}</span>
          )}
          {state?.recording ? (
            <Tag tone="danger" className="rec-tag">
              <span className="rec-dot" aria-hidden="true" />
              {t('Aufnahme {time}', { time: formatDuration(now - state.recording.startedAt) })}
            </Tag>
          ) : (
            <Tag tone="neutral">{t('Keine Aufnahme')}</Tag>
          )}
        </div>
      </section>

      <Card
        title={t('Verlauf')}
        aside={
          <Segmented<Range>
            label={t('Zeitraum')}
            value={range}
            onChange={setRange}
            options={[
              { value: '1', label: '1 min' },
              { value: '5', label: '5 min' },
              { value: '10', label: '10 min' },
            ]}
          />
        }
      >
        {points.length ? (
          <HrGraph
            points={points}
            xMin={xMax - rangeMs}
            xMax={xMax}
            label={t('Herzfrequenz der letzten {n} Minuten', { n: range })}
            timeLabels={[t('vor {n} min', { n: range }), t('jetzt')]}
          />
        ) : (
          <p className="muted-line">{t('Der Verlauf erscheint, sobald Werte kommen.')}</p>
        )}
      </Card>

      <Card title={t('Quellen')}>
        {state && state.sources.length === 0 ? (
          <EmptyState
            compact
            art={<Nyu shell="box" mood="sleepy" size={96} />}
            title={t('Noch keine Quelle')}
            body={t('Kopple dein iPhone mit der Watch oder verbinde einen Brustgurt.')}
            action={
              <Button variant="primary" icon={ICONS.device} onClick={() => go('devices')}>
                {t('Zu den Geräten')}
              </Button>
            }
          />
        ) : (
          <ul className="rows">
            {state?.sources.map((source) => (
              <SourceRow key={source.id} source={source} now={now} />
            ))}
          </ul>
        )}
      </Card>
    </Page>
  );
}

function SourceRow({ source, now }: { source: Source; now: number }) {
  const [busy, setBusy] = useState<WatchAction | null>(null);
  const running = /running|läuft/i.test(source.detail ?? '');
  const send = async (action: WatchAction) => {
    setBusy(action);
    try {
      await api.sendWatchCommand(source.id, action);
      toasts.show(action === 'start' ? t('Workout wird gestartet …') : t('Workout wird beendet …'));
    } catch (error) {
      toasts.show(t('Die Watch hat nicht reagiert: {reason}', { reason: errorText(error) }), {
        tone: 'error',
      });
    } finally {
      setBusy(null);
    }
  };
  return (
    <li className="row">
      <span className="row-icon">
        <Icon icon={source.kind === 'phone' ? ICONS.device : Bluetooth} size="md" />
      </span>
      <div className="row-main">
        <div className="row-title">
          <StatusDot state={source.connected ? 'online' : 'offline'} />
          {source.name}
        </div>
        <div className="row-sub">
          {[
            source.connected ? t('Verbunden') : t('Getrennt'),
            source.detail,
            source.lastTs ? formatAgo(source.lastTs, now) : null,
          ]
            .filter(Boolean)
            .join(' · ')}
        </div>
      </div>
      <span className="row-bpm">{formatBpm(source.lastBpm)}</span>
      {source.kind === 'phone' && (
        <div className="row-actions">
          <Button
            size="sm"
            variant={running ? 'secondary' : 'primary'}
            icon={ICONS.start}
            busy={busy === 'start'}
            disabled={!source.connected}
            onClick={() => void send('start')}
          >
            {t('Watch starten')}
          </Button>
          <Button
            size="sm"
            variant={running ? 'primary' : 'secondary'}
            icon={ICONS.stop}
            busy={busy === 'stop'}
            disabled={!source.connected}
            onClick={() => void send('stop')}
          >
            {t('Stoppen')}
          </Button>
        </div>
      )}
    </li>
  );
}
