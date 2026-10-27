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
type FontCategory = "serif" | "sans" | "display" | "handwriting" | "mono";
type Family = { key: string; family: string; foundry: string; styles: number; isVariable: boolean; license: License;
  activeState: "all" | "some" | "none"; adminSet: boolean; hasDuplicates: boolean; system: boolean;
  category: FontCategory | null;  // from the upright regular face: post/PANOSE/OS/2 family class, then name words
  tags: string[];
  pinned: boolean;  // set_pinned; pinned families sort first in every order
  faces: Face[] };
type Axis = { tag: string; min: number; default: number; max: number };
type FileRow = { path: string; version: string; status: "active" | "older_version" | "alternate" | "grouped" | "inactive" | "system" };
type FamilyDetail = Family & { axes: Axis[]; features: string[]; glyphCount: number; designer: string;
  files: FileRow[]; licenseInfo: { label: License; file: string | null; note: string };
  details: FontDetails };
// Read from the representative face's file (upright, weight nearest 400) when the detail is opened;
// all null / "" when no copy is readable. Blank name-table strings are null.
type FontDetails = { copyright: string | null; trademark: string | null; manufacturer: string | null; designer: string | null;
  description: string | null; license: string | null; licenseUrl: string | null; vendorUrl: string | null;
  designerUrl: string | null; version: string | null; uniqueId: string | null;
  format: string;  // "TrueType" | "OpenType (CFF)" | "OpenType (CFF2)", "… Collection" for TTC/OTC, "Variable …" with fvar
  upm: number | null; glyphCount: number };
