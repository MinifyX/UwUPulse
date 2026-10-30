# UwUPulse for OBS Studio

The UwUPulse OBS plugin runs the whole app inside OBS Studio (31 or newer), with no separate desktop app needed. It includes:

- **UwUPulse dock** (View → Docks): the app's full UI. You can pair an iPhone (QR code or code comparison), edit widgets, connect chest straps, record, and use the local API.
- **UwUPulse overlay source**: choose a widget and a size, and the source shows its overlay.

The plugin acts as a separate desktop next to the UwUPulse app. It has its own key, paired phones and widgets, and lives in OBS's plugin config folder. It listens on port **8747** (the app uses 8746) and announces itself over Bonjour as "UwUPulse OBS (\<computer\>)". The app and the plugin can run at the same time. Each one is paired with the iPhone separately.

## How it fits together

```
crates/uwupulse-core   Pulse hub, server (/link, /overlay, /api/v1, /ui), SQLite, BLE
crates/uwupulse-obs    Rust staticlib with a C ABI (include/uwupulse_obs.h) around it
plugins/obs/src        C++: module, dock (obs-browser QCef), source uwupulse_overlay
apps/desktop/dist      the React UI, served by the core at /ui and shown in the dock
```

- The dock loads `http://127.0.0.1:8747/ui/#s=<secret>`. The secret is new on every start.
- Overlay sources wrap a private `browser_source` that points at the widget's overlay URL.
- Callbacks from the core go back to OBS through the C ABI:
  - log goes to the OBS log;
  - opening a link uses `QDesktopServices`;
  - saving a CSV uses `QFileDialog`;
  - "changed" refreshes the dock and the sources.

Notes:

- **No Bluetooth on macOS.** OBS.app has no `NSBluetoothAlwaysUsageDescription`, so macOS would terminate OBS on the first CoreBluetooth call. On macOS, chest straps work through the iPhone.
- **No obs-browser** (e.g. OBS on Wayland without browser panels): the dock shows a button that opens the UI in the system browser instead.

## Build

```sh
pnpm build:obs                       # this computer's platform
node plugins/obs/scripts/build.mjs --platform macos|windows-x64|windows-arm64|linux \
     [--deps <dir>] [--out <dir>] [--skip-ui]
```

The build needs CMake 3.28+, Rust, pnpm and a C++ compiler (Xcode / Visual Studio 2022 / GCC). It runs these steps:

1. **SDK** (`cmake/deps.cmake`) for Windows and macOS:
   - downloads the OBS sources (pinned in `buildspec.json`), obs-deps and Qt 6, checking each sha256;
   - builds libobs and obs-frontend-api only;
   - keeps the result in `--deps` (default `~/.cache/uwupulse-obs/deps`, or `UWUPULSE_OBS_DEPS`).

   On Linux the SDK comes from the system packages (see below). Only obs-browser's `browser-panel.hpp` is downloaded on every platform.
2. **UI**: `pnpm --filter @uwupulse/desktop build`.
3. **Core**: `crates/uwupulse-obs` as a static library. On macOS it is built for arm64 and x86_64 and joined with `lipo`.
4. **Plugin**: CMake. It exports only OBS's `obs_module_*` symbols, so nothing from the Rust core is visible to other plugins. The result is installed into `--out` (default `target/obs/<platform>`).

Linux (Ubuntu 24.04, as in CI):

```sh
sudo add-apt-repository ppa:obsproject/obs-studio
sudo apt install obs-studio libsimde-dev qt6-base-dev qt6-base-private-dev libdbus-1-dev pkg-config
```

CI (`.github/workflows/obs-plugin.yml`) builds every platform on each pull request, and on main as part of the Installers run (`installers.yml` calls it, and the setups take the plugin from there instead of building it again). Each run uploads these unsigned artifacts:

| Artifact | Contents |
|---|---|
| `uwupulse-obs-windows-x64` | `uwupulse/` |
| `uwupulse-obs-windows-arm64` | `uwupulse/` |
| `uwupulse-obs-macos-universal` | `uwupulse.plugin` |
| `uwupulse-obs-linux-x64` | `uwupulse/` |

Each artifact is a `.tar.gz` of the folder listed.

## Installation

