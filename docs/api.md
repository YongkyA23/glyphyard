# Server API contract

Server: `apps/server`, default `http://localhost:8080`. JSON everywhere, timestamps RFC 3339 UTC.
Errors: `{ "error": "<code>", "message": "<human text>" }` with a fitting status.
The admin UI is built into `apps/server/static` and served at `/` (SPA fallback to `index.html`).

## Desktop API — `/api/v1` (types in `crates/protocol`)

Every request carries `X-Install-Key: <install key>` (403 `bad_install_key` if wrong).
Everything except registration also carries `Authorization: Bearer <install token>` (401 `bad_token` if wrong,
401 `revoked` once an admin removed the computer).

| Method | Path | Body → Response |
|---|---|---|
| GET | `/health` | → 204 when the install key is right (used by "Test connection"; no Bearer needed) |
| POST | `/installs` | `RegisterRequest` → `RegisterResponse`. First name is recorded as `by: "first"`. Re-registering an existing `install_id` that has never made an authenticated request (its first response was lost) returns 200 with a new token and the old token stops working; once the install has used its token it returns 409 `already_registered`. An `install_id` an admin removed registers as a new install (the old one's history moves to `<id>~revoked-<hex>`). |
| GET | `/me` | → `{ name, next_name_change, policy }` (also updates `last_seen`) |
| PUT | `/me/name` | `NameChange` → 200 `NameStatus`, or 409 `{ error: "name_locked", next_name_change }` |
| POST | `/me/events` | `EventBatch` → 204. Stored in the timeline; `FontActivated/FontDeactivated` also update `font_states`. `FontRequested { name, note? }` (kind `font_requested`) also creates a font request for the person (the install plus everything merged into it): name cut to 200 characters and note to 1000, an empty name ignored, no new row while the same name (ASCII case-insensitive) is open for that person or the person has 50 open requests. A server older than this kind answers 422 for the batch; the desktop then resends the events one at a time and drops only the rejected ones. |
| PUT | `/me/fonts` | `FontSnapshot` → 204. Replaces this install's `font_states`. |
| GET | `/me/commands` | → `Command[]` not yet acked (used after reconnect) |
| POST | `/me/commands/{id}/ack` | `{ ok, error? }` → 204. The first ack of a command stands; a repeat is accepted and ignored. A failed ack becomes a `failure` timeline event, except that a command queued to a merged group (one copy per computer) that fails with `file_missing: … is not in this library` is held back while another copy succeeded, or was delivered to its computer and is not acked yet; copies never delivered (computer offline, gone or split away) do not hold it back. So a computer that gets the command and succeeds after the failure was recorded leaves that failure on the timeline. The WebSocket `Ack` follows the same rules. |
| GET | `/me/ws` | WebSocket. Server sends `ServerMessage`, client sends `ClientMessage`. Token may also be passed as `?token=` and key as `?key=`. While connected the install counts as online. |
| GET | `/library/files/{hash}` | → font bytes, only if delivered to this install, to the install it was merged into, or to all, and not blocked; else 404. |
| GET | `/update` | → `{ manifest, signature }`: the current app release's manifest and Ed25519 signature, as text exactly as uploaded; 404 `no_release` when there is none. The app verifies the signature with the public key built into it before trusting anything in the manifest. |
| GET | `/update/file` | → the release's setup program; 404 `no_release` when there is none. The app checks it against the manifest's size and BLAKE3 hash. |

## Admin API — `/api/admin` (session cookie `ts_session`, HttpOnly, SameSite=Lax, plus Secure when the request arrived over https or the public URL is https)

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
  overrides: { font: FontState; admin_at: string; user_at: string }[];
};
type TimelineEvent = { id: number; at: string; by: By; kind: "name" | "font" | "override" | "failure" | "organize" | "system"; text: string };
type NameChangeRow = { at: string; install_ids: string[]; old_name: string | null; new_name: string; by: "user" | "first" | "merge" | "admin_reset"; next_allowed: string | null };
type LibraryFile = { hash: string; filename: string; ps_name: string; family: string; style: string; version: string;
  license: "licensed" | "personal" | "client" | "unknown"; size: number;
  delivered_to: "all" | "none" | string[]; active_on: number; blocked: boolean; uploaded_at: string };
