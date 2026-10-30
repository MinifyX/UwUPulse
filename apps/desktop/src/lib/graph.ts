/**
 * SVG paths for heart rate graphs. Points are [ts, bpm]; gaps longer than
 * `gapMs` break the line instead of drawing a bridge across a dropout.
 * Pure, tested in test/graph.test.ts.
 */

export type Point = [number, number];

export interface Frame {
  width: number;
  height: number;
  xMin: number;
  xMax: number;
  yMin: number;
  yMax: number;
}

/** A y range with some air, on multiples of 10, at least `minSpan` tall. */
export function yRange(points: Point[], minSpan = 40): { yMin: number; yMax: number } {
  if (!points.length) return { yMin: 50, yMax: 50 + Math.max(minSpan, 100) };
  let lo = Infinity;
  let hi = -Infinity;
  for (const [, v] of points) {
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  let yMin = Math.floor((lo - 5) / 10) * 10;
  let yMax = Math.ceil((hi + 5) / 10) * 10;
  if (yMax - yMin < minSpan) {
    const mid = (yMin + yMax) / 2;
    yMin = Math.floor((mid - minSpan / 2) / 10) * 10;
    yMax = yMin + Math.ceil(minSpan / 10) * 10;
  }
  return { yMin: Math.max(0, yMin), yMax };
}

/**
 * At most `max` points, keeping each bucket's lowest and highest value in
 * time order, so peaks survive.
 */
export function downsample(points: Point[], max: number): Point[] {
  if (points.length <= max || max < 4) return points;
  const buckets = Math.floor(max / 2);
  const size = points.length / buckets;
  const out: Point[] = [];
  for (let b = 0; b < buckets; b++) {
    const start = Math.floor(b * size);
    const end = Math.min(points.length, Math.floor((b + 1) * size));
    let lo = points[start]!;
    let hi = points[start]!;
    for (let i = start + 1; i < end; i++) {
      const p = points[i]!;
      if (p[1] < lo[1]) lo = p;
      if (p[1] > hi[1]) hi = p;
    }
    if (lo === hi) out.push(lo);
    else if (lo[0] <= hi[0]) out.push(lo, hi);
    else out.push(hi, lo);
  }
  return out;
}

const r = (n: number) => Math.round(n * 10) / 10;

function project(f: Frame) {
  const xs = f.xMax - f.xMin || 1;
  const ys = f.yMax - f.yMin || 1;
  return ([t, v]: Point): [number, number] => [
    r(((t - f.xMin) / xs) * f.width),
    r(f.height - ((v - f.yMin) / ys) * f.height),
  ];
}

/** Line segments, split at gaps. */
function segments(points: Point[], gapMs: number): Point[][] {
  const out: Point[][] = [];
  let current: Point[] = [];
  let last: number | null = null;
  for (const p of points) {
    if (last !== null && p[0] - last > gapMs) {
      if (current.length) out.push(current);
      current = [];
    }
    current.push(p);
    last = p[0];
  }
  if (current.length) out.push(current);
  return out;
}

/** The line as an SVG path `d`. A lone point becomes a short dash. */
export function linePath(points: Point[], frame: Frame, gapMs = 15000): string {
  const at = project(frame);
  return segments(points, gapMs)
    .map((seg) => {
      const xy = seg.map(at);
      if (xy.length === 1) {
        const [x, y] = xy[0]!;
        return `M${r(x - 1)} ${y}L${r(x + 1)} ${y}`;
      }
      return xy.map(([x, y], i) => `${i ? 'L' : 'M'}${x} ${y}`).join('');
    })
    .join('');
}

/** The area under the line, closed at the bottom, for a soft fill. */
export function areaPath(points: Point[], frame: Frame, gapMs = 15000): string {
  const at = project(frame);
  return segments(points, gapMs)
    .filter((seg) => seg.length > 1)
    .map((seg) => {
      const xy = seg.map(at);
      const first = xy[0]!;
      const last = xy[xy.length - 1]!;
      return (
        `M${first[0]} ${frame.height}` +
        xy.map(([x, y]) => `L${x} ${y}`).join('') +
        `L${last[0]} ${frame.height}Z`
      );
    })
    .join('');
}

/** Evenly spaced tick values inside [min, max], on a step of 10, 20, 50 … */
export function ticks(min: number, max: number, count = 4): number[] {
  const span = max - min;
  if (span <= 0) return [min];
  const raw = span / count;
  const step = [10, 20, 25, 50, 100].find((s) => s >= raw) ?? 100;
  const out: number[] = [];
  for (let v = Math.ceil(min / step) * step; v <= max; v += step) out.push(v);
  return out;
}
