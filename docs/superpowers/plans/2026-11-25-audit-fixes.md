# Audit fixes: data loss, wrong font state, sync/server, installer

Source: the code audit of 2026-11-25 (section "Bugs, most severe first"). Each task fixes the listed bugs at
their root cause and leaves a regression test where a test harness exists.

Branch: `fix/audit-bugs` (local only, never pushed by this plan).

## Global Constraints

- **Test first.** Where a harness exists (engine `crates/engine/src/tests/`, server `apps/server/tests/api.rs`,
  sync `crates/sync/tests/client.rs`, bridge `crates/adobe-bridge/tests/bridge.rs`, `#[cfg(test)]` unit tests
  elsewhere), write the failing test, run it and record the failure, then fix, then record the pass.
- **The UIs have no test runner. Do not add one.** Verify UI changes with
  `bunx tsc --noEmit -p tsconfig.app.json` and `bunx oxlint` in that UI's folder; no new errors or warnings.
- **No new dependencies** (Rust crates or npm packages).
- **Smallest diff that fixes the root cause.** No refactors, renames or "while I'm here" changes outside the task.
  Fix a shared function once rather than patching each caller.
- **Do not run `cargo fmt`.** The repo has no rustfmt config and uses long lines; formatting whole files would
  bury the change. Match the surrounding style by hand.
- `cargo clippy -p <crate> --all-targets -- -D warnings` stays clean for every crate a task touches, and the
  crate's full test suite passes before committing.
- **Commits:** on `fix/audit-bugs`, stage only the files you changed by name (never `git add -A` or `git add .`),
  never push, never amend someone else's commit. End every commit message with the line
  `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.
- **User-facing text** is plain English, says what happened and what the person can do, and never promises
  something the code does not do.
- This is a Windows machine. In the Bash tool, heredocs containing single quotes fail; create and edit files
  with the Write/Edit tools instead.
- Existing `// ponytail:` comments mark deliberate simplifications. Keep that convention for any shortcut you
  take: name the ceiling and the upgrade path.

---

## Task 1: Engine data-loss and font-state fixes

Crate: `crates/engine`. Tests: `crates/engine/src/tests/audit.rs` (new module, declared at the end of
`tests.rs`).

Bugs fixed:

1. **Auto-organize could bin the last copy of a font.** `trash_copy` (`organize.rs`) trusted a cached plan.
   It must refuse (step fails, file stays) unless the kept copy still exists, is a different file, and has the
   same bytes.
2. **Undo failed silently and was still marked done.** `undo_history` must return error code
   `undo_incomplete` when a step cannot be undone, leave the history entry not undone, and be safe to repeat.
   A moved file the user has since moved or deleted is skipped, not counted as a failure.
3. **Overlapping folders.** `locate_folder` and the automatic follow-a-moved-folder path must reject a location
   that contains, or sits inside, another watched folder (error code `overlap`; the auto-follow case leaves the
   folder offline). Same rule `insert_folder` already applies; one shared helper.
4. **A scan error deleted library rows.** In `scan_folder`, an entry that cannot be read (walk error, metadata
   error) must not cause rows under that path to be deleted. Deletions elsewhere in the folder still apply.
5. **Auto-activate fired on rename or move inside a folder.** A file counts as "arrived" only when its bytes
   (hash) were not already in that folder before the scan.
6. **License edits wiped each other.** `Engine::set_license(key, label, file, note)`: `None` for `file` or
   `note` keeps the stored value.
7. **Blocking a second file with the same PostScript name unblocked the first.** Migration 5 makes the
   `blocked` table keyed by `(ps_name, hash)`; an empty hash blocks every file with that name.
8. **Fonts on a drive that was missing at startup stayed unregistered when it came back.** When an offline
   folder is found online again its active (ledger) fonts are registered again and the folder is watched. New
   public method `Engine::reconnect_folders() -> Result<bool>`: a cheap check (is any offline folder's path a
   directory again?) that runs a rescan when one is, and returns whether anything came back. The desktop shell
   polls it (Task 6).

Verification: `cargo test -p typeshelf-engine` and `cargo clippy -p typeshelf-engine --all-targets -- -D warnings`.

---

## Task 2: Server fixes (login, registration, settings, merge, library quarantine)

Crate: `apps/server` (plus `crates/core/src/name_rule.rs` for item 3). Tests: `apps/server/tests/api.rs`, and
unit tests in `name_rule.rs`. Read `apps/server/src/lib.rs` first for the router and `AppState`, then the
handler files.

