# UwUPulse – Plan und Spezifikation

Herzfrequenz von der Apple Watch (und BLE-Brustgurten) **komplett lokal** an den Desktop: OBS-Overlay, Aufzeichnung, offene lokale API. Keine Cloud, kein Konto.

Entscheidungen (2026-10-30): UwUSuite-App, MIT-Lizenz, Tauri-Desktop (macOS/Windows/Linux), iOS 18 / watchOS 11, App Store + Open Source, Sessions nur auf dem Desktop (SQLite), Overlay mit Presets + Editor + Custom CSS, BLE-Gurte an iPhone **oder** Desktop, Kopplung per QR oder Code-Vergleich, API im LAN mit Token.

## Architektur

```
Apple Watch ──HKWorkoutSession (Mirroring)──▶ iPhone ──WebSocket, E2E-verschlüsselt──▶ Desktop (Tauri)
                                              ▲                                            │
BLE-Gurt ─────────────────────────────────────┘ (oder direkt an den Desktop) ──────────────┤
                                                                                           ├─ /overlay/<id>  (OBS Browser Source)
                                                                                           ├─ /api/v1/*      (REST + WebSocket, Token)
                                                                                           └─ SQLite         (Sessions)
```

- **Watch**: `HKWorkoutSession` + `HKLiveWorkoutBuilder` (Aktivität `.other`, indoor), startet Mirroring zum iPhone (`startMirroringToCompanionDevice`) und schickt jede HF per `sendToRemoteWorkoutSession(data:)`. Workout wird am Ende verworfen (`discardWorkout`), nichts landet in Health.
- **iPhone**: empfängt den gespiegelten Workout (`workoutSessionMirroringStartHandler`, gibt Hintergrundlaufzeit), kann die Watch-App per `startWatchApp(toHandle:)` starten, verbindet optional BLE-Gurte (CoreBluetooth, Service `0x180D`), findet Desktops per Bonjour, hält die WebSocket-Verbindung.
- **Desktop**: ein Server (axum) auf einem Port (Standard **8746**) für `/link` (iPhone), `/overlay/*` und `/api/v1/*`. Bonjour-Anzeige `_uwupulse._tcp`. BLE-Gurte direkt per btleplug. Aufzeichnung in SQLite.

## Repo-Layout

| Pfad | Inhalt |
|---|---|
| `crates/uwupulse-proto` | Link-Protokoll: Handshake, Schlüsselableitung, Frame-Verschlüsselung, Nachrichtentypen, BLE-HR-Parser. Keine I/O. |
| `crates/uwupulse-core` | `Pulse`-Hub: Quellen, Broadcast, Sessions (rusqlite), Server (axum: Link, Overlay, API), Bonjour (mdns-sd), BLE (btleplug, Feature `ble`), Konfiguration. |
| `crates/uwupulse-core/overlay/` | Overlay-Seite (handgeschriebenes HTML/CSS/JS, kein Build), per `include_str!` eingebettet. |
| `apps/desktop` | Tauri 2 + React/TS (`@uwupulse/desktop`), `@uwusuite/design`. |
| `apple/` | XcodeGen-Projekt: iOS-App, watchOS-App, Swift-Paket `PulseKit` (Protokoll, geteilte Modelle). |
| `docs/protocol-vectors.json` | Testvektoren aus Rust erzeugt, von Swift geprüft. |

Bundle-IDs: Desktop `app.uwupulse.desktop`, iOS `app.uwupulse.ios`, Watch `app.uwupulse.ios.watchkitapp`. Team `7N8YX2CL7J`.

## Link-Protokoll (iPhone ↔ Desktop), Version 1

Transport: WebSocket `ws://<host>:<port>/link`. Kein Token; Authentisierung über Schlüssel.

### Schlüssel
- Jedes Gerät hat ein statisches X25519-Schlüsselpaar (Desktop: Datei `link.key` im Konfig-Ordner, 0600; iPhone: Keychain) und eine Geräte-ID (UUID v4, Kleinbuchstaben).
- Base64 überall: **base64url ohne Padding** („b64“).

### Pairing-Wege
1. **QR**: Desktop erzeugt ein Angebot (5 min gültig, einmalig) mit 32 Byte Zufalls-Secret `s`.
   URI: `uwupulse://pair?v=1&id=<desk-id>&n=<name, urlencoded>&pk=<b64 desk spk>&s=<b64 secret>&p=<port>&h=<ip1>,<ip2>,…`
   iPhone kennt damit `pk` des Desktops (MITM-sicher), `psk = s`.
