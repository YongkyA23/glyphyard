# macOS Release and Auto-Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Glyphyard for macOS as a universal DMG and let Mac apps update themselves from the team server with the same Ed25519 key and `update-sign` step as Windows.

**Architecture:**
- The signed manifest gains a platform; only macOS manifests carry the new line.
- The server keeps one release per platform. The desktop routes select the slot with `?platform=`, and no query means Windows.
- The Mac app verifies the `.app.tar.gz` in memory, unpacks it next to its own bundle and swaps the bundle atomically at quit. It does not relaunch.

**Tech Stack:** Rust (axum server, Tauri 2 desktop, `typeshelf-core`, `typeshelf-sync`), React + TS admin UI (bun, oxlint), bash on macOS for the build script.

**Spec:** `docs/superpowers/specs/2026-11-27-macos-release-design.md`

## Global Constraints

**Compatibility**
- Windows manifest text stays byte-for-byte: `typeshelf-update 1\nversion V\nsize N\nblake3 H\n`.
- A macOS manifest is that text plus `platform macos\n`. The header stays `typeshelf-update 1`.
- `GET /update` and `GET /update/file` with no query return exactly today's responses (the Windows slot, `<data>/update/setup.exe|manifest|signature`).
- The Windows desktop path (`launch`, `--silent`, Setup mutex, `Typeshelf-Setup-<v>.exe` names) is unchanged.

**Dependencies**
- No new crates and no new npm packages.
- `renamex_np` and `access` are declared with `extern "C"` under `cfg`.

**Storage and limits**
- Mac slot: `<data>/update/macos/` with `app.tar.gz`, `manifest`, `signature`.
- Upload limit stays at 100 MB (`MAX_UPLOAD`).

**File names**
- Mac update file: `Glyphyard-<version>.app.tar.gz`.
- DMG: `dist/Glyphyard-<version>.dmg`.
- The Mac bundle is built as `Glyphyard.app` via `--config` `productName`. `tauri.conf.json` is not edited, because its productName names the Windows Run value.

**Behaviour**
- No relaunch after a Mac update.
- UI strings go through `t()` with Indonesian entries in `src/i18n/id/`.
- **Checks that must pass:**
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - admin and desktop UI: `bunx tsc --noEmit -p tsconfig.app.json`, `bun src/i18n/i18n.check.ts`, `bun ../../scripts/i18n-check.ts src` (desktop: `../../../scripts/...`), `bunx oxlint`
  - CI creates `apps/desktop/ui/dist` and `plugins/dist` first; do the same locally if they are missing.

**Exact strings** (keep the English text verbatim as keys)
- `MOVE`: `Move Glyphyard to your Applications folder to get updates.`
- `BLOCKED`: `macOS blocked the update. Allow Glyphyard under System Settings › Privacy & Security › App Management, then quit Glyphyard again.`
- Unsigned build line: `Unsigned build: no --sign-identity given, so the DMG is ad-hoc signed and Gatekeeper will warn on first open.`

## Review Focus

1. **Mac app on an older server.** The server ignores `?platform=macos` and returns the Windows release. The Mac app must ignore it silently: `Ok(None)`, no Settings warning. *Test in Task 3.*
2. **A Mac release uploaded while old Windows apps are in the field.** It must land in the Mac slot only. `GET /update` with no query must still return the Windows manifest. *Test in Task 2.*
3. **A tarball whose contents aren't a single app.** Two `.app` dirs, no `Contents/MacOS/typeshelf`, or a path outside staging: nothing is swapped, staging is removed, and the old bundle is untouched. *Test in Task 4.*
4. **A swap that fails.** The old bundle must still be at its path with its contents, and the downloaded file kept only when the failure is "blocked". *Test in Task 4.*
5. **`update-sign` on a file whose name decides no platform** (e.g. `release.tar.gz`). It fails with a message naming `--platform`, rather than guessing Windows. *Test in Task 1.*

---

### Task 1: Manifest platform + `update-sign`