The easy way is the **UwUPulse setup** (`UwUPulse-windows-<arch>-setup.exe`, `UwUPulse-macos-universal.dmg`, and on Linux x64 the setup the app updates with). It offers two components, **Desktop app** and **OBS plugin**. The plugin is preselected when OBS Studio is found:

- macOS: `/Applications/OBS.app` or the plugins folder;
- Windows: the OBS registry entry, `C:\Program Files\obs-studio` or `%ProgramData%\obs-studio`;
- Linux: `obs` in the PATH or `/usr/share/obs`.

Without OBS it still installs the plugin, which loads the next time OBS starts. OBS Studio 31 or newer is required. The setup never closes OBS: if OBS is open, restart it afterwards. The setup notes the plugin in its install record (`install.json`, or the registry on Windows). An update of UwUPulse updates the plugin too, if it was installed, and uninstalling removes it (its data only when you don't keep it).

Releases also carry the plugin on its own, unsigned:

| File | What to do |
|---|---|
| `UwUPulse-obs-windows-x64.zip`, `UwUPulse-obs-windows-arm64.zip` | copy `uwupulse\` to `%ProgramData%\obs-studio\plugins\` |
| `UwUPulse-obs-macos-universal.zip` | copy `uwupulse.plugin` to `~/Library/Application Support/obs-studio/plugins/` |
| `UwUPulse-obs-linux-x64.tar.gz` | unpack into `~/.config/obs-studio/plugins/` |
| `UwUPulse-obs-linux-x64.deb` | `sudo apt install ./UwUPulse-obs-linux-x64.deb` (package `uwupulse-obs`, system paths, needs `obs-studio` 31+ from the OBS PPA) |

Install it only one way: the per-user copy and the `.deb` together would give OBS the module twice.

There is no plugin for Linux on ARM, so the setup there offers only the app.

## Layout

What `cmake --install` produces:

```
macOS          uwupulse.plugin/Contents/MacOS/uwupulse
               uwupulse.plugin/Contents/Resources/{locale,ui}
Windows/Linux  uwupulse/bin/64bit/uwupulse.{dll,so}
               uwupulse/data/{locale,ui}
```

Where OBS loads it from:

| OS | For | Path |
|---|---|---|
| macOS | user | `~/Library/Application Support/obs-studio/plugins/uwupulse.plugin` |
| Windows | all users (recommended) | `%ProgramData%\obs-studio\plugins\uwupulse\` (copy the folder as is) |
| Windows | inside the OBS install | `C:\Program Files\obs-studio\obs-plugins\64bit\uwupulse.dll` and `C:\Program Files\obs-studio\data\obs-plugins\uwupulse\{locale,ui}` |
| Linux | user | `~/.config/obs-studio/plugins/uwupulse/` (copy the folder as is) |
| Linux | system (Debian/Ubuntu) | `/usr/lib/x86_64-linux-gnu/obs-plugins/uwupulse.so` and `/usr/share/obs/obs-plugins/uwupulse/{locale,ui}` |

The plugin keeps its data (key, paired phones, widgets, recordings) in OBS's plugin config folder:

- macOS: `~/Library/Application Support/obs-studio/plugin_config/uwupulse/`
- Windows: `%APPDATA%\obs-studio\plugin_config\uwupulse\`
- Linux: `~/.config/obs-studio/plugin_config/uwupulse/`

Uninstalling the plugin leaves this folder in place.

## Licence

The plugin's source code is MIT, like the rest of UwUPulse. The built plugin links libobs, which is GPL-2.0-or-later, so a distributed plugin binary falls under the GPL. The complete source for it is this repository.

## Open points

- **Linux and OBS versions.** Linux builds against the OBS version from the PPA, which is always the newest one. libobs refuses to register a source whose `obs_source_info` is larger than the one it knows. So a plugin built against a newer OBS may fail to register its overlay source on an older OBS (the dock still works). Distribution packages should be built against the OBS version they ship.
- **Flatpak OBS** loads plugins only as Flatpak extensions. That packaging does not exist yet.
- **Signing.** The macOS bundle and the Windows DLL are unsigned.
- **Windows.** OBS 31 loads plugins only from `%ProgramData%` (no per-user folder), so the setup writes there; without the right to, it says so. While OBS is open, Windows may keep the old DLL locked: the setup then asks to close OBS and try again.
- **No `.rpm` of the plugin** yet; on Fedora/openSUSE use the `.tar.gz` (or the setup).