2. **Code-Vergleich (SAS)**: iPhone findet den Desktop per Bonjour, verbindet sich unauthentisiert, beide zeigen einen 6-stelligen Code; der Nutzer bestätigt am Desktop, dass er übereinstimmt. `psk = 32 Nullbytes`.
3. **Bekannt** (nach dem Pairing): `psk = 32 Nullbytes`; Authentisierung über DH der statischen Schlüssel, beide prüfen den gespeicherten statischen Schlüssel der Gegenseite.

### Handshake
1. iPhone → Desktop, Text-Frame (JSON, UTF-8, Bytes exakt wie gesendet = `m1`):
   `{"t":"hello","v":1,"id":"<phone-id>","name":"<Gerätename>","spk":"<b64>","epk":"<b64>","mode":"known"|"qr"}`
   Bei `sas` statt `epk` ein **Commitment**: `{"t":"hello","v":1,"id",…,"spk":"<b64>","commit":"<b64 SHA-256("UwUPulse-commit-v1" ‖ epk_phone)>","mode":"sas"}` (kein `epk`-Feld). Ohne Commitment könnte ein Angreifer in der Mitte sein `m2` so lange variieren, bis beide Codes gleich sind.
2. Desktop prüft: `v == 1`; bei `known` muss `spk` (und `id`) gepaart sein; bei `qr` muss ein gültiges Angebot offen sein; bei `sas` darf höchstens eine SAS-Anfrage gleichzeitig laufen. Sonst Text-Frame `{"t":"error","code":"version"|"unknown_device"|"no_offer"|"busy"}` und Schließen.
3. Desktop → iPhone, Text-Frame (`m2`):
   `{"t":"hello","v":1,"id":"<desk-id>","name":"<Name>","spk":"<b64>","epk":"<b64>"}`
4. iPhone prüft bei `known` und `qr`, dass `spk` dem gespeicherten bzw. dem QR-`pk` entspricht und bei `qr` zusätzlich `id` der Desktop-ID aus dem QR-Code, sonst Abbruch.
4a. Nur `sas`: iPhone → Desktop, Text-Frame (`m3`): `{"t":"reveal","epk":"<b64>"}`. Desktop prüft das Commitment (konstante Zeit), sonst `{"t":"error","code":"rejected"}` und Schließen. Handshake-Timeout gilt bis hier.
5. Beide berechnen:
   - `th = SHA-256("UwUPulse-link-v1" ‖ m1 ‖ m2)`, bei `sas` `SHA-256("UwUPulse-link-v1" ‖ m1 ‖ m2 ‖ m3)`
   - `ikm = X25519(e_phone, e_desk) ‖ X25519(s_phone, s_desk) ‖ psk` (je 32 Byte, zusammen 96)
   - `okm = HKDF-SHA256(salt = th, ikm, info = "uwupulse link keys", L = 68)`
   - `k_p2d = okm[0..32]`, `k_d2p = okm[32..64]`, `sas = u32_be(okm[64..68]) mod 1_000_000`, 6-stellig mit führenden Nullen.
6. Ab jetzt nur noch **Binär-Frames**: `ChaCha20-Poly1305(key, nonce = 0x00000000 ‖ u64_le(counter), aad = leer, plaintext = JSON UTF-8)`. Zähler je Richtung ab 0, pro Frame +1. Jeder Entschlüsselungsfehler, Text-Frame oder Frame > 16 KiB → Verbindung schließen.
7. iPhone sendet als erstes verschlüsselt `{"t":"auth"}`.
   - `known`: Desktop antwortet `{"t":"ready"}`.
   - `qr`: Desktop speichert das Gerät, verbraucht das Angebot, antwortet `{"t":"paired","id":"<desk-id>","name":"<Name>"}`, dann `{"t":"ready"}`.
   - `sas`: **Beide Seiten bestätigen.** Desktop zeigt die Anfrage mit Code sofort nach der Schlüsselableitung (nicht erst nach `auth`). Das iPhone zeigt den Code und sendet `auth` erst, nachdem die Person dort „Codes stimmen überein“ getippt hat; vorher speichert es nichts. Desktop schließt das Pairing erst ab, wenn die Person am Desktop bestätigt hat **und** `auth` angekommen ist, dann wie `qr`. Ablehnung (auf einer Seite), Timeout (120 s) oder Verbindungsende → `{"t":"error","code":"rejected"}` bzw. Schließen.
   - Pairing (`qr`/`sas`) mit einer Telefon-`id`, die schon mit einem **anderen** `spk` gepaart ist → `{"t":"error","code":"rejected"}`. (Neuinstallation erzeugt eine neue ID.)

