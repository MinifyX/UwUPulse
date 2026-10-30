import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  defaultWidget,
  isBackground,
  LIMITS,
  newWidgetId,
  normalizeWidget,
  sameWidget,
  sortZones,
  validateWidget,
} from '../src/lib/widget.ts';

test('the default widget is valid', () => {
  assert.deepEqual(validateWidget(defaultWidget('w1')), {});
});

test('normalize fills missing fields and clamps numbers', () => {
  const w = normalizeWidget({ id: 'a', name: 'X', fontSize: 9999, preset: 'nope', color: 'red' });
  assert.equal(w.id, 'a');
  assert.equal(w.name, 'X');
  assert.equal(w.fontSize, LIMITS.fontSize.max);
  assert.equal(w.preset, 'classic');
  assert.equal(w.color, defaultWidget('a').color);
  assert.equal(w.background, 'transparent');
  assert.equal(typeof w.zones.enabled, 'boolean');
});

test('normalize sorts zones and drops duplicate bounds', () => {
  const w = normalizeWidget({
    id: 'a',
    zones: {
      enabled: true,
      items: [
        { min: 150, color: '#ff0000' },
        { min: 90, color: '#00ff00' },
        { min: 150, color: '#0000ff' },
        'junk',
      ],
    },
  });
  assert.deepEqual(
    w.zones.items.map((z) => z.min),
    [90, 150],
  );
});

test('normalize keeps a valid widget unchanged', () => {
  const w = { ...defaultWidget('x'), preset: 'ecg' as const, customCss: '.uwp-bpm{}' };
  assert.ok(sameWidget(normalizeWidget(w), w));
});

test('validate reports bad fields', () => {
  const w = defaultWidget('a');
  const errors = validateWidget({
    ...w,
    name: ' ',
    color: '#12',
    background: 'blue',
    font: 'x; }',
    fontSize: 2,
    graphSeconds: 1,
    zones: {
      enabled: true,
      items: [
        { min: 10, color: '#fff' },
        { min: 10, color: '#000' },
      ],
    },
  });
  assert.deepEqual(Object.keys(errors).sort(), [
    'background',
    'color',
    'font',
    'fontSize',
    'graphSeconds',
    'name',
    'zones',
  ]);
});

test('validate refuses a widget over the size limit', () => {
  const w = { ...defaultWidget('a'), customCss: 'x'.repeat(LIMITS.customCss + 1) };
  assert.equal(validateWidget(w).customCss, 'long');
});

test('backgrounds', () => {
  assert.ok(isBackground('transparent'));
  assert.ok(isBackground('#00000080'));
  assert.ok(!isBackground('rgba(0,0,0,0)'));
});

test('sortZones', () => {
  assert.deepEqual(
    sortZones([
      { min: 3, color: '#000' },
      { min: 1, color: '#111' },
    ]).map((z) => z.min),
    [1, 3],
  );
});

test('widget ids are short and URL safe', () => {
  const id = newWidgetId(() => 'AB12-cd34-ef56-7890');
  assert.equal(id, 'wab12cd34ef5');
  assert.match(newWidgetId(), /^w[0-9a-f]{11}$/);
});

test('sameWidget ignores key order', () => {
  const a = defaultWidget('a');
  const b = Object.fromEntries(Object.entries(a).reverse()) as typeof a;
  assert.ok(sameWidget(a, b));
  assert.ok(!sameWidget(a, { ...a, fontSize: 10 }));
});
