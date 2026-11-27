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
  system: boolean;  // in an OS font folder: always on, read-only
  temporaryUntil: string | null };  // on temporarily (set_active_for): "restart" or the ISO time it turns off
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
  details: FontDetails;
  /** Representative face's stored x-height / unitsPerEm; 0 when the font does not say. */ xHeight: number;
  // Stored metrics per face hash (face 0 of a collection), lengths per em; 0 when the font does not say.
  // ascender / descender (negative below the baseline) are 0 for fonts last scanned before they were added.
  faceMetrics: { [hash: string]: { x_height: number; cap_height: number; ascender: number; descender: number; avg_width: number; width_class: number; category: FontCategory | null; panose: number[]; fixed_pitch: boolean } } };
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
type Collection = { id: number; name: string; parentId: number | null; count: number; active: number /* families with a font on */; builtin: "favorites" | "recent" | null };
type ProjectSet = { id: number; name: string; note: string; familyKeys: string[]; on: boolean; lastUsed: string | null };
type OrganizeChange = { id: number; case: "exact_copy" | "different_version" | "other_format" | "name_clash" | "variable_static" | "in_system" | "junk" | "tidy";
  action: { op: string; [k: string]: unknown }; note: string };
type Connection = { status: "connected" | "offline" | "error"; serverUrl: string; installKey: string; editable: boolean; unlockable: boolean /* locked, but the admin set an unlock password */;
  lastSync: string | null; installId: string; error: string | null };
type AppState = { onboarded: boolean; name: string; nextNameChange: string | null; nameChangeDays: number;
  brandName: string; brandColor: string; brandLogo: string; /* set by the admin; "" = Glyphyard's own. brandColor is "#rrggbb", brandLogo an image data URL */
  serverConfigured: boolean; /* false = standalone: no server address, nothing syncs */
  registered: boolean; /* holds a server token: the name rule applies (false after "Continue offline" until it joins) */
  connection: Connection; platform: "windows" | "macos"; appVersion: string };
