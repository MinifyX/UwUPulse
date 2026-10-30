/**
 * Heart rate zones: which zone a value is in and how long a session spent in
 * each. A zone starts at its `min` and runs up to the next zone's `min`.
 * Pure, tested in test/zones.test.ts.
 */

import { sortZones, type Zone } from './widget.ts';

/** Index into the sorted zones, or -1 below the first zone. */
export function zoneIndex(bpm: number, sorted: Zone[]): number {
  let index = -1;
  for (let i = 0; i < sorted.length; i++) {
    if (bpm >= sorted[i]!.min) index = i;
    else break;
  }
  return index;
}

export function zoneColor(bpm: number | null, zones: Zone[], fallback: string): string {
  if (bpm == null) return fallback;
  const sorted = sortZones(zones);
  const i = zoneIndex(bpm, sorted);
  return i < 0 ? fallback : sorted[i]!.color;
}

export interface ZoneShare {
  zone: Zone;
  /** Upper bound (exclusive), null for the last zone. */
  max: number | null;
  ms: number;
  share: number;
}

/**
 * Time in each zone. A sample counts until the next one, but a gap longer than
 * `maxGapMs` (the source dropped out) counts as `maxGapMs` only; the last
 * sample counts as one typical interval (1 s).
 */
export function zoneShares(
  samples: [number, number][],
  zones: Zone[],
  maxGapMs = 5000,
): ZoneShare[] {
  const sorted = sortZones(zones);
  const ms = new Array<number>(sorted.length).fill(0);
  for (let i = 0; i < samples.length; i++) {
    const [ts, bpm] = samples[i]!;
    const next = samples[i + 1];
    const span = next ? Math.min(Math.max(next[0] - ts, 0), maxGapMs) : 1000;
    const z = zoneIndex(bpm, sorted);
    // Below the first zone counts towards the first one.
    if (sorted.length) ms[Math.max(z, 0)]! += span;
  }
  const total = ms.reduce((a, b) => a + b, 0);
  return sorted.map((zone, i) => ({
    zone,
    max: sorted[i + 1]?.min ?? null,
    ms: ms[i]!,
    share: total ? ms[i]! / total : 0,
  }));
}

export interface Stats {
  min: number;
  max: number;
  avg: number;
}

export function stats(samples: [number, number][]): Stats | null {
  if (!samples.length) return null;
  let min = Infinity;
  let max = -Infinity;
  let sum = 0;
  for (const [, bpm] of samples) {
    if (bpm < min) min = bpm;
    if (bpm > max) max = bpm;
    sum += bpm;
  }
  return { min, max, avg: sum / samples.length };
}
