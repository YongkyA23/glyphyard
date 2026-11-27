# One person, several computers: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:**
- One name and one name limit per person, kept live on all of their computers.
- An admin can remove a single computer of a person.
- Collections and project sets sync across a person's computers.

**Architecture:**
- **Already on the base branch `feat/person-devices`:**
  - `ServerMessage::Identity` and `ServerMessage::OrganizerChanged`, both WebSocket-only.
  - `Hub::notify` and `hub::notify_person`.
  - `db::person` and `db::person_devices`.
  - The server socket loop forwards the non-command messages.
- **Order:** Task 1 (identity and per-computer removal) and Task 2 (organizer core and server) run in parallel. Task 3 (organizer on the desktop) starts after both are merged.

**Tech Stack:** Rust (Axum, rusqlite, Tauri 2), React 19, TypeScript 6, bun.

**Spec:** `docs/superpowers/specs/2026-11-27-person-devices-design.md`

## Global Constraints

- **Dependencies:** no new crates and no new npm packages.
- **Rust:**
  - `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
  - In a fresh worktree, first copy the gitignored `apps/desktop/ui/dist` and `plugins/dist` from `C:/Users/lenovo/Documents/GitHub/typeshelf`.
- **UIs:**
  - tsc is clean.
  - Desktop oxlint `grep -c warning` stays ≤ 53, admin ≤ 19.
  - The i18n checker reports 0 missing and 0 other problems: desktop `cd apps/desktop/ui && bun ../../../scripts/i18n-check.ts src`, admin `cd apps/admin-ui && bun ../../scripts/i18n-check.ts src`.
  - Every new visible string goes through `t()` (English is the key) and gets Indonesian in an area file under `src/i18n/id/`. Voice is casual-polite "kamu". Terms: komputer, pengguna, koleksi, set proyek, hapus.
  - Mock mode supports every new feature: admin `?mock=1` and desktop `bun run dev`.
- **Compatibility:**
  - Old desktops must keep working: no new `CommandKind`, and the new messages are WebSocket-only.
  - New desktops must work against old servers: a 404 from `/me/organizer` turns sync off silently.
- **Docs:** update `docs/api.md` (server) and `docs/desktop-commands.md` (desktop) for every changed contract.
- **Git:**
  - Never stage `.claude/`, `.superpowers/` or `docs/deploy.local.md`.
  - No push.
  - Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - Each worktree starts with `git merge --ff-only feat/person-devices`.
- **Browser pane:** it is shared with other agents. Open your own tab and run your own dev-server port.

## Review Focus

1. **A merged computer renames itself:** the admin sees the new name on the person, the laptop's app updates live, and the shared limit blocks a second change from either computer. Covered by `merged_computer_renames_the_person` in Task 1.
2. **Removing the person's own install while a Mac remains:** the Mac becomes the person, keeps the person's name, and the history export still includes the removed laptop's rows. Covered by `removing_the_primary_promotes_the_next` in Task 1.
3. **Concurrent edits on two computers:** a family added to "Client X" on the laptop and another added on the Mac both survive, and a collection deleted on one stays deleted. Covered by `merge_is_commutative_and_keeps_concurrent_adds` in Task 2, and by Task 3's engine round-trip.
4. **A stale push:** a `PUT` with an old `base_rev` returns 409 with the current document, the desktop merges and retries once, and nothing is lost. Covered by `stale_put_conflicts` in Task 2 and `push_retries_after_conflict` in Task 3.
5. **Re-registration after revoke:** a computer that registers again under a revoked id never gets the old person's synced document. Covered by `revoked_id_does_not_leak_organizer` in Task 2.

---

### Task 1: One name per person, and removing one computer (parallel with Task 2)

**Files:**
- **Server:**
  - Modify `apps/server/src/db.rs`: `next_name_change` and `install_name` resolve the person.
  - Modify `apps/server/src/desktop.rs`: `change_name`, `me`, and the register token-refresh path.
  - Modify `apps/server/src/installs.rs`:
    - `merge` and `split` notify Identity.
    - The admin name-limit reset notifies Identity.
    - New `remove_computer` handler and route.
    - `InstallDetail.computers`.
  - Test: `apps/server/tests/api.rs`
- **Client and desktop:**
  - Modify `crates/sync/src/lib.rs`: forward non-command `ServerMessage`s to a notices channel.
  - Modify `apps/desktop/src-tauri/src/sync.rs`: handle `Identity` the way `on_connect` handles `/me`, then emit `name-changed`.
  - Test: `crates/sync/tests/client.rs`
- **Admin UI:**
  - Modify `apps/admin-ui/src/api.ts`, `src/mock.ts` and `src/pages/UserDetail.tsx`.
  - Add Indonesian to `src/i18n/id/installs.ts`.
- **Docs:** `docs/api.md` and `docs/desktop-commands.md`.

**Interfaces (produces):**
- **Server person rules:**
  - `db::install_name(c, id)` returns the person's name.
  - `db::next_name_change(c, id, days)` uses `GROUP` of the person.
  - `change_name` updates the person's row, inserts `name_changes(install_id = the calling computer)`, then calls `hub::notify_person(.., except: Some(caller), Identity{..})` after commit.
- **`DELETE /installs/{person}/computers/{computer}`** returns `200 { "person": string }`.
  - 404 if `computer` is not in the person's group, or either id is revoked.
  - It follows the spec §2 rules exactly.
  - It records the audit action `computer_removed`.
  - It kicks the removed computer's socket.
- **`InstallDetail`** gains `computers: { id: string; os: string; app_version: string; first_seen: string; last_seen: string; online: boolean; merged_at: string | null }[]`, with the person first.
- **Sync crate:** `SyncClient::notices(&self) -> tokio::sync::mpsc::UnboundedReceiver<ServerMessage>`.
  - It may be called once. Later calls return a receiver that never yields, or make it explicit with `Option`; document the choice.
  - Every received non-`Command` message is forwarded to it, and dropped if nobody subscribed.
- **Desktop:** a task in `sync.rs` reads the notices.
  - `Identity` → `engine.set_identity(name, next)`, then emit `name-changed` with the same payload `on_connect` uses.
  - `OrganizerChanged` → no-op for now (Task 3 wires it).

- [ ] **Step 1: Write the failing server tests** in `api.rs`, using the existing helpers:
  - `merged_computer_renames_the_person`:
    - Register A, then B, merge B into A keeping A's name, then call `PUT /me/name` with B's token.
    - `GET /installs/{A}` shows the new name.
    - `GET /me` with A's token shows the new name, and its `next_name_change` is set.
    - A second `PUT /me/name` from A gets 409 `name_locked`.
    - A WebSocket open on A receives an `identity` message with the new name.
  - `merge_tells_the_merged_computer_its_name`: B's socket receives `identity` with A's name after the merge.
  - `removing_a_merged_computer_keeps_the_person`:
    - `DELETE /installs/{A}/computers/{B}` → `200 {person: A}`.
    - B's token gets 401 `revoked`.
    - A is still listed.
  - `removing_the_primary_promotes_the_next`:
    - A, B and C are merged into A, which keeps the name "Budi".
    - `DELETE /installs/{A}/computers/{A}` → `{person: B}`. B is listed as "Budi" with C in its `computers`.
    - A's token gets 401.
    - The name-history export still lists A's rows under the person.
  - `remove_computer_validates`: an unrelated computer → 404.
- [ ] **Step 2:** Run `cargo test -p typeshelf-server --test api merged_computer merge_tells removing remove_computer` → FAIL.
- [ ] **Step 3:** Implement the server side.
- [ ] **Step 4:** Implement the sync crate notices, with a test in `crates/sync/tests/client.rs` that a fake server's `identity` message arrives on `notices()`. Then wire the desktop `sync.rs` handler.
- [ ] **Step 5:** Admin UI: the Computers card, per-computer remove with confirm, navigating to the returned person id, the relabelled whole-person delete, and api/mock support. Translate everything into Indonesian.
- [ ] **Step 6:** Run the full checks, both docs files, and a browser check with admin `?mock=1` (remove a merged computer, remove the primary). Then commit `Person identity follows every computer; remove one computer`.

### Task 2: Organizer document, merge and server endpoints (parallel with Task 1)

**Files:**
- **Core:** create `crates/core/src/organizer.rs` and export it in `crates/core/src/lib.rs`. It contains its tests.
- **Server:**
  - Create `apps/server/migrations/0006_organizer.sql`, registered the way the other migrations are.
  - Create `apps/server/src/organizer.rs`, with its routes in `desktop.rs`'s router. Touch `desktop.rs` only for the route lines.
  - Modify `apps/server/src/installs.rs`: only the `free_revoked_id` table list gets `"organizer_docs"`.
  - Test: `apps/server/tests/api.rs`, appended at the end of the file.
- **Docs:** `docs/api.md`.

**Interfaces (produces):**
- **`typeshelf_core::organizer` types** (serde, `rename_all = "snake_case"`, timestamps as RFC 3339 `String`s so the type stays chrono-free if core is):
  - `OrganizerDoc { collections: Vec<SyncCollection>, sets: Vec<SyncSet> }`
  - `SyncCollection { uid: String, name: String, parent: Option<String>, updated_at: String, deleted_at: Option<String>, items: Vec<SyncItem> }`. The Favorites collection has `uid == "favorites"`, and its name and parent are ignored on apply.
  - `SyncSet { uid: String, name: String, note: String, updated_at: String, deleted_at: Option<String>, items: Vec<SyncItem> }`
  - `SyncItem { key: String, at: String, removed: bool }`
- **`merge(a: &OrganizerDoc, b: &OrganizerDoc) -> OrganizerDoc`** is pure, deterministic, commutative, associative and idempotent.
  - Per uid, the newer `updated_at` wins the item's own fields. A tie goes to the lexicographically greater field tuple, so the result is deterministic.
  - `deleted_at` is the max of both sides. An item is deleted if `deleted_at >= updated_at`.
  - Items merge per `key`, newest `at` wins. A tie goes to `removed = true`.
  - Live items with different uids but the same kind, name and parent are collapsed into the smaller uid:
    - their families are unioned by the same per-key rule;
    - the larger uid becomes a tombstone (`deleted_at = max(updated_at)`);
    - children whose `parent` pointed at the larger uid are re-pointed.
  - Output is sorted by uid, and items by key.
- **`prune(doc: &OrganizerDoc, now: &str, keep_days: u32) -> OrganizerDoc`:** drops tombstoned items and removed families older than `keep_days`. Callers use 180.
- **Server:**
  - **Table:** `organizer_docs(install_id TEXT PRIMARY KEY, rev INTEGER NOT NULL, doc_json TEXT NOT NULL, updated_at TEXT NOT NULL)`, with no foreign key. `free_revoked_id` moves it.
  - **`GET /me/organizer`** returns `{ rev: number, doc: OrganizerDoc }`. `rev` is the person row's rev, or 0. `doc` is the merge of every row whose `install_id` is in `GROUP(person)`, including revoked members, then pruned.
  - **`PUT /me/organizer`** takes `{ base_rev: number, doc: OrganizerDoc }`.
    - Body over 2 MiB → 413 `organizer_too_large`.
    - Doc doesn't parse → 400 `invalid_organizer`.
    - `base_rev` doesn't equal the current rev → 409 `organizer_conflict`, with extra fields `rev` and `doc` holding the current merged document.
    - Otherwise:
      - Upsert the person's row with `doc = prune(merge(stored_group_doc, sent_doc))`. Merging again keeps it safe against clients that skip their merge.
      - `rev = base_rev + 1`.
      - Delete the other `GROUP(person)` rows.
      - Commit.
      - `hub::notify_person(.., except: Some(caller), OrganizerChanged{rev})`.
      - Return `{ rev, doc }`.

- [ ] **Step 1: Write the failing core tests** in `organizer.rs`:
  - `merge_is_commutative_and_keeps_concurrent_adds`: add family X on side a and family Y on side b to the same uid; check both orders are equal and contain X and Y.
  - `delete_wins_over_older_edit`.
  - `newer_edit_after_delete_revives`.
  - `same_name_siblings_collapse`: the smaller uid is kept, families are unioned, and children are re-pointed.
  - `merge_is_idempotent_and_associative`, on three docs.
  - `prune_drops_old_tombstones`.
- [ ] **Step 2:** Run `cargo test -p typeshelf-core organizer` → FAIL. Then implement → PASS.
- [ ] **Step 3: Write the failing server tests:**
  - `organizer_round_trip`.
  - `stale_put_conflicts`: 409 with the current doc.
  - `merged_group_sees_one_organizer`: A and B each PUT a collection, then merge B into A; `GET` from either returns both collections and one rev.
  - `organizer_put_notifies_other_computers`: B's socket gets `organizer_changed`.
  - `organizer_limits`: 413 and 400.
  - `revoked_id_does_not_leak_organizer`: A PUTs, A is revoked and registers again under the same id; `GET` returns an empty doc with rev 0.
- [ ] **Step 4:** Implement the server side, then run `cargo test --workspace` and clippy. Update `docs/api.md`. Commit `Organizer: shared document, merge, server endpoints`.

### Task 3: Organizer sync on the desktop (after Tasks 1 and 2 are merged)

**Files:**
- **Engine:**
  - Modify `crates/engine/src/store.rs`: a new migration step.
  - Modify `crates/engine/src/library.rs`: collection and project-set mutations, plus the family-key rename at about line 846.
  - Modify `crates/engine/src/backup.rs`: restore and import stamp their changes.
  - Create `crates/engine/src/organizer.rs`.
  - Tests go in `crates/engine/src/tests/organizer.rs`.
- **Client and desktop:**
  - Modify `crates/sync/src/lib.rs`: `get_organizer` and `put_organizer` client calls.
  - Modify `apps/desktop/src-tauri/src/sync.rs`: pull, merge and push on connect, on `OrganizerChanged`, and debounced after local changes.
  - Possibly modify `apps/desktop/src-tauri/src/commands.rs`, only if a mutation path needs a "dirty" nudge.
- **Desktop UI:** `apps/desktop/ui` gets only what's needed to refresh lists when the backend emits a change event. Reuse existing events if present.
- **Docs:** `docs/desktop-commands.md`.

**Interfaces (consumes):**
- `typeshelf_core::organizer::{OrganizerDoc, merge, prune}` from Task 2.
- `SyncClient::notices()` and `ServerMessage::OrganizerChanged` from Task 1.
- `GET` and `PUT /me/organizer` from Task 2.

**Interfaces (produces):**
- **Engine migration:**
  - `collections.uid` and `collections.updated_at`, backfilled: Favorites gets `'favorites'`, others random 128-bit hex, `updated_at` = now.
  - The same two columns on `project_sets`.
  - Table `organizer_tombstones(kind TEXT, uid TEXT, deleted_at TEXT, PRIMARY KEY(kind, uid))`, where `kind` is `'collection'` or `'set'`.
  - Table `organizer_removed(kind TEXT, owner_uid TEXT, family_key TEXT, removed_at TEXT, PRIMARY KEY(kind, owner_uid, family_key))`.
  - Table `organizer_state(id INTEGER PRIMARY KEY CHECK(id = 1), rev INTEGER NOT NULL DEFAULT 0, dirty INTEGER NOT NULL DEFAULT 0)`.
- **Every mutation sets `dirty = 1`.** Specifically:
  - Create, rename and move set `updated_at`.
  - Add stamps the family `at`, from `added_at` for collections and a new `added_at` for set items (add the column).
  - Remove writes an `organizer_removed` row.
  - Delete writes a tombstone; children cascade as tombstones too.
  - A family-key rename is a remove plus an add.
  - Restoring a backup and importing collections stamp everything they touch.
  - Recent and project-set on/off never set dirty.
- **Engine API:**
  - `Engine::organizer_doc(&self) -> Result<OrganizerDoc>`
  - `Engine::apply_organizer(&self, doc: &OrganizerDoc) -> Result<bool /* changed */>`. It maps by uid and keeps local integer ids for existing uids. It creates or deletes local rows to match, and never touches Recent, `is_on` or `last_used`. A `parent` that points at a missing or deleted uid lands at the root.
  - `Engine::organizer_sync_state(&self) -> Result<(i64 /* rev */, bool /* dirty */)>`
  - `Engine::set_organizer_synced(&self, rev: i64) -> Result<()>` clears dirty.
- **Desktop flow:**
  - **Pull:** `doc = merge(remote, local)`, then apply. If the result differs from remote, or local was dirty, PUT with `base_rev = remote.rev`.
    - On 409, merge with the returned doc and retry once.
    - On 404, disable sync until the next connect.
  - **Triggers:** on connect, on `OrganizerChanged`, and when dirty (checked about 2 s after a change, debounced).
  - **After apply changes anything:** emit the existing collections and project-sets refresh events, or add `organizer-changed` if none exists, and make the UI reload both lists.

- [ ] **Step 1: Write the failing engine tests:**
  - `migration_backfills_uids` (Favorites is `"favorites"`).
  - `mutations_mark_dirty_and_stamp`.
  - `doc_round_trips_through_apply`: export from engine 1, apply to engine 2, then lists are equal by name, families and parent.
  - `apply_keeps_recent_and_set_state`.
  - `delete_becomes_tombstone_and_applies_remotely`.
  - `push_retries_after_conflict`: a unit test of the desktop sync helper against a fake client, or a `crates/sync` test with a fake server returning 409 once.
- [ ] **Step 2:** Run `cargo test -p typeshelf-engine organizer` → FAIL. Then implement → PASS.
- [ ] **Step 3:** Add the sync crate calls and wire the desktop flow. Make the UI refresh.
- [ ] **Step 4:** Run `cargo test --workspace`, clippy, desktop tsc and oxlint. Update `docs/desktop-commands.md`. Commit `Collections and project sets sync across a person's computers`.
