# UwUPulse brand

App icon and symbols in the UwUSuite style (`@uwusuite/design`, `docs/app-icons.md`):
Nyu as a heart with a pulse line, on the pink suite tile.

| File | Use |
|---|---|
| `uwupulse-app-icon.svg` | App icon (512 grid, tile with rx 116). Also `assets/icon.svg` and the README. |
| `uwupulse-taskbar-icon.svg` | Windows taskbar / Linux, without tile, large sizes |
| `uwupulse-taskbar-icon-small.svg` | Same for 16–32 px: thicker outline, no face |
| `uwupulse-symbol.svg` | Nyu without tile, for in-app use |
| `uwupulse-symbol-mono.svg` | Monochrome outline (`currentColor`) |

## Generating

```sh
pnpm --filter @uwupulse/desktop icons   # Tauri icons -> apps/desktop/src-tauri/icons/
node apple/scripts/app-icons.mjs        # iOS + watchOS AppIcon.png (1024, no alpha)
node apple/scripts/nyu-art.mjs          # Nyu-happy/-sleepy/-sad imagesets for the iOS app
```

`assets/icon.svg` is a copy of `uwupulse-app-icon.svg`; copy it again after changes.
