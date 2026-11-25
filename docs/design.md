# Typeshelf — design

Status: agreed in conversation on 2026-10-27. UI reference: the "Typeshelf UI" design canvas (desktop app + web admin pages).
"Typeshelf" is a placeholder product name.

## 1. What it is

A FontBase-style font manager for designers, written in Rust, with one difference that drives most of the design:
there are **no user accounts**, but every install reports to a server run by an **admin**, who can see each
person's name history and active fonts and can turn fonts on or off on their computer.

### Goals
- Fast library of local font folders: browse, search, preview, activate/deactivate, collections, tags.
- Activation that **never silently breaks** (self-repair) and never needs admin rights.
- Folders that survive moves and drive letter changes; duplicates handled on purpose.
- Name chosen at first launch, changeable once every N days (default 30), enforced by the server.
- Admin web dashboard: name history, per-user timeline, remote font control, overrides, merge installs, shared library.
- Windows 10/11 and macOS (one universal binary for Intel + Apple Silicon).

### Non-goals (first version)
- User login, accounts, paid tiers, team seats, license seat counting.
- Discover feed, 3D view, sending font inventories to any third party.
- Editing a font's internal name tables.

### Anti-goals
- Leaving broken registry/font entries behind (the FontBase failure we saw: 1,208 stale entries).
- Treating two different font files as one because they share a PostScript name.
- Any telemetry beyond what the admin server needs.

## 2. Decisions

| Area | Decision |
|---|---|
| Desktop shell | Tauri v2. Rust backend, UI in React + TypeScript (Vite). |
| Server | Rust, Axum, SQLite (sqlx, migrations). One binary also serves the admin UI. |
| Admin UI | React + TypeScript (Vite), built into `apps/server/static`. |
| Font parsing | `skrifa` / `read-fonts` (fontations). |
| Identity of a font file | BLAKE3 hash of the file bytes. PostScript name is a grouping key only. |
| Folder identity | OS file ID (`file-id` crate) + volume, path is the last-known location. |
| Watching | `notify` with the full debouncer. One full rescan at startup and on demand. |
| Local store | SQLite (`rusqlite`, bundled) in the app data dir. |
| Windows activation | `AddFontResourceW` + `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts` value `"<Full name> (Typeshelf)"` + `WM_FONTCHANGE`. |
| macOS activation | `CTFontManagerRegisterFontsForURL` with user scope (persistent, no password). |
| Desktop ↔ server | HTTPS REST for requests + one WebSocket for pushed admin commands. |
| Install auth | Install key (built into installer, editable in Settings › Connection unless admin locks it) + per-install token issued at registration. |
| Admin auth | Email + password (Argon2id), server-side sessions in an HttpOnly cookie. |
| Name rule | `name_change_days` (default 30) checked **on the server**. Changing name needs a connection. |
| Renames by auto-organize | File name + display name only. Internal font names never change. |
| JS package manager | Bun (already installed). |

## 3. Desktop app

### Screens (see canvas, desktop page)
Onboarding (name only) · Library · Font detail (Preview / Glyphs / Files and versions / License) · Playground ·
Project sets · Auto-organize preview · Fonts in a PDF · Google Fonts · Settings · Quick activate popup.

### Local data model (SQLite)
- `folders(id, path, file_id, volume_id, status[watching|offline], added_at)`
- `files(id, hash, path, folder_id, size, mtime, format, missing)` — one row per file on disk.
- `faces(hash, ps_name, family, style, full_name, version, foundry, designer, weight, width, italic, is_variable, axes_json, features_json, glyph_count)` — keyed by hash, shared by identical copies.
- `families` derived view; `display_overrides(hash, display_name)` for auto-organize renames.
- `activations(hash, active, source[user|admin|set], activated_at, os_ref)` — the **ledger**.
- `collections`, `collection_items`, `tags`, `face_tags`, `project_sets`, `project_set_items`.
- `licenses(family_key, label[licensed|personal|client|unknown], file_path, note)`.
- `history(id, kind, payload_json, at, undone)` — journal for undo (auto-organize, restore).
- `settings(key, value)` — includes server URL, install key, install token, name, theme, toggles.
- `outbox(id, event_json)` — events waiting for the server when offline.

### Activation and self-repair
1. Activate = make sure one file per PostScript name is active (newest version wins unless the user picked one),
   register it with the OS, write the ledger row, then emit an event to the server.
2. Never activate two files with the same PostScript name. Warn when the same name is installed system-wide.
3. Before moving, renaming or deleting a file: unregister it first (Windows locks loaded fonts).
4. **Reconcile** at every startup and from Settings › Repair now:
   - Windows: every registry value ending in ` (Typeshelf)` that is not in the ledger, or whose file is gone → remove.
     Every active ledger row without a registry value → re-register. Files missing → mark, report to server.
   - macOS: compare `CTFontManagerCopyRegisteredFontDescriptors(user scope)` with the ledger the same way.
5. Errors are specific per font: file missing / file in use / not a valid font / blocked by admin.

### Folder watching
- Folders are identified by file ID + volume. On startup, a folder whose path is gone is looked up by file ID on the
  same volume (handles move/rename); if the volume is absent the folder is **Offline** (kept with all tags and
  collections) and resumes when the volume appears. "Locate…" lets the user point to the new path.
- OneDrive/Dropbox placeholders (Windows `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`) are listed but not read until needed.

### Auto-organize (user and admin)
Reads metadata from every face, groups them, and produces a **plan** (nothing changes until Apply; every run can be undone):

