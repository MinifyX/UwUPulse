import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHttpBackend, uiSecret } from '../src/lib/httpBackend.ts';

const SECRET = 'ab'.repeat(32);
const ORIGIN = 'http://127.0.0.1:8746';

test('the secret comes only from /ui/#s=<64 hex>', () => {
  assert.equal(uiSecret({ pathname: '/ui/', hash: `#s=${SECRET}` }), SECRET);
  assert.equal(uiSecret({ pathname: '/ui/index.html', hash: `#s=${SECRET}` }), SECRET);
  assert.equal(uiSecret({ pathname: '/', hash: `#s=${SECRET}` }), null);
  assert.equal(uiSecret({ pathname: '/ui/', hash: '' }), null);
  assert.equal(uiSecret({ pathname: '/ui/', hash: '#s=short' }), null);
});

function fakeFetch(status: number, body: unknown) {
  const calls: { url: string; init: RequestInit }[] = [];
  const fetch = (async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    return new Response(JSON.stringify(body), { status });
  }) as unknown as typeof globalThis.fetch;
  return { calls, fetch };
}

test('invoke posts the arguments and returns the JSON', async () => {
  const { calls, fetch } = fakeFetch(200, { port: 8746 });
  const backend = createHttpBackend({ origin: ORIGIN, secret: SECRET, fetch });
  assert.deepEqual(await backend.invoke('get_settings', { a: 1 }), { port: 8746 });
  assert.equal(calls[0]!.url, `${ORIGIN}/ui/rpc/get_settings`);
  assert.equal(calls[0]!.init.method, 'POST');
  assert.equal(calls[0]!.init.body, '{"a":1}');
  assert.equal((calls[0]!.init.headers as Record<string, string>)['X-UwUPulse-UI'], SECRET);
});

test('invoke rejects with the error text, like Tauri', async () => {
  const backend = createHttpBackend({
    origin: ORIGIN,
    secret: SECRET,
    fetch: fakeFetch(400, { error: 'not found' }).fetch,
  });
  await assert.rejects(backend.invoke('remove_paired', { id: 'x' }), (e) => e === 'not found');
  const plain = createHttpBackend({
    origin: ORIGIN,
    secret: SECRET,
    fetch: fakeFetch(401, null).fetch,
  });
  await assert.rejects(plain.invoke('get_state'), (e) => e === 'HTTP 401');
});

test('events arrive over one socket that reconnects', async () => {
  type Fake = {
    url: string;
    onmessage: ((e: { data: unknown }) => void) | null;
    onclose: (() => void) | null;
    close: () => void;
  };
  const sockets: Fake[] = [];
  const timers: { run: () => void; ms: number }[] = [];
  const backend = createHttpBackend({
    origin: ORIGIN,
    secret: SECRET,
    openSocket: (url) => {
      const s: Fake = { url, onmessage: null, onclose: null, close: () => undefined };
      sockets.push(s);
      return s;
    },
    backoff: [10, 20],
    setTimer: (run, ms) => timers.push({ run, ms }),
  });
  const seen: unknown[] = [];
  const stop = await backend.listen<{ bpm: number }>('pulse://hr', (p) => seen.push(p.bpm));
  await backend.listen('pulse://state', () => undefined);
  assert.equal(sockets.length, 1);
  assert.equal(sockets[0]!.url, `ws://127.0.0.1:8746/ui/events?s=${SECRET}`);

  sockets[0]!.onmessage!({ data: '{"event":"pulse://hr","payload":{"bpm":91}}' });
  sockets[0]!.onmessage!({ data: 'not json' });
  sockets[0]!.onclose!();
  sockets[0]!.onclose!();
  assert.deepEqual(
    timers.map((t) => t.ms),
    [10],
  );
  timers[0]!.run();
  assert.equal(sockets.length, 2);
  sockets[1]!.onclose!();
  timers[1]!.run();
  sockets[2]!.onmessage!({ data: '{"event":"pulse://hr","payload":{"bpm":92}}' });
  stop();
  sockets[2]!.onmessage!({ data: '{"event":"pulse://hr","payload":{"bpm":93}}' });
  assert.deepEqual(seen, [91, 92]);
  assert.deepEqual(
    timers.map((t) => t.ms),
    [10, 20],
  );
});
