# Admin dashboard UI/UX pass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add toasts with undo, dark mode, a phone layout, studio branding, a Ctrl+K palette with shortcuts, richer empty states, server-side column sorting and a 30-day trend chart to the Glyphyard admin dashboard.

**Architecture:** The server gains `sort`/`dir` on three list endpoints and a `trend` array on `/overview`, all inside the existing handlers. The client adds small modules: `toast.tsx`, `theme.ts`, `palette.tsx` and `TrendChart` in `Overview.tsx`. New component CSS goes in its own `.css` file imported by the component, so parallel tasks don't collide in `app.css`. Tasks run in two waves. Wave A (Tasks 1–3) touch separate files. Wave B (Tasks 4–6) start after wave A is merged.

**Tech Stack:** React 19, react-router 8, motion 14, Vite 8, TypeScript 6, oxlint; Rust, Axum, rusqlite (SQLite), chrono.

**Spec:** `docs/superpowers/specs/2026-11-26-admin-ui-ux-design.md`

## Global Constraints

- No new npm or cargo dependencies.
- Without `sort`, every list returns exactly today's order: installs by name, library by `family COLLATE NOCASE, style, hash`, requests by `at DESC, id DESC`.
- Unknown `sort` or `dir` → `400 invalid_sort`, the same shape as `invalid_filter`. `dir` without `sort` is ignored.
- Internal names stay `typeshelf`; the display name is "Glyphyard".
- `cd apps/admin-ui && bunx tsc --noEmit -p tsconfig.app.json` is clean. `bunx oxlint` reports ≤ 19 warnings.
- `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
- Never stage `.claude/` or `docs/deploy.local.md`. Never print the install key. No push, no deploy.
- Respect `prefers-reduced-motion` (use `useReducedMotion` as `ui.tsx` does).
- `?mock=1` must exercise every feature (`src/mock.ts`).

## Review Focus

1. **A brand colour too light or too dark.** Button text still meets 4.5:1 (`#ffff00` gets black text) → Task 3 `onAccent` check.
2. **A sorted page 2.** `sort=last_seen&dir=desc&offset=2` continues the same order and doesn't restart → Task 1 test `lists_sort_on_the_server`.
3. **Undo after the row has gone.** Undoing "mark done" while on the Open filter brings the request back after a reload, and an Undo that fails shows a `bad` toast instead of nothing → Task 2 manual check.
4. **Ctrl+K while a dialog is open, or `/` while typing in an input.** Neither shortcut fires inside inputs, textareas, selects or open `<dialog>`s → Task 4 `isTyping` guard.
5. **Trend with no data.** A fresh server returns 30 zero rows and the chart renders an empty baseline, not NaN heights → Task 1 test, Task 6 `max(…, 1)`.

---

## Wave A (parallel)

### Task 1: Server sort + trend, client types and mock

**Files:**
- Modify: `apps/server/src/installs.rs` (`ListQuery`, `list`, `overview`)
- Modify: `apps/server/src/library.rs` (`ListQuery`, `list`)
- Modify: `apps/server/src/requests.rs` (`ListQuery`, `list`)
- Modify: `apps/server/tests/api.rs` (append tests)
- Modify: `docs/api.md` (rows for `/overview`, `/installs`, `/library`, `/requests`)
- Modify: `apps/admin-ui/src/api.ts`, `apps/admin-ui/src/mock.ts`

