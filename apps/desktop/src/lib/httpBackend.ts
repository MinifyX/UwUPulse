/**
 * The page's backend when the core serves it at `/ui/` (the OBS plugin's
 * dock, docs/plan.md "/ui"): commands as `POST /ui/rpc/<command>`, events
 * over one WebSocket at `/ui/events` that reconnects by itself. Same command
 * names, arguments and event payloads as with Tauri.
 */

export type Unlisten = () => void;

export interface Backend {
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  listen: <T>(event: string, handler: (payload: T) => void) => Promise<Unlisten>;
}

const HEX64 = /^[0-9a-f]{64}$/;
/** Where a page opened with a ticket keeps the secret: this tab only, never in the URL. */
export const SECRET_KEY = 'uwupulse-ui-secret';

/**
 * How the page gets in when the core serves it at `/ui/`: the secret of this
 * run (`#s=`, OBS's own dock), a one-time ticket (`#t=`, the system browser)
 * or the secret an earlier ticket bought in this tab. Null when the page
 * isn't served that way.
 */
export type UiAuth = { secret: string } | { ticket: string };

export function uiAuth(
  location: { pathname: string; hash: string },
  storage?: Pick<Storage, 'getItem'> | null,
): UiAuth | null {
  if (!location.pathname.startsWith('/ui/')) return null;
  const params = new URLSearchParams(location.hash.replace(/^#/, ''));
  const secret = params.get('s');
  if (secret && HEX64.test(secret)) return { secret };
  const ticket = params.get('t');
  if (ticket && HEX64.test(ticket)) return { ticket };
  let kept: string | null = null;
  try {
    kept = storage?.getItem(SECRET_KEY) ?? null;
  } catch {
    // Storage blocked: the page needs a new ticket.
  }
  return kept && HEX64.test(kept) ? { secret: kept } : null;
}

/**
 * Trades a ticket (single use, 30 s) for the secret at `POST /ui/ticket` and
 * keeps it in `storage` for reloads of this tab. Rejects with the error text.
 */
export async function redeemTicket(
  origin: string,
  ticket: string,
  options: { fetch?: typeof fetch; storage?: Pick<Storage, 'setItem'> | null } = {},
): Promise<string> {
  const doFetch = options.fetch ?? ((input, init) => fetch(input, init));
  const response = await doFetch(`${origin}/ui/ticket`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ticket }),
  });
  const body: unknown = await response.json().catch(() => null);
  const field = (name: string) =>
    body && typeof body === 'object' && name in body
      ? (body as Record<string, unknown>)[name]
      : null;
  const secret = field('secret');
  if (!response.ok || typeof secret !== 'string' || !HEX64.test(secret)) {
    const error = field('error');
    throw typeof error === 'string' ? error : `HTTP ${response.status}`;
  }
  try {
    options.storage?.setItem(SECRET_KEY, secret);
  } catch {
    // Without storage a reload needs a new ticket; nothing else changes.
  }
  return secret;
}

type Socket = Pick<WebSocket, 'close'> & {
  onmessage: ((event: { data: unknown }) => void) | null;
  onclose: (() => void) | null;
};

export interface HttpBackendOptions {
  /** The page's own origin, e.g. `http://127.0.0.1:8746`. */
  origin: string;
  /** The secret, or its promise while a ticket is being traded. */
  secret: string | Promise<string>;
  fetch?: typeof fetch;
  openSocket?: (url: string) => Socket;
  /** Reconnect delays in ms, the last one repeats. */
  backoff?: number[];
  setTimer?: (run: () => void, ms: number) => unknown;
}

export function createHttpBackend(options: HttpBackendOptions): Backend {
  const doFetch = options.fetch ?? ((input, init) => fetch(input, init));
  const openSocket = options.openSocket ?? ((url: string) => new WebSocket(url) as Socket);
  const backoff = options.backoff ?? [250, 500, 1000, 2000, 5000];
  const setTimer = options.setTimer ?? ((run, ms) => setTimeout(run, ms));
  const handlers = new Map<string, Set<(payload: unknown) => void>>();
  let socket: Socket | null = null;
  let started = false;
  let failures = 0;
  let known = typeof options.secret === 'string' ? options.secret : null;
  const secret = Promise.resolve(options.secret).then((s) => (known = s));
  // A failed ticket rejects every call; the socket just never opens.
  secret.catch(() => undefined);

  function connect() {
    if (known) open(known);
    else void secret.then(open, () => undefined);
  }

  function open(key: string) {
    const ws = openSocket(`${options.origin.replace(/^http/, 'ws')}/ui/events?s=${key}`);
    socket = ws;
    ws.onmessage = (message) => {
      failures = 0;
      let parsed: { event?: unknown; payload?: unknown };
      try {
        parsed = JSON.parse(String(message.data));
      } catch {
        return;
      }
      if (typeof parsed.event !== 'string') return;
      for (const handler of handlers.get(parsed.event) ?? []) handler(parsed.payload ?? null);
    };
    ws.onclose = () => {
      if (socket !== ws) return;
      socket = null;
      const delay = backoff[Math.min(failures, backoff.length - 1)] ?? 1000;
      failures += 1;
      setTimer(connect, delay);
    };
  }

  return {
    async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
      const key = await secret;
      const response = await doFetch(`${options.origin}/ui/rpc/${encodeURIComponent(command)}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'X-UwUPulse-UI': key },
        body: JSON.stringify(args ?? {}),
      });
      const body: unknown = await response.json().catch(() => null);
      if (!response.ok) {
        // Rejected with the text, as Tauri rejects with the Rust error's string.
        const error = body && typeof body === 'object' && 'error' in body ? body.error : null;
        throw typeof error === 'string' ? error : `HTTP ${response.status}`;
      }
      return body as T;
    },
    async listen<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
      const set = handlers.get(event) ?? new Set();
      const h = handler as (payload: unknown) => void;
      set.add(h);
      handlers.set(event, set);
      if (!started) {
        started = true;
        connect();
      }
      return () => set.delete(h);
    },
  };
}
