# One person, several computers — design

## Why

The user asked how things work when an employee has a laptop and a Mac. Each computer registers as its own install, and an admin can merge them into one person. Reading the code turned up three gaps, and the user asked for all three to be fixed ("kerjakan 1-3"):

1. **The name doesn't follow on a merged computer.**
   - A merge renames only the primary install, so the Mac keeps showing its old name.
   - A name change made on the Mac edits the Mac's own row. The admin keeps seeing the primary's name, while the name history records the change.
   - The once-every-N-days limit, checked from the Mac, ignores changes made on the laptop.
2. **Removing a computer removes the whole person.**
   - `DELETE /installs/{id}` revokes the install and everything merged into it.
   - An old laptop can't be dropped without also dropping the Mac.
   - Splitting first only works within 30 days of the merge.
3. **Collections and project sets don't follow the person.** Each computer keeps its own, and only the manual backup/move can copy them across.

The user opted out of questions, so every call below that isn't the user's is marked *(my call)*.

## Goals

### 1. One name per person

- **The name lives on the person.** The person is the install a computer was merged into, or the computer itself; this is `db::person`.
  - `GET /me` returns the person's name and the person's next allowed change.
  - So does the token refresh in `POST /installs`.
- **The limit is shared.** `next_name_change` counts user name changes made from every computer of the person.
- **Changing the name from any computer** changes the person's name. The `name_changes` row keeps the computer that did it.
- **Live updates.** After a name change, a merge, a split or an admin limit reset, the server sends `ServerMessage::Identity { name, next_name_change }` over WebSocket to every connected computer of the person; the one that made the change is skipped. The desktop stores it and refreshes, as it already does from `GET /me` on connect.
  - Computers that were offline get it on their next connect.
  - Older desktops ignore the message.
- **Split restores the split-off computer's own name.** Its row was never renamed, so this already holds. The split also tells both sides.

### 2. Remove one computer

- **New endpoint** `DELETE /installs/{person}/computers/{computer}` returns `200 { person }`, the id the person is now listed under.
  - **A computer that is merged into the person:** only that computer is revoked, the same way `revoke` does it.
  - **The person's own install, while others remain:** the earliest-registered remaining computer becomes the person.
    - Its `merged_into` and `merged_at` become NULL.
    - The other computers are re-pointed to it.
    - It takes the person's current name. If that differs from its own row's name, a `name_changes` row with `by = 'merge'` records the change.
    - The removed install is revoked, and its `merged_into` is set to the new person so its history still maps to the person.
  - **The only computer:** the same as removing the person.
- **Unchanged:** the existing `DELETE /installs/{id}` keeps removing the whole person, and the split rule stays at 30 days.
- **Audit:** `computer_removed`, with the person as target and the computer as detail.
- **Data:** `InstallDetail` gains `computers: [{ id, os, app_version, first_seen, last_seen, online, merged_at }]`, with the person first. The existing `merged_from` stays, for compatibility.
- **Admin UI** (`UserDetail`):
  - A "Computers" card lists each computer with OS, version, last seen and an online dot.
  - Each computer has "Remove this computer", behind a confirm dialog.
  - "Split" stays on merged rows inside its window.
  - The existing delete button is relabelled to say it removes the person and all their computers.
  - When the person id changes, the page navigates to the new id.
  - All copy is translated, ID and EN.

### 3. Collections and project sets follow the person

- **What syncs:**
  - Non-builtin collections: name, parent and families.
  - The Favorites builtin's families. It has fixed uid `favorites`.
  - Project sets: name, note and families.
- **What doesn't sync:**
  - The Recent builtin.
  - Whether a project set is on, and when it was last used (per computer, *my call*).
  - Tags, license notes, pins and sample texts (YAGNI, *my call*).
- **Document and merge:** a pure, deterministic `OrganizerDoc` with a `merge(a, b)` in `crates/core`, shared by server and desktop.
  - Each item has a stable `uid`, `updated_at` for its own fields, and an optional `deleted_at` tombstone.
  - Families are a last-writer-wins element set: `{ key, at, removed }` per family.
  - Two different uids with the same kind, name and parent are merged into the lexicographically smaller uid, so two computers that both had "Client X" end up with one *(my call)*.
  - Tombstones older than 180 days are pruned *(my call)*.
- **Server:**
  - `GET /me/organizer` returns `{ rev, doc }`: the merge of every stored document in the person's group.
  - `PUT /me/organizer` takes `{ base_rev, doc }`.
    - If `base_rev` doesn't match, it returns 409 with `{ rev, doc }`.
    - Otherwise it stores the document under the person, removes the other group rows' documents, increments `rev` and sends `OrganizerChanged { rev }` to the person's other connected computers.
  - Limits: 2 MiB cap, 413 `organizer_too_large`. A document that doesn't parse gets 400 `invalid_organizer`.
  - Merge, split and remove-a-computer need no hooks. A read merges the whole group. After a split, the split-off computer's next push re-uploads its local state.
- **Desktop:**
  - **On connect:** pull, merge with local, apply, and push if the result differs from the remote.
  - **After any local change:** push, debounced to about 2 s.
  - **On `OrganizerChanged`:** pull, merge and apply, then tell the UI to refresh collections and sets.
  - **No server, or an older server (404):** sync stays off silently.
- **Out of scope:** an admin view of synced documents, and conflict UI. Last-writer-wins is silent.

## Non-goals

- Syncing which fonts are turned on between computers. Activation is per machine.
- Automatic merging of installs.
- Changing the 30-day split window.

## Anti-goals

- Old desktops must keep working: new messages are WebSocket-only and ignored by older clients, and no new `CommandKind` is introduced.
- A revoked install's synced document must never reach a new install that re-registers under the same id: `free_revoked_id` moves `organizer_docs` with the other tables.
- No new crates or npm packages.

## Approaches considered

- **Live name push:** a new queued `CommandKind` vs. a WebSocket-only `ServerMessage` (chosen).
  - The client's `GET /me/commands` parses the whole list, so an unknown command kind would break older desktops.
  - `GET /me` on connect already covers computers that were offline.
- **Removing one computer:** require split-then-delete (blocked after 30 days) vs. a dedicated per-computer removal with promotion (chosen).
- **Sync model:**
  - Whole-document last-writer-wins is simplest, but loses concurrent edits.
  - A full CRDT is overkill.
  - Chosen: per-item last-writer-wins with per-family element sets and tombstones. It's small, deterministic and order-independent.
- **Where merge lives:** server only vs. client only vs. **shared pure function in `crates/core` (chosen)**, so both sides converge on the same result.
