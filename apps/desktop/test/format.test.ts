import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  formatBpm,
  formatCode,
  formatDuration,
  formatPercent,
  isLoopback,
  overlayUrl,
  secondsLeft,
  trimRecent,
} from '../src/lib/format.ts';

test('formatBpm', () => {
  assert.equal(formatBpm(72.6), '73');
  assert.equal(formatBpm(null), '--');
  assert.equal(formatBpm(NaN), '--');
});

test('formatDuration', () => {
  assert.equal(formatDuration(5000), '0:05');
  assert.equal(formatDuration(754_000), '12:34');
  assert.equal(formatDuration(3_723_000), '1:02:03');
  assert.equal(formatDuration(-1), '0:00');
});

test('formatCode groups six digits', () => {
  assert.equal(formatCode('042137'), '042 137');
  assert.equal(formatCode('12'), '12');
});

test('formatPercent and secondsLeft', () => {
  assert.equal(formatPercent(0.256), '26 %');
  assert.equal(secondsLeft(10_500, 0), 11);
  assert.equal(secondsLeft(0, 10_000), 0);
});

test('overlayUrl', () => {
  assert.equal(
    overlayUrl('http://192.0.2.10:8746/', 'w1', 'a-b_c', { preview: true, demo: true }),
    'http://192.0.2.10:8746/overlay/w1?token=a-b_c&preview=1&demo=1',
  );
  assert.equal(
    overlayUrl('http://127.0.0.1:8746', 'w1', 't'),
    'http://127.0.0.1:8746/overlay/w1?token=t',
  );
});

test('isLoopback', () => {
  assert.ok(isLoopback('http://127.0.0.1:8746'));
  assert.ok(isLoopback('http://localhost:8746'));
  assert.ok(isLoopback('http://[::1]:8746'));
  assert.ok(!isLoopback('http://192.0.2.10:8746'));
  assert.ok(!isLoopback('nonsense'));
});

test('trimRecent drops old points', () => {
  const points: [number, number][] = [
    [1, 1],
    [5, 2],
    [9, 3],
  ];
  assert.deepEqual(trimRecent(points, 10, 6), [
    [5, 2],
    [9, 3],
  ]);
  assert.equal(trimRecent(points, 10, 100), points);
});
