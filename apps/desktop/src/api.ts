/**
 * Typed wrappers for every Tauri command and event of the core (docs/plan.md,
 * "Desktop-App: Tauri-Commands und Events"). Command names are snake_case,
 * argument names camelCase as Tauri expects them.
 *
 * Served by the core at `/ui/#s=<secret>` (the OBS plugin's dock) they go
 * over HTTP and a WebSocket (lib/httpBackend.ts). Otherwise, outside Tauri
 * (plain `pnpm dev` in a browser), they go to a small mock (dev/mock.ts), so
 * the UI can be worked on without the Rust side.
 */

import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen, type UnlistenFn } from '@tauri-apps/api/event';
import { createHttpBackend, uiSecret } from './lib/httpBackend.ts';
import type { UpdateChannel } from './lib/prefs.ts';
import type { ProjectPage, UpdateInfo } from './lib/updates.ts';
import type { WidgetConfig } from './lib/widget.ts';

export type { WidgetConfig } from './lib/widget.ts';
export type { ProjectPage, UpdateInfo } from './lib/updates.ts';

export type Src = 'watch' | 'ble';

export interface HrSample {
  ts: number;
  bpm: number;
  src: Src;
  dev: string;
}

export interface Source {
  id: string;
  kind: 'phone' | 'ble';
  name: string;
  connected: boolean;
  lastBpm: number | null;
  lastTs: number | null;
  /** e.g. "Watch: running" */
  detail: string | null;
}

export interface ServerInfo {
  port: number;
  lanEnabled: boolean;
  token: string;
  /** Base URLs: http://127.0.0.1:8746, http://<LAN-IP>:8746 */
  urls: string[];
  /** Why the server is not running (port taken, …); null while it runs. */
  error: string | null;
}

export interface Recording {
  sessionId: string;
  startedAt: number;
}

export interface AppState {
  current: HrSample | null;
  live: boolean;
  sources: Source[];
  server: ServerInfo;
  recording: Recording | null;
}

export interface PairedDevice {
  id: string;
  name: string;
  pairedAt: number;
  lastSeen: number | null;
  connected: boolean;
}

export interface PairingOffer {
  uri: string;
  qrSvg: string;
  expiresAt: number;
}

export interface PairRequest {
  requestId: string;
  deviceName: string;
  code: string;
}

export interface PairDone {
  requestId: string;
  ok: boolean;
}

export interface BleDevice {
  id: string;
  name: string;
  rssi: number | null;
  connected: boolean;
  connecting: boolean;
}

export interface SessionSummary {
  id: string;
  startedAt: number;
  endedAt: number | null;
  minBpm: number;
  maxBpm: number;
  avgBpm: number;
  sampleCount: number;
  sources: string;
}

export interface SessionDetail extends SessionSummary {
  /** [ts, bpm] */
  samples: [number, number][];
}

export interface Settings {
  port: number;
  lanEnabled: boolean;
  idleTimeoutSecs: number;
  recordSessions: boolean;
}

export type Widget = WidgetConfig;
export type WatchAction = 'start' | 'stop';
export type ExportFormat = 'csv' | 'json';

/** Who shows the page (`get_host_info`). */
export interface HostInfo {
  kind: 'desktop' | 'obs';
  /** Whether the update commands do anything. */
  updater: boolean;
}

/** True inside the Tauri webview. */
export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const secret = !inTauri && typeof window !== 'undefined' ? uiSecret(window.location) : null;

/** How commands and events travel: Tauri IPC, the core's `/ui` over HTTP, or the dev mock. */
export const backendKind: 'tauri' | 'http' | 'mock' = inTauri ? 'tauri' : secret ? 'http' : 'mock';

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type Listen = <T>(event: string, handler: (payload: T) => void) => Promise<UnlistenFn>;

let backend: { invoke: Invoke; listen: Listen } | null = inTauri
  ? {
      invoke: (command, args) => tauriInvoke(command, args),
      listen: (event, handler) => tauriListen(event, (e) => handler(e.payload as never)),
    }
  : secret
    ? createHttpBackend({ origin: window.location.origin, secret })
    : null;

async function getBackend() {
  if (!backend) {
    if (!import.meta.env.DEV) throw new Error('UwUPulse runs inside its Tauri window only');
    backend = (await import('./dev/mock.ts')).mockBackend;
  }
  return backend;
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return (await getBackend()).invoke<T>(command, args);
}

