# Admin dashboard UI/UX pass — design

## Why

The admin dashboard (`apps/admin-ui`) works, and every Admin API endpoint already has a screen. It still feels plain: actions give no confirmation, there is no dark mode or phone layout, the studio's own branding never shows up in the dashboard, tables can't be sorted, there are no shortcuts, and the Overview shows no history. The user approved all eight improvements below.

## Goals

1. **Toasts.** Every action that changes something shows a short success or failure notice. Actions that can be undone (block, unblock, mark request done/reopen, font on/off on a computer) include an **Undo** button.
2. **Dark mode.** Follows the OS by default. A sidebar control switches between System, Light and Dark, and the choice is remembered in `localStorage`.
3. **Phone layout.** At 700px and narrower, the sidebar turns into a top bar (brand, open-request count, menu button) and the nav opens as a drop-down panel.
4. **Branded dashboard.** Settings → `brand_name`, `brand_logo` and `brand_color` are used in the sidebar brand block and as the accent colour. Empty values keep Glyphyard's defaults.
5. **Command palette and shortcuts.** `Ctrl/⌘+K` opens a palette that searches pages, users (via `/installs?q=`) and library fonts (via `/library?q=`), with arrow keys and Enter. `/` focuses the page's search box. A `?` key shows the shortcut list.
6. **Better empty states.** `Empty` gains an optional title, explanation and action button. It is used on Users (no installs yet → link to Settings for the install key), Library (→ Upload fonts), Requests, Overview activity and Audit.
7. **Sortable columns.** Users (name, last seen, active fonts), Library (family, uploaded, size) and Requests (asked, font). Clicking a header toggles the order; `aria-sort` marks the sorted column. The sort is sent to the server as `sort` and `dir` query parameters.
8. **Overview trends.** A 30-day chart of daily activity events, failures and new installs, shown above "Recent activity".

## Non-goals

- No new npm dependencies. Toasts, the palette and the chart are hand-written with the existing `motion` library.
- No sorting on Name history, Audit log or a user's font list (not asked for).
- No history of "online" counts. The server never stored it, so trends use what the database already has: `events.at`, `events.kind` and `installs.first_seen`.
- No rename of internal `typeshelf` names. No redeploy.

## Anti-goals

- Changing the server's default list orders. With no `sort` given, every list must return exactly what it returns today (older desktop or admin builds rely on it).
- Brand colours that make button text unreadable. Text on the accent picks black or white by contrast.
- Toasts that steal focus or hide the error text that pages already show inline.

## Constraints

- `bunx tsc --noEmit -p tsconfig.app.json` is clean. `oxlint` warnings stay at the baseline of 19 or fewer.
- `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass. New server behaviour gets tests in `apps/server/tests/api.rs`.
- `?mock=1` (`src/mock.ts`) supports the new params and the `trend` field, so every feature can be checked without a server.
- `docs/api.md` documents the new params and field.
- Respect `prefers-reduced-motion`, as the existing motion code does.

## Approaches considered

- **Sorting: client-side vs server-side.** Sorting only the visible page of 50 rows would mislead on Users and Library once they pass 50 entries. **Chosen: server-side**, using a whitelist of column names mapped to fixed `ORDER BY` clauses (no SQL built from input). Users is sorted in Rust, because its list is already built in memory.
- **Trends: new endpoint vs extra field.** A separate `/overview/trend` endpoint means a second request and its own loading state. **Chosen: a `trend` array on `/overview`**: 30 rows of `{ day, activity, failures, new_installs }`. It's one indexed `GROUP BY` on `events_at` plus one on `installs`, so the 10-second poll stays cheap.
- **Dark mode: new palette vs existing one.** `web/shared/tokens.css` already has `:root[data-theme="dark"]` for the desktop app. **Chosen: reuse it.** The admin UI sets `data-theme` on `<html>`, and the few hard-coded colours in `app.css` (`#fff` on buttons and chips, shadows, the shimmer, `--bad-*` washes) move to tokens.
- **Brand accent: derived colours.** **Chosen: CSS `color-mix()`** derives hover, soft and line shades from one `--brand` variable, with separate light and dark mixes. JS only sets `--brand` and `--on-accent`.

## Decided details

- Toast API: `toast(text, { tone?: 'ok' | 'bad', undo?: () => Promise<void> })`, a module-level store plus a `<Toaster/>` mounted once in `main.tsx`. Notices sit bottom-right and fade out after 5 s (8 s when there's an Undo). They live in an `aria-live="polite"` region. The existing `RejectionToast` routes through the same store.
- Theme toggle and brand load in the sidebar. Brand data comes from `GET /settings`, which the dashboard already calls.
- Sort params: `sort=<column>&dir=asc|desc`. Unknown values return `400 invalid_sort`, the same shape as the existing `invalid_filter`.

## Calls made after review (builder's)

- **Sort contract.** Users: `sort=name|last_seen|fonts`. Library: `family|uploaded|size`. Requests: `at|font`. `dir=asc|desc`. With no `dir`, text columns sort ascending and dates or numbers sort descending; the first click on a header uses that default. `dir` without `sort` is ignored. Ties fall back to today's order as the secondary key.
- **Trend meaning.** Each `trend` row counts, for one UTC day: `activity`, every `events` row whose person is not removed (the same join as `failures_week`); `failures`, the subset with `kind = 'failure'`; and `new_installs`, primary installs (`merged_into IS NULL`, not revoked) by `first_seen`. The array always has exactly 30 entries, today included, oldest first, with empty days filled as zero.
- **Undo** is the plain inverse call: unblock or block, reopen or done, and the opposite `setFont` for a font on a computer. No server change. A font on/off undo shows as two timeline entries, which is honest about what was sent.
- **Palette.** Searches run 150 ms after typing stops, with up to 5 users and 5 fonts. Enter on a user opens `/users/:id`. Enter on a font opens `/library?q=<family>`, so the Library reads its initial search from the URL.
- **Dark mode accent:** `--accent` is `color-mix(brand 65%, white)`, which keeps dark brand colours readable.

## Left to the builder

Exact toast wording, chart styling (follow the dataviz guidance; bars or area), palette ranking, icon choices, and which secondary actions get toasts beyond the ones listed in goal 1.
