import assert from 'node:assert/strict';
import { test } from 'node:test';
import { areaPath, downsample, linePath, ticks, yRange, type Point } from '../src/lib/graph.ts';

const frame = { width: 100, height: 50, xMin: 0, xMax: 10, yMin: 0, yMax: 100 };

test('linePath projects points into the frame', () => {
  assert.equal(
    linePath(
      [
        [0, 0],
        [10, 100],
      ],
      frame,
    ),
    'M0 50L100 0',
  );
});

test('linePath breaks at gaps', () => {
  const d = linePath(
    [
      [0, 50],
      [1, 50],
      [9, 50],
      [10, 50],
    ],
    frame,
    5,
  );
  assert.equal(d, 'M0 25L10 25M90 25L100 25');
});

test('a lone point becomes a dash', () => {
  assert.equal(linePath([[5, 50]], frame), 'M49 25L51 25');
});

test('areaPath closes at the bottom', () => {
  assert.equal(
    areaPath(
      [
        [0, 100],
        [10, 100],
      ],
      frame,
    ),
    'M0 50L0 0L100 0L100 50Z',
  );
});

test('downsample keeps peaks and order', () => {
  const points: Point[] = Array.from({ length: 1000 }, (_, i) => [i, i === 500 ? 200 : 80]);
  const out = downsample(points, 100);
  assert.ok(out.length <= 100);
  assert.ok(out.some(([, v]) => v === 200));
  for (let i = 1; i < out.length; i++) assert.ok(out[i]![0] > out[i - 1]![0]);
  assert.equal(downsample(points.slice(0, 10), 100).length, 10);
});

test('yRange pads, rounds and keeps a minimum span', () => {
  assert.deepEqual(
    yRange([
      [0, 72],
      [1, 75],
    ]),
    { yMin: 50, yMax: 90 },
  );
  assert.deepEqual(
    yRange([
      [0, 60],
      [1, 180],
    ]),
    { yMin: 50, yMax: 190 },
  );
  const empty = yRange([]);
  assert.ok(empty.yMax > empty.yMin);
});

test('ticks', () => {
  assert.deepEqual(ticks(50, 190), [50, 100, 150]);
  assert.deepEqual(ticks(60, 100, 4), [60, 70, 80, 90, 100]);
  assert.deepEqual(ticks(5, 5), [5]);
});
