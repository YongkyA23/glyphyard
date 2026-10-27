# Server API contract

Server: `apps/server`, default `http://localhost:8080`. JSON everywhere, timestamps RFC 3339 UTC.
Errors: `{ "error": "<code>", "message": "<human text>" }` with a fitting status.
The admin UI is built into `apps/server/static` and served at `/` (SPA fallback to `index.html`).

## Desktop API — `/api/v1` (types in `crates/protocol`)

Every request carries `X-Install-Key: <install key>` (403 `bad_install_key` if wrong).
Everything except registration also carries `Authorization: Bearer <install token>` (401 if wrong).

| Method | Path | Body → Response |
|---|---|---|
| GET | `/health` | → 204 when the install key is right (used by "Test connection"; no Bearer needed) |
| POST | `/installs` | `RegisterRequest` → `RegisterResponse`. First name is recorded as `by: "first"`. Re-registering an existing `install_id` returns 409 `already_registered`. |
| GET | `/me` | → `{ name, next_name_change, policy }` (also updates `last_seen`) |
| PUT | `/me/name` | `NameChange` → 200 `NameStatus`, or 409 `{ error: "name_locked", next_name_change }` |
| POST | `/me/events` | `EventBatch` → 204. Stored in the timeline; `FontActivated/FontDeactivated` also update `font_states`. |
| PUT | `/me/fonts` | `FontSnapshot` → 204. Replaces this install's `font_states`. |
| GET | `/me/commands` | → `Command[]` not yet acked (used after reconnect) |
| POST | `/me/commands/{id}/ack` | `{ ok, error? }` → 204 |
| GET | `/me/ws` | WebSocket. Server sends `ServerMessage`, client sends `ClientMessage`. Token may also be passed as `?token=` and key as `?key=`. While connected the install counts as online. |
| GET | `/library/files/{hash}` | → font bytes, only if delivered to this install (or to all) and not blocked; else 404. |

## Admin API — `/api/admin` (session cookie `ts_session`, HttpOnly, SameSite=Lax)

Shapes (TypeScript notation):
```ts
type By = "user" | "admin" | "system";
type Activity = { id: number; install_id: string; install_name: string; kind: string; text: string; by: By; at: string };
type InstallSummary = {
  id: string; name: string; previous_names: string[]; os: string; app_version: string;
  active_fonts: number; last_seen: string; online: boolean; next_name_change: string | null;
  flags: ("new" | "possible_duplicate" | "override" | "failures" | "stale")[];
};
type FontState = { hash: string; ps_name: string; family: string; style: string; active: boolean;
  source: "user" | "admin" | "project_set" | "library"; overridden: boolean };
type InstallDetail = InstallSummary & {
  first_seen: string; merged_from: string[];
  name_history: { name: string; from: string; to: string | null }[];
  fonts: FontState[];
  overrides: { font: FontState; admin_at: string; user_at: string }[];
};
type TimelineEvent = { id: number; at: string; by: By; kind: "name" | "font" | "override" | "failure" | "organize" | "system"; text: string };
type NameChangeRow = { at: string; install_ids: string[]; old_name: string | null; new_name: string; by: "user" | "first" | "merge" | "admin_reset"; next_allowed: string | null };
type LibraryFile = { hash: string; filename: string; ps_name: string; family: string; style: string; version: string;
  license: "licensed" | "personal" | "client" | "unknown"; size: number;
  delivered_to: "all" | "none" | string[]; active_on: number; blocked: boolean; uploaded_at: string };
type OrganizeChange = { case: string; action: { op: string; [k: string]: unknown }; note: string }; // = core::organize::Change JSON
type Settings = { name_change_days: number; allow_connection_edit: boolean; deliver_library_to_new: boolean;
  activate_delivered: boolean; notify_admin_changes: boolean; timeline_retention_days: number | null;
  server_url: string; install_key: string };
```

| Method | Path | Body → Response |
|---|---|---|
| POST | `/login` | `{ email, password }` → 204 + cookie; 401 `bad_credentials` |
| POST | `/logout` | → 204 |
| GET | `/me` | → `{ email, role }` or 401 |
| GET | `/overview` | → `{ installs, online, os: {windows, macos}, name_changes_month, overrides_week, failures_week, recent: Activity[], attention: { overrides, failures, possible_duplicates, stale }, top_fonts: { family, users }[] }` |
| GET | `/installs?q=&filter=all\|online\|overrides\|duplicates\|stale` | → `InstallSummary[]` (q matches current name, old names, install id) |
| GET | `/installs/{id}` | → `InstallDetail` |
| GET | `/installs/{id}/timeline?kind=all\|names\|fonts\|overrides\|failures` | → `TimelineEvent[]` newest first |
| POST | `/installs/{id}/fonts` | `{ hash, active }` → `{ command_id }` (queues Activate/Deactivate, pushes over WS) |
| POST | `/installs/{id}/reset-name-limit` | → 204 |
| POST | `/installs/{id}/organize-request` | → `{ command_id }` |
| POST | `/installs/merge` | `{ primary, secondary, keep_name: "primary" \| "secondary" }` → `InstallDetail` of primary |
| POST | `/installs/{id}/split` | → 204 (undo a merge within 30 days; `id` = the retired install) |
| GET | `/name-changes?q=&since=` | → `NameChangeRow[]` newest first |
| GET | `/name-changes.csv` | → CSV of the same |
| GET | `/library` | → `LibraryFile[]` |
| POST | `/library` | multipart, field `files` (one or more font files) → `LibraryFile[]` added |
| PUT | `/library/{hash}` | `{ license }` → `LibraryFile` |
| POST | `/library/{hash}/block` | `{ blocked }` → `LibraryFile` (pushes Block/Unblock to every install) |
| POST | `/library/{hash}/deliver` | `{ target: "all" \| string[], activate }` → `LibraryFile` |
| GET | `/library/organize` | → `{ changes: OrganizeChange[] }` |
| POST | `/library/organize/apply` | → `{ applied }` |
| GET | `/organize/reports` | → `{ install_id, name, duplicates, version_conflicts, name_clashes, last_organized: string \| null }[]` |
| GET | `/settings` | → `Settings` |
| PUT | `/settings` | `Partial<Settings>` without `install_key` → `Settings` (pushes `PolicyChanged`) |
| POST | `/settings/install-key` | → `{ install_key }` (regenerates) |
| GET | `/admins` | → `{ email, role, last_login }[]` |
| POST | `/admins` | `{ email, password }` → 201 |

Online = WebSocket connected, or `last_seen` within the last 5 minutes. Stale = not seen for 14+ days.
Possible duplicate = another install had the same name (case-insensitive, ignoring spaces) or a name one is a prefix of the other ("Budi S." ~ "Budi Santoso").

CLI: `typeshelf-server serve [--port 8080] [--data ./data]`, `typeshelf-server create-admin <email>` (prompts for a password).