**Interfaces:**
- Produces (HTTP): `GET /installs?sort=name|last_seen|fonts&dir=asc|desc`, `GET /library?sort=family|uploaded|size&dir=…`, `GET /requests?sort=at|font&dir=…`. Default `dir`: `name`, `family` and `font` are `asc`; the others are `desc`.
- Produces (HTTP): `/overview` gains `trend: { day: "YYYY-MM-DD", activity: number, failures: number, new_installs: number }[]`, exactly 30 entries, oldest first, the last one today (UTC).
- Produces (TS, `api.ts`): `export type SortDir = 'asc' | 'desc'`; `export type Sort<K extends string> = { key: K; dir: SortDir } | null`; `export type InstallSort = 'name' | 'last_seen' | 'fonts'`; `export type LibrarySort = 'family' | 'uploaded' | 'size'`; `export type RequestSort = 'at' | 'font'`; `export type TrendDay = { day: string; activity: number; failures: number; new_installs: number }`; `Overview` gains `trend: TrendDay[]`.
- Produces (TS): `api.installs(q, filter, page, sort?: Sort<InstallSort>)`, `api.library(q, page, size, sort?: Sort<LibrarySort>)`, `api.fontRequests(state, page, sort?: Sort<RequestSort>)`. Each sends `sort` and `dir` only when `sort` is set.

- [ ] **Step 1: Write the failing tests** in `apps/server/tests/api.rs`

```rust
#[tokio::test]
async fn lists_sort_on_the_server() {
    let s = Server::new().await;
    for (id, name) in [("a", "Ana"), ("b", "Budi"), ("c", "Citra")] { s.register(id, name).await; }
    let names = |v: Value| v["items"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(names(s.admin(Method::GET, "/installs?sort=name&dir=desc", None).await.json()), ["Citra", "Budi", "Ana"]);
    assert_eq!(names(s.admin(Method::GET, "/installs?sort=name&dir=desc&offset=2", None).await.json()), ["Ana"]);
    assert_eq!(names(s.admin(Method::GET, "/installs?dir=desc", None).await.json()), ["Ana", "Budi", "Citra"]); // dir alone ignored
    assert_eq!(s.admin(Method::GET, "/installs?sort=evil", None).await.json()["error"], "invalid_sort");
    assert_eq!(s.admin(Method::GET, "/installs?sort=name&dir=up", None).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(s.admin(Method::GET, "/library?sort=size%3Bdrop", None).await.json()["error"], "invalid_sort");
    assert_eq!(s.admin(Method::GET, "/library?sort=uploaded", None).await.status, StatusCode::OK);
    // requests: Zeta posted first, Alpha second (same `at` possible → id breaks the tie)
    let token = s.register("d", "Dewi").await;
    for f in ["Zeta Sans", "Alpha Serif"] { s.desktop(Method::POST, "/me/events", &token, Some(request(f, None))).await; }
    let fonts = |v: Value| v["items"].as_array().unwrap().iter().map(|i| i["font_name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(fonts(s.admin(Method::GET, "/requests?sort=font", None).await.json()), ["Alpha Serif", "Zeta Sans"]);
    assert_eq!(fonts(s.admin(Method::GET, "/requests", None).await.json()), ["Alpha Serif", "Zeta Sans"]); // newest first = last posted
    assert_eq!(fonts(s.admin(Method::GET, "/requests?sort=at&dir=asc", None).await.json()), ["Zeta Sans", "Alpha Serif"]);
}

#[tokio::test]
async fn overview_trend_has_thirty_utc_days() {
    let s = Server::new().await;
    let o = s.admin(Method::GET, "/overview", None).await.json();
    let t = o["trend"].as_array().unwrap();
    assert_eq!(t.len(), 30);
    assert_eq!(t[29]["day"], Utc::now().date_naive().to_string());
    assert!(t.iter().all(|d| d["activity"] == 0 && d["failures"] == 0 && d["new_installs"] == 0));
    let token = s.register("a", "Ana").await;
    s.desktop(Method::POST, "/me/events", &token, Some(event(json!({ "kind": "activation_failed", "font": font("h1"), "reason": "broken" })))).await;
    let today = &s.admin(Method::GET, "/overview", None).await.json()["trend"][29];
    assert_eq!((today["new_installs"].clone(), today["failures"].clone()), (json!(1), json!(1)));
    assert!(today["activity"].as_i64().unwrap() >= 1);
    s.admin(Method::DELETE, "/installs/a", None).await;
    let today = &s.admin(Method::GET, "/overview", None).await.json()["trend"][29];
    assert_eq!((today["new_installs"].clone(), today["activity"].clone()), (json!(0), json!(0)));
}
```