1. **Login throttling.** After 5 failed logins for the same email (compare lowercased, trimmed) within
   15 minutes, further attempts for that email return HTTP 429 with error code `too_many_attempts` until the
   window has passed, without checking the password. A successful login clears the count. Keep the state in
   memory on `AppState` (a mutex-guarded map is enough); cap the map so it cannot grow without bound (drop
   expired entries when inserting). Unknown emails count too, so the endpoint does not reveal which emails
   exist through this path.
2. **Session cookie `Secure`.** `session_cookie` (`admin.rs`) adds `; Secure` when the request came over HTTPS:
   the `X-Forwarded-Proto` header is `https`, or the server's configured public URL starts with `https://`.
   Plain-HTTP local development keeps working without it. Apply the same rule to the logout cookie.
3. **Settings bounds.** `put_settings` rejects (400 `invalid_setting`) `name_change_days` outside `1..=3650`
   and a non-null `timeline_retention_days` outside `1..=3650`. Independently, `next_change` in
   `crates/core/src/name_rule.rs` must not panic for any `days` value (use checked arithmetic; an
   unrepresentable date means "cannot change yet" far in the future, not a panic).
4. **Registration survives a lost response.** `POST /installs` for an install id that already exists currently
   always returns 409 `already_registered`, so a desktop that never received its token is stuck forever. New
   rule: if the existing install has never made an authenticated request (its `last_seen` still equals its
   `first_seen`), re-issue: store a new token hash for that row and return the normal success body. An install
   that has authenticated at least once still gets 409. Check how `last_seen` is updated (`auth.rs`,
   `Install` extractor) and make sure the comparison is reliable (the timestamps have one-second resolution: if
   a first authenticated request can land in the same second as registration, add a dedicated nullable column
   or flag via a new migration instead of comparing timestamps; pick whichever is actually correct and say
   which in your report).
5. **Merge no longer cuts a computer off.** Today the newer install gets `merged_into` set and is then excluded
   from every broadcast (`db::active_install_ids` filters `merged_into IS NULL`), and commands aimed at the
   surviving install never reach it. A merged install is still a real computer with its own token and socket.
   Required behaviour:
   - Broadcasts (block/unblock, policy change, deliver to all) are queued for every install, merged or not.
   - A command aimed at one install (turn a font on/off, deliver to chosen installs, organize request, and any
     other per-install command) is queued for that install and for every install merged into it.
   - The admin fonts list for an install covers the whole merged group: one row per font hash, `active` true if
     it is active on any member, newest `updated_at` wins for the other fields.
   - Check every caller of `active_install_ids` and of the per-install queueing so nothing that genuinely needs
     "primary installs only" (counts, lists shown to the admin) changes meaning.
6. **Library organize keeps the files.** `organize/apply` for a `Trash` or `Quarantine` action currently
   deletes the database row and the stored file. It must move the stored file into
   `<data dir>/quarantine/` (create it; name the file by its library filename, prefixed with the first 8 hash
   characters if that name is taken) and then remove the row. Nothing is deleted from disk. A move failure
   leaves the row in place and does not count as applied.

Each of the six items gets at least one test in `api.rs` that fails before the change (item 3 also gets a
unit test for the overflow). Verification: `cargo test -p typeshelf-server -p typeshelf-core` and clippy for
both crates.

---

## Task 3: Sync outbox no longer blocks on a rejected batch

Crate: `crates/sync`. Tests: `crates/sync/tests/client.rs` (there is a mock server there; extend it).

`flush` in `crates/sync/src/lib.rs` sends up to 200 queued events and only removes them from the outbox file
when the request succeeds. When the server rejects a batch with a client error that will never succeed on
retry (an event kind it does not know, a payload too large), the same batch is retried forever and nothing
behind it is ever delivered.

Required behaviour: when the server answers a batch with a 4xx status other than 401, 403, 408 and 429, the
client logs a warning with the status and the number of events, removes that batch from the outbox, and
carries on with the next batch. 401/403/408/429, 5xx and network errors keep the batch and stop the flush as
today. Look at how `SyncError` represents HTTP failures and extend it only as far as needed to tell these
cases apart.

Tests: (a) a batch rejected with 400 is dropped and a later, valid event is delivered; (b) a batch answered
with 500 stays in the outbox. Verification: `cargo test -p typeshelf-sync` and clippy.

---

## Task 4: Adobe-activated fonts are released when the Adobe app or Typeshelf goes away

Files: `crates/adobe-bridge/src/lib.rs`, `crates/adobe-bridge/tests/bridge.rs`,
`apps/desktop/src-tauri/src/adobe.rs` (only if the handler interface must change),
`plugins/shared/typeshelf-client.js`. Read `plugins/README.md` for the protocol first.

