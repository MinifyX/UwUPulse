/**
 * Overlay widget configuration: defaults, normalisation and validation.
 * Pure (no DOM, no Tauri), tested in test/widget.test.ts. The overlay page
 * (crates/uwupulse-core/overlay/overlay.js) has the same defaults.
 */

export const PRESETS = ['classic', 'minimal', 'ring', 'graph', 'ecg'] as const;
export type Preset = (typeof PRESETS)[number];

export interface Zone {
  min: number;
  color: string;
}

export interface WidgetConfig {
  id: string;
  name: string;
  preset: Preset;
  color: string;
  textColor: string;
  background: string;
  font: string;
  fontSize: number;
  showIcon: boolean;
  showLabel: boolean;
  animateBeat: boolean;
  showGraph: boolean;
  graphSeconds: number;
  zones: { enabled: boolean; items: Zone[] };
  idleText: string;
  hideWhenIdle: boolean;
  customCss: string;
}

export const LIMITS = {
  name: 64,
  fontSize: { min: 12, max: 400 },
  graphSeconds: { min: 10, max: 600 },
  zoneBpm: { min: 0, max: 250 },
  zones: 8,
  idleText: 64,
  font: 200,
  customCss: 32 * 1024,
  /** The core refuses a widget whose JSON is larger. */
  total: 64 * 1024,
} as const;

export const SYSTEM_FONT = "system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif";

/** Fonts offered in the editor; any CSS font-family may be typed instead. */
export const FONT_CHOICES: { label: string; value: string }[] = [
  { label: 'System', value: SYSTEM_FONT },
  { label: 'Rounded', value: "ui-rounded, 'SF Pro Rounded', 'Nunito', system-ui, sans-serif" },
  { label: 'Serif', value: "Georgia, 'Times New Roman', serif" },
  { label: 'Mono', value: "ui-monospace, 'SF Mono', Menlo, Consolas, monospace" },
  { label: 'Impact', value: "Impact, 'Arial Black', sans-serif" },
];

export const DEFAULT_ZONES: Zone[] = [
  { min: 0, color: '#4dabf7' },
  { min: 100, color: '#51cf66' },
  { min: 130, color: '#fcc419' },
  { min: 160, color: '#ff6b6b' },
];

export function defaultWidget(id: string, name = 'Classic'): WidgetConfig {
  return {
    id,
    name,
    preset: 'classic',
    color: '#ff4d6d',
    textColor: '#ffffff',
    background: 'transparent',
    font: SYSTEM_FONT,
    fontSize: 72,
    showIcon: true,
    showLabel: true,
    animateBeat: true,
    showGraph: false,
    graphSeconds: 60,
    zones: { enabled: false, items: DEFAULT_ZONES.map((z) => ({ ...z })) },
    idleText: '',
    hideWhenIdle: false,
    customCss: '',
  };
}

/** A new widget id: short, URL-safe, lower case. */
export function newWidgetId(random: () => string = () => crypto.randomUUID()): string {
  return 'w' + random().replace(/-/g, '').slice(0, 11).toLowerCase();
}

const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

export function isHexColor(value: string): boolean {
  return HEX.test(value);
}

/** Accent and text colours are hex; the background may also be 'transparent'. */
export function isBackground(value: string): boolean {
  return value === 'transparent' || isHexColor(value);
}

const clamp = (n: number, min: number, max: number) => Math.min(max, Math.max(min, n));

function num(value: unknown, fallback: number, min: number, max: number): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? clamp(Math.round(value), min, max)
    : fallback;
}

function str(value: unknown, fallback: string, max: number): string {
  return typeof value === 'string' ? value.slice(0, max) : fallback;
}

function bool(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback;
}

function color(value: unknown, fallback: string, allowTransparent = false): string {
  if (typeof value !== 'string') return fallback;
  const v = value.trim();
  return isHexColor(v) || (allowTransparent && v === 'transparent') ? v : fallback;
}

/** Zones sorted by their lower bound, duplicates of the same bound dropped. */
export function sortZones(items: Zone[]): Zone[] {
  const seen = new Set<number>();
  return [...items]
    .sort((a, b) => a.min - b.min)
    .filter((z) => (seen.has(z.min) ? false : (seen.add(z.min), true)));
}

