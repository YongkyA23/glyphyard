# macOS release and auto-updates — design

## Why

Releases so far are Windows-only. The owner wants Mac users to get Glyphyard as a DMG they can open, and to get automatic updates from the team server the way Windows users do. The update system must stay the same one: the same Ed25519 key pair (created 2026-11-27 with `typeshelf-server update-keygen`, private key offline on the owner's PC) and the same `typeshelf-server update-sign` step. Windows apps already in the field must keep updating exactly as today.

## Decisions the owner made

- **No Apple Developer account yet.** The DMG ships unsigned (ad-hoc signed, below). The script still accepts signing and notarization inputs, so adding an account later is only a matter of passing them.
- **One universal build** (Apple Silicon + Intel in one app): one DMG, one update file, one server slot.
- **Built on the owner's own Mac.** CI stays Windows-only.
- **Approach: extend our own updater** (picked over Tauri's updater plugin and over "open the DMG for the user").

## Goals

1. `scripts/build-macos.sh` builds `dist/Glyphyard-<version>.dmg` (new installs) and `dist/Glyphyard-<version>.app.tar.gz` (the update file), with the server URL, install key and update key baked in exactly as `build-windows.ps1` does.
2. The server holds one release per platform: Windows and macOS.
3. A Mac app built with an update key checks daily, verifies, downloads, and replaces itself when the user quits.
4. Admin Settings › App updates shows both platforms' current releases and accepts uploads for either.
5. `docs/deploy.md` says how to cut a Mac release; `docs/api.md` documents the changed endpoints.

## Non-goals

- Linux builds or updates.
- Per-architecture Mac builds.
- A macOS CI job.
- Delta updates, rollback, or more than one release per platform.
- Updating the Windows setup program (`apps/setup`) in any way.

## Anti-goals (fails even if it works)

- An old Windows app (already installed) stops updating, or gets a different answer from `GET /update` than today.
- Anything unverified is ever unpacked or moved into place.
- A failed or interrupted Mac update leaves the user with no working app.
- A second key, a second signing tool, or a new crate or npm package.

## Approaches considered

1. **Extend our own updater (picked).** We add per-platform slots on the server, ship the Mac update as the zipped `.app`, and have the app swap its own bundle at quit. It's the most new code, but it is the only option that is fully automatic with one key and one signing command, and it leaves the Windows path untouched.
2. **Tauri's updater plugin.** It brings a new crate, a second key type (minisign), a second signing tool and a second server endpoint format. That's two update systems to guard. Rejected.
3. **Download the DMG and open it for the user.** It's the least code, but it isn't automatic. On an unsigned build the "downloaded from the internet" warning can come back with each update. Rejected.

## Design

### 1. Manifest carries the platform (`crates/core/src/update.rs`)

- **New field.** `Manifest` gets `platform: Platform`, where `enum Platform { Windows, Macos }`.
- **Windows text is unchanged.** A Windows manifest's canonical text stays byte-for-byte what it is today: four lines, no platform line. Old Windows apps parse it as before.
- **macOS adds one line.** A macOS manifest appends one line after `blake3 …`: `platform macos`. The header stays `typeshelf-update 1`. Old Windows apps (strict parsers) reject such a manifest, which is what we want.
- **Parsing.** `parse` accepts exactly these two canonical forms. No platform line means Windows; `platform macos` means macOS. Anything else, including `platform windows`, is `BadManifest`.
- **The signature covers it.** The signature is over the exact bytes, so the platform is covered by the signature too.
- **Constructor.** `Manifest::for_bytes(version, platform, bytes)` takes the platform. Existing callers pass `Platform::Windows`.

### 2. Signing (`typeshelf-server update-sign`)

- **The platform comes from the file name:**
  - `Glyphyard-Setup-<v>.exe` / `Typeshelf-Setup-<v>.exe` → Windows (today's behaviour).
  - `Glyphyard-<v>.app.tar.gz` → macOS.
- **New flag.** `--platform windows|macos` overrides the name. When neither the name nor the flag decides it, the command fails with a message naming both.
- **The version comes from the same name patterns**, or from `--version` as today.
- **Output is unchanged:** `<file>.manifest` and `<file>.manifest.sig`, and the printed line names the platform.

### 3. Server (`apps/server/src/update.rs`)

**Storage**
- Windows stays in `<data>/update/` (`setup.exe`, `manifest`, `signature`), so there is no migration.
- macOS goes in `<data>/update/macos/` (`app.tar.gz`, `manifest`, `signature`).

**Desktop routes**
- `GET /update` and `GET /update/file` take an optional query `platform`.
  - Absent or `windows` → the Windows slot, giving exactly today's responses.
  - `macos` → the Mac slot.
  - Any other value → 404 `no_release`. *(my call: an unknown platform simply has no release.)*
- The Mac file's `Content-Disposition` filename is `Glyphyard.app.tar.gz`.

**Admin routes**
- `GET /admin/update` changes shape to `{ windows: {version,size}|null, macos: {version,size}|null }`. The admin UI ships with the server, so nothing else reads the old shape.
- `POST /admin/update` takes the same three fields (`file`, `manifest`, `signature`). The server reads the platform from the manifest and stores the release in that slot.
  - There is no platform form field, so a release can't land in the wrong slot. *(my call)*
- The response adds `platform`, and the audit entry's detail becomes the platform (`windows` / `macos`); the target stays the version.
- The upload limit stays at 100 MB. A universal tarball is expected around 30–40 MB.
- The existing upload mutex covers both slots.

### 4. Sync client (`crates/sync/src/lib.rs`)

- `update_release` and `download_update` take a `platform: &str` argument.
- When it is `"windows"`, the request path is exactly `/update` / `/update/file`, with no query, as today.
- Otherwise `?platform=<p>` is appended.

### 5. Desktop (`apps/desktop/src-tauri/src/update.rs`)

**Which platforms update**
- `key()` returns the baked-in key on Windows and macOS (today: Windows only).
- A `const PLATFORM: Platform` is chosen by `cfg`.

**Check (shared by both platforms)**
- **Platform match.** After the signature verifies, a manifest whose `platform` isn't this app's is treated as "no release": `Ok(None)`, no Settings warning.
  - This covers a Mac app talking to an older server, which ignores `?platform=` and returns the Windows release.
- **File names.**
  - Windows: unchanged, `Typeshelf-Setup-<v>.exe`.
  - macOS: `Glyphyard-<v>.app.tar.gz`, and the partial download is `Glyphyard-update.part`.
- `finish`, `reuse` and `wanted` otherwise work as today.

**Install at quit, macOS (`on_quit` → `install_mac`)**
1. **Find the bundle.** It comes from `std::env::current_exe()`: `<X>.app/Contents/MacOS/typeshelf` → `<X>.app`. Skip with a log line if it doesn't end in `.app`.
2. **Check that it is writable.** Both the bundle and its parent folder must be writable by this process: moving a directory to another parent needs write permission on the directory itself. If not, skip, log it, and delete the update file. This happens when the app runs from the DMG, from App Translocation, or from an `/Applications` the user can't write. *(my call)*
   - The same check runs **before downloading**, so an app that can never apply an update doesn't download 40 MB a day.
   - In that case it sets `update_problem` to "Move Glyphyard to your Applications folder to get updates." (a new `t()` string with an Indonesian entry).
3. **Verify the file in memory.** Read the whole update file into memory (≈40 MB) and check it against the manifest. On a mismatch, delete it and stop.
   - Verifying and unpacking the same bytes means nothing can swap the file in between.
4. **Unpack into a staging folder.** Create the staging folder `<parent>/.glyphyard-update-<version>/`; it sits on the same volume, so renames are atomic. Pipe the bytes to `/usr/bin/tar -xzf - -C <staging>`.
   - The result must be exactly one `*.app` directory containing `Contents/MacOS/typeshelf`.
   - Otherwise delete the staging folder and stop.
5. **Swap atomically** with `renamex_np(<X>.app, <staging>/<new>.app, RENAME_SWAP)`. It's one libc call, declared with `extern "C"` under `cfg(target_os = "macos")`, so no crate is needed. *(my call: no moment exists without an app in place, even on power loss.)*
   - The new app takes the user's existing path and name, so Dock items and Login Items keep working.
   - A running macOS app may have its bundle swapped and deleted: the process keeps its open files.
   - **If the swap fails** (for example `EPERM` from macOS 13+ App Management, which can block an ad-hoc-signed app from changing an app in `/Applications`):
     - nothing has changed;
     - delete the staging folder, keep the downloaded file, and log it;
     - set `update_problem` to "macOS blocked the update. Allow Glyphyard under System Settings › Privacy & Security › App Management, then quit Glyphyard again."
     - A Developer ID–signed build is expected not to hit this.
   - On non-macOS (tests only), the swap is two renames with rollback.
6. **Clean up.** Delete the staging folder (including `old.app`) and the update file.
7. **Skip conditions.** Same gate as Windows (`should_launch`): updates still on, key present. There is no Setup mutex on macOS.
8. **Which quits count.** Any quit that reaches the existing `shutdown` → `on_quit` path (menu, tray, ⌘Q, Dock › Quit). If logout or shutdown cuts it short, nothing is lost: the verified file is kept and reused, and the next quit installs it. *(my call)*

No helper process and no shell script are needed: the swap happens inside the quitting app, before it exits.

**Relaunch** — *open question, see below.*

- **Testability.** Every step except reading `current_exe` and calling `tar` is written as plain functions that compile on all platforms:
  - bundle path from an exe path;
  - the staging check;
  - the rename-with-rollback.
- They are unit-tested on Windows CI. `tar` is present on Windows 10+ too, so the unpack test can run there. Only the `cfg(target_os = "macos")` call site is untested by CI.

### 6. Admin UI (`apps/admin-ui/src/pages/Settings.tsx`, `api.ts`)

- **Current releases.** The "App updates" card shows two lines, "Windows: version X (N MB)" / "macOS: version Y (N MB)", or "No release uploaded" for each.
- **One upload form.** The three file inputs stay. The file input accepts `.exe,.gz`, and the field label becomes "Release file". The result message names the platform the server stored it under.
- **Hint text.** The hint is updated to mention both platforms.
- **Language.** New strings go through `t()` with Indonesian entries in `src/i18n/id/`.

### 7. Build script (`scripts/build-macos.sh`)

**Flags and checks**
- It runs on a Mac with bash.
- Flags: `--server-url`, `--install-key`, `--update-key`, `--sign-identity "Developer ID Application: …"`.
- Validation matches `build-windows.ps1`: URL shape, install key required with a server URL, update key 64 hex, and env vars cleared when not given.

**Notarization**
- Notarization inputs come from env vars, which Tauri's CLI reads natively:
  - `APPLE_API_KEY`, `APPLE_API_ISSUER` and `APPLE_API_KEY_PATH`, or
  - `APPLE_ID`, `APPLE_PASSWORD` (app-specific) and `APPLE_TEAM_ID`.
- They are only used together with `--sign-identity`.

**Prerequisites it checks**
- Rust targets `aarch64-apple-darwin` and `x86_64-apple-darwin` are installed; if missing, it prints the `rustup target add` line.
- `bun` and Xcode command-line tools are present.

**Steps**
1. `bun install --frozen-lockfile` in `apps/desktop/ui`.
2. `bunx @tauri-apps/cli build --target universal-apple-darwin --bundles app,dmg --config <json>`.
   - The JSON sets `productName` to `Glyphyard`, so the bundle is `Glyphyard.app`. The identifier and app data location don't change.
   - Without `--sign-identity`, it also sets `bundle.macOS.signingIdentity` to `"-"` (ad-hoc), which Apple Silicon requires to run at all.
   - Tauri signs and, with the Apple env vars, notarizes and staples the `.app`.
3. **With notarization inputs:** `xcrun notarytool submit <dmg> --wait` then `xcrun stapler staple <dmg>`, so the DMG itself is notarized too.
4. **Copy out.**
   - The DMG goes to `dist/Glyphyard-<version>.dmg`.
   - `COPYFILE_DISABLE=1 tar -czf dist/Glyphyard-<version>.app.tar.gz -C <bundle/macos> Glyphyard.app`.
   - The version comes from `cargo pkgid`, as on Windows.
5. **Report.**
   - Without a signing identity it prints one line: `Unsigned build: no --sign-identity given, so the DMG is ad-hoc signed and Gatekeeper will warn on first open.`
   - It ends with the same summary line as Windows, plus the `update-sign` command for the `.app.tar.gz`.

### 8. Docs

- **`docs/deploy.md`** — a "Mac release" section:
  - prerequisites (Mac, Xcode CLT, Rust targets, bun);
  - the build command;
  - signing and notarizing later, once there's an Apple account;
  - what unsigned means for users: first open via System Settings › Privacy & Security › Open Anyway. Updates the app downloads itself aren't quarantined, so they don't prompt;
  - `update-sign` for the tarball and the upload;
  - the requirement to install in `/Applications` (or any folder the user can write) for updates to apply.
  - **Known costs of unsigned builds:**
    - macOS 13+ may ask once to allow Glyphyard under App Management before an update can apply.
    - Each ad-hoc build is a new code identity, so folder-access prompts (Documents, Desktop, external drives) can come back after an update.
    - Both go away with a Developer ID build.
- **`docs/api.md`** — the `platform` query on the desktop routes, and the new admin `GET`/`POST /update` shapes.

## Relaunch (owner's decision)

**No relaunch after a Mac update**, matching Windows: the update installs at quit and the next launch is the new version. This departs from the original brief's "relaunch". Reasons: the app would otherwise reopen right after the user quit it, including during logout or shutdown, and it would need a helper process.

## Mac verification (owner's Mac)

Version 0.1.0 has run fine on the owner's Mac; the app has changed a lot since. The plan ends with a short checklist for the owner:

1. Run `cargo check` / `cargo clippy` on the Mac.
2. Build 0.3.x with `build-macos.sh` and install it from the DMG.
3. Smoke test: activate a font, sync.
4. Bump the version, build and sign, upload to the macOS slot.
5. Quit, then confirm the next launch is the new version.

## Left to the builder

- Exact log wording.
- Admin layout details within the existing card styles.
- Test names and fixtures.
- The exact JSON passed to `--config`.