The bridge turns on fonts a document needs and records them in an in-memory `Docs { open, owned }`. They are
released only when the plug-in reports the document closed. Two cases leak fonts that then stay on forever:

- The Adobe app quits or crashes: no close is ever sent.
- Typeshelf restarts: `owned` is forgotten, so even a later close releases nothing.

Required behaviour:

1. **Heartbeat.** While it is paired and connected, the plug-in client sends an authenticated ping at a fixed
   interval (15 s; check what it already does and reuse it). The bridge records, per paired app, when it last
   heard an authenticated request.
2. **Expiry.** When an app that has open documents has not been heard from for 60 s, the bridge treats all of
   that app's documents as closed and releases the fonts exactly as a normal close would (fonts still used by
   another app's open document stay on). Make the two durations constants and let tests shorten them (a config
   value on the bridge, not a global).
3. **Persistence.** The set of fonts the bridge turned on (`owned`) is saved to a file in the bridge's data
   folder whenever it changes and loaded at start. After a start, fonts in the loaded set that no open document
   has claimed within the expiry window (60 s) are released. A document re-reported in time keeps its fonts and
   stays owned.
4. **Re-report after reconnect.** When the plug-in client finds the app again after losing it, it re-sends its
   currently open documents so they are claimed again. Check what it does today when it sees the app offline
   and keep the result consistent: no duplicate tracking, no fonts stranded.

There is an unused `adobe-bridge.json` written by the bridge today; check whether it is the natural home for
the persisted state before adding another file.

Tests in `bridge.rs` (they drive the real HTTP bridge with a fake handler; follow their pattern): (a) an app
that stops pinging has its fonts released after the expiry; (b) an app that keeps pinging keeps its fonts;
(c) fonts owned by a previous run are released after the window when nobody re-claims them, and kept when a
document re-opens with them in time. The JavaScript client has no test runner: keep the change small and
re-read it for mistakes; `bun plugins/build.ts` must still succeed if dependencies are installed (do not
install anything to check it).

Verification: `cargo test -p typeshelf-adobe-bridge`, clippy for the bridge crate, and
`cargo check -p typeshelf-desktop` (use the real package name from `apps/desktop/src-tauri/Cargo.toml`) if you
touched `adobe.rs`.

---

## Task 5: Installer: uninstall keeps fonts, silent mode works, updates are staged

Crate: `apps/setup` (Windows-only, Tauri). Read `docs/setup.md` and `apps/setup/src/main.rs` first.

1. **"Also delete my library data" must not delete font files.** `uninstall.rs` removes the whole app-data
   folder (`%APPDATA%\app.typeshelf.desktop` and the `%LOCALAPPDATA%` twin). The desktop app keeps three font
   folders inside its data folder: `Imported fonts`, `Google Fonts`, `Shared library` (see
   `managed_folder` in `crates/engine/src/scan.rs`). The uninstall screen promises "Your font files stay
   exactly where they are" and the done screen says "Untouched".
   Required: wiping data removes everything in those data folders except these three folders when they
   contain at least one file; empty ones are removed. The uninstall report gains the path of the data folder
   that still holds fonts (if any), and the done screen shows one line saying fonts imported into Typeshelf
   were kept and where (check how `rm-note` is filled in `apps/setup/ui/app.js`). Put the keep/remove logic in
   a function that takes a directory so it can be unit tested with a temp folder.
2. **Silent mode.** `--silent` install/update fails with "app running" whenever Typeshelf is open, and
   `uninstall.exe --uninstall --silent` exits 0 before the temp-copy child has done anything. Required: silent
   mode closes the running app the same way the GUI flow does before installing or uninstalling (find what the
   GUI calls in `process.rs`), and when the uninstaller relaunches itself from `%TEMP%` in silent mode the
   parent waits for the child and exits with the child's exit code.
3. **Staged update.** `install.rs` deletes `plugins/` and overwrites `typeshelf.exe` in place, so a failure
   mid-way leaves a broken install, and the uninstall registry entry is written last. Required: extract the
   payload into a staging folder inside the install folder, and only when extraction has fully succeeded swap
   the new files in (keep the previous `typeshelf.exe` and `plugins/` aside until the swap is complete, put
   them back if it fails, remove them when it succeeds). Write the uninstall registry entry as soon as the
   files are in place, before shortcuts and the start-at-login step, so a failure in those optional steps still
   leaves an install that can be removed from Add/Remove Programs.

Tests: unit tests (`#[cfg(test)]`) for the wipe function (fonts kept, everything else gone, empty font
folders removed) and for the staged swap helper (a failing swap restores the previous files; a successful one
leaves no staging or backup folders). Process and registry code is exercised only by building.

