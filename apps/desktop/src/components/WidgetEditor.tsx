import {
  Button,
  Card,
  Field,
  Hint,
  IconButton,
  ICONS,
  Select,
  Switch,
  TextArea,
  TextInput,
  Toggle,
} from '@uwusuite/design';
import { Activity, CircleDot, Hash, Heart, LineChart } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { N_, t, useLanguage } from '../lib/i18n';
import {
  DEFAULT_ZONES,
  FONT_CHOICES,
  LIMITS,
  PRESETS,
  type Preset,
  type WidgetConfig,
  type WidgetField,
} from '../lib/widget.ts';

const PRESET_INFO: Record<Preset, { icon: LucideIcon; label: string; hint: string }> = {
  classic: { icon: Heart, label: N_('Klassisch'), hint: N_('Herz und Zahl') },
  minimal: { icon: Hash, label: N_('Minimal'), hint: N_('Nur die Zahl') },
  ring: { icon: CircleDot, label: N_('Ring'), hint: N_('Pulsierender Ring') },
  graph: { icon: LineChart, label: N_('Graph'), hint: N_('Zahl mit Verlauf') },
  ecg: { icon: Activity, label: N_('EKG'), hint: N_('Laufende Kurve') },
};

const ERRORS: Record<string, string> = {
  empty: N_('Darf nicht leer sein.'),
  long: N_('Zu lang.'),
  color: N_('Eine Hex-Farbe wie #ff4d6d.'),
  range: N_('Außerhalb des erlaubten Bereichs.'),
  invalid: N_('Ungültig.'),
};