`request()` (line ~801) wraps a `font_requested` event, posted to `/me/events`. The test registers 4 installs, so `/installs` assertions after the `d` registration would change; keep them before it, as written.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p typeshelf-server --test api lists_sort_on_the_server overview_trend_has_thirty_utc_days`
Expected: FAIL (`invalid_sort` missing, `trend` is null).

- [ ] **Step 3: Implement sorting**

Add `sort: Option<String>, dir: Option<String>` to each `ListQuery`. Add one shared helper in `apps/server/src/lib.rs`: `pub fn sort_dir(sort: Option<&str>, dir: Option<&str>, allowed: &[(&str, bool /* desc by default */)]) -> Result<Option<(String, bool /* desc */)>, ApiError>`. It returns `Ok(None)` when `sort` is `None`, and `400 invalid_sort` for an unknown sort or a `dir` other than asc/desc.
- **Installs:** after filtering, call `sort_by` on the Vec: name (lowercase), `last_seen` string, `active_fonts`, each followed by a lowercase-name tie-break. Then `paging.slice`.
- **Library:** map the key to a fixed `ORDER BY`: `family COLLATE NOCASE {d}, style, hash`, `uploaded_at {d}, family COLLATE NOCASE, hash`, `size {d}, family COLLATE NOCASE, hash`. Interpolate only these constants.
- **Requests:** `r.at {d}, r.id {d}` and `r.font_name COLLATE NOCASE {d}, r.at DESC, r.id DESC`.

- [ ] **Step 4: Implement `trend`** in `overview`

Run two `GROUP BY substr(x.at, 1, 10)` queries over the last 30 UTC days (`since` = midnight of today − 29 days), reusing the `live` join.
- First query: `COUNT(*)` plus `SUM(x.kind = 'failure')` from `events`.
- Second query: `COUNT(*)` from `installs` where `merged_into IS NULL AND revoked_at IS NULL AND first_seen >= ?1`, grouped by `substr(first_seen,1,10)`.

Then zero-fill into 30 entries. Check `events.at` and `installs.first_seen` are stored as RFC 3339 UTC strings (see `db::ts`); if not, adjust the `substr`.

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test --workspace` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 6: Update `docs/api.md`** rows for `/overview` (add `trend`), `/installs`, `/library` and `/requests` (add `sort`/`dir` with defaults and `invalid_sort`).

- [ ] **Step 7: Client types and mock**

In `api.ts`, add the types and parameters from Interfaces. In `mock.ts`:
- `paged()` gains an optional `cmp` argument that sorts before slicing.
- `/installs`, `/library` and `/requests` read `sort`/`dir` and map them to comparators.
- `overview.trend` holds 30 generated days (deterministic, for example `activity: 8 + ((i * 7) % 13)`, `failures: i % 6 === 0 ? 2 : 0`, `new_installs: i % 5 === 0 ? 1 : 0`, dates from today − 29).

Run: `cd apps/admin-ui && bunx tsc --noEmit -p tsconfig.app.json`. Expected: clean. (Pages compile unchanged, since the new arguments are optional.)

- [ ] **Step 8: Commit**

```bash
git add apps/server apps/admin-ui/src/api.ts apps/admin-ui/src/mock.ts docs/api.md
git commit -m "Server-side sorting for installs, library and requests; 30-day overview trend"
```

### Task 2: Toasts with undo

**Files:**
- Create: `apps/admin-ui/src/toast.tsx`, `apps/admin-ui/src/toast.css`
- Modify: `apps/admin-ui/src/main.tsx` (mount `<Toaster/>` in place of `<RejectionToast/>`), `apps/admin-ui/src/ErrorBoundary.tsx` (delete `RejectionToast`)
- Modify: action handlers in `pages/Library.tsx`, `pages/Requests.tsx`, `pages/UserDetail.tsx`, `pages/Organize.tsx`, `pages/Merge.tsx`

