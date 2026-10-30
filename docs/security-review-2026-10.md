# Security-Review 2026-10 (vor v0.1.0)

Umfang: `uwupulse-proto`, Link-Server, API und Token in `uwupulse-core`, Overlay-Seite, Tauri-Seite, `apple/PulseKit` und die Dienste der iPhone-App. Durchgeführt vom `security-reviewer`-Agent, nur lesend. Nicht geprüft: Watch-App, HealthKit-Handling, Abhängigkeiten.

## Befunde

| Schwere | Ort | Ursache | Fix | Stand |
|---|---|---|---|---|
| High | `uwupulse-proto/src/handshake.rs`, `server/link.rs` | SAS ohne Commitment: Ein MITM im LAN kann sein `m2` variieren, bis beide Codes gleich sind (~10^6 Hash-Versuche). | `sas`: `m1` trägt `commit = H(epk)`, `epk` folgt erst in `m3` nach `m2`; `m3` im Transkript. | behoben |
| High | `DesktopConnection.swift`, `UwUPulseApp.swift`, `PairingURI.swift` | iPhone koppelt einseitig: speichert den Desktop auf `paired` ohne Bestätigung; Deep Link verbindet sofort, `h=` erlaubt öffentliche Hosts. | `auth` erst nach Bestätigung am iPhone; Deep Link nur vorausfüllen und bestätigen; `h=` auf private/Link-Local/`.local` beschränkt. | behoben |
| Medium | `DesktopConnection.swift`, `LinkManager.swift` | Desktop-`id` aus `paired` nicht an das Angebot gebunden; Speichern ersetzt vorhandene Einträge. | `qr`: `id` muss zum QR passen; Konflikt → Nachfrage. | behoben |
| Medium | `DesktopConnection.swift`, `LinkManager.swift` | Unauthentisierte Ablehnung im Modus `known` galt als endgültig. | Nächste Adresse probieren; Meldung erst nach wiederholter Ablehnung überall. | behoben |
| Medium | `server/link.rs`, `hub.rs` | `/link` aus dem Browser erreichbar (kein `Origin`-Check); wenige IPs belegen alle Slots vor dem Handshake. | `Origin` → 403; höchstens 4 Verbindungen vor `ready`, 2 pro IP. | behoben |
| Low | `hub.rs` (`add_paired`) | Pairing mit vorhandener Telefon-`id` ersetzt deren Schlüssel. | Andere `spk` → `rejected`. | behoben |
| Low | `ble.rs` | Scan-Liste wächst unbegrenzt. | Obergrenze 64, älteste nicht verbundene fallen raus. | behoben |
| Info | `.gitignore` | `apple/PulseKit/.build/` nicht ignoriert. | ergänzt | behoben |
| Info | `apple/Config/UwUPulse.xcconfig` | Team-ID im Repo (nicht geheim). | – | akzeptiert |
| Info | `src-tauri/src/lib.rs` (`export_session`) | Schreibt an jeden Pfad aus der Webview; nur per XSS im eigenen Frontend nutzbar, CSP streng. | Später: Pfad aus dem Dialog in Rust merken. | offen |
| – | `axum::serve` | Kein Header-Read-Timeout. | Später über hyper-util. | offen |

## Ohne Befund

Frame-Krypto (Nonce je Richtung, Zähler nur nach Erfolg), Low-Order-Prüfung, Base64 auf 32 Byte, QR- und HRM-Parser, Token-Vergleich in konstanter Zeit, SQL parametrisiert, `Content-Disposition` gefiltert, Schlüsseldateien 0600, Overlay nur `textContent`, Custom CSS als Text, Farben per Whitelist, Keychain `AfterFirstUnlockThisDeviceOnly`, keine echten Domains oder IPs im Repo.
