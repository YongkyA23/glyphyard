# Clearer App Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Running apps notice a new release within the hour (or on a "Check for updates" click). When one is ready
they show it, and "Restart now" installs it and brings the app back.

**Architecture:** `update.rs` gets one shared `check_now` that is used by the hourly loop and by a new
`check_update` command. A finished download emits `update-ready` and adds a tray item. A new `restart_to_update`
command sets a relaunch flag and exits. `on_quit` then installs even with auto-update off, and relaunches: on Windows
through Setup's new `--launch` flag, on macOS through a detached waiter that runs `open`.

**Tech Stack:** Rust (Tauri 2, tokio), React 19 + TypeScript (motion), Bun.

**Spec:** `docs/superpowers/specs/2026-11-27-update-restart-design.md`

## Global Constraints

- Nothing about the server, the manifest, signing or verification changes. Nothing unverified is ever started.
- Startup never waits on the network.
- First check is 15 s after start, then every 1 h. A failed check retries in 1 h, as today.
- A plain Quit keeps today's gate (`has_key && auto_update && !setup_running`) and never relaunches.
- Restart must never leave the user without a running app. If the install is skipped or fails, relaunch the current
  app.
- Every new UI string goes through `t()`, with an Indonesian entry in `ui/src/i18n/id/settings.ts`
  (`bun scripts/i18n-check.ts` must pass).