type OrganizeChange = { case: string; action: { op: string; [k: string]: unknown }; note: string }; // = core::organize::Change JSON
type Page<T> = { items: T[]; total: number }; // total = size of the whole filtered list
type Settings = { name_change_days: number; allow_connection_edit: boolean; deliver_library_to_new: boolean;
  activate_delivered: boolean; notify_admin_changes: boolean; timeline_retention_days: number | null;
  server_url: string; install_key: string };
```

Paged lists take `offset` (default 0) and `limit` (default 50, clamped to 1–200 unless noted) and return `Page<T>`.

| Method | Path | Body → Response |
|---|---|---|
| POST | `/login` | `{ email, password }` → 204 + cookie; 401 `bad_credentials`; 429 `too_many_attempts` after 5 failed sign-ins for the same email within 15 minutes |
| POST | `/logout` | → 204 |
| GET | `/me` | → `{ email, role }` or 401 |
| GET | `/overview` | → `{ installs, online, os: {windows, macos}, name_changes_month, overrides_week, failures_week, recent: Activity[], attention: { overrides, failures, possible_duplicates, stale, requests }, top_fonts: { family, users }[] }` |
| GET | `/installs?q=&filter=all\|online\|overrides\|duplicates\|stale&offset=&limit=` | → `Page<InstallSummary>` sorted by name (q matches current name, old names, install id) |
| GET | `/installs/{id}` | → `InstallDetail` (fonts are paged separately) |
| GET | `/installs/{id}/fonts?q=&active=true\|false&offset=&limit=` | → `Page<FontState>` active first, then family (q matches family, style, PostScript name). Covers the install and every install merged into it: one row per font, `active` if on in any of them |
| GET | `/installs/{id}/timeline?kind=all\|names\|fonts\|overrides\|failures&offset=&limit=` | → `Page<TimelineEvent>` newest first (limit default 100, max 500) |
| POST | `/installs/{id}/fonts` | `{ hash, active }` → `{ command_id }` (queues Activate/Deactivate for the install and every install merged into it, pushes over WS; `command_id` is the one for `{id}`) |
| POST | `/installs/{id}/reset-name-limit` | → 204 |
| POST | `/installs/{id}/organize-request` | → `{ command_id }` |
| POST | `/installs/merge` | `{ primary, secondary, keep_name: "primary" \| "secondary" }` → `InstallDetail` of primary |
| POST | `/installs/{id}/split` | → 204 (undo a merge within 30 days; `id` = the retired install) |
| DELETE | `/installs/{id}` | → 204. Removes the computer and every install merged into it: tokens get 401 `revoked`, open sockets close, they leave every list and count, their reported fonts, unsent commands and deliveries aimed at them are dropped. Name changes and timeline events stay (name-history export). Revoked installs answer 404 on every `/installs/{id}…` endpoint. |
| GET | `/name-changes?q=&since=&offset=&limit=` | → `Page<NameChangeRow>` newest first |
| GET | `/name-changes.csv?q=&since=` | → CSV of every matching row (not paged) |
| GET | `/library?q=&offset=&limit=` | → `Page<LibraryFile>` by family, style (q matches family, style, PostScript name, file name) |
| POST | `/library` | multipart, field `files` (one or more font files) → `LibraryFile[]` added |
| PUT | `/library/{hash}` | `{ license }` → `LibraryFile` |
| POST | `/library/{hash}/block` | `{ blocked }` → `LibraryFile` (pushes Block/Unblock to every install) |
| DELETE | `/library/{hash}` | → 204. Moves the stored file to `<data>/quarantine/` (as organize does; a taken name gets a `<hash8>-` and then a counter prefix) and drops the row and its deliveries; desktops keep their copies. 409 `blocked` for a blocked file (unblock it first) |
| POST | `/library/{hash}/deliver` | `{ target: "all" \| string[], activate }` → `LibraryFile`. **Sets** the targets (replaces them): ids left out, or "all" narrowed to a list, stop getting the file (new installs, downloads, `/me/library`); `[]` = nobody. Computers that already downloaded it keep it; unacked `Deliver` commands for this file to computers no longer covered are dropped. Queues `Deliver` to every recipient. 409 `blocked`, 404 for an unknown or removed install |
| POST | `/library/bulk` | `{ action: "deliver" \| "block" \| "unblock" \| "delete", hashes?: string[], q?: string, target?, activate? }` → `{ changed, skipped, failed }`. Acts on `hashes`, or without them on every file the list search `q` matches (`""` = all), so "select all" spans pages. `deliver` needs `target` (same meaning as above). Unknown files, files already in the asked block state and blocked files asked to be delivered or deleted are skipped. A `delete` that fails for one file counts it in `failed` and goes on with the rest. 400 `no_selection` without `hashes` and `q` |
| GET | `/library/organize` | → `{ changes: OrganizeChange[] }` |
| POST | `/library/organize/apply` | → `{ applied }`. Removals move the stored file to `<data>/quarantine/` (nothing is deleted); a file that cannot be moved keeps its row and is not counted |
| GET | `/organize/reports?offset=&limit=` | → `Page<{ install_id, name, duplicates, version_conflicts, name_clashes, last_organized: string \| null }>` by name |
| GET | `/requests?state=all\|open\|done&offset=&limit=` | → `Page<{ id, install_id, install_name, font_name, note: string \| null, at, done_at: string \| null }>` newest first. `install_id`/`install_name` are the person the request belongs to now (the merge primary). Default `all`; another state is 400 `invalid_state`. `attention.requests` on `/overview` counts the open ones |
| POST | `/requests/{id}/done` | → 204 (an already done request keeps its first `done_at`); 404 if there is no such request |
| POST | `/requests/{id}/reopen` | → 204; 404 if there is no such request |
| GET | `/settings` | → `Settings` |
| PUT | `/settings` | `Partial<Settings>` without `install_key` → `Settings` (pushes `PolicyChanged`). 400 `invalid_setting` unless `name_change_days` is 1–3650 and `timeline_retention_days` is null or 1–3650 |
| POST | `/settings/install-key` | → `{ install_key }` (regenerates). Owner only |
| GET | `/update` | → `{ version, size } \| null`: the current app release |
| POST | `/update` | multipart (at most 100 MB, else 413 `bad_upload`), fields `file` (setup program), `manifest` and `signature` (from `typeshelf-server update-sign`) → `{ version, size }`; replaces the current release. 400 `missing_field`, `bad_manifest`, `bad_signature` (not 128 hex characters) or `file_mismatch` (size or hash differs from the manifest). The server cannot check whose key signed it; the apps do. Any admin today; owner only once roles exist. |
| GET | `/admins` | → `{ id, email, role: "owner" \| "admin", last_login }[]` |
| POST | `/admins` | `{ email, password }` → 201 (role `admin`). Owner only |
| DELETE | `/admins/{id}` | → 204; ends their sessions. Owner only; 409 `last_owner` for the only owner |
| PUT | `/admins/{id}/role` | `{ role: "owner" \| "admin" }` → 204. Owner only; 409 `last_owner` when demoting the only owner |
| POST | `/admins/{id}/password` | `{ password }` → 204; ends their sessions. Owner only, not for yourself (400 `own_password`) |
| PUT | `/me/password` | `{ current, password }` → 204 + a fresh cookie; ends every other session. 403 `wrong_password` (counts toward the sign-in lock) |
| GET | `/audit?admin=&action=&offset=&limit=` | → `Page<{ id, at, admin, action, target, detail }> & { admins: string[], actions: string[] }` newest first |

Owner-only routes answer a plain admin with 403 `owner_only`. Roles and sessions are read on every request, so a role
change, a removal or a password change applies to sessions already signed in on their next request. The audit log
never stores passwords, tokens or the install key; failed sign-ins are recorded once, when they lock a real admin's email.

Online = WebSocket connected, or `last_seen` within the last 5 minutes. Stale = not seen for 14+ days.
Possible duplicate = another install had the same name (case-insensitive, ignoring spaces) or a name one is a prefix of the other ("Budi S." ~ "Budi Santoso").

CLI: `typeshelf-server serve [--port 8080] [--data ./data]`, `typeshelf-server create-admin <email>` (prompts for a password; the first admin is the owner; for an existing email it sets a new password and ends that admin's sessions).
