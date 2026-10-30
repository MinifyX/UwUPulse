<p align="center"><img src="assets/icon.svg" width="120" alt="UwUPulse"></p>

<h1 align="center">UwUPulse</h1>

<p align="center">Your heart rate from Apple Watch or a Bluetooth chest strap — on stream, in your tools, on your disk. <b>Fully local.</b> No cloud, no account.</p>

---

UwUPulse is a local alternative to cloud heart-rate services for streamers and tinkerers. The Apple Watch measures, the iPhone relays, and the desktop app shows your pulse as an OBS overlay, records your sessions and offers a small local API. Nothing ever leaves your network.

## Features

- **Apple Watch** live heart rate (about once per second) through a workout session mirrored to the iPhone; nothing is saved to Apple Health.
- **BLE chest straps** (any standard Heart Rate Service device: Polar, Garmin, Wahoo, …), connected to the iPhone *or* straight to the desktop.
- **OBS overlays** as a Browser Source: five presets (Classic, Minimal, Ring, Graph, ECG), a visual editor with live preview, colour zones and custom CSS.
- **Session recording** in a local SQLite database, with graphs, min/max/average, zone shares and CSV/JSON export.
- **Local API**: REST and WebSocket in your LAN, protected by a token.
- **Secure pairing**: scan a QR code, or compare a six-digit code. The iPhone–desktop link is end-to-end encrypted (X25519, HKDF-SHA256, ChaCha20-Poly1305).

## How it works

```
Apple Watch ──workout mirroring──▶ iPhone ──encrypted WebSocket (LAN)──▶ UwUPulse desktop
BLE strap ───────────────────────▶ (iPhone or desktop)                      ├─ OBS overlay
                                                                            ├─ REST / WebSocket API
                                                                            └─ SQLite sessions
```

## Getting started

1. Install the desktop app from the [releases](https://github.com/MinifyX/UwUPulse/releases) (macOS, Windows, Debian/Ubuntu, Arch).
2. Install UwUPulse on your iPhone; the Watch app comes with it.
3. Desktop → **Devices** → **Pair iPhone**, scan the QR code with the iPhone app.
4. Start the workout on the Watch (or from the iPhone or desktop).
5. Desktop → **Overlay**: pick a preset, copy the URL, add it to OBS as a Browser Source.

## Local API

Base URL: `http://<desktop>:8746`. Every request needs the token from **Settings**, as `?token=…` or `Authorization: Bearer …`.

| Endpoint | |
|---|---|
| `GET /api/v1/current` | latest value: `{"bpm":72,"ts":…,"src":"watch","live":true}` |
| `GET /api/v1/ws` | WebSocket: `hello`, `hr`, `live`, `widget` messages |
| `GET /api/v1/sessions` | recorded sessions |
| `GET /api/v1/sessions/{id}` | one session with all samples |
| `GET /api/v1/sessions/{id}/export?format=csv\|json` | export |
| `GET /overlay/{widgetId}` | overlay page for OBS |

```bash
curl "http://127.0.0.1:8746/api/v1/current?token=YOUR_TOKEN"
```

The protocol between iPhone and desktop is specified in [docs/plan.md](docs/plan.md).

## Development

Requirements: Rust (stable), Node 22+ with pnpm, and for the Apple apps Xcode 16+ and XcodeGen.

```bash
pnpm install
pnpm tauri dev      # desktop app with hot reload
cargo test --workspace
cd apple && ./scripts/build.sh
```

| Path | |
|---|---|
| `crates/uwupulse-proto` | link protocol, crypto, BLE parser |
| `crates/uwupulse-core` | hub, server, recording, Bonjour, BLE, overlay page |
| `apps/desktop` | Tauri 2 + React desktop app |
| `apple/` | iPhone and Watch apps, `PulseKit` Swift package |

## License

[MIT](LICENSE) © Lorin (MinifyX). Part of the UwUSuite.