**Interfaces:**
- Produces: `export function toast(text: string, opts?: { tone?: 'ok' | 'bad'; undo?: () => Promise<unknown> }): void` and `export function Toaster(): JSX.Element` from `toast.tsx`.
- Tasks 4–6 may call `toast()`.

- [ ] **Step 1: Implement `toast.tsx`**

- A module-level array plus a listener `Set`, read through `useSyncExternalStore`. At most 3 toasts are visible; the oldest is dropped.
- Auto-dismiss after 5000 ms, or 8000 ms with `undo`. The timer pauses while the pointer is over the toast.
- The container is `<div aria-live="polite" class="toasts">`, fixed bottom-right (bottom-centre under 700px). Each toast springs in with `AnimatePresence`; skip the motion under reduced motion.
- **Undo:** awaits `opts.undo()`, then shows `toast('Undone.')`. On a throw, shows `toast(errorText(e), { tone: 'bad' })`.
- **`bad` toasts** use `role="alert"`.
- **Rejections:** `Toaster` also listens for `unhandledrejection` with the dedupe `Set` copied from `RejectionToast` (keep its ponytail comment) and shows `toast('Something didn’t work: ' + msg, { tone: 'bad' })`.
- Toasts never take focus.

Style `toast.css` with tokens only (`--ink` background, `--on-ink` text, `--toast-link` for the Undo button). Those tokens already flip in dark mode.

- [ ] **Step 2: Wire the actions** (keep the existing inline `ErrorBox` errors and add toasts on success)
- **Library:** replace the `notice` state and its `<span role="status">` with `toast(...)`, keeping the same texts.
  - Block/unblock: `toast('Blocked X.' / 'Unblocked X.', { undo: () => api.block(hash, !blocked).then(replace) })`.
  - License change: `toast('License set to …')`.
  - Deliver: `toast('Delivering X to everyone / N users.')`.
  - Bulk and upload keep their current texts.
- **Requests:** `toast('Marked “Font” done.' / 'Reopened “Font”.', { undo: () => api.setRequestDone(id, !done).then(() => { list.reload(true); dispatchEvent(new Event('requests-changed')) }) })`.
- **UserDetail:**
  - Font toggle: `toast('Turning X on/off on <first name>’s computer.', { undo: () => set(hash, !active) })`.
  - Reset name limit, request organize and remove: success toasts. Remove navigates away, so its toast must come from the store, which survives route changes.
- **Organize apply:** `toast('Applied N changes.')`. **Merge:** `toast('Merged into <name>.')` before navigating.

- [ ] **Step 3: Verify in the preview** (`admin-ui`, `?mock=1`, 1440×900)

- Block Bebas Neue → toast appears. Undo → it shows "Blocked" again and "Undone." appears.
- Mark a request done → Undo brings it back.
- Run `Promise.reject(new Error('x'))` from the console → one bad toast.
- Take a screenshot.

- [ ] **Step 4: tsc + lint** (commands in Global Constraints). Expected: clean, ≤ 19.

- [ ] **Step 5: Commit** `git add apps/admin-ui/src && git commit -m "Admin toasts with undo"`

### Task 3: Dark mode, branding, phone layout

**Files:**
- Create: `apps/admin-ui/src/theme.ts`
- Modify: `apps/admin-ui/src/app.css` (hard-coded colours → tokens; brand mixes; ≤700px top bar), `apps/admin-ui/src/ui.tsx` (`Sidebar`, `Layout` only), `apps/admin-ui/src/main.tsx` (call `initTheme()` before render), `apps/admin-ui/src/pages/Settings.tsx` (after a successful save, `dispatchEvent(new Event('brand-changed'))`)
- Do NOT edit `web/shared/tokens.css` (the desktop app uses it too).

