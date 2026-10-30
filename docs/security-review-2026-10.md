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
| Info | `src-tauri/src/lib.rs` (`export_session`) | Schreibt an jeden Pfad aus der Webview; nur per XSS im eigenen Frontend nutzbar, CSP streng. | Speicherdialog in Rust (siehe 1.0.0). | behoben |
| – | `axum::serve` | Kein Header-Read-Timeout. | Eigene Accept-Schleife (siehe 1.0.0). | behoben |

## Ohne Befund

Frame-Krypto (Nonce je Richtung, Zähler nur nach Erfolg), Low-Order-Prüfung, Base64 auf 32 Byte, QR- und HRM-Parser, Token-Vergleich in konstanter Zeit, SQL parametrisiert, `Content-Disposition` gefiltert, Schlüsseldateien 0600, Overlay nur `textContent`, Custom CSS als Text, Farben per Whitelist, Keychain `AfterFirstUnlockThisDeviceOnly`, keine echten Domains oder IPs im Repo.

## 1.0.0 (Nachtrag)

Umfang: alles seit v0.1.0, vor allem Setup (Installer, Updater, Deinstallation), Updater der App, `scripts/release.mjs`, CI-Workflows und Capabilities.

| Schwere | Ort | Ursache | Fix | Stand |
|---|---|---|---|---|
| Medium | `uwupulse-core/src/server/mod.rs` | `axum::serve` ohne hyper-Timer: kein Header-Read-Timeout, keine Verbindungsgrenze. Leere Verbindungen aus dem LAN erschöpfen die FDs (`lanEnabled` ist Standard). | Eigene Accept-Schleife mit hyper (HTTP/1, Upgrades): Header-Timeout 10 s (auch Leerlauf zwischen Keep-Alive-Requests), höchstens 128 Verbindungen, 32 pro IP; der Platz hängt am Socket und gilt damit auch für WebSockets. Graceful Shutdown wie zuvor. Test `silent_connection_is_closed`. | behoben |
| Low | `scripts/release.mjs` (`ciRun`) | Nahm jeden grünen Installers-Lauf zum Commit, egal welcher Branch oder Auslöser. | Nur `push`/`workflow_dispatch` auf `main` (`ci/**` nur bei `--dry-run`), `head_repository` muss `MinifyX/UwUPulse` sein. | behoben |
| Low | `capabilities/default.json` | `opener:default` erlaubte der Webview, beliebige URLs und Pfade zu öffnen; genutzt wird nur `open_project_page` aus Rust. | Berechtigung und `@tauri-apps/plugin-opener` (JS) entfernt. | behoben |
| Low | `apps/setup/src-tauri/src/app.rs` | WebView-Daten unter Linux im festen `/tmp/UwUPulse-Setup-WebView`: vorab anlegbar oder als Symlink von anderen Nutzern. | Neuer Ordner mit Zufallsnamen und 0700 (`$XDG_RUNTIME_DIR`, sonst Temp), beim Beenden gelöscht. | behoben |
| Low | `apps/setup/src-tauri/src/install.rs` | Eigener Installationsordner außerhalb des Profils (z. B. `D:\Apps`) erbt ACLs, die anderen Nutzern Schreiben erlauben → `UwUPulse.exe` austauschbar. | Geschützte DACL (Nutzer, SYSTEM, Administratoren; nichts geerbt), nur für selbst angelegte Ordner oder eine frühere Installation. | behoben |
| Info | `src-tauri/src/lib.rs` (`export_session`) | Schrieb an jeden Pfad aus der Webview. | Speicherdialog in Rust, Befehl nimmt nur `id` und Format; `dialog:allow-save` und `@tauri-apps/plugin-dialog` (JS) entfernt. | behoben |
| Info | `src-tauri/src/updates.rs` (`open_locked`, macOS/Linux) | TOCTOU zwischen Prüfen und Öffnen der heruntergeladenen Datei; nur durch denselben Nutzer ausnutzbar. | – | akzeptiert |
| Info | Setup (WebView2) | Der Bootstrapper von Microsoft wird nicht auf Signatur geprüft. | – (HTTPS von Microsoft) | akzeptiert |
| Info | Update-Feed, Release-Notes | Nicht signiert; nur die Pakete sind es. | – (so gewollt; Branch-Schutz für den Feed-Branch empfohlen) | akzeptiert |

### Ohne Befund

Signaturprüfung des Updaters (auch noch einmal auf den Bytes vor dem Start), Bindung der Signatur an Version und Dateinamen, Entpacken der Setup-Nutzlast, Pfade der Deinstallation, CI-Actions per SHA gepinnt, Signaturschlüssel nie im Repo, Demo-Modus nur in `DEBUG`.