/** Every field of a WidgetConfig. Changes go to `onChange`; saving is the page's job. */
export function WidgetEditor({
  widget,
  errors,
  onChange,
}: {
  widget: WidgetConfig;
  errors: Partial<Record<WidgetField, string>>;
  onChange: (widget: WidgetConfig) => void;
}) {
  useLanguage();
  const set = <K extends keyof WidgetConfig>(key: K, value: WidgetConfig[K]) =>
    onChange({ ...widget, [key]: value });
  const error = (field: WidgetField) => (errors[field] ? t(ERRORS[errors[field]!]!) : undefined);
  const zones = widget.zones;
  const setZones = (patch: Partial<WidgetConfig['zones']>) => set('zones', { ...zones, ...patch });
  const customFont = !FONT_CHOICES.some((f) => f.value === widget.font);

  return (
    <div className="editor">
      <Card title={t('Widget')}>
        <div className="form">
          <Field label={t('Name')} error={error('name')}>
            {(id, describedBy) => (
              <TextInput
                id={id}
                aria-describedby={describedBy}
                aria-invalid={!!errors.name}
                value={widget.name}
                maxLength={LIMITS.name}
                onChange={(e) => set('name', e.target.value)}
              />
            )}
          </Field>
          <div role="radiogroup" aria-label={t('Darstellung')} className="preset-grid">
            {PRESETS.map((preset) => {
              const info = PRESET_INFO[preset];
              const Glyph = info.icon;
              return (
                <button
                  key={preset}
                  type="button"
                  role="radio"
                  aria-checked={widget.preset === preset}
                  className="preset-tile"
                  onClick={() => set('preset', preset)}
                >
                  <Glyph size={22} strokeWidth={1.8} aria-hidden="true" />
                  <span className="preset-name">{t(info.label)}</span>
                  <span className="preset-hint">{t(info.hint)}</span>
                </button>
              );
            })}
          </div>
        </div>
      </Card>

      <Card title={t('Farben und Schrift')}>
        <div className="form">
          <div className="form-row">
            <ColorField
              label={t('Akzent')}
              value={widget.color}
              error={error('color')}
              onChange={(v) => set('color', v)}
            />
            <ColorField
              label={t('Text')}
              value={widget.textColor}
              error={error('textColor')}
              onChange={(v) => set('textColor', v)}
            />
          </div>
          <div className="form-row items-end">
            <ColorField
              label={t('Hintergrund')}
              value={widget.background === 'transparent' ? '#000000' : widget.background}
              disabled={widget.background === 'transparent'}
              error={error('background')}
              onChange={(v) => set('background', v)}
            />
            <label className="inline-switch">
              <Switch
                checked={widget.background === 'transparent'}
                onChange={(on) => set('background', on ? 'transparent' : '#000000')}
              />
              {t('Transparent')}
            </label>
          </div>
          <div className="form-row">
            <Field label={t('Schrift')} className="flex-1">
              {(id) => (
                <Select
                  id={id}
                  value={customFont ? '' : widget.font}
                  onChange={(e) => set('font', e.target.value || widget.font + ' ')}
                >
                  {FONT_CHOICES.map((f) => (
                    <option key={f.label} value={f.value}>
                      {f.label === 'System' ? t('Systemschrift') : f.label}
                    </option>
                  ))}
                  <option value="">{t('Eigene …')}</option>
                </Select>
              )}
            </Field>
            {customFont && (
              <Field label={t('CSS font-family')} error={error('font')} className="flex-1">
                {(id, describedBy) => (
                  <TextInput
                    id={id}
                    aria-describedby={describedBy}
                    aria-invalid={!!errors.font}
                    className="uwu-mono"
                    value={widget.font}
                    placeholder="'Comic Neue', sans-serif"
                    onChange={(e) => set('font', e.target.value)}
                  />
                )}
              </Field>
            )}
          </div>
          <RangeField
            label={t('Größe der Zahl')}
            value={widget.fontSize}
            min={LIMITS.fontSize.min}
            max={200}
            unit="px"
            error={error('fontSize')}
            onChange={(v) => set('fontSize', v)}
          />
        </div>
      </Card>

      <Card title={t('Anzeige')}>
        <div className="form">
          <Toggle
            label={t('Herz-Symbol')}
            description={t('Bei „Klassisch“ und „Graph“.')}
            checked={widget.showIcon}
            onChange={(v) => set('showIcon', v)}
          />
          <Toggle
            label={t('„BPM“ anzeigen')}
            checked={widget.showLabel}
            onChange={(v) => set('showLabel', v)}
          />
          <Toggle
            label={t('Im Takt schlagen')}
            description={t('Herz, Ring und Zahl pulsieren mit der Herzfrequenz.')}
            checked={widget.animateBeat}
            onChange={(v) => set('animateBeat', v)}
          />
          <Toggle
            label={t('Verlauf anzeigen')}
            description={t('Eine kleine Kurve unter der Zahl; „Graph“ hat sie immer.')}
            checked={widget.showGraph || widget.preset === 'graph'}
            disabled={widget.preset === 'graph' || widget.preset === 'ecg'}
            onChange={(v) => set('showGraph', v)}
          />
          <RangeField
            label={t('Länge des Verlaufs')}
            value={widget.graphSeconds}
            min={LIMITS.graphSeconds.min}
            max={LIMITS.graphSeconds.max}
            step={10}
            unit="s"
            error={error('graphSeconds')}
            onChange={(v) => set('graphSeconds', v)}
          />
        </div>
      </Card>

      <Card
        title={t('Zonen')}
        subtitle={t('Färbt den Akzent je nach Herzfrequenz.')}
        aside={
          <Switch
            label={t('Zonen')}
            checked={zones.enabled}
            onChange={(enabled) => setZones({ enabled })}
          />
        }
      >
        {zones.enabled && (
          <div className="form">
            {zones.items.map((zone, i) => (
              <div className="zone-row" key={i}>
                <input
                  type="color"
                  className="color-swatch"
                  aria-label={t('Farbe der Zone {n}', { n: i + 1 })}
                  value={toSwatch(zone.color)}
                  onChange={(e) =>
                    setZones({
                      items: zones.items.map((z, j) =>
                        j === i ? { ...z, color: e.target.value } : z,
                      ),
                    })
                  }
                />
                <span className="zone-from">{t('ab')}</span>
                <TextInput
                  type="number"
                  aria-label={t('Untergrenze der Zone {n}', { n: i + 1 })}
                  className="zone-min"
                  min={LIMITS.zoneBpm.min}
                  max={LIMITS.zoneBpm.max}
                  value={Number.isFinite(zone.min) ? zone.min : ''}
                  onChange={(e) =>
                    setZones({
                      items: zones.items.map((z, j) =>
                        j === i ? { ...z, min: Math.round(e.target.valueAsNumber) } : z,
                      ),
                    })
                  }
                  onBlur={() => setZones({ items: [...zones.items].sort((a, b) => a.min - b.min) })}
                />
                <span className="zone-from">BPM</span>
                <span className="spacer" />
                <IconButton
                  icon={ICONS.delete}
                  size="sm"
                  label={t('Zone entfernen')}
                  onClick={() => setZones({ items: zones.items.filter((_, j) => j !== i) })}
                />
              </div>
            ))}
            {errors.zones && (
              <Hint tone="danger">
                {t('Jede Zone braucht eine eigene Untergrenze zwischen 0 und 250 und eine Farbe.')}
              </Hint>
            )}
            <div className="form-row">
              <Button
                size="sm"
                icon={ICONS.add}
                disabled={zones.items.length >= LIMITS.zones}
                onClick={() => {
                  const top = zones.items.reduce((m, z) => Math.max(m, z.min), 0);
                  setZones({
                    items: [...zones.items, { min: Math.min(250, top + 20), color: widget.color }],
                  });
                }}
              >
                {t('Zone hinzufügen')}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                icon={ICONS.reset}
                onClick={() => setZones({ items: DEFAULT_ZONES.map((z) => ({ ...z })) })}
              >
                {t('Standardzonen')}
              </Button>
            </div>
          </div>
        )}
      </Card>

      <Card title={t('Leerlauf')} subtitle={t('Wenn seit 10 s kein Wert kam.')}>
        <div className="form">
          <Field
            label={t('Text im Leerlauf')}
            hint={t('Leer lassen für „--“.')}
            error={error('idleText')}
          >
            {(id, describedBy) => (
              <TextInput
                id={id}
                aria-describedby={describedBy}
                value={widget.idleText}
                maxLength={LIMITS.idleText}
                placeholder="--"
                onChange={(e) => set('idleText', e.target.value)}
              />
            )}
          </Field>
          <Toggle
            label={t('Im Leerlauf ausblenden')}
            checked={widget.hideWhenIdle}
            onChange={(v) => set('hideWhenIdle', v)}
          />
        </div>
      </Card>

      <Card title={t('Eigenes CSS')}>
        <div className="form">
          <Field
            label={t('CSS')}
            error={error('customCss')}
            hint={
              <>
                {t('Klassen:')}{' '}
                <code>
                  .uwp-root .uwp-widget .uwp-icon .uwp-ring .uwp-value .uwp-bpm .uwp-label .uwp-idle
                  .uwp-graph .uwp-ecg
                </code>
                {' · '}
                {t('Variablen:')}{' '}
                <code>--uwp-color --uwp-text --uwp-size --uwp-beat --uwp-pulse</code>
              </>
            }
          >
            {(id, describedBy) => (
              <TextArea
                id={id}
                aria-describedby={describedBy}
                className="uwu-mono css-input"
                spellCheck={false}
                rows={8}
                value={widget.customCss}
                placeholder={'.uwp-bpm {\n  text-shadow: 0 2px 8px #0008;\n}'}
                onChange={(e) => set('customCss', e.target.value)}
              />
            )}
          </Field>
        </div>
      </Card>
    </div>
  );
}

