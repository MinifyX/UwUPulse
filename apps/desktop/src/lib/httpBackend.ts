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

/** The secret of this run, from `/ui/#s=<secret>`; null when the page isn't served that way. */
export function uiSecret(location: { pathname: string; hash: string }): string | null {
  if (!location.pathname.startsWith('/ui/')) return null;
  const secret = new URLSearchParams(location.hash.replace(/^#/, '')).get('s');
  return secret && /^[0-9a-f]{64}$/.test(secret) ? secret : null;
}

type Socket = Pick<WebSocket, 'close'> & {
  onmessage: ((event: { data: unknown }) => void) | null;
  onclose: (() => void) | null;
};

export interface HttpBackendOptions {
  /** The page's own origin, e.g. `http://127.0.0.1:8746`. */
  origin: string;
  secret: string;
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

  const wsUrl = `${options.origin.replace(/^http/, 'ws')}/ui/events?s=${options.secret}`;

  function connect() {
    const ws = openSocket(wsUrl);
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
      const response = await doFetch(`${options.origin}/ui/rpc/${encodeURIComponent(command)}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'X-UwUPulse-UI': options.secret },
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