type ActivationResult = { hash: string; ok: boolean; error: "file_missing" | "in_use" | "invalid_font" | "blocked" | "name_conflict" | "os" | null; message: string | null };
```

## Commands
| Command | Args → Result |
|---|---|
| `get_app_state` | → `AppState` |
| `complete_onboarding` | `{ name, offline? }` → `AppState` (registers with the server; rejects `{code:"offline"}` without a connection). With no server configured, or `offline: true` ("Continue offline"), the name is saved locally and the sync loop registers with it once the server answers; if the server then refuses the name, the name is cleared (back to onboarding) and `connection.error` says why |
| `change_name` | `{ name }` → `{ name, nextNameChange }`; rejects `{code:"name_locked", nextNameChange}` or `{code:"offline"}`. Before registering (no server, or not reached yet) the name changes locally with no lock |
| `list_families` | `{ query: { search?, filter?: "all"\|"active"\|"inactive"\|"admin"\|"unknown_license"\|"duplicates"\|"system", collectionId?, folderId?, path?, keys?, tag?, license?, category?: FontCategory, sort?: "name"\|"name_desc"\|"date"\|"foundry"\|"random", seed?: number, includeHidden?: boolean, chars?: string, lang?: string } }` → `Family[]`. `chars`: glyph search, only families whose representative face (the one `category` comes from) maps every character (whitespace ignored). `search` is SuperSearch: whitespace-separated tokens that AND together — `tag:<name>` (exact, any case), `foundry:<text>`, `designer:<text>` (substring), `category:<FontCategory>`, `license:<License>`, `weight:<n>` / `weight:>n` / `weight:<n` / `weight:n-m` (any face's weight), `italic`, `variable`, `active`, `inactive`, `pinned`, `system` (families with a copy in an OS font folder, system-only ones included), `char:<chars>` / `glyph:<chars>` (same as `chars`), `lang:<id>` (same as `lang`: the representative face has every required character of that language, see `language_counts`; unknown ids are plain words, and an unknown `lang` field matches nothing); quotes allow spaces (`foundry:"Monotype Imaging"`); keys are case-insensitive; unknown keys and bad values are plain words. The plain words, joined by single spaces, match family / foundry / PostScript / full name as one phrase, as before. Pinned families come first in every `sort`. `path`: a folder or subfolder (from `folder_tree`); `keys`: only these families. System-only families appear only with `filter: "system"`, `keys` or in a collection/tag. `sort: "random"` + `seed`: the same seed gives the same order, so pages line up. Families whose every file is in a hidden folder are left out unless `includeHidden`, `keys`, or `path` is inside a hidden folder. Lists that can grow use `family_page` |
| `family_page` | `{ query, offset, limit }` (limit ≤ 200) → `{ items: Family[], total }` |
| `query_targets` | `{ query }` (as `list_families`) → `{ keys: string[], hashes: string[] }`: every matching family's key, in list order, and the hashes of their fonts that can be switched (system fonts left out). The Library's "Select all N results" feeds these to `set_active`, `set_active_for`, `add_to_collection` and `edit_tags` (pinning, renaming and deleting stay with a page selection) |
| `edit_tags` | `{ familyKeys, add: string[], remove: string[] }` → number of families whose tags changed. Adds and removes tags on every family in one transaction; a tag in both lists is removed; names follow `set_tags` (trimmed, blanks dropped). Used by the Tags dialog for any multi-family selection |
| `library_counts` | → `{ library, active, system, licenses: { [License]: number } }` (hidden folders left out, like `list_families`) |
| `similar_families` | `{ key, limit }` (limit ≤ 50) → `Family[]`, most similar first, without `key` itself; system fonts included. Distance over category, weight, italic, width class, x-height, cap height, average width and PANOSE serif/proportion/contrast |
| `pairing_suggestions` | `{ key }` → `Family[]` (at most 8): families whose category is known and differs from `key`'s, nearest stored x-height first (those without one last); system fonts included. Rejects `{code:"not_found"}` |
| `list_axis_presets` | `{ key }` → `AxisPreset[]` = `{ name, values: { [axisTag]: number } }[]`, the family's saved variable-axis settings in save order (settings store, `axis_presets:<key>`) |
| `save_axis_preset` | `{ key, name, values }` → `AxisPreset[]`; a preset with the same (trimmed) name is replaced. Rejects `{code:"invalid"}` for an empty name or one over 80 characters, a value that is not a finite number, or a 51st preset for the family |
| `delete_axis_preset` | `{ key, name }` → `AxisPreset[]` |
| `set_family_name` | `{ hashes, name: string \| null }` → void. FontBase "override family": these files are grouped and shown under `name` everywhere (lists, search, collections, project sets); `null` restores the real family. Survives rescans; never edits the font file. A family renamed as a whole keeps its collections, tags, license, project sets and axis presets |
| `delete_fonts` | `{ hashes }` → `DeleteResult`: turns them off, then moves every copy in the user's folders and the Imported/Google folders to the Recycle Bin / Trash (never OS font folders or the shared library) |
| `import_paths` | `{ paths: string[] }` → `{ imported, skipped, folder }`: copies dropped font files / folders (recursive, font extensions only; a folder keeps its name as a subfolder) into the app-managed "Imported fonts" folder (shown in `folder_tree`), skipping files whose bytes are already in the library and never overwriting (`name (2).ttf`); then scans |
| `get_faces` | `{ hashes }` → `Face[]` |
| `language_counts` | → `{ id, name, families }[]`: every language the filter knows (English, Indonesian, Malay, Vietnamese, French, German, Spanish, Portuguese, Polish, Turkish, Russian, Greek, Arabic, Hebrew, Thai, Hindi, Japanese, Korean, Chinese) with how many library families fully cover it (representative face, hidden folders left out, like `list_families`). `id` is the lowercase English name, the value of `list_families` `lang` and of the `lang:<id>` token |
| `face_glyphs` | `{ hash, index? }` (face in a TTC, default 0) → `{ total, blocks: GlyphBlock[] }`: the face's cmap read from the file (control characters left out), grouped by Unicode block in codepoint order; codepoints outside the built-in table of ~130 common blocks come last as `"Other"`. Rejects `{code:"not_found"}` when no copy is readable |
| `glyph_svg` | `{ hash, index?, cp }` (face in a TTC, default 0; `cp` a Unicode codepoint) → `string`: a standalone SVG of that character (default instance, unhinted), one `<path>` in font units flipped to y-down; the viewBox covers 0..advance and ascent..descent, widened to the outline (control points included) where it reaches past them, and the baseline is y = 0. Rejects `{code:"not_found"}` when no copy is readable or the face does not map `cp` |
| `set_pinned` | `{ keys, pinned }` → void; emits `library-changed` |
| `folder_tree` | → `FolderNode[]` = `{ path, name, fontCount, activeCount, autoActivate, hidden, children: FolderNode[] }`, one root per watched folder (app-managed ones included: Imported fonts, Google Fonts, Shared library) |
| `set_folder_active` | `{ path, active }` → `ActivationResult[]` (every font in the folder and below) |
| `set_folder_hidden` | `{ path, hidden }` → `FolderNode[]` (the new tree); a hidden folder hides its subfolders too |
| `delete_folder` | `{ path }` → `DeleteResult` (`trashed` = font files in it): turns its fonts off, moves the folder to the Recycle Bin / Trash, then removes a watched root from the library or rescans the root of a subfolder. Rejects `{code:"not_allowed"}` for OS font folders and the shared library |
| `set_folder_auto` | `{ path, on }` → `ActivationResult[]`; on: turns on what is there now and every font that lands in the folder later |
| `get_family` | `{ key }` → `FamilyDetail` |
| `set_active` | `{ hashes, active }` → `ActivationResult[]`. No license confirmation: fonts without a license label turn on like any other |
| `set_active_for` | `{ hashes, until }` (`until` = `"restart"` or `{ minutes }`) → `ActivationResult[]`: turns on like `set_active`, then off again by itself at the next start or after the minutes (checked every minute). Turning the font on or off any other way cancels that; a font an admin, a library push or an active project set also wants stays on |
| `list_folders` / `add_folder {path}` / `remove_folder {id}` / `locate_folder {id, path}` | → `Folder[]` |
| `list_collections` / `create_collection {name, parentId?}` / `rename_collection {id, name}` / `delete_collection {id}` | → `Collection[]` |
| `add_to_collection` / `remove_from_collection` | `{ id, familyKeys }` → `Collection[]` |
| `set_collection_active` | `{ id, active }` → `ActivationResult[]`: every family in the collection and its sub-collections (a font with another version already on is left alone); same unknown-license rejection as `set_active` |
| `export_collection` | `{ id, dest }` → `{ files, path }`: a .zip at `dest` with one copy of each font file in the collection and its sub-collections (OS font folders left out) |
| `export_collections` | `{ dest }` → `{ collections }`: a JSON file `{ version: 1, collections: [{ name, families: [{ key, family }], children: [...] }] }` with every collection except Recent (Favorites included), nested; `collections` counts the nested ones too |
| `import_collections` | `{ path }` → `{ collections, families, missing }`: merges such a file — a collection with the same name (any case) under the same parent is reused, otherwise created; each family entry matches by key, then by family name (any case); `missing` counts entries with no match. Rejects `{code:"invalid"}` for a file that is not one |
| `list_tags` | → `{ name, count }[]`; `set_tags {familyKey, tags}` → `string[]` |
| `set_license` | `{ familyKey, label, file?, note? }` → `FamilyDetail["licenseInfo"]`. Omitted or null `file` / `note` keep the stored value, so a license file cannot be detached through this command |
| `list_project_sets` / `save_project_set {id?, name, note, familyKeys}` / `delete_project_set {id}` | → `ProjectSet[]` |
| `toggle_project_set` | `{ id, on }` → `ActivationResult[]` (same unknown-license rejection) |
| `organize_plan` | `{ tidy }` → `{ changes: OrganizeChange[], scanned: number, scannedAt: string }` |
| `organize_apply` | `{ tidy, excludeIds: number[] }` → `{ historyId, applied, failed: string[] }` (`failed`: "path: reason" for skipped changes, at most 20) |
| `list_history` / `undo_history {id}` | → `{ id, kind, summary, at, undone }[]` |
| `health` / `repair_now` | → `{ lastCheck, active, problems: { hash, message }[], removed: number, reregistered: number }` |
| `usage_stats` | `{ period: "month"\|"year" }` → `{ period, since, mostUsed: { key, family, days }[], neverUsed: { key, family }[], neverUsedNote: string\|null, totalFonts, addedInPeriod, recordingSince: string\|null }`. Local only, never sent anywhere. `mostUsed`: top 10 by days (UTC) a font of the family was turned on, by the user, a project set or an Adobe document (not the admin). `neverUsed`: families in the user's folders for at least 30 days, not turned on for 12 months (or since `recordingSince`) and not on now; empty with `neverUsedNote` until both the oldest font file and the recording are 30 days old. `totalFonts` / `addedInPeriod`: font files in the user's folders (system fonts not counted) |
| `badges` | → `{ id: "collector"\|"archaeologist"\|"quiet", name, description, earnedAt: string\|null }[]`; each is earned once |
| `pdf_fonts` | `{ path }` → `{ psName, display, pages: number, status: "active"\|"inactive"\|"version"\|"missing", detail, hash: string\|null, google: boolean }[]` |
| `document_fonts` | `{ path }` (a .pdf, .psd, .ai or .indd file) → `{ fonts: <same rows as pdf_fonts>, partial: string\|null }`. Design files have no page numbers (`pages` is 0) and never report "version"; `partial` says in plain words why only part of the file was read (size cap, name cap), else null |
| `request_font` | `{ name, note: string\|null }` → void: asks the admin for a font (name 1–200 characters, note up to 1000); queued in the outbox, sent when connected. `invalid_request` when the text is empty or too long |
| `google_catalog` | `{ search?, category? }` → `{ family, category, styles, variable, installed }[]` |
| `google_install` / `google_remove` | `{ family }` → `{ family, installed }` |
| `backup_create {path}` / `backup_restore {path}` | → `{ path, at }`; a backup holds collections, tags, pins, licenses, project sets and settings |
| `get_settings` / `update_settings {patch}` | → `{ startAtLogin, startHidden, autoUpdate, deactivateOnQuit, quickShortcut, theme: "system"\|"light"\|"dark", language: "system"\|"en"\|"id" (default "system"), sampleText, sampleTexts: string[], playful }` (`sampleText`: the current text; `sampleTexts`: presets for the sample picker, 5 by default; `playful`: "Playful extras" easter eggs and hidden games, on by default). Read-only extras: `canUpdate` (false when the build has no update key; the "Update automatically" switch is hidden) and `updateReady: string \| null` (a downloaded, verified version that installs when the app quits), `updateProblem: string \| null` (plain text when the last check refused the server's release: bad signature, malformed manifest or a file that does not match; cleared by the next good check) |
| `get_connection` / `test_connection {serverUrl, installKey}` / `save_connection {serverUrl, installKey, password?}` / `reset_connection {password?}` | → `Connection` (`test_connection` → `{ ok, error }`). Saving an empty address keeps it empty across restarts (the installer defaults are not filled in again until a save with an address or `reset_connection`). When the admin removed this computer, `connection.error` is "This computer was removed by your admin. Save the connection again to rejoin." and saving the same values registers again as a new install. When the admin locked the connection, `save_connection`/`reset_connection` without `password` reject `{code:"connection_locked"}`; with the unlock password they verify it first (editable: the password is ignored). Wrong or throttled passwords reject `wrong_password` / `unlock_throttled` (extra `retryAfterSecs`; 5 wrong in a row lock attempts for 5 minutes, kept across restarts) / `unlock_unavailable` (no password set) |
| `unlock_connection {password}` | → `void`; verifies the unlock password only (same error codes as above). `Connection.unlockable` says whether the admin set one |
| `set_tray_language {lang}` | → `void`; `"id"` labels the tray menu "Buka Glyphyard" / "Keluar", anything else "Open Glyphyard" / "Quit" |
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
| `events-dropped` | `{ count }` (the server refused some queued events for good, e.g. a font request sent to an older server; they are not retried) |
| `folder-status` | `Folder` |
| `activation-progress` | `{ done, total, active }`: a `set_active` batch bigger than one chunk (64 files) reports after each chunk; the database is free between chunks |
| `activation-done` | `{ total, failed: string[] }`: turning fonts on returns at once and the OS registration runs in the background; when it ends, fonts that failed are already switched back off and `failed` says why ("Family Style: reason", at most 20) |
| `badge-earned` | `{ id, name, description, earnedAt }` (UI shows a toast) |
| `name-changed` | `{ name, nextNameChange }` (also after the sync loop registers, with the name the server stored; an empty name means the server refused it) |
| `adobe-pair-request` | `{ requestId, app, appVersion }` (UI asks "Allow Photoshop to turn on fonts automatically?") |
| `adobe-activated` | `{ app, document, activated: string[], missing: string[] }` (UI shows a short notice) |
