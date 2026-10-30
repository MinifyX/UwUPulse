import {
  Button,
  Card,
  EmptyState,
  Hint,
  IconButton,
  ICONS,
  Nyu,
  Pill,
  Segmented,
} from '@uwusuite/design';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api, errorText, type ServerInfo, type Widget } from '../api';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { Page } from '../components/Page';
import { WidgetEditor } from '../components/WidgetEditor';
import { copy } from '../lib/clipboard';
import { isLoopback, overlayUrl } from '../lib/format.ts';
import { t, useLanguage } from '../lib/i18n';
import { usePulse } from '../lib/pulse';
import { toasts } from '../lib/toasts';
import {
  defaultWidget,
  newWidgetId,
  normalizeWidget,
  sameWidget,
  validateWidget,
} from '../lib/widget.ts';

/** Unsaved edits survive switching pages (not restarts). */
let draftCache: { drafts: Record<string, Widget>; selected: string | null } = {
  drafts: {},
  selected: null,
};

export function OverlayPage() {
  useLanguage();
  const server = usePulse().state?.server ?? null;
  const [saved, setSaved] = useState<Widget[] | null>(null);
  const [drafts, setDrafts] = useState<Record<string, Widget>>(draftCache.drafts);
  const [selected, setSelected] = useState<string | null>(draftCache.selected);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    draftCache = { drafts, selected };
  }, [drafts, selected]);

  useEffect(() => {
    api
      .listWidgets()
      .then((list) => {
        const widgets = list.map(normalizeWidget);
        setSaved(widgets);
        setSelected((s) => s ?? widgets[0]?.id ?? null);
      })
      .catch((error) => setLoadError(errorText(error)));
  }, []);

  // Saved widgets first, then new ones that exist only as drafts.
  const all = useMemo(() => {
    const list = (saved ?? []).map((w) => drafts[w.id] ?? w);
    const fresh = Object.values(drafts).filter((d) => !saved?.some((w) => w.id === d.id));
    return [...list, ...fresh];
  }, [saved, drafts]);
  const widget = all.find((w) => w.id === selected) ?? null;
  const original = saved?.find((w) => w.id === selected) ?? null;
  const dirty = !!widget && (!original || !sameWidget(widget, original));
  const errors = widget ? validateWidget(widget) : {};
  const valid = Object.keys(errors).length === 0;

  const change = (next: Widget) => setDrafts((d) => ({ ...d, [next.id]: next }));
  const dropDraft = (id: string) =>
    setDrafts((d) => {
      const { [id]: _, ...rest } = d;
      return rest;
    });

  const create = () => {
    const w = defaultWidget(newWidgetId(), t('Neues Widget'));
    change(w);
    setSelected(w.id);
  };

  const save = async () => {
    if (!widget || !valid) return;
    setSaving(true);
    try {
      const stored = normalizeWidget(await api.saveWidget(normalizeWidget(widget)));
      setSaved((list) => {
        const rest = (list ?? []).filter((w) => w.id !== widget.id);
        const index = (list ?? []).findIndex((w) => w.id === widget.id);
        if (index < 0) return [...rest, stored];
        rest.splice(index, 0, stored);
        return rest;
      });
      dropDraft(widget.id);
      setSelected(stored.id);
      toasts.show(t('Widget gespeichert.'), { tone: 'success' });
    } catch (error) {
      toasts.show(t('Nicht gespeichert: {reason}', { reason: errorText(error) }), {
        tone: 'error',
      });
    } finally {
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!widget) return;
    if (original) await api.deleteWidget(widget.id);
    setSaved((list) => (list ?? []).filter((w) => w.id !== widget.id));
    dropDraft(widget.id);
    setSelected(all.find((w) => w.id !== widget.id)?.id ?? null);
    toasts.show(t('Widget gelöscht.'));
  };

  return (
    <Page
      wide
      title={t('Overlay')}
      subtitle={t('Widgets für OBS und andere Streaming-Programme.')}
      actions={
        <Button icon={ICONS.add} onClick={create}>
          {t('Neues Widget')}
        </Button>
      }
    >
      {loadError && (
        <Hint tone="danger">{t('Widgets nicht geladen: {reason}', { reason: loadError })}</Hint>
      )}

      {saved && all.length === 0 && (
        <EmptyState
          art={<Nyu shell="box" mood="puzzled" size={110} />}
          title={t('Noch kein Widget')}
          body={t('Ein Widget ist eine Browserquelle für OBS mit deiner Herzfrequenz.')}
          action={
            <Button variant="primary" icon={ICONS.add} onClick={create}>
              {t('Neues Widget')}
            </Button>
          }
        />
      )}

      {all.length > 0 && (
        <div className="widget-pills" role="tablist" aria-label={t('Widgets')}>
          {all.map((w) => {
            const isDirty = !!drafts[w.id];
            return (
              <Pill
                key={w.id}
                role="tab"
                aria-selected={w.id === selected}
                active={w.id === selected}
                onClick={() => setSelected(w.id)}
              >
                {w.name || t('Ohne Namen')}
                {isDirty && (
                  <span
                    className="dirty-dot"
                    title={t('Nicht gespeichert')}
                    aria-label={t('Nicht gespeichert')}
                  />
                )}
              </Pill>
            );
          })}
        </div>
      )}

      {widget && (
        <div className="overlay-layout">
          <WidgetEditor widget={widget} errors={errors} onChange={change} />
          <aside className="overlay-side">
            <OverlayPreview widget={widget} server={server} saved={!!original} />
            <div className="save-bar">
              <Button
                variant="primary"
                icon={ICONS.save}
                busy={saving}
                disabled={!dirty || !valid}
                onClick={() => void save()}
              >
                {t('Speichern')}
              </Button>
              <Button
                variant="ghost"
                icon={ICONS.undo}
                disabled={!dirty || !original}
                onClick={() => dropDraft(widget.id)}
              >
                {t('Verwerfen')}
              </Button>
              <span className="spacer" />
              <IconButton
                icon={ICONS.delete}
                label={t('Widget löschen')}
                onClick={() => setConfirmDelete(true)}
              />
            </div>
            {!valid && <Hint tone="warning">{t('Bitte die markierten Felder prüfen.')}</Hint>}
          </aside>
        </div>
      )}

      <ConfirmDialog
        open={confirmDelete}
        title={t('Widget löschen?')}
        action={t('Löschen')}
        onClose={() => setConfirmDelete(false)}
        onConfirm={async () => {
          try {
            await remove();
          } catch (error) {
            toasts.show(t('Nicht gelöscht: {reason}', { reason: errorText(error) }), {
              tone: 'error',
            });
          }
        }}
      >
        {t('„{name}“ wird gelöscht. Browserquellen mit seiner URL zeigen danach nichts mehr an.', {
          name: widget?.name ?? '',
        })}
      </ConfirmDialog>
    </Page>
  );
}

