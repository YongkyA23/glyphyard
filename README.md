# Typeshelf

A font manager for Windows and macOS, with a web dashboard for the admin who runs the server.
Design: [docs/design.md](docs/design.md) · API: [docs/api.md](docs/api.md) · Desktop commands: [docs/desktop-commands.md](docs/desktop-commands.md)

| Path | What |
|---|---|
| `crates/core` | Font parsing, file identity, auto-organize planner, name rule |
| `crates/protocol` | Desktop ↔ server messages |
| `crates/activation` | OS font activation (Windows registry/GDI, macOS CoreText) + repair |
| `crates/engine` | Desktop backend: library DB, scanning, watching, organize, PDF, Google Fonts, backup |
| `crates/sync` | Desktop ↔ server client (REST, WebSocket, offline outbox) |
| `crates/adobe-bridge` | Loopback bridge for the Adobe plug-ins + plug-in install/remove |
| `apps/desktop` | Tauri app (`src-tauri`) and its React UI (`ui`) |
| `apps/server` | Admin server (Axum + SQLite), serves the admin UI |
| `apps/admin-ui` | React admin dashboard |
| `plugins` | Photoshop/InDesign (UXP) and Illustrator (CEP) plug-ins |

## Requirements
- Rust (stable) via rustup; on Windows, Visual Studio Build Tools with "Desktop development with C++"
- Bun

## Run
```
# 1. Server (first time: create an admin)
cargo run -p typeshelf-server -- create-admin you@example.com --data ./data
cargo run -p typeshelf-server -- serve --port 8080 --data ./data

# 2. Admin UI (dev, proxies /api to :8080) — or `bun run build` to serve it from the server
cd apps/admin-ui && bun install && bun run dev

# 3. Plug-in packages (bundled into the desktop app)
bun plugins/build.ts

# 4. Desktop app (dev). Set the server URL and install key the installer should carry:
cd apps/desktop/ui && bun install && cd ..\src-tauri
$env:TYPESHELF_SERVER_URL="http://localhost:8080"; $env:TYPESHELF_INSTALL_KEY="<key from admin Settings>"
bunx @tauri-apps/cli dev
```
Both UIs also run in a plain browser with sample data: desktop `bun run dev` in `apps/desktop/ui`, admin with `?mock=1`.

## Windows installer
```
cargo run -p typeshelf-server -- install-key --data ./data   # the key users' apps will carry
./scripts/build-windows.ps1 -ServerUrl https://fonts.example.com -InstallKey <key>
```
Produces `dist/Typeshelf_<version>_x64-setup.exe` (NSIS, per-user install, no admin prompt) with the server URL
and install key compiled in and the Adobe plug-in packages bundled. Uninstalling first removes every font
registration Typeshelf made. The installer is unsigned, so Windows SmartScreen shows "More info → Run anyway".
Regenerating the install key on the server means rebuilding the installer.

## Test
```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Not done yet
- macOS universal `.dmg` (must be built on a Mac; unsigned without an Apple Developer account).
- Code signing for the Windows installer.
- Auto-updater.
- Plug-ins and macOS code are compile-checked only; they need a run inside the real Adobe apps and on a Mac.
