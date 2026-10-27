# Build plan

Each phase ends with `cargo test` (and `bun run build` for UI phases) passing and a commit.

## Phase 1 — Foundation
- [ ] Workspace, `.gitignore`, README, rust-toolchain.
- [ ] `crates/core`: font parsing (faces, TTC, variable axes, features), BLAKE3 hashing.
- [ ] `crates/core`: grouping + auto-organize planner (all cases in design §3), file naming.
- [ ] `crates/core`: name-change rule.
- [ ] `crates/protocol`: shared API types.

## Phase 2 — Desktop backend
- [ ] `crates/activation`: Windows (AddFontResourceW + HKCU + WM_FONTCHANGE), macOS (CoreText user scope), ledger reconcile.
- [ ] Local SQLite store + migrations.
- [ ] Folder identity, scanning, watcher, offline folders.
- [ ] Auto-organize apply + undo journal; Recycle Bin via `trash` crate.
- [ ] Sync client (REST + WebSocket, outbox).

## Phase 3 — Server
- [ ] Axum app, SQLite migrations, settings.
- [ ] Desktop API: register, name change rule, events, snapshot, WebSocket commands.
- [ ] Admin auth, installs, timeline, name history + CSV, remote control, overrides, merge/split.
- [ ] Shared library: upload, block, deliver, download; organize plan for library.
- [ ] Integration tests with an in-memory database.

## Phase 4 — Desktop UI (Tauri)
- [ ] Tauri v2 shell, tray, global shortcut, single instance.
- [ ] Screens: onboarding, library, font detail, playground, project sets, auto-organize, PDF fonts, Google Fonts, settings, quick activate.

## Phase 5 — Admin UI
- [ ] Screens: sign-in, overview, users, user detail, name history, shared library, auto-organize, merge, settings.

## Phase 6 — Packaging
- [ ] Windows NSIS installer with server URL + install key baked in.
- [ ] macOS universal `.app`/`.dmg` (built on a Mac or CI; ad-hoc signed without an Apple account).
- [ ] Server: single binary + Dockerfile.
