# Install fonts card Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the flat "Active fonts" list on an install's page with a family-grouped list that has filters, sorting, batch selection and batch on/off/block, all backed by the server.

**Architecture:** The server gains `GET /installs/{id}/families`, extra filters and fields on `GET /installs/{id}/fonts`, and `POST /installs/{id}/fonts/bulk`, all in `apps/server/src/installs.rs`. They reuse the merged-group CTE in `fonts()`. The client moves the card out of `UserDetail.tsx` into a new `pages/InstallFonts.tsx`, with types in `api.ts` and fixtures in `mock.ts`. Task 1 (server) and Task 2 (client) share no files and run in parallel against the API contract written below.

**Tech Stack:** Rust (Axum, rusqlite), React 19, motion 14, TypeScript 6.

**Spec:** `docs/superpowers/specs/2026-11-26-install-fonts-design.md`

## Global Constraints

- No new npm or cargo dependencies.
- Existing endpoints behave exactly as before when the new parameters are absent.
- One audit row per batch: `fonts_bulk_on`, `fonts_bulk_off` or `fonts_bulk_block`, with target = install id and detail = "N fonts".
- `on` never turns on a blocked library font. It skips it and counts it in `skipped`.
- Family grouping and the `family=` filter use `COLLATE NOCASE`.
- `cd apps/admin-ui && bunx tsc --noEmit -p tsconfig.app.json` is clean, and `bunx oxlint` shows ≤ 19 warnings.
- `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass. In a fresh worktree, copy the gitignored `apps/desktop/ui/dist` and `plugins/dist` from `C:/Users/lenovo/Documents/GitHub/typeshelf` first.
- `?mock=1` supports everything. Dark mode uses CSS variables only. Respect reduced motion. Use the existing `toast()` (src/toast.tsx) and `useConfirm()` (src/ui.tsx).
- Never stage `.claude/`, `.superpowers/` or `docs/deploy.local.md`. No push. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## API contract (both tasks build to this)

```
GET  /installs/{id}/families?q=&active=true|false&source=user|admin|library|project_set&overridden=true&sort=name|recent&offset=&limit=
  → { items: FamilyRow[], total: number /* families */, fonts: number /* matching styles */ }
  FamilyRow = { family: string, styles: number, on: number, sources: FontSource[], overridden: number,
                in_library: number, updated_at: string }
  default order family COLLATE NOCASE; sort=recent → updated_at DESC, family; bad sort → 400 invalid_sort; bad source → 400 invalid_source
GET  /installs/{id}/fonts  (+ family= (NOCASE exact), source=, overridden=true; existing order unchanged)
  FontState += { updated_at: string, in_library: boolean, blocked: boolean }
POST /installs/{id}/fonts/bulk
  { action: 'on'|'off'|'block', families?: string[], hashes?: string[], all?: true, q?, active?, source?, overridden? }
  exactly one of families | hashes | all → else 400 invalid_selection; bad action → 400 invalid_action
  → { changed: string[] /* hashes */, skipped: number }