/** <input type="color"> takes #rrggbb only. */
function toSwatch(value: string): string {
  if (/^#[0-9a-f]{6}$/i.test(value)) return value.toLowerCase();
  if (/^#[0-9a-f]{8}$/i.test(value)) return value.slice(0, 7).toLowerCase();
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])[0-9a-f]?$/i.exec(value);
  return short
    ? `#${short[1]}${short[1]}${short[2]}${short[2]}${short[3]}${short[3]}`.toLowerCase()
    : '#000000';
}

function ColorField({
  label,
  value,
  error,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  error?: string;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <Field label={label} error={error} className="flex-1">
      {(id, describedBy) => (
        <div className="color-field">
          <input
            type="color"
            className="color-swatch"
            aria-label={label}
            disabled={disabled}
            value={toSwatch(value)}
            onChange={(e) => onChange(e.target.value)}
          />
          <TextInput
            id={id}
            aria-describedby={describedBy}
            aria-invalid={!!error}
            className="uwu-mono"
            disabled={disabled}
            value={value}
            maxLength={9}
            onChange={(e) => {
              const v = e.target.value.trim();
              onChange(v.startsWith('#') || !v ? v : `#${v}`);
            }}
          />
        </div>
      )}
    </Field>
  );
}

function RangeField({
  label,
  value,
  min,
  max,
  step = 1,
  unit,
  error,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  unit: string;
  error?: string;
  onChange: (value: number) => void;
}) {
  return (
    <Field label={label} error={error}>
      {(id, describedBy) => (
        <div className="range-field">
          <input
            id={id}
            aria-describedby={describedBy}
            type="range"
            min={min}
            max={max}
            step={step}
            value={Math.min(max, value)}
            onChange={(e) => onChange(e.target.valueAsNumber)}
          />
          <span className="range-value">
            {value} {unit}
          </span>
        </div>
      )}
    </Field>
  );
}