**Files:**
- Modify: `crates/core/src/update.rs`: `Manifest`, `for_bytes`, `to_text`, `parse`, tests.
- Modify: `apps/server/src/main.rs:51-60,105-118`: the `UpdateSign` args and handler.
- Modify the callers of `for_bytes` to pass `Platform::Windows`:
  - `apps/desktop/src-tauri/src/update.rs:298,385`
  - `apps/server/tests/api.rs:1655`

**Interfaces:**
- Produces in `typeshelf_core::update`:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Platform { Windows, Macos }`
  - `impl Platform { pub fn as_str(self) -> &'static str /* "windows" | "macos" */; pub fn from_name(s: &str) -> Option<Platform> }`
  - `Manifest { version, size, blake3, pub platform: Platform }`
  - `Manifest::for_bytes(version: Version, platform: Platform, bytes: &[u8]) -> Manifest`
- Produces in `apps/server/src/main.rs`:
  - `fn platform_and_version(file_name: &str) -> (Option<Platform>, Option<&str>)`, a pure helper used by the `UpdateSign` handler.
  - It reads:
    - `Glyphyard-Setup-<v>.exe` / `Typeshelf-Setup-<v>.exe` → `(Some(Windows), Some(v))`
    - `Glyphyard-<v>.app.tar.gz` → `(Some(Macos), Some(v))`
    - anything else → `(None, None)`

- [ ] **Step 1: Write the failing core tests** in `crates/core/src/update.rs` tests:

```rust
#[test]
fn windows_manifest_text_is_unchanged() {
    let m = Manifest::for_bytes(Version::parse("1.2.3").unwrap(), Platform::Windows, b"x");
    assert_eq!(m.to_text(), format!("typeshelf-update 1\nversion 1.2.3\nsize 1\nblake3 {}\n", m.blake3));
    assert_eq!(Manifest::parse(m.to_text().as_bytes()).unwrap().platform, Platform::Windows);
}

#[test]
fn macos_manifest_adds_one_line_and_round_trips() {
    let m = Manifest::for_bytes(Version::parse("1.2.3").unwrap(), Platform::Macos, b"x");
    assert_eq!(m.to_text(), format!("typeshelf-update 1\nversion 1.2.3\nsize 1\nblake3 {}\nplatform macos\n", m.blake3));
    assert_eq!(Manifest::parse(m.to_text().as_bytes()), Ok(m));
}

#[test]
fn other_platform_lines_are_refused() {
    let w = Manifest::for_bytes(Version::parse("1.2.3").unwrap(), Platform::Windows, b"x").to_text();
    for extra in ["platform windows\n", "platform linux\n", "platform macos\nmore\n", "platform  macos\n"] {
        assert_eq!(Manifest::parse(format!("{w}{extra}").as_bytes()), Err(UpdateError::BadManifest), "{extra:?}");
    }
}
```

- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p typeshelf-core update`. Expected: compile error (`Platform` not found, `for_bytes` arity).

- [ ] **Step 3: Implement the core changes.**
  - `to_text` appends `platform macos\n` only for `Macos`.
  - `parse` reads an optional 5th field `platform`, accepting only `macos`. It keeps the existing byte-for-byte canonical check: re-render and compare.
  - Update the existing core tests' `for_bytes` calls to pass `Platform::Windows`.

- [ ] **Step 4: Write the failing CLI test** at the bottom of `apps/server/src/main.rs` (`#[cfg(test)] mod tests`):

```rust
#[test]
fn update_sign_reads_platform_and_version_from_the_name() {
    assert_eq!(platform_and_version("Glyphyard-Setup-0.4.0.exe"), (Some(Platform::Windows), Some("0.4.0")));
    assert_eq!(platform_and_version("Typeshelf-Setup-0.1.0.exe"), (Some(Platform::Windows), Some("0.1.0")));
    assert_eq!(platform_and_version("Glyphyard-0.4.0.app.tar.gz"), (Some(Platform::Macos), Some("0.4.0")));
    assert_eq!(platform_and_version("release.tar.gz"), (None, None));
}
```

- [ ] **Step 5: Implement the CLI changes.**
  - `UpdateSign` gets `#[arg(long)] platform: Option<String>`.
  - The platform is `--platform` (via `Platform::from_name`), else taken from the name.
  - When neither decides it: `bail!("say which platform with --platform windows|macos (or name the file Glyphyard-Setup-<version>.exe / Glyphyard-<version>.app.tar.gz)")`.
  - The version error message names both file patterns.
  - The final println adds `for {platform}`.
  - Fix the remaining `for_bytes` callers (desktop tests, server test helper) to pass `Platform::Windows`.

- [ ] **Step 6: Run** `cargo test -p typeshelf-core -p typeshelf-server` and `cargo test -p typeshelf-desktop update`. Expected: all pass.

- [ ] **Step 7: Commit** with `git commit -m "Update manifests carry the platform; update-sign signs Mac releases"`.

---

### Task 2: Server per-platform slots

**Files:**
- Modify: `apps/server/src/update.rs` (whole module).
- Test: `apps/server/tests/api.rs`, the update tests around `:1650-1740`.

**Interfaces:**
- Consumes: `Platform`, `Manifest::platform`, `Manifest::for_bytes(v, platform, bytes)` (Task 1).
- Produces the HTTP API:
  - `GET /api/v1/update[?platform=windows|macos]` and `GET /api/v1/update/file[?platform=…]`.
    - No query or `windows` → `<data>/update/`.
    - `macos` → `<data>/update/macos/`.
    - Any other value → 404 `no_release`.
    - The Mac file's `Content-Disposition` is `attachment; filename="Glyphyard.app.tar.gz"`.
  - `GET /api/admin/update` → `{ "windows": {version,size}|null, "macos": {version,size}|null }`.
  - `POST /api/admin/update` → `{ version, size, platform }`.
    - The slot comes from `manifest.platform`; there is no form field.
    - Audit: target = version, detail = `platform.as_str()`.

- [ ] **Step 1: Write the failing tests.**
  - Change `signed_release(version, file)` to `signed_release(version, platform, file)`.
  - Update `update_release_is_stored_as_uploaded_and_served_to_installs`:
    - admin GET before any upload → `json!({"windows": null, "macos": null})`;
    - upload response → `json!({"version":"0.2.0","size":file.len(),"platform":"windows"})`;
    - admin GET → `{"windows": {...}, "macos": null}`;
    - audit detail → `Some("windows")`.
  - Update the other tests that read admin GET as `Value::Null` to expect `{"windows": null, "macos": null}`.
  - Add:

```rust
#[tokio::test]
async fn mac_release_has_its_own_slot_and_old_clients_still_get_windows() {
    let s = Server::new().await;
    let token = s.register("a", "Ana").await;
    use typeshelf_core::update::Platform;
    let win = b"MZ windows".repeat(100);
    let (wm, ws, _) = signed_release("0.3.0", Platform::Windows, &win);
    let mac = b"\x1f\x8b mac".repeat(100);
    let (mm, ms, _) = signed_release("0.4.0", Platform::Macos, &mac);
    assert_eq!(s.upload_update(&[("file", &win), ("manifest", wm.as_bytes()), ("signature", ws.as_bytes())], &s.cookie).await.status, StatusCode::OK);
    let r = s.upload_update(&[("file", &mac), ("manifest", mm.as_bytes()), ("signature", ms.as_bytes())], &s.cookie).await;
    assert_eq!(r.json()["platform"], "macos");
    // Old clients: no query, Windows release unchanged.
    assert_eq!(s.desktop(Method::GET, "/update", &token, None).await.json()["manifest"], wm.as_str());
    assert_eq!(s.desktop(Method::GET, "/update/file", &token, None).await.body, win);
    assert_eq!(s.desktop(Method::GET, "/update?platform=windows", &token, None).await.json()["manifest"], wm.as_str());
    // Mac clients.
    assert_eq!(s.desktop(Method::GET, "/update?platform=macos", &token, None).await.json()["manifest"], mm.as_str());
    assert_eq!(s.desktop(Method::GET, "/update/file?platform=macos", &token, None).await.body, mac);
    assert_eq!(s.desktop(Method::GET, "/update?platform=linux", &token, None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s.admin(Method::GET, "/update", None).await.json(),
        json!({ "windows": {"version":"0.3.0","size":win.len()}, "macos": {"version":"0.4.0","size":mac.len()} }));
}
```

- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p typeshelf-server --test api update`. Expected: FAIL on the shape assertions.

- [ ] **Step 3: Implement it in `apps/server/src/update.rs`.**
  - A `fn slot(s: &AppState, p: Platform) -> (PathBuf, &'static str /* file name */)` returns `(data/update, "setup.exe")` or `(data/update/macos, "app.tar.gz")`.
  - Desktop handlers take `Query<PlatformQuery { platform: Option<String> }>`.
  - `read` takes the slot dir.
  - Upload picks the slot from `m.platform` and writes `.new` files and renames there. The `file_mismatch` message says "this release file".
  - Update the module doc comment.

- [ ] **Step 4: Run** `cargo test -p typeshelf-server`. Expected: all pass.

- [ ] **Step 5: Commit** with `git commit -m "Server keeps one app release per platform"`.

---

### Task 3: Sync client platform + desktop check

**Files:**
- Modify: `crates/sync/src/lib.rs:316-370`, `update_release` and `download_update`.
- Modify: `apps/desktop/src-tauri/src/update.rs`: `key`, `check`, `finish`, `reuse`, `run`, plus the problem messages.
- Modify: `apps/desktop/ui/src/pages/Settings.tsx:301`, which renders `{t(s.updateProblem /* i18n-dynamic */)}`.
- Modify: `apps/desktop/ui/src/i18n/id/settings.ts`, adding Indonesian for the existing `PROBLEM` text plus `MOVE` and `BLOCKED`.

**Interfaces:**
- Consumes: `Platform` (Task 1). The server API (Task 2).
- Produces in `typeshelf_sync::SyncClient`:
  - `update_release(&self, platform: Platform) -> Result<Option<(String, String)>, SyncError>`
  - `download_update(&self, to: &Path, max: u64, platform: Platform) -> Result<(), SyncError>`
  - Windows uses path `/update` / `/update/file` with no query; macOS appends `?platform=macos`.
- Produces in desktop `update`:
  - `pub const PLATFORM: Platform`, `Macos` under `cfg(target_os = "macos")` and `Windows` otherwise.
  - `pub fn file_name(platform: Platform, version: Version) -> String`: `Typeshelf-Setup-<v>.exe` or `Glyphyard-<v>.app.tar.gz`.
  - `check(client, key, dir, current, have, platform: Platform)`.
  - `finish(part, manifest, dir)` and `reuse(dir, manifest)` use `file_name(manifest.platform, …)`.
  - `pub const MOVE: &str`, `pub const BLOCKED: &str` (exact text in Global Constraints).
  - `key()` returns the key on `cfg!(any(windows, target_os = "macos"))`.
- **Startup (`run`)**:
  - Before the loop, if `<dir>/blocked` exists, set `problem = Some(BLOCKED)` and remove the marker.
  - In the loop, before `check`, on macOS: when `crate::mac_install::blocker()` (Task 4) returns `Some(msg)`, set the problem to `msg` and `continue`.
  - Use `#[cfg(target_os = "macos")]` around that call. Task 3 lands first, so add a stub `fn blocker() -> Option<&'static str> { None }` in a new `mac_install.rs` that Task 4 replaces.

- [ ] **Step 1: Write the failing desktop tests** in the `update.rs` tests:
  - `manifest_for(bytes)` gains a platform parameter: `manifest_for(platform, bytes)`.

```rust
#[test]
fn a_release_for_another_platform_is_no_release() {
    // An older server ignores ?platform= and returns the Windows release to a Mac app.
    let dir = tempfile_dir("otherplatform");
    let (private, public) = update::generate_key().unwrap();
    let key = update::public_key(Some(&public)).unwrap();
    let text = Manifest::for_bytes(Version::parse("9.0.0").unwrap(), Platform::Windows, b"file").to_text();
    let (sig, _) = update::sign(&private, text.as_bytes()).unwrap();
    let m = update::verify_manifest(&key, text.as_bytes(), &sig).unwrap();
    assert!(!for_this_app(&m, Platform::Macos));
    assert!(for_this_app(&m, Platform::Windows));
    assert!(!dir.join("update").exists());
}

#[test]
fn mac_update_files_are_named_for_the_tarball() {
    assert_eq!(file_name(Platform::Macos, Version::parse("1.0.0").unwrap()), "Glyphyard-1.0.0.app.tar.gz");
    assert_eq!(file_name(Platform::Windows, Version::parse("1.0.0").unwrap()), "Typeshelf-Setup-1.0.0.exe");
}
```

  - The platform match is the pure `pub fn for_this_app(m: &Manifest, platform: Platform) -> bool`. `check` calls it right after `verify_manifest`, and returns `Ok(None)` without touching `problem` when it is false.
  - Update `finish`/`reuse` tests to cover a Mac manifest producing `Glyphyard-9.0.0.app.tar.gz`.

- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p typeshelf-desktop update`. Expected: compile errors (`for_this_app`, `file_name`).

- [ ] **Step 3: Implement the sync and desktop changes** to the interfaces above. `run` passes `PLATFORM` to `check`. The partial download name is `Glyphyard-update.part` on macOS and `Typeshelf-Setup.part` on Windows, and `run` removes the right one.

- [ ] **Step 4: Update the desktop UI.**
  - Render the problem with `t(... /* i18n-dynamic */)`.
  - Add Indonesian entries for the three Rust strings.
  - Run the desktop UI checks (tsc, i18n.check, i18n-check, oxlint) in `apps/desktop/ui`. Expected: clean.

- [ ] **Step 5: Run** `cargo test -p typeshelf-sync -p typeshelf-desktop`. Expected: pass.

- [ ] **Step 6: Commit** with `git commit -m "Desktop asks for its own platform's release and ignores others"`.

---

### Task 4: Mac install at quit

**Files:**
- Create/replace: `apps/desktop/src-tauri/src/mac_install.rs`. It compiles on every platform; only the call sites are cfg-gated.
- Modify: `apps/desktop/src-tauri/src/update.rs`, where `on_quit` branches by `PLATFORM`.
- Modify: `apps/desktop/src-tauri/src/main.rs` for `mod mac_install;`.

**Interfaces:**
- Consumes: `Ready`, `PLATFORM`, `MOVE`, `BLOCKED`, `dir()` (Task 3). `update::check_file`.
- Produces in `mac_install`:
  - `pub fn bundle_of(exe: &Path) -> Option<PathBuf>`: `<X>.app/Contents/MacOS/<bin>` → `<X>.app`; `None` unless the 3rd ancestor's name ends with `.app`.
  - `pub fn can_replace(bundle: &Path) -> bool`: the bundle and its parent are both writable.
    - On unix this uses `access(path, W_OK=2) == 0` via `extern "C"`.
    - Elsewhere it is `!metadata.permissions().readonly()`.
  - `pub fn blocker() -> Option<&'static str>`: `current_exe` → `bundle_of` → `can_replace`. It returns `Some(update::MOVE)` when the app can't be replaced.
  - `pub enum InstallError { Blocked(String), Failed(String) }` with Display.
  - `pub fn install(ready: &Ready, bundle: &Path) -> Result<(), InstallError>`. Steps:
    1. Read the file into memory and `check_file`. On a mismatch, delete the file and return `Failed`.
    2. Create `staging = <parent>/.glyphyard-update-<version>`, after removing any leftover one.
    3. Pipe the bytes to `tar -xzf - -C <staging>`. Use `/usr/bin/tar` on macOS and `tar` elsewhere (tests).
    4. `pub fn new_app(staging: &Path) -> Option<PathBuf>`: exactly one entry, a dir named `*.app`, containing `Contents/MacOS/typeshelf`.
    5. `swap(bundle, new_app)`.
    6. Remove the staging folder and the update file.
    - **On any failure after step 2,** remove the staging folder.
    - **Error mapping:** `swap` failing with `PermissionDenied` / `EPERM` → `Blocked` (keep the update file). Every other failure → `Failed` (delete the update file).
  - `fn swap(a: &Path, b: &Path) -> std::io::Result<()>`:
    - On macOS: `renamex_np(a, b, RENAME_SWAP = 0x2)`.
    - Elsewhere: rename `a`→`a.swap`, `b`→`a`, `a.swap`→`b`, restoring `a` if the second rename fails.
- **`update::on_quit`, macOS branch**, after the existing `should_launch(key, auto_update, false)` gate:

```rust
let Some(bundle) = std::env::current_exe().ok().as_deref().and_then(mac_install::bundle_of).filter(|b| mac_install::can_replace(b)) else { /* log, delete file */ return };
match mac_install::install(&ready, &bundle) {
    Ok(()) => tracing::info!(...),
    Err(InstallError::Blocked(e)) => { tracing::warn!(...); let _ = std::fs::write(dir.join("blocked"), b""); }
    Err(e) => tracing::warn!(...),
}
```

  The Windows branch is today's code, untouched.

- [ ] **Step 1: Write the failing tests** in `mac_install.rs`.
  - Use a `fake_app(dir, name)` helper that creates `name/Contents/MacOS/typeshelf` with given bytes.
  - Use a `tarball(src_dir, name) -> Vec<u8>` helper that runs `tar -czf - -C src_dir name`. Windows 10+ ships `tar`.

```rust
#[test] fn bundle_path_comes_from_the_exe() {
    assert_eq!(bundle_of(Path::new("/Applications/Glyphyard.app/Contents/MacOS/typeshelf")), Some(PathBuf::from("/Applications/Glyphyard.app")));
    assert_eq!(bundle_of(Path::new("/usr/local/bin/typeshelf")), None);
}
#[test] fn a_verified_tarball_replaces_the_bundle() {
    // old Glyphyard.app with bin "old"; tarball of Glyphyard.app with bin "new"; Ready with Platform::Macos manifest
    // install(&ready, &old_bundle) is Ok; old_bundle/Contents/MacOS/typeshelf == b"new";
    // no ".glyphyard-update-*" left in the parent; the update file is gone.
}
#[test] fn a_tampered_tarball_changes_nothing() {
    // the file differs from the manifest → Err(Failed); the old bundle is unchanged; no staging; the file is deleted.
}
#[test] fn a_tarball_without_exactly_one_app_changes_nothing() {
    // two apps; one dir without Contents/MacOS/typeshelf → Err(Failed), the old bundle unchanged, no staging left.
}
#[test] fn a_failed_swap_keeps_the_old_bundle() {
    // swap(missing_path, existing) is Err, and `existing` is still there with its contents.
}
#[test] fn an_unwritable_folder_cannot_be_replaced() {
    // set the parent dir readonly via permissions → !can_replace(bundle); restore permissions after.
}
```

  Fill in the bodies from the comments; the assertions are as written there.

- [ ] **Step 2: Run them to verify they fail.** Run `cargo test -p typeshelf-desktop mac_install`. Expected: compile errors.

- [ ] **Step 3: Implement** `mac_install.rs` to the interfaces above, and the `on_quit` macOS branch.
  - Mark the in-memory read: `// ponytail: reads the whole update (~40 MB) so the verified bytes are the unpacked bytes`.
  - Add `#[cfg_attr(not(target_os = "macos"), allow(dead_code))]` where clippy flags items only used on macOS.

- [ ] **Step 4: Run** `cargo test -p typeshelf-desktop` and `cargo clippy --workspace --all-targets -- -D warnings`. Expected: pass and clean.

- [ ] **Step 5: Commit** with `git commit -m "Mac app installs a verified update by swapping its bundle at quit"`.

---

### Task 5: Admin UI per-platform releases

**Files:**
- Modify: `apps/admin-ui/src/api.ts:76,187-189`
- Modify: `apps/admin-ui/src/pages/Settings.tsx:350-395` (the `Updates` component and `UPDATE_FIELDS`)
- Modify: `apps/admin-ui/src/i18n/id/admin.ts`, `common.ts`

**Interfaces:**
- Consumes: the admin API from Task 2.
- Produces:
  - `export type UpdateRelease = { version: string; size: number }`
  - `export type UpdateReleases = { windows: UpdateRelease | null; macos: UpdateRelease | null }`
  - `updateRelease: () => request<UpdateReleases>('GET', '/update')`
  - `uploadUpdate: (form) => request<UpdateRelease & { platform: 'windows' | 'macos' }>('POST', '/update', form)`

**Steps**
- [ ] **Step 1: Implement the admin changes.**
  - **Release lines.** The card shows two lines, "Windows: version {version} ({size} MB)" / "macOS: version {version} ({size} MB)". When a slot is empty it shows "{platform}: no release uploaded". Below them: "Desktop apps keep the version they have."
  - **After an upload,** only that slot updates.
  - **File field.** The first `UPDATE_FIELDS` entry becomes `['file', tk('Release file'), '.exe,.gz']`.
  - **Validation error** text: "Choose the release file, its manifest and its signature."
  - **Success text:** "Version {version} for {platform} is now offered to every computer." `{platform}` is shown as "Windows" or "macOS"; those are not translated.
  - **Hint text:** "Sign the Windows setup program or the Mac .app.tar.gz on the computer that keeps the private key, with typeshelf-server update-sign; the server files it under the platform its manifest names. Apps built with the matching public key check once a day, check the signature and the file, and install the new version when the user quits Glyphyard. The server refuses a file that doesn't match its manifest."
  - **Indonesian:** add entries for every new key, and remove entries for keys no longer used.
- [ ] **Step 2: Run the checks** in `apps/admin-ui`: `bunx tsc --noEmit -p tsconfig.app.json && bun src/i18n/i18n.check.ts && bun ../../scripts/i18n-check.ts src && bunx oxlint`. Expected: clean.
- [ ] **Step 3: Check it in the browser.** Run the server with the admin UI per `.claude/launch.json` (create it if missing), open Settings › App updates, and screenshot both lines (empty state).
- [ ] **Step 4: Commit** with `git commit -m "Admin shows and uploads app releases per platform"`.

---

### Task 6: `build-macos.sh` + docs + full run

**Files:**
- Create: `scripts/build-macos.sh` (mode 755: `git update-index --chmod=+x`)
- Modify: `docs/deploy.md`, "Installer for this server" (around `:55-80`), adding a "Mac release" subsection
- Modify: `docs/api.md:27-28,108-109`
- Modify: `scripts/build-windows.ps1`, only its last line: it names "upload all three files in admin Settings › App updates" (unchanged text is fine; edit only if it mentions a single release)

**Interfaces:** consumes `update-sign` naming (Task 1) and the admin upload (Task 5).

- [ ] **Step 1: Write `scripts/build-macos.sh`.**

  **Setup and arguments**
  - `#!/usr/bin/env bash`, `set -euo pipefail`.
  - A usage block mirroring the ps1 `.SYNOPSIS`.
  - Flags `--server-url`, `--install-key`, `--update-key`, `--sign-identity`, `-h`.
  - Validation identical to `build-windows.ps1`:
    - an install key without a server URL is an error;
    - the URL must start with `^https?://`;
    - a server URL needs an install key;
    - the update key must match `^[0-9a-fA-F]{64}$`.
  - It exports `TYPESHELF_SERVER_URL`, `TYPESHELF_INSTALL_KEY` and `TYPESHELF_UPDATE_KEY`, or unsets each one when not given.

  **Prerequisite checks**
  - It must run on macOS: `uname` = `Darwin`.
  - `bun` must be present.
  - `xcode-select -p` must succeed.
  - `rustup target list --installed` must contain both `aarch64-apple-darwin` and `x86_64-apple-darwin`; otherwise print `rustup target add aarch64-apple-darwin x86_64-apple-darwin` and exit 1.

  **Signing**
  - With `--sign-identity`: `export APPLE_SIGNING_IDENTITY`.
  - Notarization runs only if `APPLE_API_KEY`+`APPLE_API_ISSUER`+`APPLE_API_KEY_PATH`, or `APPLE_ID`+`APPLE_PASSWORD`+`APPLE_TEAM_ID`, are set. Tauri reads these itself.
  - Without `--sign-identity`: unset `APPLE_SIGNING_IDENTITY` and the notarization vars, add `"bundle":{"macOS":{"signingIdentity":"-"}}` to the config JSON, and echo the exact unsigned line.

  **Build**
  - `(cd apps/desktop/ui && bun install --frozen-lockfile)`.
  - `(cd apps/desktop/src-tauri && bunx @tauri-apps/cli build --target universal-apple-darwin --bundles app,dmg --config "$CONFIG")`, where `CONFIG` is `{"productName":"Glyphyard"[,"bundle":{"macOS":{"signingIdentity":"-"}}]}`.

  **Outputs**
  - Version: `cargo pkgid --manifest-path Cargo.toml -p typeshelf-desktop | sed 's/.*[#@]//'`.
  - `BUNDLE=target/universal-apple-darwin/release/bundle`.
  - Copy `$BUNDLE/dmg/Glyphyard_*_universal.dmg` to `dist/Glyphyard-$VERSION.dmg`.
  - With a signing identity and notarization vars:
    - `xcrun notarytool submit dist/Glyphyard-$VERSION.dmg --wait`, using either `--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER"` or `--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID"`;
    - then `xcrun stapler staple dist/Glyphyard-$VERSION.dmg`.
    - No command line containing a password is echoed.
  - `COPYFILE_DISABLE=1 tar -czf dist/Glyphyard-$VERSION.app.tar.gz -C "$BUNDLE/macos" Glyphyard.app`.

  **Final output**
  - `DMG: <path>  (version V, server: …, updates: on|off (no --update-key), signed: <identity>|ad-hoc)`.
  - When there is an update key: `To publish the update: typeshelf-server update-sign "dist/Glyphyard-$VERSION.app.tar.gz" --key <private key file> (on the computer that keeps it), then upload the three files in admin Settings › App updates.`

- [ ] **Step 2: Check the script.** Run `bash -n scripts/build-macos.sh`; expected: no output. If `shellcheck` is installed, run `shellcheck scripts/build-macos.sh` and fix any warnings.

- [ ] **Step 3: Write the docs.**

  **`docs/deploy.md`, "Mac release":**
  - prerequisites (Mac, Xcode CLT, `rustup target add …`, bun);
  - the build command example;
  - the same server URL, install key and update public key as Windows;
  - the private key stays on the signing PC: carry the `.app.tar.gz` there, run `update-sign`, and upload;
  - signing later: `--sign-identity "Developer ID Application: …"` plus either set of Apple env vars;
  - unsigned consequences:
    - first open via System Settings › Privacy & Security › Open Anyway;
    - App Management may need allowing once;
    - folder-access prompts may return after updates;
  - install into `/Applications` (or a folder the user can write) for updates to apply;
  - no relaunch: the new version starts next time.

  **`docs/api.md`:**
  - the `platform` query on both desktop routes;
  - the admin GET shape `{ windows, macos }`;
  - POST: the slot comes from the manifest, the response adds `platform`, and the audit detail is the platform;
  - the manifest's optional `platform macos` line.

- [ ] **Step 4: Run the full verification.** Create `apps/desktop/ui/dist` and `plugins/dist` if they are missing, then run:
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - the desktop UI and admin UI checks from Global Constraints.

  Expected: all pass. Paste the tail of each.

- [ ] **Step 5: Commit** with `git commit -m "build-macos.sh and docs for cutting a Mac release"`.

---

### Owner's Mac checklist (not automatable here; listed in the final report)

1. **Check the code compiles on the Mac.** Run `cargo check -p typeshelf-desktop` and `cargo clippy -p typeshelf-desktop --all-targets -- -D warnings`. The `cfg(target_os = "macos")` code has never been compiled by CI.
2. **Build and install.** Run `./scripts/build-macos.sh --server-url … --install-key … --update-key …`, open the DMG, drag the app to Applications, and do "Open Anyway". Then activate a font and check that sync connects.
3. **Test an update.** Bump the version, build again, `update-sign` the `.app.tar.gz` on the PC that holds the key, and upload it. Wait for the check (a minute after start), quit, launch, and check About / Settings for the new version. If App Management blocks it, allow it, then quit again.
