/**
 * A stand-in for the Rust core when the UI runs in a plain browser
 * (`pnpm dev` without Tauri). Fake values, in memory, nothing persisted.
 * `window.uwupulseMock.pairRequest()` shows the code-comparison dialog.
 */

import type { AppState, BleDevice, PairedDevice, SessionDetail, Settings, Widget } from '../api';
import { defaultWidget } from '../lib/widget.ts';

type Handler = (payload: unknown) => void;
const handlers = new Map<string, Set<Handler>>();

function emit(event: string, payload: unknown) {
  for (const handler of handlers.get(event) ?? []) handler(payload);
}

const now = Date.now();
let settings: Settings = {
  port: 8746,
  lanEnabled: true,
  idleTimeoutSecs: 120,
  recordSessions: true,
};
let widgets: Widget[] = [defaultWidget('wdefault0001')];
let paired: PairedDevice[] = [
  {
    id: 'p1',
    name: 'iPhone von Mia',
    pairedAt: now - 86_400_000 * 3,
    lastSeen: now,
    connected: true,
  },
];
let ble: BleDevice[] = [];
let scanTimer: ReturnType<typeof setInterval> | null = null;

function fakeSession(id: string, startedAt: number, minutes: number, base: number): SessionDetail {
  const samples: [number, number][] = [];
  for (let i = 0; i < minutes * 60; i++) {
    const bpm = Math.round(
      base + 35 * Math.sin(i / 200) + 10 * Math.sin(i / 23) + Math.random() * 4,
    );
    samples.push([startedAt + i * 1000, bpm]);
  }
  const bpms = samples.map(([, b]) => b);
  return {
    id,
    startedAt,
    endedAt: startedAt + minutes * 60_000,
    minBpm: Math.min(...bpms),
    maxBpm: Math.max(...bpms),
    avgBpm: bpms.reduce((a, b) => a + b, 0) / bpms.length,
    sampleCount: samples.length,
    sources: 'watch',
    samples,
  };
}

let sessions: SessionDetail[] = [
  fakeSession('s2', now - 86_400_000, 42, 120),
  fakeSession('s1', now - 86_400_000 * 2, 95, 100),
];

const state: AppState = {
  current: null,
  live: false,
  sources: [
    {
      id: 'p1',
      kind: 'phone',
      name: 'iPhone von Mia',
      connected: true,
      lastBpm: null,
      lastTs: null,
      detail: 'Watch: running',
    },
  ],
  server: {
    port: 8746,
    lanEnabled: true,
    token: 'mock-token-0123456789abcdefghijklmnopqrstu',
    urls: ['http://127.0.0.1:8746', 'http://192.0.2.10:8746'],
    error: null,
  },
  recording: { sessionId: 's2', startedAt: now - 120_000 },
};

let t = 0;
setInterval(() => {
  t += 1;
  const bpm = Math.round(95 + 30 * Math.sin(t / 40) + 6 * Math.sin(t / 7));
  const sample = { ts: Date.now(), bpm, src: 'watch' as const, dev: 'Apple Watch' };
  state.current = sample;
  state.live = true;
  state.sources[0]!.lastBpm = bpm;
  state.sources[0]!.lastTs = sample.ts;
  emit('pulse://hr', sample);
}, 1000);

const summary = ({ samples: _, ...rest }: SessionDetail) => rest;

async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  await new Promise((r) => setTimeout(r, 60));
  const result = ((): unknown => {
    switch (command) {
      case 'get_state':
        return structuredClone(state);
      case 'list_paired':
        return paired;
      case 'remove_paired':
        paired = paired.filter((d) => d.id !== args.id);
        emit('pulse://paired', paired);
        return null;
      case 'create_pairing': {
        const cells = Array.from({ length: 21 * 21 }, (_, i) =>
          Math.random() > 0.5
            ? `<rect x="${i % 21}" y="${Math.floor(i / 21)}" width="1" height="1"/>`
            : '',
        ).join('');
        setTimeout(() => {
          paired = [
            ...paired,
            {
              id: 'p2',
              name: 'iPhone 17',
              pairedAt: Date.now(),
              lastSeen: Date.now(),
              connected: true,
            },
          ];
          emit('pulse://paired', paired);
        }, 12_000);
        return {
          uri: 'uwupulse://pair?v=1',
          qrSvg: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="-2 -2 25 25"><rect x="-2" y="-2" width="25" height="25" fill="#fff"/>${cells}</svg>`,
          expiresAt: Date.now() + 300_000,
        };
      }
      case 'cancel_pairing':
      case 'answer_pair_request':
      case 'send_watch_command':
        if (command === 'answer_pair_request')
          setTimeout(
            () => emit('pulse://pair-done', { requestId: args.requestId, ok: !!args.accept }),
            300,
          );
        return null;
      case 'ble_set_scanning':
        if (scanTimer) clearInterval(scanTimer);
        scanTimer = null;
        if (args.on) {
          scanTimer = setInterval(() => {
            if (ble.length < 3) {
              ble = [
                ...ble,
                {
                  id: `b${ble.length}`,
                  name: `Polar H10 ${ble.length + 1}`,
                  rssi: -50 - ble.length * 9,
                  connected: false,
                  connecting: false,
                },
              ];
              emit('pulse://ble', ble);
            }
          }, 1500);
        }
        return null;
      case 'ble_list':
        return ble;
      case 'ble_connect':
      case 'ble_disconnect':
        ble = ble.map((d) =>
          d.id === args.id ? { ...d, connected: command === 'ble_connect' } : d,
        );
        emit('pulse://ble', ble);
        return null;
      case 'list_widgets':
        return widgets;
      case 'save_widget': {
        const w = args.widget as Widget;
        widgets = widgets.some((x) => x.id === w.id)
          ? widgets.map((x) => (x.id === w.id ? w : x))
          : [...widgets, w];
        return w;
      }
      case 'delete_widget':
        widgets = widgets.filter((w) => w.id !== args.id);
        return null;
      case 'list_sessions':
        return sessions.map(summary);
      case 'get_session':
        return sessions.find((s) => s.id === args.id) ?? Promise.reject('not found');
      case 'delete_session':
        sessions = sessions.filter((s) => s.id !== args.id);
        emit('pulse://sessions', null);
        return null;
      case 'export_session':
        return null;
      case 'get_settings':
        return settings;
      case 'set_settings':
        settings = args.settings as Settings;
        return settings;
      case 'regenerate_token':
        state.server = { ...state.server, token: `mock-${Math.random().toString(36).slice(2)}` };
        emit('pulse://state', structuredClone(state));
        return state.server;
      default:
        throw new Error(`mock: unknown command ${command}`);
    }
  })();
  return (await result) as T;
}

export const mockBackend = {
  invoke,
  async listen<T>(event: string, handler: (payload: T) => void) {
    const set = handlers.get(event) ?? new Set();
    set.add(handler as Handler);
    handlers.set(event, set);
    return () => set.delete(handler as Handler);
  },
};

(window as unknown as Record<string, unknown>).uwupulseMock = {
  pairRequest: () =>
    emit('pulse://pair-request', { requestId: 'r1', deviceName: 'iPhone 17', code: '042137' }),
};
