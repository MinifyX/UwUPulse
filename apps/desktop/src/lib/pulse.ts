/**
 * The live picture of the core: AppState from `get_state` and `pulse://state`,
 * the current value from `pulse://hr`, and the last minutes of samples for the
 * Live graph (backfilled from the running session, if one records).
 */

import { useEffect, useState, useSyncExternalStore } from 'react';
import { api, errorText, events, type AppState, type HrSample } from '../api';
import { trimRecent } from './format.ts';

/** The Live page keeps this much history. */
export const RECENT_MS = 10 * 60 * 1000;
/** A value older than this is not live any more (docs/plan.md, API `live`). */
export const LIVE_MS = 10_000;

export interface PulseSnapshot {
  state: AppState | null;
  error: string | null;
  recent: [number, number][];
  /** Local clock when the last value arrived (the phone's clock may differ). */
  receivedAt: number | null;
}

let snapshot: PulseSnapshot = { state: null, error: null, recent: [], receivedAt: null };
const listeners = new Set<() => void>();

function update(patch: Partial<PulseSnapshot>) {
  snapshot = { ...snapshot, ...patch };
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function usePulse(): PulseSnapshot {
  return useSyncExternalStore(subscribe, () => snapshot);
}

export function getPulse(): PulseSnapshot {
  return snapshot;
}

/** Replace the AppState locally after a command returned a part of it. */
export function patchState(patch: Partial<AppState>) {
  if (snapshot.state) update({ state: { ...snapshot.state, ...patch } });
}

async function backfill(sessionId: string) {
  try {
    const detail = await api.getSession(sessionId);
    const now = Date.now();
    const old = trimRecent(detail.samples, now, RECENT_MS);
    const first = snapshot.recent[0]?.[0] ?? Infinity;
    update({ recent: [...old.filter(([ts]) => ts < first), ...snapshot.recent] });
  } catch {
    // No history is fine: the graph fills from here on.
  }
}

function onSample(sample: HrSample) {
  const now = Date.now();
  const recent = trimRecent([...snapshot.recent, [sample.ts, sample.bpm]], now, RECENT_MS);
  const state = snapshot.state ? { ...snapshot.state, current: sample, live: true } : null;
  update({ recent, receivedAt: now, state });
}

let started = false;

/** Loads the state and follows the core's events. Once per window. */
export function startPulse() {
  if (started) return;
  started = true;
  events.onHr(onSample);
  events.onState((state) => {
    const fresh = state.live && !snapshot.receivedAt ? Date.now() : snapshot.receivedAt;
    update({ state, error: null, receivedAt: fresh });
  });
  api
    .getState()
    .then((state) => {
      update({ state, error: null, receivedAt: state.live ? Date.now() : null });
      if (state.recording) void backfill(state.recording.sessionId);
    })
    .catch((error) => update({ error: errorText(error) }));
}

/** Date.now(), refreshed every `ms`. */
export function useNow(ms = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(timer);
  }, [ms]);
  return now;
}

export function isLive(p: PulseSnapshot, now: number): boolean {
  return p.receivedAt != null && now - p.receivedAt <= LIVE_MS;
}
