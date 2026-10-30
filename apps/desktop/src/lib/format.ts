/**
 * Small formatters without language: numbers, durations, codes, URLs.
 * Pure, tested in test/format.test.ts. Words go through lib/i18n.ts.
 */

export function formatBpm(bpm: number | null | undefined): string {
  return bpm == null || !Number.isFinite(bpm) ? '--' : String(Math.round(bpm));
}

/** 0:05, 12:34, 1:02:03 */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** A pairing code as "123 456", easier to compare at a glance. */
export function formatCode(code: string): string {
  const digits = code.replace(/\D/g, '');
  return digits.length === 6 ? `${digits.slice(0, 3)} ${digits.slice(3)}` : code;
}

export function formatPercent(share: number): string {
  return `${Math.round(share * 100)} %`;
}

/** Seconds left until `deadline`, never negative. */
export function secondsLeft(deadline: number, now: number): number {
  return Math.max(0, Math.ceil((deadline - now) / 1000));
}

/** The overlay URL of a widget on one of the server's base URLs. */
export function overlayUrl(
  base: string,
  id: string,
  token: string,
  options: { preview?: boolean; demo?: boolean } = {},
): string {
  const params = new URLSearchParams({ token });
  if (options.preview) params.set('preview', '1');
  if (options.demo) params.set('demo', '1');
  return `${base.replace(/\/+$/, '')}/overlay/${encodeURIComponent(id)}?${params}`;
}

/** True for 127.0.0.1, ::1 and localhost: a URL only this computer can open. */
export function isLoopback(url: string): boolean {
  try {
    const host = new URL(url).hostname.replace(/^\[|\]$/g, '');
    return host === 'localhost' || host === '::1' || /^127\./.test(host);
  } catch {
    return false;
  }
}

/** A dense sample buffer: drops what is older than `keepMs` before `now`. */
export function trimRecent<T extends [number, number]>(points: T[], now: number, keepMs: number) {
  const cutoff = now - keepMs;
  let i = 0;
  while (i < points.length && points[i]![0] < cutoff) i++;
  return i ? points.slice(i) : points;
}