**Interfaces:**
- Produces (`theme.ts`): `export type ThemePref = 'system' | 'light' | 'dark'`; `initTheme(): void`; `getThemePref(): ThemePref`; `setThemePref(p: ThemePref): void` (stored in `localStorage['admin-theme']` inside try/catch; sets `document.documentElement.dataset.theme` to the resolved `light`/`dark`; follows `matchMedia('(prefers-color-scheme: dark)')` changes while on `system`); `applyBrand(color: string): void` (sets `--brand` and `--on-accent` on `<html>`, or removes both when `color` is `''`); `onAccent(hex: string): '#000' | '#fff'` (whichever has the higher WCAG contrast).
- Produces (DOM): `.main` stays the scroll container. Task 4 mounts its palette inside `Layout`.

- [ ] **Step 1: Write the contrast check.** Add an exported `demo()`-style assertion block guarded by `import.meta.env.DEV` in `theme.ts`, or a `console.assert` run once in dev:

```ts
console.assert(onAccent('#ffff00') === '#000' && onAccent('#2547d0') === '#fff' && onAccent('#777777') === '#000')
```

- [ ] **Step 2: Implement `theme.ts`** per Interfaces (WCAG relative luminance; contrast = (L1 + 0.05) / (L2 + 0.05)).

- [ ] **Step 3: CSS.** In `app.css`, replace hard-coded `#fff` on `.btn-primary`, `.chip[aria-pressed]`, `.pager-num[aria-current]`, `.toggle-knob` and the shimmer with `var(--on-accent)`, `var(--on-ink)`, `var(--knob)` and `var(--shimmer)`. Replace `rgb(28 25 23 / …)` shadows with `rgb(var(--shadow-rgb) / …)`. Add `:root[data-theme="dark"]` values for the `app.css`-local tokens (`--stone-100`, `--track`, `--indigo-200`, `--bad-*`, `--accent-wash`). Then add:

```css
:root[style*="--brand"] { --accent: var(--brand); --accent-hover: color-mix(in oklab, var(--brand) 82%, black);
  --accent-ink: color-mix(in oklab, var(--brand) 85%, black); --accent-soft: color-mix(in oklab, var(--brand) 12%, white);
  --accent-soft-2: color-mix(in oklab, var(--brand) 8%, white); --accent-line: color-mix(in oklab, var(--brand) 30%, white); }
:root[data-theme="dark"][style*="--brand"] { --accent: color-mix(in oklab, var(--brand) 65%, white); --accent-hover: color-mix(in oklab, var(--brand) 50%, white);
  --accent-ink: color-mix(in oklab, var(--brand) 55%, white); --accent-soft: color-mix(in oklab, var(--brand) 25%, black);
  --accent-soft-2: color-mix(in oklab, var(--brand) 18%, black); --accent-line: color-mix(in oklab, var(--brand) 45%, black); }
```

In dark mode `--on-accent` must follow the mixed accent, so `applyBrand` computes `onAccent` against the dark-mixed colour while the theme is dark. Re-apply on theme change.

- [ ] **Step 4: Sidebar.**
- **Brand:** `useLoad(() => api.settings(), [])`, reloaded on `brand-changed`. The brand block shows `brand_logo` (an `<img alt="">`, max 28px tall) and `brand_name || 'Glyphyard'`, with sub-text "Admin". Call `applyBrand(brand_color)` when it loads.
- **Theme control:** in the account box, three small buttons (System / Light / Dark) using `Chips`.
- **≤700px:** the sidebar becomes a sticky top bar showing the brand, the open-requests count when > 0, and a "Menu" button (`aria-expanded`, `aria-controls="admin-nav"`). The nav list and account box drop down below it. Navigating closes the menu.

- [ ] **Step 5: Verify in the preview** (`?mock=1`)
- Toggle Dark → screenshots of Overview, Library and a dialog. Check that no white blocks remain.
- In Settings, set the brand colour to `#e11d48` and save → the sidebar and primary buttons turn rose.
- Try `#ffff00` → button text turns black.
- Set the viewport to mobile → top bar, menu opens and closes. Take a screenshot.
- Restore the brand colour to `''`.