- The tray label is fixed text with no version (builder's call, allowed by the spec):
  - "Restart to update"
  - "Restart untuk update"

## Review Focus

- **Manual check while the hourly check is downloading.** The second check must wait, not write the same `.part`.
  Pinned in Task 1 by a lock in `check_now`.
- **Manual check with "Update automatically" off.** It still downloads, and Restart still installs. Pinned in Task 1:
  `should_install(.., restart=true)`.
- **Restart with nothing ready, or Setup running.** The app must come back on the current version. Pinned in Task 1:
  `on_quit` always relaunches when the flag is set.
- **Second instance at relaunch.** The single-instance plugin would hand the new process to the dying one. The
  relaunch must wait for this PID to exit. Pinned in Task 1 (waiter) and Task 2 (Setup waits via `close_app`).
- **`update-ready` emitted twice** (hourly loop and manual check). The banner shows once and the tray item is
  inserted once. Pinned in Task 1 (an `announced` flag) and Task 3.

---

### Task 1: Backend — check timing, manual check, restart, tray item

**Files:**
- Modify:
  - `apps/desktop/src-tauri/src/update.rs`
  - `apps/desktop/src-tauri/src/commands.rs` (handler list, two commands, tray labels)
  - `apps/desktop/src-tauri/src/main.rs` (tray menu kept in `TrayItems`)
- Test: `apps/desktop/src-tauri/src/update.rs` (existing `mod tests`)

**Interfaces:**
- Produces, for Task 3:
  - Command `check_update` → `{ status: "upToDate" | "ready", version: string }`. On failure it returns the error
    string the way other commands do (`R<T>`).
  - Command `restart_to_update` → `()`. The app exits right after.
  - Event `update-ready` with payload `{ version: string }`.
- Consumes from Task 2: Setup accepts `--launch` next to `--silent`.

Steps:

- [ ] **Step 1: Write the failing tests** in `update.rs` `mod tests`:
  - `restart_installs_even_with_auto_update_off`:
    - `assert!(should_install(true, false, false, true));`
    - `assert!(!should_install(true, false, false, false));`
    - `assert!(!should_install(true, true, true, true));` (Setup running)
    - `assert!(!should_install(false, true, false, true));` (no key)
  - `checks_soon_then_hourly`:
    - `assert_eq!(FIRST_CHECK, Duration::from_secs(15));`
    - `assert_eq!(CHECK_EVERY, Duration::from_secs(60 * 60));`
  - Update the existing tests that call `should_launch` to the new name and arity.

- [ ] **Step 2: Run the tests and see them fail.**
  - Run: `cargo test -p typeshelf-desktop update::`
  - Expected: compile errors for `should_install`, `FIRST_CHECK` and `CHECK_EVERY`.

- [ ] **Step 3: Implement in `update.rs`.**
  - `pub const FIRST_CHECK: Duration` (15 s) and `pub const CHECK_EVERY: Duration` (1 h). `run` waits `FIRST_CHECK`,
    then `CHECK_EVERY`. On error it still waits 1 h, which equals `CHECK_EVERY`.
  - Rename `should_launch` to `pub fn should_install(has_key: bool, auto_update: bool, setup_running: bool, restart: bool) -> bool`:
    true when `has_key && !setup_running && (auto_update || restart)`.
  - `Pending` gains three fields:
    - `checking: tokio::sync::Mutex<()>`
    - `relaunch: AtomicBool`
    - `announced: Mutex<Option<Version>>`
  - `pub async fn check_now(app: &AppHandle) -> Result<Option<Version>, CheckError>`:
    - Holds `checking` for the whole call.
    - Does the body the loop does today: the blocker check on macOS, `check(...)`, and storing `ready` / `problem`.
    - Returns the version now waiting (`None` when up to date).
    - When a version is waiting and differs from `announced`, store it, emit `update-ready {version}`, and call
      `crate::commands::show_update_item(app)`.
    - The loop calls it only when `should_check(...)` passes, as today.
  - `pub fn request_restart(app: &AppHandle)`: sets `relaunch` and calls `app.exit(0)`.
  - `on_quit`:
    - Read `restart = relaunch.load()`. Gate with `should_install(key().is_some(), auto_update, setup_running(), restart)`.
    - Windows, when installing: `launch(&ready, &["--silent", "--launch"])` if `restart`, else `["--silent"]` as
      today. Setup then relaunches the app.
    - macOS, after `mac_install::install` returns `Ok` with `restart`: `relaunch_after_exit(&bundle)`.
    - Whenever `restart` is set and no install was started (nothing ready, gate false, or install/launch failed):
      `relaunch_after_exit(current app)`. On Windows the current app is `current_exe()`; on macOS it is its bundle.
  - `fn relaunch_after_exit(target: &Path)`: spawns a detached waiter that polls until this PID is gone, then starts
    `target`.
    - Windows: `powershell -NoProfile -WindowStyle Hidden -Command "Wait-Process -Id <pid> -ErrorAction SilentlyContinue; Start-Process -FilePath '<exe>'"`,
      with `CREATE_NO_WINDOW | DETACHED_PROCESS`.
    - macOS: `sh -c 'while kill -0 "$0" 2>/dev/null; do sleep 0.2; done; open "$1"' <pid> <bundle>`.
    - Log a spawn error. It cannot be reported anywhere else.
  - Update the module doc comment (cadence, Restart) and the `ponytail:` comment above `on_quit`.

- [ ] **Step 4: Commands and tray.**
  - In `commands.rs`, `#[tauri::command] pub async fn check_update(app: AppHandle, ctx: St<'_>) -> R<CheckView>`:
    - Errors with `EngineError::new("update", ...)` when there is no key, no server, or no token.
    - Otherwise calls `update::check_now`, mapping `CheckError` to `EngineError::new("update", e.to_string())`.
    - `CheckView` is `{ status, version }`, serialized camelCase.
    - For `upToDate`, `version` is `CARGO_PKG_VERSION`.
  - `#[tauri::command] pub fn restart_to_update(app: AppHandle)` calls `update::request_restart(&app)`.
  - Add both to `handler()`.
  - `tray_labels(indonesian)` returns a third label: `"Restart to update"` / `"Restart untuk update"`.
  - `TrayItems` gains `menu: Menu<Wry>`, `update: MenuItem<Wry>` and `shown: AtomicBool`.
  - `pub fn show_update_item(app: &AppHandle)` inserts `update` at index 1 of `menu`, once (`shown`).
  - `set_tray_language` also relabels `update`.
  - In `main.rs`:
    - Build the `"update"` item.
    - Keep the `Menu` in `TrayItems`.
    - `on_menu_event` maps `"update"` to `update::request_restart(app)`.

- [ ] **Step 5: Run the tests and see them pass.**
  - Run `cargo test -p typeshelf-desktop`. Expected: PASS.
  - Run `cargo clippy -p typeshelf-desktop -- -D warnings`. Expected: clean.

- [ ] **Step 6: Commit.**
  - Message: `App checks for updates hourly and can restart into a ready one`.

### Task 2: Setup — `--launch` after a silent install

**Files:**
- Modify: `apps/setup/src/main.rs` (`Args`, `parse_args`, `silent`)
- Test: `apps/setup/src/main.rs` (new `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces, for Task 1: `--launch` flag. After a successful silent install, start the installed app with
  `process::launch_app(&opts.dir)`. It is ignored on failure and on uninstall.

Steps:

- [ ] **Step 1: Write the failing test.**
  - `parse_reads_launch`:
    - `let a = parse(vec!["--silent".into(), "--launch".into()]); assert!(a.silent && a.launch);`
    - `assert!(!parse(vec!["--silent".into()]).launch);`

- [ ] **Step 2: Run it and see it fail.**
  - Run: `cargo test -p typeshelf-setup parse_reads_launch`
  - Expected: compile error, because `parse` is not defined.

- [ ] **Step 3: Implement.**
  - Split `parse_args()` into `fn parse(raw: Vec<String>) -> Args`, with `parse_args` calling it with the env args.
  - Add `launch: bool` (`"--launch"`).
  - In `silent()`, on `Ok(())` of an install with `args.launch`: call `process::launch_app(&opts.dir)`. Log the
    result. The exit code stays 0 even if the launch fails, because the install succeeded.
  - Update the `close_app` doc comment only if it still says otherwise.

- [ ] **Step 4: Run it and see it pass.**
  - Run: `cargo test -p typeshelf-setup`
  - Expected: PASS.

- [ ] **Step 5: Commit.**
  - Message: `Setup --launch starts the app after a silent install`.

### Task 3: UI — banner, Settings check/restart, i18n, mock

**Files:**
- Create: `apps/desktop/ui/src/UpdateBanner.tsx`
- Modify:
  - `apps/desktop/ui/src/api.ts`:
    - `Events['update-ready']: { version: string }`
    - `api.checkUpdate()`
    - `api.restartToUpdate()`
    - `type UpdateCheck = { status: 'upToDate' | 'ready'; version: string }`
  - `apps/desktop/ui/src/Shell.tsx`: render `<UpdateBanner />` at the top of `.shell-main`.
  - `apps/desktop/ui/src/pages/Settings.tsx`, in the autoUpdate row:
    - a "Check for updates" button
    - a status line
    - "Restart now" next to the "will be installed when you quit" line
  - `apps/desktop/ui/src/mock.ts`:
    - `check_update` returns `{ status: 'ready', version: '1.1.0' }`
    - `restart_to_update` returns `undefined`
  - `apps/desktop/ui/src/i18n/id/settings.ts`: the new strings.

**Interfaces:**
- Consumes from Task 1: `check_update`, `restart_to_update`, event `update-ready`.

Steps:

- [ ] **Step 1: `UpdateBanner`.**
  - On mount it reads `api.getSettings()` (shows when `updateReady`) and subscribes `on('update-ready')`.
  - It renders only when a version is known and the banner was not dismissed this session. Session state lives in
    component state, so it comes back on the next start.
  - Text: `t('Glyphyard {v} is ready to install', { v })`.
  - Buttons:
    - primary `t('Restart now')` calls `api.restartToUpdate()`
    - icon close `aria-label={t('Dismiss')}`
  - Use `role="status"` and the existing `.btn` / accent tokens. No new colors.

- [ ] **Step 2: Settings.**
  - "Check for updates" (`.btn`) shows when `s?.canUpdate && app.serverConfigured`, regardless of `autoUpdate`.
  - On click:
    - status `t('Checking…')`
    - `upToDate` → `t('Glyphyard is up to date ({v})', { v })`
    - `ready` → refresh settings (`api.getSettings()`) so the existing line and the Restart button show
    - error → `errMsg(e)` in the `--bad` color
  - The "will be installed when you quit" line shows when `s.updateReady`, with or without autoUpdate. A `Restart now`
    button sits next to it.

- [ ] **Step 3: i18n and mock.**
  - Add Indonesian entries:
    - `'Glyphyard {v} is ready to install': 'Glyphyard {v} siap dipasang'`
    - `'Restart now': 'Restart sekarang'`
    - `'Check for updates': 'Cek update'`
    - `'Checking…': 'Mengecek…'`
    - `'Glyphyard is up to date ({v})': 'Glyphyard sudah versi terbaru ({v})'`
    - `'Dismiss'`, only if it is missing.
  - Add the mock commands.

- [ ] **Step 4: Verify.**
  - `cd apps/desktop/ui && bun run build`: passes.
  - `bun run lint`: clean.
  - `bun scripts/i18n-check.ts` (from the repo root): passes.
  - In the browser preview with the mock (`bun run dev` on its own port), the banner shows "Glyphyard 1.1.0 is ready to
    install". Settings › Check for updates shows the ready state. Take a screenshot.

- [ ] **Step 5: Commit.**
  - Message: `Update banner, Check for updates and Restart now`.
