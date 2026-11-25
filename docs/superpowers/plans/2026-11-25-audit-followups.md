# Audit follow-ups: the four items left open by the audit-fixes branch

Source: the "Known and not fixed" list at the end of the audit-fixes work (merged into `main` at 1e369f5).

Branch: `fix/audit-followups` (local only, never pushed by this plan).

## Global Constraints

- **Test first.** Where a harness exists (engine `crates/engine/src/tests/`, server `apps/server/tests/api.rs`,
  sync `crates/sync/tests/client.rs`, bridge `crates/adobe-bridge/tests/bridge.rs`, `#[cfg(test)]` unit tests
  elsewhere), write the failing test, run it and record the failure, then fix, then record the pass.
- **The UIs and the plug-in JavaScript have no test runner. Do not add one.**
- **No new dependencies** (Rust crates or npm packages). A crate already used elsewhere in the workspace still
  counts as new for a crate that does not depend on it today.
- **Smallest diff that fixes the root cause.** No refactors, renames or "while I'm here" changes outside the task.
  Fix a shared function once rather than patching each caller.
- **Do not run `cargo fmt`.** The repo has no rustfmt config and uses long lines; formatting whole files would
  bury the change. Match the surrounding style by hand.
- `cargo clippy -p <crate> --all-targets -- -D warnings` stays clean for every crate a task touches, and the
  crate's full test suite passes before committing.
- **Commits:** stage only the files you changed by name (never `git add -A` or `git add .`), never push, never
  amend someone else's commit. End every commit message with the line
  `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.
- **Compatibility:** desktops, the server and installed Adobe plug-ins update at different times. Every change
  must keep an old peer working at least as well as it does today (old desktop with new server, new desktop
  with old server, old plug-in with new bridge). Say in your report how each pairing behaves.
- **User-facing text** is plain English, says what happened and what the person can do, and never promises
  something the code does not do.
- This is a Windows machine. In the Bash tool, heredocs containing single quotes fail; create and edit files
  with the Write/Edit tools instead.
- Existing `// ponytail:` comments mark deliberate simplifications. Keep that convention for any shortcut you
  take: name the ceiling and the upgrade path.

---

## Task 1: Adobe bridge tracks documents per plug-in instance, not per app

Files: `crates/adobe-bridge/src/lib.rs`, `crates/adobe-bridge/tests/bridge.rs`, `plugins/README.md`;
`apps/desktop/src-tauri/src/adobe.rs` and `plugins/shared/typeshelf-client.js` only if the change requires it.
Read `plugins/README.md` and all of `lib.rs` first.

The problem: two running instances of the same Adobe application (two Photoshop versions, say) each pair as
their own client (tokens are per app + client id), but the bridge keys open documents, "last heard" and the
ping's `documents` count by app only. After the bridge forgets its documents (a Typeshelf restart, or an
expiry), the first instance to re-report restores the per-app count, so the second instance's check
("the bridge has fewer documents than I sent") never fires. Its documents stay forgotten, their fonts stay off
or are released, for as long as both instances run. Two same-named documents in the two instances also share
one key, so closing one drops the other's claim.

Required behaviour:

1. Open documents are tracked per paired client (the client behind the request's token). Two clients of the
   same app with a document of the same name hold two separate claims; closing one leaves the other's fonts on.
2. An authenticated `GET /v1/ping` returns `documents` = the number of documents the calling client has open
   on the bridge. An unauthenticated ping is unchanged.
3. Silence ("last heard") is tracked per client. A client's documents expire when that client has been silent
   for the expiry window AND its Adobe application's process is not running. The process check stays per app
   (two instances of one app share an image name, so while either runs neither expires; that is the safe
   direction, mark it with a `ponytail:` note).
4. A font is released only when no open document of any client uses it, exactly as today across apps.
5. The persisted owned-fonts state keeps working, and a state file written by the current code still loads.
   Change its format only if you must, and then the old format must still load safely.
6. The plug-in client should need no change: with a per-client count its existing "fewer than I sent" rule
   becomes exact. Confirm by reading it; change it only if you find a real gap, and say what.

Tests in `bridge.rs`, each watched failing first: (a) two paired clients of the same app: after client A opens
documents, client B's authenticated ping reports 0, and A's reports A's count; (b) two clients of the same app
each open a document with the same name and a font; closing one keeps the font on, closing the second
releases it; (c) the existing expiry and restart tests still pass, adapted only where the keying changed.

Update the protocol description in `plugins/README.md` where it is now wrong.

Verification: `cargo test -p typeshelf-adobe-bridge`, clippy for the crate, and
`cargo check -p typeshelf-desktop` if `adobe.rs` changed.

---

## Task 2: A merged group reports a failure only when none of its computers could apply the command

Files: `apps/server/src/hub.rs`, `apps/server/src/db.rs` (migration), `apps/server/src/desktop.rs` and
`installs.rs` as needed, `apps/server/tests/api.rs`; `crates/engine/src/admin.rs` and a new engine test module
`crates/engine/src/tests/merged.rs` (declare it at the end of `crates/engine/src/tests.rs` with `mod merged;`;
do not add tests to `tests/audit.rs`, another task is editing it); `docs/api.md` where behaviour it documents
changes.

The problem: since commands fan out to every computer in a merged group, "turn this font on" also reaches a
computer that does not have the font. That computer reports a failure twice over: the desktop emits its own
`ActivationFailed` event (`crates/engine/src/admin.rs`, the `Activate` arm), and it acks the command as failed,
which the server also records as a failure event (`apps/server/src/hub.rs`, `ack`). The whole person is then
flagged "failures" on the admin dashboard although the font was turned on where it exists.

Required behaviour:

1. **Engine.** An admin `Activate` for a font that is not in this library no longer emits its own
   `ActivationFailed` event. It still fails the command (the ack reports it), with the existing `file_missing`
   error code. Other failure reasons, and failures of activations the user started, are unchanged.
2. **Server: link the copies.** Commands created by one fan-out in `hub::queue` are linked (a new nullable
   column on `commands` set by a new migration; one fan-out = one shared value). A command sent to a single
   install is not part of a fan-out.
3. **Server: decide at ack time.** When an ack reports failure with the "this computer does not have the
   font" code:
   - command not part of a fan-out: record the failure event exactly as today;
   - part of a fan-out: record nothing if any linked command has been acked as succeeded; record nothing while
     any linked command is still unacked (a computer that is offline may yet succeed); when the last linked
     command is acked and none succeeded, record one failure event (on the install the acking command belongs
     to is fine) so the admin sees that nobody had the font.
   A failed ack with any other error is recorded per computer exactly as today, fan-out or not.
   Check what the desktop actually sends in the ack's error field (code, message or both: see
   `crates/sync/src/lib.rs` and `apps/desktop/src-tauri/src/sync.rs`) and match on it robustly; if only a
   message is sent today, extend the ack so the code is available without breaking an old desktop or an old
   server (an added optional field), and say what you did.