### Nachrichten nach `ready`
iPhone → Desktop:
- `{"t":"hr","ts":<unix ms>,"bpm":<int>,"src":"watch"|"ble","dev":"<Quellgerät>","rr":[<ms>,…]}` (`rr` optional)
- `{"t":"status","watch":"unavailable"|"idle"|"running","ble":["<Name>",…],"battery":<0..1>|null}`
- `{"t":"ping"}` alle 10 s

Desktop → iPhone:
- `{"t":"pong"}`
- `{"t":"cmd","action":"start"|"stop"}` (Workout auf der Watch starten/stoppen)

Grenzen: `bpm` 20–250 (sonst verwerfen), höchstens 20 Nachrichten/s pro Verbindung, `dev`/`name` ≤ 64 Zeichen, Desktop schließt nach 30 s ohne Nachricht. Höchstens 8 gleichzeitige Link-Verbindungen, davon höchstens 4 vor `ready` und höchstens 2 vor `ready` pro IP. Upgrades mit `Origin`-Header werden abgelehnt (`403`), damit Webseiten im Browser `/link` nicht erreichen; die iPhone-App sendet keinen.

### Regeln für das iPhone
- Deep Link `uwupulse://pair?…` füllt nur vor: Die App zeigt Desktop-Name und Host und verbindet erst nach Bestätigung. `h=` darf nur private (RFC 1918), Link-Local-, ULA- (`fc00::/7`), Loopback-Adressen oder `*.local` enthalten; sonst wird der Code abgewiesen.
- Ein gespeicherter Desktop wird nie stillschweigend ersetzt: `paired.id` muss bei `qr` der QR-`id` entsprechen; gibt es die Desktop-`id` schon mit anderem `spk`, fragt die App, ob der alte ersetzt werden soll.
- Im Modus `known` sind Ablehnungen vor `ready` (Klartext-`error`, falscher `spk`) **nicht** endgültig: nächste Adresse probieren; erst wenn alle Adressen mehrfach mit `unknown_device` ablehnen, „Auf dem Desktop entfernt – neu koppeln“ anzeigen.

### Bonjour
`_uwupulse._tcp.local.`, Instanzname = Desktop-Name, TXT: `v=1`, `id=<desk-id>`, `pk=<b64 spk>`.

## Lokale HTTP-API (Desktop)

Bindet an `0.0.0.0:<port>` wenn `lanEnabled` (Standard **an**), sonst `127.0.0.1`. Alles außer `/link` braucht das Token (32 Byte zufällig, b64): Query `?token=` oder `Authorization: Bearer <token>`, Vergleich in konstanter Zeit, sonst `401`. CORS: `GET` von jedem Origin erlaubt (Token schützt).

| Route | Antwort |
|---|---|
| `GET /overlay/{widgetId}` | Overlay-HTML (Token und ID liest die Seite aus der URL) |
| `GET /overlay/assets/overlay.js`, `overlay.css` | statische Dateien (ohne Token, enthalten nichts Geheimes) |
| `GET /api/v1/current` | `{"bpm":72\|null,"ts":<ms>\|null,"src":"watch"\|"ble"\|null,"dev":…,"live":bool}` |
| `GET /api/v1/ws` | WebSocket, siehe unten |
| `GET /api/v1/widgets/{id}` | Widget-Konfiguration (JSON) |
| `GET /api/v1/sessions` | `SessionSummary[]` |
| `GET /api/v1/sessions/{id}` | `SessionDetail` |
| `GET /api/v1/sessions/{id}/export?format=csv\|json` | Datei (`Content-Disposition`) |

WebSocket `/api/v1/ws` (Server → Client, Text-JSON):
- beim Verbinden `{"t":"hello","version":"<app>","current":<wie /current>}`
- `{"t":"hr","ts","bpm","src","dev"}`
- `{"t":"live","live":bool}` (kein Wert seit `idleTimeoutSecs`/10 s → `false`)
- `{"t":"widget","id","config"}` wenn ein Widget gespeichert wurde

