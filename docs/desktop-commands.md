# Desktop UI ↔ backend contract (Tauri commands and events)

The React UI calls these with `invoke(name, args)`; arguments are camelCase in TS and snake_case in Rust
(Tauri converts). Errors reject with `{ code: string, message: string, ...extra }`.
The UI has a typed wrapper `apps/desktop/ui/src/api.ts` plus a mock implementation used when not running inside Tauri.

## Types
```ts
type License = "licensed" | "personal" | "client" | "unknown";
type Source = "user" | "admin" | "project_set" | "library";
type Face = { hash: string; psName: string; family: string; style: string; fullName: string; version: string;
  path: string; active: boolean; source: Source | null; weight: number; italic: boolean; isVariable: boolean;
  system: boolean };  // in an OS font folder: always on, read-only
type Family = { key: string; family: string; foundry: string; styles: number; isVariable: boolean; license: License;
  activeState: "all" | "some" | "none"; adminSet: boolean; hasDuplicates: boolean; system: boolean; faces: Face[] };
type Axis = { tag: string; min: number; default: number; max: number };
type FileRow = { path: string; version: string; status: "active" | "older_version" | "alternate" | "grouped" | "inactive" | "system" };
type FamilyDetail = Family & { axes: Axis[]; features: string[]; glyphCount: number; designer: string;
  files: FileRow[]; licenseInfo: { label: License; file: string | null; note: string } };
type Folder = { id: number; path: string; status: "watching" | "offline" | "scanning"; fontCount: number; offlineSince: string | null };
type Collection = { id: number; name: string; parentId: number | null; count: number; builtin: "favorites" | "recent" | null };
type ProjectSet = { id: number; name: string; note: string; familyKeys: string[]; on: boolean; lastUsed: string | null };
type OrganizeChange = { id: number; case: "exact_copy" | "different_version" | "other_format" | "name_clash" | "variable_static" | "in_system" | "junk" | "tidy";
  action: { op: string; [k: string]: unknown }; note: string };
type Connection = { status: "connected" | "offline" | "error"; serverUrl: string; installKey: string; editable: boolean;
  lastSync: string | null; installId: string; error: string | null };
type AppState = { onboarded: boolean; name: string; nextNameChange: string | null; nameChangeDays: number;
  connection: Connection; platform: "windows" | "macos"; appVersion: string };
type ActivationResult = { hash: string; ok: boolean; error: "file_missing" | "in_use" | "invalid_font" | "blocked" | "name_conflict" | "os" | null; message: string | null };
```

## Commands
| Command | Args → Result |
|---|---|
| `get_app_state` | → `AppState` |
| `complete_onboarding` | `{ name }` → `AppState` (registers with the server; rejects `{code:"offline"}` without a connection) |
| `change_name` | `{ name }` → `{ name, nextNameChange }`; rejects `{code:"name_locked", nextNameChange}` or `{code:"offline"}` |
| `list_families` | `{ search?, filter?: "all"\|"active"\|"inactive"\|"admin"\|"unknown_license"\|"duplicates"\|"system", collectionId?, folderId?, tag?, license?, sort?: "name"\|"name_desc"\|"date"\|"foundry" }` → `Family[]`. System-only families appear only with `filter: "system"` (or in a collection/tag) |
| `get_family` | `{ key }` → `FamilyDetail` |
| `set_active` | `{ hashes, active, confirmUnknownLicense? }` → `ActivationResult[]`; rejects `{code:"unknown_license", families}` when not confirmed |
| `list_folders` / `add_folder {path}` / `remove_folder {id}` / `locate_folder {id, path}` | → `Folder[]` |
| `list_collections` / `create_collection {name, parentId?}` / `rename_collection {id, name}` / `delete_collection {id}` | → `Collection[]` |
| `add_to_collection` / `remove_from_collection` | `{ id, familyKeys }` → `Collection[]` |
| `list_tags` | → `{ name, count }[]`; `set_tags {familyKey, tags}` → `string[]` |
| `set_license` | `{ familyKey, label, file?, note? }` → `FamilyDetail["licenseInfo"]` |
| `list_project_sets` / `save_project_set {id?, name, note, familyKeys}` / `delete_project_set {id}` | → `ProjectSet[]` |
| `toggle_project_set` | `{ id, on, confirmUnknownLicense? }` → `ActivationResult[]` (same unknown-license rejection) |
| `organize_plan` | `{ tidy }` → `{ changes: OrganizeChange[], scanned: number, scannedAt: string }` |
| `organize_apply` | `{ tidy, excludeIds: number[] }` → `{ historyId, applied }` |
| `list_history` / `undo_history {id}` | → `{ id, kind, summary, at, undone }[]` |
| `health` / `repair_now` | → `{ lastCheck, active, problems: { hash, message }[], removed: number, reregistered: number }` |
| `pdf_fonts` | `{ path }` → `{ psName, display, pages: number, status: "active"\|"inactive"\|"version"\|"missing", detail, hash: string\|null, google: boolean }[]` |
| `google_catalog` | `{ search?, category? }` → `{ family, category, styles, variable, installed }[]` |
| `google_install` / `google_remove` | `{ family }` → `{ family, installed }` |
| `backup_create {path}` / `backup_restore {path}` | → `{ path, at }` |
| `get_settings` / `update_settings {patch}` | → `{ startAtLogin, startHidden, autoUpdate, deactivateOnQuit, quickShortcut, theme: "system"\|"light"\|"dark", sampleText }` |
| `get_connection` / `test_connection {serverUrl, installKey}` / `save_connection {serverUrl, installKey}` / `reset_connection` | → `Connection` (`test_connection` → `{ ok, error }`) |
| `quick_search` | `{ query }` → `Face[]` (max 20) |
| `show_in_folder` | `{ path }` → void |
| `adobe_status` | → `{ app: "photoshop"\|"illustrator"\|"indesign", label: string, versions: string[], installed: boolean, paired: boolean }[]` |
| `adobe_install` / `adobe_remove` | `{ app }` → same row |
| `adobe_pair_respond` | `{ requestId, allow }` → void |
| `indesign_running` | → boolean: InDesign is open (it only sees font changes after a restart) |
| `pick_folder` / `pick_file {filters}` | → `string \| null` (native dialogs) |

## Events (backend → UI, `listen(name, cb)`)
| Event | Payload |
|---|---|
| `library-changed` | `{ reason: "scan" \| "watch" \| "activation" \| "admin" \| "organize" }` |
| `admin-change` | `{ action: "activated" \| "deactivated" \| "blocked" \| "delivered", fonts: { family: string, style: string }[] }` (UI shows the blue banner) |
| `organize-requested` | `{}` (admin asked; UI opens the auto-organize preview) |
| `connection-changed` | `Connection` |
| `folder-status` | `Folder` |
| `name-changed` | `{ name, nextNameChange }` |
| `adobe-pair-request` | `{ requestId, app, appVersion }` (UI asks "Allow Photoshop to turn on fonts automatically?") |
| `adobe-activated` | `{ app, document, activated: string[], missing: string[] }` (UI shows a short notice) |