/** Any JSON (from the core, an old version, an import) as a complete, sane widget. */
export function normalizeWidget(input: unknown): WidgetConfig {
  const raw = (input && typeof input === 'object' ? input : {}) as Record<string, unknown>;
  const id = typeof raw.id === 'string' && raw.id ? raw.id : newWidgetId();
  const d = defaultWidget(id);
  const zonesRaw = (raw.zones && typeof raw.zones === 'object' ? raw.zones : {}) as Record<
    string,
    unknown
  >;
  const items = Array.isArray(zonesRaw.items)
    ? zonesRaw.items
        .filter((z): z is Record<string, unknown> => !!z && typeof z === 'object')
        .map((z) => ({
          min: num(z.min, 0, LIMITS.zoneBpm.min, LIMITS.zoneBpm.max),
          color: color(z.color, d.color),
        }))
        .slice(0, LIMITS.zones)
    : d.zones.items;
  const font = str(raw.font, d.font, LIMITS.font).trim();
  return {
    id,
    name: str(raw.name, d.name, LIMITS.name),
    preset: PRESETS.includes(raw.preset as Preset) ? (raw.preset as Preset) : d.preset,
    color: color(raw.color, d.color),
    textColor: color(raw.textColor, d.textColor),
    background: color(raw.background, d.background, true),
    font: font || d.font,
    fontSize: num(raw.fontSize, d.fontSize, LIMITS.fontSize.min, LIMITS.fontSize.max),
    showIcon: bool(raw.showIcon, d.showIcon),
    showLabel: bool(raw.showLabel, d.showLabel),
    animateBeat: bool(raw.animateBeat, d.animateBeat),
    showGraph: bool(raw.showGraph, d.showGraph),
    graphSeconds: num(
      raw.graphSeconds,
      d.graphSeconds,
      LIMITS.graphSeconds.min,
      LIMITS.graphSeconds.max,
    ),
    zones: { enabled: bool(zonesRaw.enabled, d.zones.enabled), items: sortZones(items) },
    idleText: str(raw.idleText, d.idleText, LIMITS.idleText),
    hideWhenIdle: bool(raw.hideWhenIdle, d.hideWhenIdle),
    customCss: str(raw.customCss, d.customCss, LIMITS.customCss),
  };
}

export type WidgetField =
  | 'name'
  | 'color'
  | 'textColor'
  | 'background'
  | 'font'
  | 'fontSize'
  | 'graphSeconds'
  | 'zones'
  | 'idleText'
  | 'customCss'
  | 'size';

/** Problems with a widget as the editor holds it; empty when it can be saved. */
export function validateWidget(w: WidgetConfig): Partial<Record<WidgetField, string>> {
  const errors: Partial<Record<WidgetField, string>> = {};
  if (!w.name.trim()) errors.name = 'empty';
  else if (w.name.length > LIMITS.name) errors.name = 'long';
  if (!isHexColor(w.color)) errors.color = 'color';
  if (!isHexColor(w.textColor)) errors.textColor = 'color';
  if (!isBackground(w.background)) errors.background = 'color';
  if (!w.font.trim()) errors.font = 'empty';
  else if (/[;{}<>]/.test(w.font) || w.font.length > LIMITS.font) errors.font = 'invalid';
  const { fontSize, graphSeconds, zoneBpm } = LIMITS;
  if (!Number.isFinite(w.fontSize) || w.fontSize < fontSize.min || w.fontSize > fontSize.max)
    errors.fontSize = 'range';
  if (
    !Number.isFinite(w.graphSeconds) ||
    w.graphSeconds < graphSeconds.min ||
    w.graphSeconds > graphSeconds.max
  )
    errors.graphSeconds = 'range';
  const mins = w.zones.items.map((z) => z.min);
  if (
    w.zones.items.length > LIMITS.zones ||
    w.zones.items.some(
      (z) =>
        !Number.isInteger(z.min) ||
        z.min < zoneBpm.min ||
        z.min > zoneBpm.max ||
        !isHexColor(z.color),
    ) ||
    new Set(mins).size !== mins.length
  )
    errors.zones = 'invalid';
  if (w.idleText.length > LIMITS.idleText) errors.idleText = 'long';
  if (w.customCss.length > LIMITS.customCss) errors.customCss = 'long';
  if (new TextEncoder().encode(JSON.stringify(w)).length > LIMITS.total) errors.size = 'long';
  return errors;
}

function stable(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stable).join(',')}]`;
  if (value && typeof value === 'object') {
    const entries = Object.entries(value).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${stable(v)}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

/** Same content, ignoring key order. */
export function sameWidget(a: WidgetConfig, b: WidgetConfig): boolean {
  return stable(a) === stable(b);
}
