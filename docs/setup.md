# Typeshelf Setup (custom Windows installer)

One program, `typeshelf-setup.exe` (Tauri v2, frameless 760×480 window, plain HTML/CSS/JS UI in `apps/setup/ui`).
Design: `docs/ui-canvas/Setup*.dc.html`, `docs/ui-canvas/Uninstall*.dc.html`.

- Embeds a zip payload (`typeshelf.exe` + `plugins/`) built by `scripts/build-windows.ps1`.
- Installs per-user (no admin): default `%LOCALAPPDATA%\Programs\Typeshelf`.
- Copies itself into the install dir as `uninstall.exe`; the uninstall registry entry runs `uninstall.exe --uninstall`.
- Silent use for admins: `typeshelf-setup.exe --silent [--dir <path>] [--no-start-menu] [--desktop] [--no-start-at-login]`,
  `uninstall.exe --uninstall --silent [--wipe-data]`. Exit code 0 on success. Silent mode closes a running
  Typeshelf first (asks it to close, forces it after 5 s) instead of stopping with `app_running`.

## UI ↔ backend contract (`window.__TAURI__.core.invoke`, `window.__TAURI__.event.listen`)

| Command | Args → Result |
|---|---|
| `info` | → `{ mode: "install" \| "update" \| "uninstall", version, installedVersion: string \| null, defaultDir, serverUrl, sizeMb, fontsActive: number \| null }` |
| `pick_dir` | `{ current }` → `string \| null` |
| `install` | `{ dir, startMenu, desktop, startAtLogin }` → `void`; rejects `{ code: "app_running" \| "io" \| "invalid_dir", message }` |
| `close_app` | → `void` once Typeshelf has exited (asks nicely, forces after 5 s) |
| `launch_app` | → `void` (starts Typeshelf, then Setup exits) |
| `uninstall` | `{ wipeData }` → `{ fontsTurnedOff, leftoverEntries, fontsKeptIn: string \| null }` (wiping data keeps the font folders that hold files); rejects like `install` |
| `quit` | → `void` |

Event `progress` → `{ pct: number (0–100), step: "copy" \| "webview2" \| "plugins" \| "shortcuts" \| "fonts" \| "files" \| "registry" }`
during `install` and `uninstall`.

Screens by mode: install → Welcome (S1) → Options (S2) → Installing (S3) → Ready (S4); `app_running` → S5 →
`close_app` → retry. update → Update (S6) → S3 → S4. uninstall → U1 → progress → U2.
