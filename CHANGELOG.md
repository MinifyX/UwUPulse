# Changelog

## Unreleased

- OBS plugin in the setup: choose Desktop app and/or OBS plugin (preselected when OBS Studio is found). Updates keep the plugin current, uninstalling removes it. OBS is never closed; the setup asks for a restart instead.
- Releases carry the OBS plugin on its own: .zip for Windows x64/ARM64 and macOS, .tar.gz and a `uwupulse-obs` .deb for Linux x64.
- The dock inside OBS points to the setup for updates.

## 1.0.0

First release.

- Apple Watch heart rate via a workout session mirrored to the iPhone, relayed to the desktop over an end-to-end encrypted local link.
- Pairing by QR code or six-digit code comparison.
- BLE chest straps on the iPhone or directly on the desktop.
- OBS overlays with five presets, a live editor, colour zones and custom CSS.
- Session recording (SQLite) with graphs and CSV/JSON export.
- Local REST and WebSocket API with token.
- UwUSuite design on desktop, iPhone and Apple Watch: UwU Sans, suite colours, Nyu, new app icon.
- Desktop appearance settings (font, contrast, motion, text size), macOS menu and close-to-hide.
- UwUPulse Setup for Windows, macOS and Linux with signed in-app updates; .deb, .rpm, portable and AUR packages.
