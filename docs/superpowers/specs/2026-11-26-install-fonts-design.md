# Install fonts card: families, filters, batch actions — design

## Why

On an install's detail page (`apps/admin-ui/src/pages/UserDetail.tsx`, `Fonts`), the "Active fonts · 669" card is one long flat list. It pages 50 rows at a time and offers one search box and one on/off toggle per font. Admins can't act on many fonts at once, can't narrow the list down, and can't see the family structure. The user asked for "batch activity, filter, and anything else that makes the admin's job easier." They chose **group by family** and **server-side grouping**, so the page stays fast with thousands of fonts while it polls every 10 s.

## Goals

1. **Family list.** One row per family: name rendered in its own font, "18 styles · 12 on", a one-line source summary, and a family-level on/off toggle. A family is on when all of its styles are on. A partly-on family shows a mixed state, and clicking it turns all of its styles on. Expanding a row loads that family's styles, each with its own toggle, as today.
2. **Filters.**
   - State: All / On / Off.
   - Source: Any / Turned on by user / by admin / Shared library / Project set.
   - "User overrides only".
   - The existing search box.
   - A live count: "42 families · 310 fonts". Filters apply to both families and styles: a family appears when any of its styles match, and its counts cover the matching styles.
3. **Sort.** A–Z (default) or Recently changed (newest `updated_at` among the family's matching styles).
4. **Batch selection.**
   - A checkbox on each family row and each expanded style row; shift-click selects a range of family rows.
   - "Select all N matching" selects everything the current filters match, across pages.
   - A selection bar shows "N families, M fonts selected" (or "All 310 matching fonts") with three actions:
     - **Turn on** and **Turn off**.
     - **Block everywhere**. It acts only on selected fonts that are in the Shared library; the others are skipped and counted.
     - Block asks for confirmation, because it affects every computer.
5. **Feedback.** One toast per batch, such as "Turning off 37 fonts on Yongky's computer · 2 skipped (blocked)", with **Undo**. Undo reverses exactly the fonts that were changed. Fonts waiting for the computer show "Waiting for this computer", and family rows show "3 waiting".
6. **Style rows show when** a font last changed: "Turned on by Yongky · 3 days ago".

## Non-goals

- No font categories (serif, sans and so on): the server has no such data.
- No blocking of fonts that aren't in the Shared library: the server doesn't have their files.
- No changes to the desktop app or the command protocol. A batch queues the same Activate and Deactivate commands as single toggles.

## Anti-goals

- No breaking change to existing endpoints. `GET /installs/{id}/fonts` keeps today's behaviour when the new parameters are absent.
- A batch must not flood the audit log with hundreds of rows. It writes one entry per batch.
- A blocked shared-library font must never be turned on by a batch. It is skipped and counted, which mirrors the single toggle's 409.

## Constraints

- Same rules as the admin-ux pass:
  - no new dependencies;
  - tsc clean, oxlint ≤ 19;
  - `cargo test --workspace` and clippy `-D warnings` pass;
  - `?mock=1` (`src/mock.ts`) supports everything;
  - `docs/api.md` is updated;
  - dark mode uses tokens;
  - reduced motion is respected;
  - use `toast()` and `useConfirm()` from the existing code.
- Merged installs: everything covers the install and every install merged into it, using the existing `GROUP` CTE and "newest report wins" logic in `fonts()`.

## Approaches considered

- **Server-side grouping (chosen)** versus **loading every font into the browser**. Loading everything is less code, but the full list is re-downloaded on every 10 s poll, which gets heavy at thousands of fonts. The user picked server-side.
- **Batch transport.** Looping the existing single `POST /installs/{id}/fonts` from the browser means N requests and N audit rows. **Chosen: one new batch endpoint** that resolves the selection on the server and returns the hashes it changed, so Undo can reverse exactly those.
- **Selection model.** A list of hashes can't cover collapsed families whose styles were never loaded. **Chosen:** a selection is `{ families: string[], hashes: string[] }` or `{ all: true }`. Either way the current filters are applied on the server, so selecting "Inter" while filtered to *On* turns off only the Inter styles that are on.

## API (decided)

All new parameters are optional.
- `GET /installs/{id}/families?q=&active=&source=user|admin|library|project_set&overridden=true&sort=name|recent&offset=&limit=`
  - Returns `Page<FamilyRow> & { fonts: number }`, where `fonts` is the number of matching styles across all pages.
  - `FamilyRow = { family, styles, on, sources: FontSource[], overridden: number, in_library: number, updated_at }`. `styles`, `on`, `overridden` and `in_library` count the matching styles. `sources` lists the distinct sources. `updated_at` is the newest among them.
  - Default order is `family COLLATE NOCASE`. `sort=recent` orders by `updated_at DESC`, then family.
  - An unknown `sort` or `source` returns 400 `invalid_sort` / `invalid_source`.
- `GET /installs/{id}/fonts` gains `family=` (exact match), `source=` and `overridden=true`. `FontState` gains `updated_at`, `in_library` (bool) and `blocked` (bool, false when not in the library).
- `POST /installs/{id}/fonts/bulk`
  - Body: `{ action: 'on'|'off'|'block', families?: string[], hashes?: string[], all?: true, q?, active?, source?, overridden? }`. Exactly one of `families`/`hashes`, or `all`, must be present; otherwise 400 `invalid_selection`.
  - The server resolves the matching hashes in the install group.
    - `on`/`off`: queues Activate or Deactivate for each font not already in that state. `on` skips blocked library fonts.
    - `block`: calls the existing `set_blocked` for matching hashes that are in the library and not yet blocked, and skips the rest.
  - Returns `{ changed: string[], skipped: number }`. Writes one audit row: `fonts_bulk_on` / `fonts_bulk_off` / `fonts_bulk_block`, with target the install id and detail "N fonts".
  - Undo uses `hashes: changed` with the opposite action. Block's Undo uses the existing `POST /library/bulk` `unblock` with those hashes.

## Calls made after review (builder's)

- **Source.** `source` keeps today's meaning and labels (`sourceText` in UserDetail.tsx): user means "Turned on by <name>", admin means "Turned on by admin", plus "Shared library" and "Project set". The filter uses those same labels. No schema change. "When" comes from `font_states.updated_at` (newest report).
- **On / waiting.** A font is on with the same rule as `fonts()`: `MAX(active)` per hash across the merged group, newest report for every other field. Family counts use that same per-hash row. "Waiting" stays client-side, as today: the browser's pending map, cleared when a poll confirms the state or after 120 s.
- **Family key.** Grouping and `family=` matching use `family COLLATE NOCASE`, so "Inter" and "inter" are one family. The row shows the newest report's spelling.
- **Size and atomicity.** A batch runs in one transaction with no cap: per-font command rows are what single toggles already create, and SQLite handles thousands of rows per transaction. Hub pushes happen as `hub::queue` does today, inside the batch (a push that races a rollback is harmless: the desktop re-syncs from its pending commands).
- **Block undo** uses the existing `set_blocked(false)` through `/library/bulk` unblock. It already re-blocks namesake files, so nothing outside the batch changes.
- **Undo after something changed.** Undo is a normal batch with `hashes` and goes through the same skips: a font that was blocked in the meantime is skipped, and the toast says so.
- **`families` selection** means every style of that family that matches the current filters, whether or not it was ever expanded in the browser.

## Left to the builder

Layout details, row heights, how the mixed toggle state looks, the selection bar's position (sticky at the card bottom is fine), and the skeleton. Expanded families keep their open state across polls and pages. Collapsed rows don't fetch their styles.