- [ ] **Step 6: tsc + lint.** **Step 7: Commit** `git add apps/admin-ui/src && git commit -m "Admin dark mode, studio branding and phone layout"`

---

## Wave B (after wave A is merged into the feature branch)

### Task 4: Command palette and shortcuts

**Files:**
- Create: `apps/admin-ui/src/palette.tsx`, `apps/admin-ui/src/palette.css`
- Modify: `apps/admin-ui/src/ui.tsx` (`Layout` renders `<Palette/>`; export the `nav` list from `Sidebar` as `NAV` so both use it)

**Interfaces:**
- Consumes: `Dialog` and `useDebounced` (`ui.tsx`), `api.installs` and `api.library` (Task 1 signatures), `NAV: [to: string, label: string][]`.
- Produces: `export function Palette(): JSX.Element` and `export function isTyping(t: EventTarget | null): boolean` (true for input, textarea, select, contenteditable, or inside an open dialog).

- [ ] **Step 1: Implement**
- **Global `keydown` in `Palette`:**
  - `Ctrl/⌘+K` toggles the palette (`preventDefault`).
  - `/` when `!isTyping` focuses the first `input[type=search]` inside `.main`.
  - `?` when `!isTyping` opens the shortcuts dialog. It's a small `Dialog` that lists `Ctrl K`, `/`, `?` and `Esc`.
- **Palette dialog:**
  - A search input with `role="combobox"` and `aria-activedescendant`, over a listbox with three groups: Pages (NAV, filtered client-side), Users (up to 5), Fonts (up to 5).
  - Users and fonts are fetched 150 ms after typing stops, with only the newest request winning (reuse `useLoad`).
  - Arrow keys move the selection, Enter opens it, Esc closes.
  - A user opens `/users/:id`; a font opens `/library?q=<family>`.
  - An empty query shows Pages only. Fetch errors show a one-line notice inside the palette.
- Add a sidebar hint "Search ⌘K" (show `Ctrl K` on non-Mac) that opens the palette.

- [ ] **Step 2: Verify in the preview.**
- Ctrl+K → type "budi" → Enter → Budi's page.
- Ctrl+K → "syne" → Enter → Library filtered to Syne. This needs Task 5's `?q=` reading; if Task 5 hasn't landed yet, check the URL only.
- `/` on Users focuses search.
- `/` typed inside the search box types a slash.
- `?` opens the help dialog.
- Take a screenshot.

- [ ] **Step 3: tsc + lint. Step 4: Commit** `git add apps/admin-ui/src && git commit -m "Admin command palette and keyboard shortcuts"`

### Task 5: Sortable headers, empty states, Library `?q=`

**Files:**
- Modify: `apps/admin-ui/src/ui.tsx` (`Table`, `TableSkeleton`, `Empty`), `apps/admin-ui/src/app.css` (sort header and empty state styles), `pages/Users.tsx`, `pages/Library.tsx`, `pages/Requests.tsx`, `pages/Audit.tsx`, `pages/Names.tsx`

**Interfaces:**
- Consumes: `Sort<K>` and the sort args from Task 1.
- Produces:
  - `export function SortHead<K extends string>({ label, k, sort, onSort, defaultDir }: { label: string; k: K; sort: Sort<K>; onSort: (s: Sort<K>) => void; defaultDir: SortDir }): JSX.Element`. It renders a `<button>` with an arrow; the parent `<th>` takes `aria-sort`.
  - `Table` `head` items may be `{ node: ReactNode; sort?: 'ascending' | 'descending' | 'none' }` or a plain `ReactNode`.
  - `Empty({ title?, children, action? })`: `action` is a ReactNode, typically a button or link.

- [ ] **Step 1: Implement `SortHead` and the `Table` `aria-sort` support.** Click cycles: default dir → the other dir → `null` (server default). Sort lives in component state per page and resets the page through the existing `usePage` key (include the sort in the key).

