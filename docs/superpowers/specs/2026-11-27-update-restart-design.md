# Clearer app updates: check often, "Restart to update" — design

## Why

Auto-update works but nobody can tell. The app checks a minute after start and then once a day, and installs only
at a full Quit. Since the close button now hides the app to the tray, a full Quit is rare. On 2026-11-27 the owner
uploaded 0.3.1, quit and reopened on Windows and with ⌘Q on macOS, and both still ran 0.3.0: neither had checked
yet. They ran Setup by hand.

## Goals

- A new release reaches a running app within an hour, or right away with a "Check for updates" button.
- When an update is ready, the user sees it and can install it with one click that brings the app back on the new
  version.
- Startup never waits on the network. The app often starts at login into the tray.

## Non-goals

- A Steam-style "Updating…" screen at launch (approach A, below).
- Download progress in the UI.
- Changing the server, the manifest format, signing or verification.

## Approaches

- **A. Check at launch, before the window shows.** Clearest, but every start waits on the server, and a login start
  into the tray would pop up an updater. Rejected.
- **B. Download in the background, then offer "Restart to update" (picked by the owner).** Checks never block
  anything, and a banner plus a tray item make a ready update visible.

## Design (B)

1. **When it checks.** First check about 15 s after start (sync has registered by then), then every hour instead of
   every day. Failures already retry hourly. The same gate applies: a key is baked in, a server is set up, the app
   is registered, and "Update automatically" is on.
2. **"Check for updates" button** in Settings, next to "Update automatically". Shown when the build has an update
   key (`canUpdate`), whether or not the switch is on. It runs one check now and shows the result in the line under
   the switch:
   - "Checking…"
   - "Glyphyard is up to date (0.3.1)"
   - "Version X is ready", with the restart button
   - the error, for example offline or "could not be verified"
   A manual check also downloads when the switch is off: pressing the button is consent.
3. **Update ready.** When a check marks a release ready, the backend emits `update-ready` with `{ version }`. Then:
   - **Banner** across the top of the main window: "Glyphyard X is ready to install · [Restart now]". It can be
     dismissed for this session.
   - **Tray menu** gains "Restart to update (X)" above Quit.
   - **Settings** keeps today's "will be installed when you quit" line and adds the same Restart button.
4. **Restart now** (new command `restart_to_update`) marks "relaunch after install" and quits the app. The normal
   `shutdown` runs, then `on_quit` installs. A Restart click installs even when the switch is off. A plain Quit keeps
   today's gate.
   - **Windows:** start the downloaded Setup with `--silent --launch`. The new Setup flag `--launch` starts the
     installed app after a successful silent install. Older Setups ignore unknown flags, so the relaunch works for
     updates to 0.3.2 and later.
   - **macOS:** after a successful bundle swap, spawn a detached `sh` that waits for this process to exit (needed for
     the single-instance plugin), then runs `open <bundle>`.
   - If the install is skipped or fails, still relaunch the current app, so Restart never leaves the user with no
     app.
5. **Plain Quit** behaves as it does today: it installs a ready update and does not relaunch.

## Left to the builder

- Where the banner component lives, and its look within the existing UI kit.
- How the tray item appears: insert it into the menu, or keep it and toggle it enabled.
- Indonesian strings in `ui/src/i18n/id`, and mock entries in `ui/src/mock.ts`.
- Tests:
  - the check interval and gate stay pure and unit-tested;
  - a unit test for Setup's `--launch` parsing;
  - the relaunch waiter on macOS is untested by CI, like the swap call.
