# UwUPulse

Herzfrequenz von Apple Watch / BLE-Gurten komplett lokal an den Desktop (OBS-Overlay, Aufzeichnung, lokale API). Tauri 2 + React/TS, Rust-Core, iOS/watchOS in Swift. MIT. Version `1.0.0`.

## Layout
- `crates/uwupulse-proto`: Link-Protokoll (Handshake, Krypto, Frames), BLE-HR-Parser, keine I/O
- `crates/uwupulse-core`: `Pulse`-Hub, axum-Server (`/link`, `/overlay`, `/api/v1`), SQLite, Bonjour, BLE (Feature `ble`); Overlay-Seite in `overlay/` (handgeschrieben, `include_str!`)
- `apps/desktop` (`@uwupulse/desktop`): Frontend `src/`, Tests `test/*.test.ts`, Tauri-Seite `src-tauri/`
- `apps/setup` (`@uwupulse/setup`, Crate `uwupulse-setup`): eigener Installer/Updater mit Nyu, packt die App per `build.rs` ein (`UWUPULSE_SETUP_PAYLOAD`)
- `apple/`: XcodeGen (`project.yml`), iOS-App, Watch-App, Swift-Paket `PulseKit`
- Spezifikation und Stand: `docs/plan.md`; Testvektoren `docs/protocol-vectors.json` (aus Rust, `UPDATE_VECTORS=1`), Swift prüft sie

## Befehle
- cargo liegt nicht im PATH: `export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`
- Gezielt: `cargo test -p uwupulse-proto|uwupulse-core [filter]`, `pnpm --filter @uwupulse/desktop test|typecheck`
- Wie CI: `pnpm lint && pnpm typecheck && pnpm test && pnpm -r build`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`
- Apple: `cd apple/PulseKit && swift test`, `apple/scripts/build.sh` (Simulator, unsigniert)

## Fallen
- Protokolländerung immer in Rust **und** Swift, Vektoren neu erzeugen, `docs/plan.md` anpassen
- `@uwusuite/design` als Release-Tarball-URL: UI-Änderungen zuerst in UwUSuite-Design
- `apps/desktop/src-tauri/gen/schemas/` ist generiert
- Öffentliches Repo: keine Schlüssel, keine echten Hosts/IPs in Tests (nur 192.0.2.0/24, example.com)

## Release
- Version anheben: `Cargo.toml` (Workspace), `apps/desktop/src-tauri/tauri.conf.json`, `apps/setup/src-tauri/tauri.conf.json`, alle `package.json`, `apple/Config/UwUPulse.xcconfig` (`MARKETING_VERSION`), neuer Abschnitt `## <version>` in `CHANGELOG.md`
- `installers.yml` baut bei jedem Push auf main unsigniert (Setup `apps/setup` mit eingepackter App): Windows x64/ARM64 `UwUPulse-windows-<arch>-setup.exe`, macOS `UwUPulse-macos-universal.dmg` (+ `UwUPulse-update-macos-universal`), Linux x64/arm64 `.deb`/`.rpm`/`-portable.tar.gz` (+ `UwUPulse-update-linux-x64.AppImage`). Lokal: `pnpm build:setup` → `target/installers/`
- Release-Commit taggen (`v<version>`), Tag pushen, Installers-Lauf grün abwarten, dann `pnpm release` (lädt die CI-Artefakte, signiert mit dem Update-Schlüssel aus `/Volumes/backups/DevKeys/UwUPulse-Update-Schluessel` bzw. `UWUPULSE_UPDATE_KEY_DIR`, legt das GitHub-Release an, aktualisiert `stable.json`/`beta.json` auf Branch `updates`, schreibt AUR `uwupulse-bin`; `aur.yml` pusht es, wenn `AUR_SSH_PRIVATE_KEY` gesetzt ist). `--dry-run` probt ohne Tag
- Setup-Version muss gleich der App-Version sein (`apps/setup/src-tauri/tauri.conf.json`, `apps/setup/package.json`)
- iOS/watchOS: `apple/scripts/testflight.sh` (TestFlight/App Store)

## Security
- Kritisch: `uwupulse-proto` (Handshake, Frames, BLE-Parser), `/link` und Token-Prüfung in `uwupulse-core`, Pairing in der iOS-App
- Vor jedem Feature-Release `security-reviewer` über den Bereich seit dem letzten Tag; Ergebnis nach `docs/security-review-<YYYY-MM>.md`