```

## Review Focus

1. **Merged installs.** A family's counts and a batch must cover the install plus every install merged into it. Each hash is counted once, with `on` = `MAX(active)` → Task 1 test `families_cover_the_merged_group`.
2. **Filters scope a family selection.** `{families:['Inter'], active:true, action:'off'}` changes only the Inter styles that are on → Task 1 test `bulk_respects_filters`.
3. **Blocked font in a batch-on.** It is skipped, counted, and gets no command → Task 1 test `bulk_on_skips_blocked`.
4. **Undo after a poll moved state.** Undo sends `hashes: changed` with the opposite action and the same skip rules. The toast shows the skipped count → Task 2 manual check in the mock.
5. **Selection reset.** Changing a filter or search clears the selection, so a stale "All 310 matching" can never act on a different filter → Task 2.

---

### Task 1: Server: families, font filters, bulk (parallel with Task 2)

**Files:**
- Modify: `apps/server/src/installs.rs` (`FontState`, `FONT_COLUMNS`, `font_row`, `FontsQuery`, `fonts`; new `families`, `bulk_fonts` and their routes)
- Modify: `apps/server/tests/api.rs` (append tests)
- Modify: `docs/api.md` (Admin API table rows)

**Interfaces:**
- Produces: the HTTP contract above.
- Internal: one shared helper builds the filtered, merged-group, per-hash CTE, so `fonts`, `families` and `bulk_fonts` can't disagree. For example, `fn font_scope(...) -> (String /* WITH … */, Vec<Value> /* params */)`, or a fixed SQL string with numbered params and `?N IS NULL OR …` guards (the pattern `fonts()` already uses). `in_library` and `blocked` come from a `LEFT JOIN library_files l ON l.hash = g.hash`.
- `bulk_fonts` reuses `hub::queue` with `CommandKind::Activate/Deactivate { font: FontRef }` for each changed hash, like `set_font`. For `block` it reuses `library::set_blocked`; make it `pub(crate)` if needed. Do it all in one transaction where the existing helpers allow; hub notifications may go out as `hub::queue` does today.

- [ ] **Step 1: Write the failing tests** in `apps/server/tests/api.rs`. Use the existing helpers `Server::new`, `register`, `desktop`, `admin`, `library_font` and `ack_all`. Report fonts with `PUT /me/fonts` `{ active: [...] }`. Look at `admin_lists_are_paged` for how fonts are reported, and give each font a `family`.
  - `families_group_and_count`: report 3 Inter styles, 2 of them on (send the off one via a desktop event or a snapshot with an inactive state; check how `/me/fonts` marks inactive fonts), and 1 Roboto style.
    - `GET /families` returns `total == 2`, `fonts == 4`, and the Inter row with `styles == 3, on == 2`.
    - `?q=robo` → 1 family. `?active=true` → Inter has `styles == 2`.
    - `?sort=bad` → 400 `invalid_sort`. `?source=bad` → 400 `invalid_source`.
    - `GET /fonts?family=inter` (lowercase) → 3 items.
  - `families_cover_the_merged_group`: two installs, both report Inter Bold, one on and one off. Merge them (`POST /installs/merge`). The primary's `/families` shows Inter with `styles == 1, on == 1`.
  - `bulk_respects_filters`: from the first fixture, `POST /fonts/bulk { action:'off', families:['Inter'], active:true }`.
    - The response has `changed.len() == 2` and `skipped == 0`.
    - `ack_all` on that install's token returns 2 `deactivate` kinds.
    - The audit list (`GET /audit`) has exactly one new `fonts_bulk_off` row.
  - `bulk_on_skips_blocked`: report a font whose hash is a library font (`library_font()`), off. Block it via `POST /library/{hash}/block {blocked:true}`. Then `bulk { action:'on', hashes:[hash] }` → `changed == []`, `skipped == 1`.
  - `bulk_selection_is_validated`: `{action:'on'}` → 400 `invalid_selection`. `{action:'on', all:true, hashes:[]}` → 400 `invalid_selection`. `{action:'zap', all:true}` → 400 `invalid_action`.
  - `fonts_rows_carry_library_and_time`: a `/fonts` item has `updated_at` (non-empty), `in_library` and `blocked`.
- [ ] **Step 2:** Run `cargo test -p typeshelf-server --test api families bulk fonts_rows` → FAIL.
- [ ] **Step 3:** Implement the shared scope helper, then the `fonts` extras, `families` and `bulk_fonts`, plus the routes `/installs/{id}/families` and `/installs/{id}/fonts/bulk`.
- [ ] **Step 4:** Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` → pass.
- [ ] **Step 5:** Update `docs/api.md`: add the rows for the two new endpoints, extend the `/fonts` row, and list the new audit actions where audit actions are listed (grep `font_on`).
- [ ] **Step 6:** Commit: `git add apps/server docs/api.md && git commit -m "Install fonts: families, filters and bulk on/off/block"`

### Task 2: Client: family-grouped fonts card (parallel with Task 1)

**Files:**
- Create: `apps/admin-ui/src/pages/InstallFonts.tsx`, `apps/admin-ui/src/pages/install-fonts.css` (import it from the .tsx)
- Modify: `apps/admin-ui/src/pages/UserDetail.tsx` (remove `Fonts` and `fontsSkeleton`, render `<InstallFonts u={u} who={who} />`; move `useFontToggle` and `sourceText` into InstallFonts.tsx if only it uses them, otherwise export them; `Overrides` still uses `useFontToggle`, so export it from UserDetail.tsx or move it and import it back)
- Modify: `apps/admin-ui/src/api.ts`, `apps/admin-ui/src/mock.ts`
- Audit labels: add `fonts_bulk_on: 'Turned fonts on in bulk'`, `fonts_bulk_off: 'Turned fonts off in bulk'` and `fonts_bulk_block: 'Blocked fonts in bulk'` to `LABELS` in `pages/Audit.tsx`.

**Interfaces:**
- Consumes: the HTTP contract above.
- Produces (`api.ts`):
  - Types: `FamilyRow`; `FontFilter = { q?: string; active?: boolean; source?: FontSource; overridden?: boolean }`; `FontSelection = { families: string[] } | { hashes: string[] } | { all: true }`; `FamilySort = 'name' | 'recent'`.
  - `FontState` gains `updated_at: string; in_library: boolean; blocked: boolean`.
  - `api.families(installId, filter: FontFilter, sort: FamilySort, page)` returns `Page<FamilyRow> & { fonts: number }`.
  - `api.fonts` gains an optional `extra?: { family?: string; source?: FontSource; overridden?: boolean }` as its last argument. Existing callers are unchanged.
  - `api.bulkFonts(installId, action: 'on'|'off'|'block', sel: FontSelection, filter: FontFilter)` returns `{ changed: string[]; skipped: number }`.
