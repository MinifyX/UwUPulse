import assert from 'node:assert/strict';
import { test } from 'node:test';
import { stats, zoneColor, zoneIndex, zoneShares } from '../src/lib/zones.ts';

const zones = [
  { min: 0, color: '#0000ff' },
  { min: 100, color: '#00ff00' },
  { min: 150, color: '#ff0000' },
];

test('zoneIndex picks the highest zone whose min is reached', () => {
  assert.equal(zoneIndex(60, zones), 0);
  assert.equal(zoneIndex(100, zones), 1);
  assert.equal(zoneIndex(149, zones), 1);
  assert.equal(zoneIndex(200, zones), 2);
  assert.equal(zoneIndex(50, [{ min: 60, color: '#000' }]), -1);
});

test('zoneColor falls back below the first zone and without a value', () => {
  assert.equal(zoneColor(120, zones, '#fff'), '#00ff00');
  assert.equal(zoneColor(null, zones, '#fff'), '#fff');
  assert.equal(zoneColor(10, [{ min: 50, color: '#000' }], '#fff'), '#fff');
  // unsorted input works too
  assert.equal(zoneColor(160, [...zones].reverse(), '#fff'), '#ff0000');
});

test('zoneShares weights by time and caps gaps', () => {
  const samples: [number, number][] = [
    [0, 80],
    [1000, 80],
    [2000, 120],
    [3000, 120],
    [60000, 160], // gap: [3000] counts 5 s only
  ];
  const shares = zoneShares(samples, zones, 5000);
  assert.deepEqual(
    shares.map((s) => s.ms),
    [2000, 6000, 1000],
  );
  assert.equal(shares[1]!.max, 150);
  assert.equal(shares[2]!.max, null);
  const sum = shares.reduce((a, s) => a + s.share, 0);
  assert.ok(Math.abs(sum - 1) < 1e-9);
});

test('zoneShares of nothing', () => {
  assert.deepEqual(
    zoneShares([], zones).map((s) => s.share),
    [0, 0, 0],
  );
});

test('stats', () => {
  assert.equal(stats([]), null);
  assert.deepEqual(
    stats([
      [0, 60],
      [1, 90],
      [2, 120],
    ]),
    { min: 60, max: 120, avg: 90 },
  );
});