type GlyphBlock = { name: string; start: number; end: number; chars: number[] };  // Unicode block + codepoints the face maps
type Folder = { id: number; path: string; status: "watching" | "offline" | "scanning"; fontCount: number; offlineSince: string | null };
type DeleteResult = { trashed: number; failed: string[] };  // failed: "path: reason"
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
| `list_families` | `{ query: { search?, filter?: "all"\|"active"\|"inactive"\|"admin"\|"unknown_license"\|"duplicates"\|"system", collectionId?, folderId?, path?, keys?, tag?, license?, category?: FontCategory, sort?: "name"\|"name_desc"\|"date"\|"foundry"\|"random", seed?: number, includeHidden?: boolean, chars?: string } }` → `Family[]`. `chars`: glyph search, only families whose representative face (the one `category` comes from) maps every character (whitespace ignored). `search` is SuperSearch: whitespace-separated tokens that AND together — `tag:<name>` (exact, any case), `foundry:<text>`, `designer:<text>` (substring), `category:<FontCategory>`, `license:<License>`, `weight:<n>` / `weight:>n` / `weight:<n` / `weight:n-m` (any face's weight), `italic`, `variable`, `active`, `inactive`, `pinned`, `system` (families with a copy in an OS font folder, system-only ones included), `char:<chars>` / `glyph:<chars>` (same as `chars`); quotes allow spaces (`foundry:"Monotype Imaging"`); keys are case-insensitive; unknown keys and bad values are plain words. The plain words, joined by single spaces, match family / foundry / PostScript / full name as one phrase, as before. Pinned families come first in every `sort`. `path`: a folder or subfolder (from `folder_tree`); `keys`: only these families. System-only families appear only with `filter: "system"`, `keys` or in a collection/tag. `sort: "random"` + `seed`: the same seed gives the same order, so pages line up. Families whose every file is in a hidden folder are left out unless `includeHidden`, `keys`, or `path` is inside a hidden folder. Lists that can grow use `family_page` |
| `family_page` | `{ query, offset, limit }` (limit ≤ 200) → `{ items: Family[], total }` |
| `library_counts` | → `{ library, active, system, licenses: { [License]: number } }` (hidden folders left out, like `list_families`) |
| `similar_families` | `{ key, limit }` (limit ≤ 50) → `Family[]`, most similar first, without `key` itself; system fonts included. Distance over category, weight, italic, width class, x-height, cap height, average width and PANOSE serif/proportion/contrast |
| `set_family_name` | `{ hashes, name: string \| null }` → void. FontBase "override family": these files are grouped and shown under `name` everywhere (lists, search, collections, project sets); `null` restores the real family. Survives rescans; never edits the font file. A family renamed as a whole keeps its collections, tags, license and project sets |
| `delete_fonts` | `{ hashes }` → `DeleteResult`: turns them off, then moves every copy in the user's folders and the Imported/Google folders to the Recycle Bin / Trash (never OS font folders or the shared library) |
| `import_paths` | `{ paths: string[] }` → `{ imported, skipped, folder }`: copies dropped font files / folders (recursive, font extensions only; a folder keeps its name as a subfolder) into the app-managed "Imported fonts" folder (shown in `folder_tree`), skipping files whose bytes are already in the library and never overwriting (`name (2).ttf`); then scans |
| `get_faces` | `{ hashes }` → `Face[]` |
| `face_glyphs` | `{ hash, index? }` (face in a TTC, default 0) → `{ total, blocks: GlyphBlock[] }`: the face's cmap read from the file (control characters left out), grouped by Unicode block in codepoint order; codepoints outside the built-in table of ~130 common blocks come last as `"Other"`. Rejects `{code:"not_found"}` when no copy is readable |
| `set_pinned` | `{ keys, pinned }` → void; emits `library-changed` |
| `folder_tree` | → `FolderNode[]` = `{ path, name, fontCount, activeCount, autoActivate, hidden, children: FolderNode[] }`, one root per watched folder (app-managed ones included: Imported fonts, Google Fonts, Shared library) |
| `set_folder_active` | `{ path, active, confirmUnknownLicense? }` → `ActivationResult[]` (every font in the folder and below) |
| `set_folder_hidden` | `{ path, hidden }` → `FolderNode[]` (the new tree); a hidden folder hides its subfolders too |
| `delete_folder` | `{ path }` → `DeleteResult` (`trashed` = font files in it): turns its fonts off, moves the folder to the Recycle Bin / Trash, then removes a watched root from the library or rescans the root of a subfolder. Rejects `{code:"not_allowed"}` for OS font folders and the shared library |
| `set_folder_auto` | `{ path, on, confirmUnknownLicense? }` → `ActivationResult[]`; on: turns on what is there now and every font that lands in the folder later |
| `get_family` | `{ key }` → `FamilyDetail` |
| `set_active` | `{ hashes, active, confirmUnknownLicense? }` → `ActivationResult[]`; rejects `{code:"unknown_license", families}` when not confirmed |
| `list_folders` / `add_folder {path}` / `remove_folder {id}` / `locate_folder {id, path}` | → `Folder[]` |
| `list_collections` / `create_collection {name, parentId?}` / `rename_collection {id, name}` / `delete_collection {id}` | → `Collection[]` |
| `add_to_collection` / `remove_from_collection` | `{ id, familyKeys }` → `Collection[]` |
| `set_collection_active` | `{ id, active, confirmUnknownLicense? }` → `ActivationResult[]`: every family in the collection and its sub-collections (a font with another version already on is left alone); same unknown-license rejection as `set_active` |
| `export_collection` | `{ id, dest }` → `{ files, path }`: a .zip at `dest` with one copy of each font file in the collection and its sub-collections (OS font folders left out) |
| `export_collections` | `{ dest }` → `{ collections }`: a JSON file `{ version: 1, collections: [{ name, families: [{ key, family }], children: [...] }] }` with every collection except Recent (Favorites included), nested; `collections` counts the nested ones too |
| `import_collections` | `{ path }` → `{ collections, families, missing }`: merges such a file — a collection with the same name (any case) under the same parent is reused, otherwise created; each family entry matches by key, then by family name (any case); `missing` counts entries with no match. Rejects `{code:"invalid"}` for a file that is not one |
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
| `backup_create {path}` / `backup_restore {path}` | → `{ path, at }`; a backup holds collections, tags, pins, licenses, project sets and settings |
| `get_settings` / `update_settings {patch}` | → `{ startAtLogin, startHidden, autoUpdate, deactivateOnQuit, quickShortcut, theme: "system"\|"light"\|"dark", sampleText, sampleTexts: string[] }` (`sampleText`: the current text; `sampleTexts`: presets for the sample picker, 5 by default) |
| `get_connection` / `test_connection {serverUrl, installKey}` / `save_connection {serverUrl, installKey}` / `reset_connection` | → `Connection` (`test_connection` → `{ ok, error }`) |
| `quick_search` | `{ query }` → `Face[]` (max 20) |
| `show_in_folder` | `{ path }` → void |
| `open_url` | `{ url }` → void; http(s) only, opens the default browser (links from font metadata) |
| `adobe_status` | → `{ app: "photoshop"\|"illustrator"\|"indesign", label: string, versions: string[], installed: boolean, paired: boolean }[]` |
| `adobe_install` / `adobe_remove` | `{ app }` → same row |
| `adobe_pair_respond` | `{ requestId, allow }` → void |
| `indesign_running` | → boolean: InDesign is open (it only sees font changes after a restart) |
| `pick_folder` / `pick_file {filters}` / `pick_save_file {defaultName, filters}` | → `string \| null` (native dialogs) |
| `pick_files` | `{ filters }` → `string[] \| null` (native multi-select open dialog) |

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