`live`: `true`, solange der letzte Wert ≤ 10 s alt ist.

## Aufzeichnung
- Neue Session beim ersten Wert, wenn keine läuft; Ende, wenn `idleTimeoutSecs` (Standard 120) kein Wert kam. Abschaltbar (`recordSessions`).
- SQLite (`pulse.db` im Datenordner): `sessions(id TEXT PK, started_at INT, ended_at INT NULL, min_bpm, max_bpm, avg_bpm REAL, sample_count, sources TEXT)`, `samples(session_id, ts INT, bpm INT, src TEXT)`, Index auf `(session_id, ts)`. Schreibt gepuffert (max. 1 s).
- Export CSV: `timestamp_ms,iso8601,bpm,source`; JSON: `SessionDetail`.

## Desktop-App: Tauri-Commands und Events

Alle Typen camelCase (serde `rename_all = "camelCase"`).

```ts
type Src = 'watch' | 'ble';
interface HrSample { ts: number; bpm: number; src: Src; dev: string }
interface Source { id: string; kind: 'phone' | 'ble'; name: string; connected: boolean;
  lastBpm: number | null; lastTs: number | null; detail: string | null }   // detail z. B. "Watch: running"
interface ServerInfo { port: number; lanEnabled: boolean; token: string; urls: string[]; error: string | null } // Basis-URLs: http://127.0.0.1:8746, http://<LAN-IP>:8746; error: z. B. „Port 8746 is not available: …“, wenn der Server nicht binden konnte
interface AppState { current: HrSample | null; live: boolean; sources: Source[]; server: ServerInfo;
  recording: { sessionId: string; startedAt: number } | null }
interface PairedDevice { id: string; name: string; pairedAt: number; lastSeen: number | null; connected: boolean }
interface PairingOffer { uri: string; qrSvg: string; expiresAt: number }
interface PairRequest { requestId: string; deviceName: string; code: string }
interface BleDevice { id: string; name: string; rssi: number | null; connected: boolean; connecting: boolean }
interface SessionSummary { id: string; startedAt: number; endedAt: number | null; minBpm: number;
  maxBpm: number; avgBpm: number; sampleCount: number; sources: string }
interface SessionDetail extends SessionSummary { samples: [number, number][] } // [ts, bpm]
interface Settings { port: number; lanEnabled: boolean; idleTimeoutSecs: number; recordSessions: boolean }
type Widget = WidgetConfig; // siehe unten; der Core prüft nur id/name (string), Größe ≤ 64 KiB und customCss (string, ≤ 32 KiB); `save_widget` ist ein Upsert über `id`
```

| Command | Argumente | Rückgabe |
|---|---|---|
| `get_state` | – | `AppState` |
| `list_paired` / `remove_paired` | – / `{ id }` | `PairedDevice[]` / – |
| `create_pairing` / `cancel_pairing` | – | `PairingOffer` / – (bei LAN aus enthält die URI keine Hosts) |
| `answer_pair_request` | `{ requestId, accept }` | – |
| `send_watch_command` | `{ deviceId, action: 'start' \| 'stop' }` | – |
| `ble_set_scanning` | `{ on }` | – |
| `ble_list` | – | `BleDevice[]` |
| `ble_connect` / `ble_disconnect` | `{ id }` | – |
| `list_widgets` | – | `Widget[]` (beim ersten Start ein Standard-Widget „Classic“) |
| `save_widget` / `delete_widget` | `{ widget }` / `{ id }` | `Widget` (normalisiert) / – (Fehler, wenn die ID unbekannt ist) |
| `list_sessions` / `get_session` / `delete_session` | – / `{ id }` / `{ id }` | … |
| `export_session` | `{ id, format: 'csv' \| 'json', path }` | – |
| `get_settings` / `set_settings` | – / `{ settings }` | `Settings` |
| `regenerate_token` | – | `ServerInfo` |

Events: `pulse://hr` (`HrSample`), `pulse://state` (`AppState`, bei Änderung von Quellen/Server/Aufzeichnung), `pulse://pair-request` (`PairRequest`), `pulse://pair-done` (`{ requestId, ok }`), `pulse://paired` (`PairedDevice[]`), `pulse://ble` (`BleDevice[]`), `pulse://sessions` (ohne Inhalt).