Verification: `cargo test -p <setup package name>` and clippy for it. The first build compiles Tauri and can
take several minutes; a dev build embeds an empty payload (`apps/setup/build.rs`), which is expected.

---

## Task 6: Desktop shell: narrow the asset scope, poll for returning folders

Crate: `apps/desktop/src-tauri` (plus one small read-only accessor in `crates/engine` if needed).

1. **Asset protocol scope.** `tauri.conf.json` has `"assetProtocol": { "enable": true, "scope": { "allow": ["**"] } }`,
   so the webview can read any file on disk through `asset:`/`http://asset.localhost`. Font previews load font
   files through it (`convertFileSrc` in `apps/desktop/ui/src/fonts.ts`). Required: the static scope allows
   nothing, and the app allows, at run time, exactly the folders that hold fonts: every folder row the engine
   knows (user folders, the system font folders, and the managed folders under the app-data folder) at
   startup, and any folder that is added, located, relocated or created later. The engine emits
   `UiEvent::FolderStatus` for folder changes (see where the shell forwards UI events); use that so a folder
   that appears later is allowed without a restart. Use Tauri 2's runtime scope API
   (`asset_protocol_scope().allow_directory(path, true)`); confirm the exact API against the Tauri version in
   `Cargo.lock` by reading its source in the cargo registry, not from memory.
   Previews are the only consumer; the PDF page and license files are opened by the OS, not through `asset:`
   (verify by searching the UI for `convertFileSrc`). If anything else uses it, allow exactly what it needs and
   say so in the report.
2. **Returning folders.** Call `Engine::reconnect_folders()` (added in Task 1) every 30 seconds from a
   background task so fonts on a drive that is plugged in while the app runs come back without a restart. It
   is a cheap check; run it off the UI thread the same way other engine calls are run, log errors, never panic.

This crate has no tests and the change cannot be exercised without the GUI. Put any non-trivial decision
logic (which paths to allow) in a small pure function with a unit test; the rest is verified by
`cargo check`/`cargo clippy` for the desktop package. State plainly in the report that previews were not
visually verified.

---

## Task 7: Admin UI and docs

Files: `apps/admin-ui/src/pages/UserDetail.tsx`, `Merge.tsx`, `Organize.tsx`; `docs/deploy.md`; `.gitignore`;
`docs/api.md` and `docs/desktop-commands.md` where earlier tasks changed documented behaviour.

1. **Font toggles snap back** (`UserDetail.tsx`, `Fonts` component). The switch sends a command that the
   desktop applies later, while the list re-polls every 10 s and overwrites the optimistic state. Required: keep
   a per-font "pending" target state. While a font is pending the switch shows the requested state, is
   disabled (no duplicate commands), and the row shows a short "Waiting for this computer" hint. Pending clears
   when a poll reports the requested state. It also clears, restoring the reported state, after 2 minutes, with
   the hint replaced by nothing (the command stays queued on the server for an offline computer; the timeline
   shows it). No new libraries.
2. **Merge copy** (`Merge.tsx`). After Task 2 both computers keep receiving commands. Keep "Both computers keep
   working". Replace the line claiming admin-delivered fonts are sent to the new computer with one that is
   true: fonts you deliver to this person from now on reach both computers.
3. **Organize copy** (`Organize.tsx`). After Task 2 organize moves files to the server's quarantine folder.
   Say so: the files leave the shared library and are kept in the server's `quarantine` folder; they are not
   sent to users. Remove the claim that users receive cleaned-up files on their next sync. Use the danger
   style for the confirm button, as other destructive confirms in the admin UI do.
4. **`docs/deploy.md` publishes infrastructure details** (origin IP, LAN addresses, a MAC address, host names).
   Copy the current file unchanged to `docs/deploy.local.md`, add `docs/deploy.local.md` to `.gitignore`, and
   in the tracked `docs/deploy.md` replace every real address, MAC, host name, user name and domain with clear
   placeholders (`<origin-ip>`, `<server-lan-ip>`, `fonts.example.com`, …) while keeping the guide usable.
   Do not rewrite git history.
5. **Docs.** Update `docs/api.md` for the Task 2 behaviour that is documented there (429 on login, settings
   bounds, registration re-issue, organize quarantine) and `docs/desktop-commands.md` for `set_license`
   (omitted `file`/`note` keep the stored value). Only lines that are now wrong or missing.

Verification: in `apps/admin-ui`, `bunx tsc --noEmit -p tsconfig.app.json` and `bunx oxlint` with no new
warnings compared with the base commit.