type Backdrop = 'checker' | 'dark' | 'light';

/**
 * The overlay page itself in an iframe, with demo values. Unsaved changes go
 * in by postMessage ({type: 'uwupulse-config', config}); the page asks for
 * them with {type: 'uwupulse-ready'} once it listens.
 */
function OverlayPreview({
  widget,
  server,
  saved,
}: {
  widget: Widget;
  server: ServerInfo | null;
  saved: boolean;
}) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [backdrop, setBackdrop] = useState<Backdrop>('checker');
  const base = server?.urls[0] ?? null;
  const src =
    base && server
      ? overlayUrl(base, widget.id, server.token, { preview: true, demo: true })
      : null;
  const origin = base ? new URL(base).origin : null;

  const push = useCallback(() => {
    if (origin)
      frame.current?.contentWindow?.postMessage(
        { type: 'uwupulse-config', config: widget },
        origin,
      );
  }, [widget, origin]);

  useEffect(push, [push]);
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      if (event.source === frame.current?.contentWindow && event.data?.type === 'uwupulse-ready')
        push();
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, [push]);

  return (
    <Card
      title={t('Vorschau')}
      aside={
        <Segmented<Backdrop>
          label={t('Hintergrund der Vorschau')}
          value={backdrop}
          onChange={setBackdrop}
          options={[
            { value: 'checker', label: t('Raster') },
            { value: 'dark', label: t('Dunkel') },
            { value: 'light', label: t('Hell') },
          ]}
        />
      }
    >
      <div className="preview-stage" data-backdrop={backdrop}>
        {src ? (
          <iframe key={src} ref={frame} src={src} title={t('Vorschau des Widgets')} onLoad={push} />
        ) : (
          <p className="muted-line">{t('Der lokale Server läuft nicht.')}</p>
        )}
      </div>
      <p className="muted-line">
        {t('Die Vorschau zeigt Beispielwerte, solange keine echten kommen.')}
      </p>

      <div className="url-list">
        <h3 className="group-title">{t('URL für die Browserquelle')}</h3>
        {!saved && (
          <Hint tone="info">{t('Speichere das Widget, damit seine URL funktioniert.')}</Hint>
        )}
        {server?.urls.map((url) => {
          const full = overlayUrl(url, widget.id, server.token);
          return (
            <div className="url-row" key={url}>
              <div className="min-w-0">
                <div className="url-kind">
                  {isLoopback(url) ? t('Dieser Computer') : t('Im LAN')}
                </div>
                {/* The token stays hidden: the window may be on stream. */}
                <code className="url-text">
                  {overlayUrl(url, widget.id, 'TOKEN').replace('TOKEN', '•••')}
                </code>
              </div>
              <Button
                size="sm"
                icon={ICONS.copy}
                disabled={!saved}
                onClick={() => void copy(full, t('URL kopiert.'))}
              >
                {t('Kopieren')}
              </Button>
            </div>
          );
        })}
        <p className="muted-line">
          {t('In OBS: Quelle hinzufügen → Browser, URL einfügen, Breite 600 und Höhe 300.')}
        </p>
      </div>
    </Card>
  );
}