- [ ] **Step 2: Wire the pages**
- **Users:** Name (asc), Active fonts (`fonts`, desc), Last seen (`last_seen`, desc).
- **Library:** Family (asc), plus new sortable "Added" (`uploaded`, desc) and Size (desc) header buttons. Size is shown in the meta line today; add an "Added" column showing `fmtDate(uploaded_at)` only if it fits `minWidth`, otherwise put the sort buttons in the toolbar as `Chips` with label "Sort").
- **Requests:** Asked (`at`, desc), Font (`font`, asc).

- [ ] **Step 3: Library reads its initial `q`** with `useSearchParams().get('q') ?? ''`, so the palette's link works.

- [ ] **Step 4: Empty states**
- **Users, no installs at all:** title "No computers yet", text "Install Glyphyard on a computer and enter this server's address and install key.", action `<Link className="btn btn-primary" to="/settings">Show install key</Link>`.
- **Library empty:** "Your shared library is empty", "Upload the fonts your team should have.", action that triggers the existing upload input.
- **Requests open empty:** "No open requests", "When someone asks for a font in the desktop app, it shows up here."
- **Audit and Names:** a title only.
- Filtered empties keep the current one-liners.

- [ ] **Step 5: Verify** in mock:
- Sort each column both ways and back; page 2 keeps the order; `aria-sort` is right in `read_page`.
- Use `?mock=1` with the library emptied (delete all three rows) for the empty state.
- Take screenshots.

- [ ] **Step 6: tsc + lint. Step 7: Commit** `git add apps/admin-ui/src && git commit -m "Sortable admin tables and better empty states"`

### Task 6: Overview trend chart

**Files:**
- Modify: `apps/admin-ui/src/pages/Overview.tsx` (add `TrendChart`; skeleton gains a chart block; the activity `Empty` gets a title)
- Create: `apps/admin-ui/src/trend.css`

**Interfaces:**
- Consumes: `Overview.trend: TrendDay[]` (Task 1).
- Produces: `function TrendChart({ days }: { days: TrendDay[] }): JSX.Element` (local to `Overview.tsx`).

- [ ] **Step 1: Load the `dataviz` skill**, then implement. Use a card titled "Last 30 days" placed between the stats and the split.
- An inline SVG of 30 bars for `activity` (`--accent`), with the `failures` portion stacked in `--bad`. New installs appear as small dots above the bars (`--ok-dot`). The height scale is `max(...activity, 1)`.
- An x-axis with 3 labels (first, middle, today); a legend of 3 swatches with totals.
- On hover or focus, each bar shows a tooltip (`<title>` plus a visible label) such as "26 Nov: 12 events, 2 failures, 1 new computer".
- Bars grow from zero on mount (skip under reduced motion) and update on polls without remounting.
- Below 700px the bars keep 30 columns with a smaller gap.

- [ ] **Step 2: Verify** in mock: the chart renders in light and dark; hovering a bar shows its numbers; a zero-data trend (temporarily set the mock to zeros) shows a flat baseline. Take a screenshot.

- [ ] **Step 3: tsc + lint. Step 4: Commit** `git add apps/admin-ui/src && git commit -m "Overview 30-day trend chart"`

---

## Task 7: Final verification (after all tasks are merged)

- [ ] `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`. Both pass.
- [ ] `cd apps/admin-ui && bunx tsc --noEmit -p tsconfig.app.json` is clean. `bunx oxlint` reports ≤ 19.
- [ ] **Real-API shape:** run a local server against a scratch data dir:
  - `cargo run -p typeshelf-server -- serve --data <scratchpad>/data` on port 8080 (check `--help` for the exact subcommand and how to create the first admin).
  - Register 3 desktop installs with `curl` using the install key, without printing it (read it into a shell variable from the DB).
  - Open the `admin-ui` preview without `?mock` (`?mock=0`) and sign in.
  - Check sorting, the trend chart, toasts with undo, the palette, dark mode, branding and phone layout.
  - Take screenshots. Stop the server.
- [ ] Mock pass: the same checks with `?mock=1`, one screenshot per feature.