4. A later success must not leave a stale failure behind and a later failure must not erase a success: walk
   through the orders (fail then succeed, succeed then fail, both fail, one never acks) in your report.

Compatibility: an old desktop still emits its own `ActivationFailed` event, so a merged group with an old
desktop keeps today's false failure until that desktop updates; that is acceptable, say so in the report.

Tests, each watched failing first: engine (in `tests/merged.rs`): an admin `Activate` for a font not in the
library returns the `file_missing` error and emits no `ActivationFailed` event; server (`api.rs`): for a merged
pair, a failed (missing font) ack from one computer plus a successful ack from the other leaves no failure
event and no "failures" flag, in both orders; two failed acks leave exactly one failure event; a single,
unmerged install's failed ack is recorded as today.

Verification: `cargo test -p typeshelf-server -p typeshelf-engine -p typeshelf-sync` and clippy for each crate
touched.

---

## Task 3: Font folders kept by the uninstaller are picked up by a fresh library

Files: `crates/engine/src/scan.rs`, `crates/engine/src/tests/audit.rs`.

The problem: the uninstaller's "delete my library data" keeps the three app-managed font folders in the data
folder (`Imported fonts`, `Google Fonts`, `Shared library`) and removes the database. After a reinstall the new
database has no folder rows for them. `managed_folder(kind, dir_name)` only creates a row the first time
something is imported, installed from Google Fonts or delivered, so the kept fonts are invisible until then;
and `import_paths` reads the set of known hashes before the kept folder has ever been scanned, so importing a
file that is already in the kept folder writes a numbered duplicate.

Required behaviour: when the engine rescans (it does at every start), each of the three managed folders that
exists in the data folder and has no folder row yet is adopted: it gets its folder row with the right managed
kind, is watched and is scanned like any other folder, so its fonts are in the library from the first start.
A managed folder that does not exist on disk is still created lazily as today (do not create empty folders at
start). Use the same kind and folder-name pairs `managed_folder` is called with today (find the three call
sites); define the pairs once and use them from both places rather than repeating the strings.

Tests in `audit.rs`, each watched failing first: (a) a font file placed in `<data>/Imported fonts` before the
first rescan is listed in the library after it, in a folder with the imported kind; (b) importing that same
file afterwards skips it (no duplicate file on disk, `skipped` counted); (c) with no managed folder on disk, a
rescan creates none.

Verification: `cargo test -p typeshelf-engine` and clippy for the crate.

---

## Task 4: The sync client drops only the events the server rejects, and says so

Files: `crates/sync/src/lib.rs`, `crates/sync/tests/client.rs`, `apps/desktop/src-tauri/src/sync.rs`.

The problem: `flush` sends queued events in batches of up to 200. Since the audit fixes, a batch the server
rejects with a permanent 4xx is dropped whole so the outbox is not blocked forever. One event the server does
not understand (a newer desktop talking to an older server) therefore takes up to 199 good events with it, and
the only trace is an `eprintln!`, which a release build of the desktop app never shows.

Required behaviour:

1. When a batch of more than one event is rejected with a permanent 4xx (same status rule as today: 4xx except
   401, 403, 408, 429), the client sends that batch's events again one at a time. An event rejected on its own
   with a permanent 4xx is dropped; the others are delivered. Any other error while resending (network, 5xx,
   401/403/408/429) stops the flush and keeps every event not yet delivered or dropped, in order, so nothing
   is lost and nothing already delivered is sent twice on the next flush.
2. A single-event batch that is rejected is dropped as today.
3. The number of dropped events is visible to the caller instead of printed: make it part of what `flush`
   returns (or an equivalent the crate can offer without a logging dependency) and remove the `eprintln!`.
   The desktop shell (`apps/desktop/src-tauri/src/sync.rs`), which already uses `tracing`, logs a warning with
   the count whenever it is not zero. Adjust every caller of `flush` for the new return value.

Tests in `client.rs`, each watched failing first: (a) a batch of several events where the mock server rejects
exactly one event kind (400 for any request containing it) ends with every other event delivered exactly once,
the outbox empty, and a dropped count of 1; (b) a 500 in the middle of the one-at-a-time resend keeps the
undelivered events in the outbox in order, and a later flush delivers them without duplicating the ones already
delivered; (c) the existing tests still pass.

Verification: `cargo test -p typeshelf-sync`, clippy for the crate, and `cargo check -p typeshelf-desktop`.