| Case | Detection | Action |
|---|---|---|
| Exact copy | same hash | keep the copy inside a watched "home" folder (else oldest path); others → Recycle Bin/Trash |
| Different version | same PostScript name, different version string | rename `Name-Style vX.YYY.ext`; newest is default; only one active |
| Other format | same PS name + version, OTF vs TTF | keep both, prefer OTF; WOFF/WOFF2 moved aside |
| Name clash | same PS name, different foundry/designer/glyph count | rename with `(Foundry)`; never active together |
| Variable + static | variable face and statics of same family | group; suggest one kind active |
| In system | PS name present in OS font directories | skip and warn |
| Junk | `__MACOSX`, `._*`, `Thumbs.db`, `.DS_Store`, zero-byte, unparsable | move to quarantine folder |
Optional tidy: move into `Foundry/Family/Family-Style vX.YYY.ext`; license files follow their family.

### Other features
- Project sets: activate/deactivate a set; deactivating keeps fonts still needed by another active set or admin rule.
- License labels with an attached file; warn before activating `unknown`.
- Fonts in a PDF: parse PDF font dictionaries (`lopdf`), match by PostScript name (ignoring subset prefixes `ABCDEF+`).
- Google Fonts: catalog from Google's public metadata, download into a managed folder, label Licensed (OFL).
- Quick activate: global shortcut (default Ctrl/Cmd+Alt+Space) opens a small window.
- Adobe auto-activation (Windows and macOS): UXP plug-ins for Photoshop and InDesign, a CEP extension for
  Illustrator. On document open they send the document's font list to the app's local bridge
  (`crates/adobe-bridge`, `http://127.0.0.1:27716`, loopback only), which turns on the fonts the library has.
  Each plug-in pairs once (the app asks the user to allow it) and then sends a token; requests must carry a
  custom header and the bridge sends no CORS headers, so web pages cannot call it.
- Backup/restore: a `.typeshelf` file (zip of a JSON export; no font files).
- App: start at login, start in tray, auto-update (Tauri updater), deactivate all on quit.

## 4. Server

### Data model (SQLite)
- `installs(id, token_hash, name, os, app_version, first_seen, last_seen, merged_into)`
- `name_changes(id, install_id, old_name, new_name, by[user|first|merge|admin_reset], at)`
- `name_limit_resets(install_id, at, admin_id)`
- `font_states(install_id, hash, ps_name, family, active, source, updated_at)`
- `events(id, install_id, kind, payload_json, at)` — the per-user timeline (name, fonts, overrides, failures, organize).
- `commands(id, install_id, kind[activate|deactivate|organize_request|deliver], payload_json, created_at, delivered_at, acked_at, ok, error, fanout)`
- `library_files(hash, filename, size, ps_name, family, license, uploaded_at, blocked)` + `deliveries(hash, target[all|install_id], activate)`
- `font_requests(id, install_id, font_name, note, at, done_at)` — "I need this font" requests; `done_at` NULL = open.
- `admins(id, email, password_hash, role)`, `sessions(token_hash, admin_id, expires_at)`
- `settings(key, value)` — name_change_days, install_key, allow_user_connection_edit, delivery defaults.

### API (all JSON, prefix `/api/v1`)
Desktop (header `X-Install-Key`, then `Authorization: Bearer <install token>`):
- `POST /installs` register `{install_id, name, os, app_version}` → `{token, next_name_change}`
- `PUT /me/name {name}` → 200 `{next_name_change}` or 409 `{next_name_change}`
- `POST /me/events` batch `{events:[…]}` (font on/off with source, failures, organize summaries, duplicates report, font requests)
- `PUT /me/fonts` full active-font snapshot (after reconcile)
- `GET /me/library` deliveries for this install; `GET /library/files/{hash}` download
- `GET /me/ws` WebSocket: server pushes `commands`; client acks.
Admin (cookie session, under `/api/admin`): login/logout, overview, installs (list/search/detail/timeline),
name history (+CSV), remote activate/deactivate, override restore, reset name limit, merge/split installs,
library upload/block/deliver, organize plan/apply for library, organize requests, font requests (list, mark done, reopen), settings, admins.

### Rules
- Name change allowed when `now >= last_change + name_change_days` (first name doesn't count as a change) or an admin reset is newer than the last change.
- An override = user deactivates a font whose latest activation source is `admin`. Logged as its own event.
- A blocked library font is deactivated on every install and cannot be activated (desktop refuses; server rejects).
- Merge keeps the older install as primary; name history, events and limits are combined; the newer one gets `merged_into`. Split allowed within 30 days.

## 5. Repository layout
```
Cargo.toml                 workspace
crates/core                parsing, hashing, grouping, auto-organize planner, name rule (pure + tests)
crates/activation          OS activation (Windows / macOS) + reconcile
crates/protocol            serde types shared by desktop and server
apps/desktop/src-tauri     Tauri app (store, watcher, sync client, commands)
apps/desktop/ui            React UI
apps/server                Axum server (+ static admin UI)
apps/admin-ui              React admin UI
web/shared                 design tokens shared by both UIs
docs/                      this file, plan.md
```

## 6. Calls made by the builder (not discussed)
- Server stack (Axum + SQLite) and React for both UIs.
- Name changes need a connection (no offline queue) so the server rule stays authoritative.
- Admin passwords: first admin is created by `typeshelf-server create-admin` on the command line.