/** Subscribes to an event; returns a function that unsubscribes (also before the listener is up). */
function on<T>(event: string, handler: (payload: T) => void): () => void {
  let stop: UnlistenFn | null = null;
  let stopped = false;
  void getBackend()
    .then((b) => b.listen<T>(event, handler))
    .then((unlisten) => {
      if (stopped) unlisten();
      else stop = unlisten;
    });
  return () => {
    stopped = true;
    stop?.();
  };
}

// ── Commands ─────────────────────────────────────────────

export const api = {
  getState: () => call<AppState>('get_state'),
  getHostInfo: () => call<HostInfo>('get_host_info'),

  listPaired: () => call<PairedDevice[]>('list_paired'),
  removePaired: (id: string) => call<void>('remove_paired', { id }),
  createPairing: () => call<PairingOffer>('create_pairing'),
  cancelPairing: () => call<void>('cancel_pairing'),
  answerPairRequest: (requestId: string, accept: boolean) =>
    call<void>('answer_pair_request', { requestId, accept }),
  sendWatchCommand: (deviceId: string, action: WatchAction) =>
    call<void>('send_watch_command', { deviceId, action }),

  bleSetScanning: (on: boolean) => call<void>('ble_set_scanning', { on }),
  bleList: () => call<BleDevice[]>('ble_list'),
  bleConnect: (id: string) => call<void>('ble_connect', { id }),
  bleDisconnect: (id: string) => call<void>('ble_disconnect', { id }),

  listWidgets: () => call<Widget[]>('list_widgets'),
  saveWidget: (widget: Widget) => call<Widget>('save_widget', { widget }),
  deleteWidget: (id: string) => call<void>('delete_widget', { id }),

  listSessions: () => call<SessionSummary[]>('list_sessions'),
  getSession: (id: string) => call<SessionDetail>('get_session', { id }),
  deleteSession: (id: string) => call<void>('delete_session', { id }),
  /** Shows the save dialog (in the app, not the page); the path written, or null if cancelled. */
  exportSession: (id: string, format: ExportFormat) =>
    call<string | null>('export_session', { id, format }),

  getSettings: () => call<Settings>('get_settings'),
  setSettings: (settings: Settings) => call<Settings>('set_settings', { settings }),
  regenerateToken: () => call<ServerInfo>('regenerate_token'),

  /** The About card's links; Rust knows the addresses. */
  openProjectPage: (page: ProjectPage) => call<void>('open_project_page', { page }),

  // Updates (src-tauri/src/updates.rs). A build without the updater answers
  // "nothing waiting" and never finds one.
  setUpdateChannel: (channel: UpdateChannel) => call<void>('set_update_channel', { channel }),
  setAutoUpdate: (on: boolean) => call<void>('set_auto_update', { on }),
  updateStatus: () => call<UpdateInfo | null>('update_status'),
  checkForUpdates: () => call<UpdateInfo | null>('check_for_updates'),
  installUpdate: () => call<void>('install_update'),
};

// ── Events ───────────────────────────────────────────────

export const events = {
  onHr: (handler: (sample: HrSample) => void) => on<HrSample>('pulse://hr', handler),
  onState: (handler: (state: AppState) => void) => on<AppState>('pulse://state', handler),
  onPairRequest: (handler: (request: PairRequest) => void) =>
    on<PairRequest>('pulse://pair-request', handler),
  onPairDone: (handler: (done: PairDone) => void) => on<PairDone>('pulse://pair-done', handler),
  onPaired: (handler: (devices: PairedDevice[]) => void) =>
    on<PairedDevice[]>('pulse://paired', handler),
  onBle: (handler: (devices: BleDevice[]) => void) => on<BleDevice[]>('pulse://ble', handler),
  onSessions: (handler: () => void) => on<unknown>('pulse://sessions', () => handler()),
  /** A new version is downloaded, checked and waiting. */
  onUpdateReady: (handler: (update: UpdateInfo) => void) => on<UpdateInfo>('update:ready', handler),
};

/** A command error as text: Tauri rejects with the Rust error's string (or an object). */
export function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  if (error && typeof error === 'object' && 'message' in error) return String(error.message);
  return JSON.stringify(error);
}