### WidgetConfig
```ts
interface WidgetConfig {
  id: string; name: string;
  preset: 'classic' | 'minimal' | 'ring' | 'graph' | 'ecg';
  color: string;        // Akzent, z. B. '#ff4d6d'
  textColor: string;    // '#ffffff'
  background: string;   // 'transparent'
  font: string;         // CSS font-family
  fontSize: number;     // px der BPM-Zahl
  showIcon: boolean; showLabel: boolean; animateBeat: boolean;
  showGraph: boolean; graphSeconds: number;
  zones: { enabled: boolean; items: { min: number; color: string }[] };
  idleText: string; hideWhenIdle: boolean;
  customCss: string;
}
```
Overlay-Seite: lädt `/api/v1/widgets/{id}`, verbindet `/api/v1/ws`, reconnectet mit Backoff, wendet `widget`-Nachrichten live an. `?preview=1` nimmt zusätzlich `postMessage({type:'uwupulse-config', config})` an (Editor-Vorschau ungespeicherter Änderungen), `?demo=1` erzeugt Fake-Werte, solange keine echten kommen. Custom CSS nur über `<style>.textContent`, nie als HTML.

## Desktop-UI (Seiten)
1. **Live** – große BPM-Anzeige mit Herz, Verlauf der letzten Minuten, Quellen-Liste, laufende Aufzeichnung, Watch starten/stoppen.
2. **Overlay** – Widget-Liste, Editor (Preset, Farben, Schrift, Größe, Zonen, Graph, Leerlauf, Custom CSS), Live-Vorschau (iframe), URL kopieren (localhost + LAN).
3. **Sessions** – Liste, Detail mit Graph (min/max/avg, Zonen-Anteile), Export, Löschen.
4. **Geräte** – gekoppelte iPhones (QR anzeigen, SAS-Anfragen bestätigen, entfernen), BLE-Gurte (scannen, verbinden).
5. **Einstellungen** – Port, LAN an/aus, Token anzeigen/kopieren/neu, Aufzeichnung, Leerlauf-Timeout, API-Doku-Link.
Sprachen: Deutsch und Englisch.

## iPhone-App
- **Start**: Watch-Status, aktuelle BPM, Workout starten/stoppen (startet Watch-App), Desktop-Verbindung.
- **Desktops**: gekoppelte Desktops, „Koppeln“ (QR-Scanner mit VisionKit, oder Bonjour-Liste → Code-Vergleich), entfernen. Verbindet automatisch zu allen gekoppelten Desktops, die per Bonjour sichtbar sind (Fallback: IPs aus dem QR).
- **Brustgurte**: BLE scannen/verbinden, Werte werden mit `src: "ble"` weitergeleitet.
- Info.plist: `NSHealthShareUsageDescription`, `NSHealthUpdateUsageDescription`, `NSLocalNetworkUsageDescription`, `NSBonjourServices = [_uwupulse._tcp]`, `NSCameraUsageDescription`, `NSBluetoothAlwaysUsageDescription`, Background Mode `bluetooth-central`. HealthKit-Entitlement.

## Watch-App
Große BPM-Anzeige, Start/Stopp, Status „gespiegelt zum iPhone“. Startbar vom iPhone (`handle(_ workoutConfiguration:)`). Keine Daten in Health speichern.

## Stand
- [x] Rust-Core + Protokoll + Testvektoren
- [x] Tauri-App
- [x] Overlay-Seite
- [x] iOS-/watchOS-App (Simulator-Build; HealthKit-Mirroring, BLE, Bonjour und QR auf echten Geräten noch ungetestet)
- [x] CI, Release-Workflows, README
- [x] Suite-Installer (`apps/setup`) und Release-Pipeline: `installers.yml` baut unsigniert, `pnpm release` signiert die Update-Dateien, veröffentlicht das Release, die Feeds (`updates`-Branch) und AUR `uwupulse-bin`
- [ ] Erstes Release per `pnpm release` (Update-Schlüssel, Branch `updates` entsteht dabei)
- [x] Security-Review (docs/security-review-2026-10.md)
- [ ] Erster Test mit echter Watch, iPhone und Desktop
- [ ] TestFlight / App-Store-Einreichung

Entwicklung: `UWUPULSE_PORT=<port>` überschreibt beim Start den Port aus den Einstellungen (wird nicht gespeichert).