- Mock: implement `/families`, `/fonts` (with the new filters and fields) and `/fonts/bulk` over the existing `fonts` fixture array. Give the fixtures real family groupings, for example 160 samples spread across about 25 families of 4–8 styles, plus the named ones. Add varied `source`, `updated_at` and some `in_library`/`blocked` values. The mock bulk mutates `active` and returns the `changed` hashes.

- [ ] **Step 1: api.ts and mock.ts.** Run `tsc` → clean.
- [ ] **Step 2: InstallFonts.tsx, the list.**
  - Card header: "Fonts · {u.active_fonts} on", keeping the "Turn on fonts…" link.
  - Toolbar:
    - The search box.
    - `Chips` for State (All/On/Off) and Sort (A–Z / Recently changed).
    - A `select` for Source (Any / Turned on by {who} / by admin / Shared library / Project set).
    - A checkbox "User overrides only".
    - The count "{total} families · {fonts} fonts".
  - `useLoad(() => api.families(...), deps, 10_000)` with `usePage(key)`, where the key includes every filter and the sort.
  - **Family row:**
    - A checkbox.
    - The family name in its own font (`fontFamily: '<family>', var(--font-ui)`).
    - "{styles} styles · {on} on", plus a source summary and "· N waiting" if any style is pending.
    - A family `Toggle`: on when `on === styles`; mixed when `0 < on < styles` (render a distinct state with `aria-pressed="mixed"`); a click on mixed turns all on.
    - An expand chevron (`aria-expanded`).
    - The family toggle calls `bulkFonts(on? 'off':'on', {families:[family]}, filter)`.
  - **Expanded family:** fetch `api.fonts(u.id, q, 1, active, { family, source, overridden })` with limit 200. For more than 200 styles, show "Showing 200 of N" (YAGNI on paging inside a family). Render style rows like today's rows: checkbox, name, `sourceText` + `· {ago(updated_at)}`, and the per-font `Toggle` via `useFontToggle`. Keep `expanded: Set<string>` in state across polls and pages.
- [ ] **Step 3: Selection and the batch bar.**
  - `selected = { families: Set, hashes: Set, all: boolean }`. It clears whenever the filter, search or sort key changes.
  - Shift-click on a family checkbox selects the range between the last-clicked row and this one on the current page.
  - "Select all {fonts} matching" appears once at least one row is checked, and sets `all`.
  - **Bar** (sticky at the card bottom, `role="region" aria-label="Selected fonts"`):
    - Text: "{n} families, {m} fonts selected", or "All {fonts} matching fonts".
    - Buttons: Turn on, Turn off, Block everywhere, Clear.
  - Selections can mix families and hashes. Send them as two calls, or send `families` when only families are selected, `hashes` when only styles are selected, and `all` when all is set. For a mix, send two requests and merge the results.
  - **Block** first asks `useConfirm()`: "Block these fonts on every computer? Only fonts in the Shared library can be blocked; others are skipped." with OK = "Block", danger.
  - **After any batch:**
    - Mark each `changed` hash pending (reuse the pending mechanism, or keep a `Set` with the same 120 s expiry).
    - Toast: "Turning off {changed.length} fonts on {who}’s computer" (or "Blocked …"), with " · {skipped} skipped" if any. Undo sends `{hashes: changed}` with the opposite action (`block` → `api.bulk('unblock', {hashes: changed})` from api.ts).
    - Reload the list quietly and clear the selection.
- [ ] **Step 4: Empty and loading states.** A skeleton of 4 family-shaped rows. An empty-filtered state: "No fonts match these filters" with a Clear filters button. A truly empty state: "No fonts reported by this computer yet." (`Empty` takes `title`).
- [ ] **Step 5:** Run `tsc` and `oxlint` (≤ 19), then commit: `git add apps/admin-ui && git commit -m "Install fonts card: families, filters, batch on/off/block"`

### Task 3: Verify (controller)

- [ ] Merge both tasks into `install-fonts`. Run tsc, oxlint, `cargo test --workspace` and clippy.
- [ ] **Preview `?mock=1`, 1440 and 375 widths, light and dark:**
  - Expand and collapse a family.
  - Filter On, then select a family and Turn off; check the toast, Undo, and that the filter change cleared the selection.
  - Shift-click a range, use Select all matching, and Block (confirm dialog, skipped count).
- [ ] **Real API (local server, scratch data):** report about 30 fonts across 5 families from a test install through `/me/fonts`, then repeat the checks.
