# Unlock password, tooltips, ID/EN, guided tours: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build these, end to end:
- an offline-verifiable admin unlock password for the locked Connection settings;
- a Folders "+" button;
- tooltips in place of explanatory sub-texts (desktop and admin);
- an Indonesian/English UI (desktop and admin);
- desktop spotlight tours with a guide library in Settings.

**Architecture:** The work runs in two phases.
- **Phase A** builds the foundations in parallel, with disjoint files:
  - server password (Task 1);
  - desktop Rust (Task 2);
  - desktop UI foundation: i18n, Hint, the tour engine and guides, api and mock (Task 3);
  - admin UI foundation: i18n, Hint, language switcher, password control, file-input CSS (Task 4).
- **Phase B** (Tasks 5–10) starts after A is merged. Each task owns a set of files and does **all** of the work in them:
  - wraps strings in `t()`;
  - turns sub-texts into `<Hint>`;
  - adds `data-tour` anchors;
  - writes its own Indonesian dictionary file.

**Tech Stack:** Rust (Axum, rusqlite, Tauri 2, argon2 0.6), React 19, react-router 8, motion 14, TypeScript 6, Vite 8, bun.

**Spec:** `docs/superpowers/specs/2026-11-27-lock-i18n-guides-design.md`

**Base branch:** `feat/lock-i18n-guides`. It already has `Policy.connection_password_hash: String` (`#[serde(default)]`) in `crates/protocol/src/lib.rs`. `apps/server/src/db.rs` `policy()` fills it with `String::new()` as a placeholder that Task 1 replaces.

## Global Constraints

- **Dependencies:** no new npm packages and no new crates. The engine may add `argon2 = "0.6.0"`, the version the server already uses.
- **Desktop UI** (`apps/desktop/ui`):
  - `bunx tsc --noEmit -p tsconfig.app.json` is clean.
  - `bunx oxlint 2>&1 | grep -c warning` stays ≤ 53.
- **Admin UI** (`apps/admin-ui`):
  - tsc is clean.
  - The same oxlint count stays ≤ 19.
- **Rust:**
  - `cargo test --workspace` passes.
  - `cargo clippy --workspace --all-targets -- -D warnings` passes.
  - In a fresh worktree, first copy the gitignored `apps/desktop/ui/dist` and `plugins/dist` from `C:/Users/lenovo/Documents/GitHub/typeshelf`.
  - Use your own `CARGO_TARGET_DIR` per worktree.
- **Mock mode:** both UIs run without a backend. Desktop: plain `bun run dev` in a browser uses `src/mock.ts`. Admin: `?mock=1` uses `src/mock.ts`. Mock mode supports every new feature.
- **Shared UI rules:**
  - Dark mode uses CSS variables (tokens) only.
  - Respect reduced motion: `MotionConfig reducedMotion="user"` already wraps the app, and CSS animations get a `@media (prefers-reduced-motion: reduce)` guard.
  - Every interactive element is reachable by keyboard and has an accessible name.
- **Secrets:** the unlock password and its hash never appear in logs, the audit log, or `GET`/`PUT /settings` responses.
- **Git:**
  - Never stage `.claude/`, `.superpowers/` or `docs/deploy.local.md`.
  - No push.
  - Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - Each worktree starts with `git merge --ff-only feat/lock-i18n-guides`.

### i18n contract (Tasks 3–10)

- `t(en: string, vars?: Record<string, string | number>): string` looks up the exact English string. In Indonesian it returns the dictionary value, or the English when the value is missing. `{key}` placeholders are replaced from `vars`, and numbers are formatted with the current locale.
- **What gets wrapped.** Wrap every user-visible English string:
  - JSX text;
  - `aria-label`, `title`, `placeholder` and `alt`;
  - toast and confirm text;
  - menu labels;
  - error messages built in the UI.
- **What doesn't.** Never wrap:
  - CSS classes, keys, routes or codes;
  - font or family names;
  - the user's own text;
  - brand names (Glyphyard, Adobe, Photoshop, Illustrator, InDesign, Google Fonts);
  - keyboard keys.
- **Literals only.** The first argument is always a string literal, never a variable or a template string. Put dynamic parts in `vars`: `t('{n} fonts active', { n })`. For conditionals, wrap each branch separately: `cond ? t('A') : t('B')`.
- **Module-level constants** that hold labels (for example `APP_TOGGLES`, `THEMES`) keep the English and are passed through `t()` at render time.
- **Dictionaries** are `src/i18n/id/<area>.ts`, shaped `export default { 'English': 'Indonesia', … } satisfies Record<string, string>`. They are merged automatically by `src/i18n/id/index.ts` through `import.meta.glob('./*.ts', { eager: true })`. Each task writes only its own area file. Duplicate keys across files are fine as long as the values are identical.
- **Checker:** `bun ../../scripts/i18n-check.ts <srcDir>`, written in Task 3, run from the app folder. It lists `t('…')` literals with no Indonesian entry and exits 1 if there are any. The scope argument limits which files it scans; the dictionary is always the whole `id/` folder. Every Phase B task finishes with **0 missing** for its own files.

### Indonesian style and glossary (all dictionaries)

- **Voice:** casual-polite "kamu", sentence case, short. Keep the UI terms below exactly as listed.
- **Fonts and library:**

  | English | Indonesian |
  |---|---|
  | font | font |
  | font family | keluarga font |
  | style | gaya |
  | turn on / turn off | aktifkan / nonaktifkan |
  | on / off (state) | aktif / nonaktif |
  | collection | koleksi |
  | project set | set proyek |
  | folder | folder |
  | watch a folder | pantau folder |
  | library | pustaka |
  | shared library | pustaka bersama |
  | tag | tag |
  | preview | pratinjau |
  | sample text | teks contoh |
  | glyph | glyph |
  | system fonts | font sistem |
  | Playground | Playground |

- **Connection and admin:**

  | English | Indonesian |
  |---|---|
  | server address | alamat server |
  | install key | kunci instalasi |
  | install (a computer running the app) | instalasi |
  | admin | admin |
  | owner | pemilik |
  | user | pengguna |
  | request | permintaan |
  | block | blokir |
  | merge | gabungkan |
  | audit log | log audit |
  | sync | sinkronisasi |
  | connected / offline | terhubung / offline |

- **App, actions and guides:**

  | English | Indonesian |
  |---|---|
  | Settings | Pengaturan |
  | search | cari |
  | upload / download | unggah / unduh |
  | save | simpan |
  | cancel | batal |
  | delete / remove | hapus |
  | undo | urungkan |
  | guide | panduan |
  | tour | tur |
  | step | langkah |
  | password | kata sandi |
  | unlock / lock | buka kunci / kunci |

### Hint contract (desktop Task 3, admin Task 4)

- **API:** `export function Hint({ text, label }: { text: string; label?: string })` in `src/Hint.tsx`. It renders an inline ⓘ `<button type="button" class="hint">` with `aria-label={label ?? t('More info')}` and `aria-describedby` pointing at the tip. The tip is `<span role="tooltip" class="hint-tip">`, portalled to `document.body`.
- **Opening and closing:** it opens on pointer enter, focus or click, and closes on pointer leave (after a 100 ms grace), blur, Escape or an outside click.
- **Positioning:** `position: fixed`, placed below the button by default, flipped above when there's no room, and clamped 8 px inside the viewport. Max width 280 px.
- **Use:** put it after the text, inside the heading or label. Callers pass already-translated text: `<h2 className="h-section">{t('Activation health')} <Hint text={t('…')} /></h2>`.

### Tour anchors (`data-tour` values; the owning task adds them, Task 3's guides use them)

| value | element | owner |
|---|---|---|
| `sidebar` | `<nav className="sidebar">` | Task 5 |
| `nav-library` | the Library nav link (`/`) | Task 5 |
| `nav-sets` | the Project sets nav link | Task 5 |
| `nav-settings` | the Settings nav link | Task 5 |
| `folders` | the Folders `side-group` div | Task 5 |
| `folders-add` | the new Folders "+" button | Task 5 |
| `collections-add` | the "New collection" + button | Task 5 |
| `library-search` | the Library search input wrapper | Task 7 |
| `library-add` | the button that opens the Library "Add fonts" menu | Task 7 |
| `font-list` | the Library results list/grid container | Task 7 |
| `font-switch` | each family row's on/off Switch (the tour targets the first) | Task 7 |
| `sets-new` | the "New project set" button | Task 8 |
| `sets-list` | the project sets list container | Task 8 |
| `settings-connection` | the Connection section | Task 6 |
| `settings-shortcut` | the quick-activate shortcut field row | Task 6 |
| `settings-language` | the language select | Task 6 |
| `settings-guides` | the Guides section | Task 6 |

## Review Focus

1. **Wrong password while offline, five times, then an app restart.** Unlocking stays refused until 5 minutes have passed, and the count survives the restart. Covered by `unlock_throttle_persists` in Task 2.
2. **Save sent without a password while the policy is locked.** The backend refuses with `connection_locked` even if the UI was tampered with. Covered by `locked_change_needs_password` in Task 2.
3. **The hash leaks through the admin API.** `GET /settings` and `PUT /settings` never contain `connection_password_hash`, and the audit row has no detail. Covered by `connection_password_never_returned` in Task 1.
4. **Language switch mid-page, including the Quick window.** Every visible string changes without a reload, the `<html lang>` updates, and the Quick window follows through the `storage` event. Manual check in Task 3 and in the final verification.
5. **A tour target is missing or appears late** (an empty library, a route change). The popover centres instead of throwing, and Esc always exits and restores focus. Manual check in Task 3: run "Turn fonts on and off" with the mock library emptied.

---

## Phase A (Tasks 1–4 run in parallel)

### Task 1: Server: connection unlock password

**Files:**
- Modify: `apps/server/src/db.rs`
  - `Settings` gains `pub connection_password_hash: String`.
  - `policy()` copies it, replacing the placeholder.
- Modify: `apps/server/src/admin.rs`
  - `get_settings` and `put_settings` return the public settings.
  - `put_settings` silently ignores `connection_password_hash` and `connection_password_set` (like `install_key`).
  - The `put_settings` audit diff skips `connection_password_hash`.
  - New route `PUT /settings/connection-password`.
- Test: `apps/server/tests/api.rs`
- Modify: `docs/api.md`

**Interfaces:**
- **Produces (HTTP):**
  - `GET /settings` and `PUT /settings` return the settings object with `connection_password_hash` **removed** and `connection_password_set: boolean` added. Use one helper, `fn public_settings(s: &db::Settings) -> serde_json::Value`.
  - `PUT /settings/connection-password` takes `{ "password": string | null }` and requires **Owner**, the extractor `regenerate_key` uses.
    - `null` or `""` clears the password.
    - Otherwise the password, untrimmed, must be 8..=128 chars, else 400 `invalid_password` "The unlock password must be 8 to 128 characters."
    - Hash with `auth::hash_password` **before** taking the database lock.
    - Save, then queue `PolicyChanged { policy }` for every `db::active_install_ids`.
    - Audit `connection_password_set` or `connection_password_cleared` with an empty target and detail.
    - Returns `{ "connection_password_set": bool }`.

- [ ] **Step 1: Write failing tests** in `apps/server/tests/api.rs`, using the existing `Server`, `admin`, `register` and `ack_all` helpers:
  - `connection_password_set_and_pushed`:
    - The owner PUTs `{password:"correct horse"}` and gets 200 `{connection_password_set:true}`.
    - `GET /settings` has `connection_password_set == true` and no `connection_password_hash` key.
    - The registered install's `ack_all` contains a `policy_changed` whose `policy.connection_password_hash` starts with `"$argon2"`.
    - PUT `{password:null}` → `set == false`, and the next pushed policy hash is `""`.
  - `connection_password_validation`:
    - `"short"` → 400 `invalid_password`.
    - A non-owner admin (create one the way the existing role tests do) → 403.
  - `connection_password_never_returned`:
    - After setting it, `PUT /settings {name_change_days: 31}` returns no hash key.
    - `PUT /settings {connection_password_hash:"x"}` doesn't change it (`set` stays true and the pushed hash still starts with `$argon2`).
    - The `GET /audit` rows for this action have an empty detail.
- [ ] **Step 2:** Run `cargo test -p typeshelf-server --test api connection_password` → FAIL.
- [ ] **Step 3:** Implement as specified in Interfaces.
- [ ] **Step 4:** `cargo test --workspace` and clippy `-D warnings` → pass.
- [ ] **Step 5:** `docs/api.md`:
  - add the endpoint row;
  - note `connection_password_set` on `/settings`;
  - add `connection_password_hash` to the policy description;
  - add the two audit actions where the audit actions are listed.
- [ ] **Step 6:** Commit `Server: connection unlock password`.

### Task 2: Desktop Rust: password check, throttle, language, tray

**Files:**
- Modify: `crates/engine/Cargo.toml` (`argon2 = "0.6.0"`)
- Modify: `crates/engine/src/settings.rs`
- Modify: `crates/engine/src/types.rs` (`Connection.unlockable`, `Settings.language`)
- Test: `crates/engine/src/tests.rs` (or a new `crates/engine/src/tests/unlock.rs` registered like the other test modules)
- Modify: `apps/desktop/src-tauri/src/commands.rs` (`change_connection`, `save_connection`, `reset_connection`, new `unlock_connection`, new `set_tray_language`)
- Modify: `apps/desktop/src-tauri/src/main.rs` (register the commands; tray items relabelable)
- Modify: `docs/desktop-commands.md`

**Interfaces:**
- **Engine:**
  - `pub fn verify_connection_password(&self, password: &str) -> Result<()>`. It fails in these cases:
    - `EngineError` code `unlock_unavailable`, "No unlock password is set for this computer.", when the stored policy hash is empty.
    - `unlock_throttled`, "Too many wrong passwords. Try again in {m} minutes." (m rounded up, at least 1), with extra field `retryAfterSecs`, while throttled.
    - `wrong_password`, "That password isn't right.", on a mismatch.
  - **Throttle:** a store key `"unlock_throttle"` holds `{ failures: u32, locked_until: Option<DateTime<Utc>> }`. The 5th consecutive failure sets `locked_until = now + 5 min` and resets `failures`. A success clears the key. Argon2 verification uses `PasswordHash::new` and `Argon2::default().verify_password`, like `apps/server/src/auth.rs::verify_password`.
- **Types:**
  - `Connection` gains `pub unlockable: bool`: `!editable && !policy.connection_password_hash.is_empty()`. It's serialized camelCase like the rest.
  - `Settings` gains `pub language: Language`, with `#[serde(rename_all = "lowercase")] enum Language { System, En, Id }`, defaulting to `System` (`#[serde(default)]` so stored settings without it still load).
- **Commands (Tauri, camelCase args):**
  - `save_connection(serverUrl, installKey, password: Option<String>)` and `reset_connection(password: Option<String>)`.
    - When `!policy.allow_connection_edit`: `None` → `connection_locked` (today's message); `Some(p)` → `verify_connection_password(p)?` and then proceed.
    - When editable, the password is ignored.
    - Verification runs inside `ctx.run`, off the async runtime thread.
  - `unlock_connection(password: String) -> R<()>`: verify only.
  - `set_tray_language(lang: String) -> R<()>`: `"id"` labels the tray items "Buka Glyphyard" / "Keluar"; anything else gives "Open Glyphyard" / "Quit". Keep the two `MenuItem`s in managed state, or rebuild the menu through `app.tray_by_id("main")`.

- [ ] **Step 1: Write failing engine tests:**
  - `unlock_requires_a_hash`: the default policy → `unlock_unavailable`.
  - `unlock_checks_the_password`:
    - Store a policy with `allow_connection_edit:false` and the hash of `"correct horse"` (create it in the test with `argon2` `PasswordHasher`).
    - Wrong → `wrong_password`; right → Ok.
    - `get_connection().unlockable == true`.
  - `unlock_throttle_persists`:
    - 5 wrong → the 6th call, even with the right password, gives `unlock_throttled`.
    - Re-open the engine on the same data dir (the pattern the existing persistence tests use) → still `unlock_throttled`.
    - Rewrite `locked_until` to the past through the store → the right password is Ok.
  - `language_defaults_to_system`: `settings().language == Language::System`, and it round-trips `Id` through `set_settings`.
- [ ] **Step 2:** Run `cargo test -p typeshelf-engine unlock language` → FAIL.
- [ ] **Step 3:** Implement the engine parts.
- [ ] **Step 4: Commands.** Add a unit-testable helper in `commands.rs`, or an engine method used by it: `fn authorize_connection_change(e: &Engine, password: Option<&str>) -> Result<()>`. Add the engine test `locked_change_needs_password`: locked + `None` → `connection_locked`; locked + right password → Ok; editable + `None` → Ok. Wire the helper into `change_connection`, and add `unlock_connection` and `set_tray_language`.
- [ ] **Step 5:** `cargo test --workspace` and clippy → pass. Update `docs/desktop-commands.md` for the new and changed commands, the `unlockable` field and `language`.
- [ ] **Step 6:** Commit `Desktop: connection unlock password, language setting, tray labels`.

### Task 3: Desktop UI foundation: i18n, Hint, tours, api/mock

**Files:**
- Create: `apps/desktop/ui/src/i18n/index.ts`, `src/i18n/id/index.ts`, `src/i18n/id/common.ts`, `src/i18n/id/tour.ts`
- Create: `apps/desktop/ui/src/Hint.tsx` (its CSS goes in `src/app.css`)
- Create: `apps/desktop/ui/src/tour/Tour.tsx`, `src/tour/guides.ts`, `src/tour/progress.ts`, `src/tour/tour.css`
- Create: `scripts/i18n-check.ts` (repo root, bun)
- Modify: `apps/desktop/ui/src/main.tsx` (mount `TourProvider` inside `BrowserRouter` + `AppProvider` for the main routes; wrap the routed content in `<Fragment key={lang}>` below `AppProvider`)
- Modify: `apps/desktop/ui/src/state.tsx`, but only the place where settings first load: call `applyLanguage(settings.language)`
- Modify: `apps/desktop/ui/src/ui.tsx`, but only these helpers:
  - `num`, `fmtDate`, `fmtWhen` and `ago` use `locale()`;
  - `plural` becomes `` `${num(n)} ${n === 1 ? t(one) : t(many)}` ``;
  - `errMsg` passes the message through `t()`.
- Modify: `apps/desktop/ui/src/api.ts` and `src/mock.ts`

**Interfaces (produces):**
- `src/i18n/index.ts`:
  - **Types:** `type Lang = 'en' | 'id'` and `type LangSetting = 'system' | 'en' | 'id'`.
  - **Language state:**
    - `resolveLang(s: LangSetting): Lang`: system → `'id'` if any `navigator.languages` entry starts with `id`, else `'en'`.
    - `getLang(): Lang`.
    - `setLang(l: Lang): void`: sets `document.documentElement.lang`, writes localStorage `typeshelf.lang` (in try/catch) and notifies subscribers.
    - `useLang(): Lang` through `useSyncExternalStore`.
    - On load the module reads `typeshelf.lang`, or resolves `'system'`. It listens for `storage` events on that key so the Quick window follows.
  - **Lookup and formatting:**
    - `t(en, vars?)` as in the contract. In dev with `lang === 'id'`, it calls `console.debug` once per missing key.
    - `locale(): string`: `'id-ID'` or `'en-GB'` (dates).
    - `numLocale(): string`: `'id-ID'` or `'en-US'`.
  - `applyLanguage(s: LangSetting): void`: `setLang(resolveLang(s))`, plus a fire-and-forget `api.setTrayLanguage(lang)`.
- `api.ts`:
  - **Types:**
    - `Settings.language: 'system' | 'en' | 'id'`.
    - `Connection.unlockable: boolean`.
  - **Calls:**
    - `saveConnection(serverUrl: string, installKey: string, password?: string)`.
    - `resetConnection(password?: string)`.
    - `unlockConnection(password: string): Promise<void>`.
    - `setTrayLanguage(lang: 'en' | 'id'): Promise<void>`.
- `mock.ts`:
  - Supports all four calls, with the same error codes and messages as Task 2.
  - `?locked=1` in the URL makes the connection locked and unlockable, with password `glyphyard`.
  - `language` persists in the mock settings.
- `Hint` as in the Hint contract.
- **`src/tour/guides.ts`:**
  - `export type GuideId = 'welcome' | 'add-fonts' | 'activate' | 'collections' | 'project-sets' | 'quick' | 'connect'`.
  - `export type Step = { target?: string; route?: string; title: string; body: string; placement?: 'top' | 'bottom' | 'left' | 'right' | 'auto' }`.
  - `export type Guide = { id: GuideId; title: string; summary: string; steps: Step[] }`.
  - `export const GUIDES: Guide[]`.
  - English text throughout; it's rendered through `t()`. The steps use only the tour anchors table. Content:
    - **welcome**: sidebar, nav-library, library-search, font-list, font-switch, folders, collections-add, then settings-guides (route `/settings`), which says the guides live there.
    - **add-fonts**:
      - route `/` → library-add: add files, copy a folder in, or watch a folder.
      - folders-add: the same from the sidebar.
      - folders: watched folders and their on dots.
      - no target: drag and drop font files or folders onto the window.
    - **activate**:
      - font-switch: turn a family on or off.
      - font-list: open a family to switch single styles.
      - no target: select several families and use the selection bar.
    - **collections**:
      - collections-add.
      - no target: drag families onto a collection in the sidebar.
    - **project-sets**: route `/sets` → sets-new, then sets-list (one click turns a whole job's fonts on or off).
    - **quick**: route `/settings` → settings-shortcut (open Quick activate from anywhere).
    - **connect**: route `/settings` → settings-connection (server address and install key from your admin; a locked connection needs the admin's unlock password).
- **`src/tour/progress.ts`:**
  - `isDone(id: GuideId): boolean`.
  - `markDone(id: GuideId): void`.
  - `useDone(): Set<GuideId>`.
  - Backed by localStorage `typeshelf.guides.done` (a JSON array) with try/catch. It notifies subscribers.
- **`src/tour/Tour.tsx`:**
  - `export function TourProvider({ children })`.
  - `export function useTour(): { start(id: GuideId): void; active: GuideId | null }`.
  - Behaviour as spec §5:
    - **Spotlight and popover:** the spotlight is a fixed element over the target's rect (padding 6, radius 10) with `box-shadow: 0 0 0 9999px` and a dim token colour. Clicks outside the popover are blocked while a tour runs.
    - **Popover contents:** the title (h2) and body, "n / N", and Back, Next (Done on the last step) and Skip.
    - **Popover semantics:** `role="dialog" aria-modal="true" aria-labelledby`. Focus moves to Next and is trapped; on close it returns to the element that had it.
    - **Following the target:** `scrollIntoView({ block: 'center' })` and a ResizeObserver/scroll/resize update, batched through rAF.
    - **Routing:** navigate to `route` with `useNavigate` when it differs from the current pathname.
    - **Waiting for the target:** poll up to 1500 ms, then centre the popover.
    - **Finishing:** Done calls `markDone`. Skip and Esc close without marking.
- `scripts/i18n-check.ts`:
  - Run from the app folder as `bun ../../scripts/i18n-check.ts src [files…]`.
  - It scans `.ts`/`.tsx` files (all of `src`, or only the listed files) for `t('…')` / `t("…")` literals, handling `\'` escapes and `’` curly quotes.
  - It imports every `src/i18n/id/*.ts` default export (dynamic `import()` of the files, not `import.meta.glob`), prints the missing keys with file:line, and exits 1 if there are any.
  - It also flags `t(` calls whose first argument isn't a string literal.

- [ ] **Step 1:** Write `i18n/index.ts` and `i18n/id/index.ts`. `common.ts` holds the shared words: More info, Back, Next, Done, Skip, Cancel, Save, Close, the language names, and the plural words `font`/`fonts`, `style`/`styles`, `problem`/`problems` and the like used by `plural()` callers. Add the self-check `src/i18n/i18n.check.ts`, run with `bun src/i18n/i18n.check.ts`. It asserts:
  - `t('Hello {name}', {name:'A'})` === `'Hello A'` in en;
  - an unknown key returns the English in id;
  - `{n}` gets `numLocale` formatting.
- [ ] **Step 2:** Run `bun src/i18n/i18n.check.ts` → passes.
- [ ] **Step 3:** Write `Hint.tsx` and its CSS.
- [ ] **Step 4:** Write the tour files and `tour.css` (tokens only; a reduced-motion guard on the spotlight transition).
- [ ] **Step 5:** Write `api.ts` and the `mock.ts` additions, then wire up `main.tsx`, `state.tsx` and the `ui.tsx` helpers.
- [ ] **Step 6:** Write `scripts/i18n-check.ts`. Run `bun ../../scripts/i18n-check.ts src src/Hint.tsx src/tour/Tour.tsx src/tour/guides.ts` → 0 missing. The guides' strings are passed through `t()` where rendered, so the checker can't see them. Instead, wrap each guide string in a no-op marker `tk('…')` exported from i18n, which returns its argument, and have the checker scan `tk(` literals too.
- [ ] **Step 7:** Run tsc and oxlint (≤ 53). Then `bun run dev`, open in the browser, and from the devtools console run the welcome tour (temporarily expose `window.__tour` in dev only): it must step through and centre on anchors that are missing (Phase B adds them). Check the Hint with the keyboard.
- [ ] **Step 8:** Commit `Desktop UI: i18n, Hint, tour engine and guides`.

### Task 4: Admin UI foundation: i18n, Hint, switcher, unlock password, file inputs

**Files:**
- Create: `apps/admin-ui/src/i18n/index.ts`, `src/i18n/id/index.ts`, `src/i18n/id/common.ts`, `apps/admin-ui/src/Hint.tsx`
- Modify:
  - `apps/admin-ui/src/ui.tsx`:
    - `Layout`: a language switcher in the sidebar footer.
    - `fmtDate`, `fmtShort`, `fmtTime`, `ago` and `plural`: locale-aware, the same way as Task 3.
    - `PageHeader`: its `sub` prop renders as a `<Hint>` next to the title instead of a paragraph.
  - `src/pages/Login.tsx`: the switcher.
  - `src/pages/Settings.tsx`: the password control and the App updates layout.
  - `src/app.css`: the Hint styles and file inputs.
  - `src/api.ts` and `src/mock.ts`.

**Interfaces (produces):**
- **i18n:** the same API as Task 3's `src/i18n/index.ts` minus `applyLanguage` and the tray. Storage key `glyphyard.admin.lang` holds the **setting** (`system|en|id`). Exports:
  - `getLangSetting()` and `setLangSetting(s)`;
  - `useLang()`, `t`, `tk`, `locale()` and `numLocale()`.
- **Language switcher:** `export function LanguageSelect()` in `src/i18n/LanguageSelect.tsx`. It's a labelled `<select>` with Match system / English / Bahasa Indonesia.
- **Remounting on a change:** `main.tsx` wraps the routes in `<Fragment key={useLang()}>`, through a tiny component.
- **API:**
  - `api.ts`: `Settings.connection_password_set: boolean`, and `setConnectionPassword(password: string | null): Promise<{ connection_password_set: boolean }>` → `PUT /settings/connection-password`.
  - `mock.ts`: implements it.
- **Settings page** (under the `allow_connection_edit` toggle):
  - **Display:** "Unlock password" with a "Set"/"Not set" badge. Owners get Set/Change (a password input with show/hide, minimum 8, and a confirm field) and Remove (through `useConfirm`). Non-owners see the state only.
  - **Feedback:** toasts.
  - **Warning:** when the toggle is off and no password is set, a `.note` says nobody can change the connection on a locked computer.
  - **Hint text:** "Stored as a one-way hash on every computer so you can unlock offline. Use a long passphrase."
- **File inputs:** style `input[type=file]` globally in `app.css`:
  - no fixed height, and no extra padding inside `.input`;
  - `::file-selector-button` styled like `.btn` (tokens, radius, hover/focus-visible);
  - the file name muted.
  - In the App updates card, the three pickers sit in a tidy grid (one column on narrow screens, three on wide). The explanatory paragraph becomes a `<Hint>` next to the card title.

- [ ] **Step 1:** i18n files + `i18n.check.ts` (same three assertions as Task 3) → `bun src/i18n/i18n.check.ts` passes.
- [ ] **Step 2:** Hint, LanguageSelect, wiring in `main.tsx`, Layout and Login, and the locale-aware helpers.
- [ ] **Step 3:** api/mock password plus the Settings control, and the file-input CSS with the App updates layout.
- [ ] **Step 4:**
  - Run tsc and oxlint (≤ 19).
  - Run `bun run dev` with `?mock=1` and check:
    - Settings in light and dark mode;
    - setting, changing and removing the password;
    - the file inputs at 375 px and on desktop;
    - the switcher re-rendering the page.
- [ ] **Step 5:** Commit `Admin UI: i18n, Hint, language switcher, unlock password, file inputs`.

---

## Phase B (Tasks 5–10 in parallel, after Phase A is merged)

Every Phase B task does the same four things in the files it owns, and touches **no other file**:

- (a) It wraps every user-visible string in `t()` per the contract, and moves module-level label constants to render-time `t()`.
- (b) It replaces static explanatory sub-texts with `<Hint>`, following the spec §3 rule. Dynamic status, errors, warnings, empty states and counts stay as text.
- (c) It adds its `data-tour` anchors from the table.
- (d) It writes Indonesian for every string in `src/i18n/id/<area>.ts`, following the glossary.

**Done when:**
- `bun ../../scripts/i18n-check.ts src <owned files>` → 0 missing;
- tsc is clean and oxlint is not above the baseline;
- in mock mode, with the language switched to Indonesian, each owned screen shows no English except names.

**Commit:** `i18n + tooltips: <area>`.

### Task 5: Desktop shell (area `shell`)

**Files:**
- `src/Shell.tsx`, `src/menu.tsx`, `src/fontActions.tsx`, `src/state.tsx`, `src/ui.tsx`
- `src/RequestFont.tsx`, `src/PreselectBar.tsx`, `src/ShortcutField.tsx`, `src/ErrorBoundary.tsx`, `src/temporary.tsx`
- `src/shortcut.ts`, `src/fonts.ts`, `src/main.tsx` (strings only)
- `src/extras/*`
- Dictionary: `src/i18n/id/shell.ts`

Extra work:
- **The Folders "+":** turn the Folders `side-head` into a `side-head-row`, like Collections, with a `side-add` button: `aria-label={t('Add folders')}`, `title={t('Add fonts or folders')}`, `aria-haspopup="menu"`, `data-tour="folders-add"`. It opens the existing context-menu `show(e, items, true)` with:
  - Add fonts… (hint F) → `add.addFonts`;
  - Add a folder (copied)… (hint D) → `add.addFolder`;
  - Watch a folder… (hint W) → `add.watchFolder`.
- **Anchors:** sidebar, nav-library, nav-sets, nav-settings, folders and collections-add.

### Task 6: Desktop Settings, onboarding, Quick, Stats (area `settings`)

**Files:**
- `src/pages/Settings.tsx`, `src/pages/Onboarding.tsx`, `src/pages/Quick.tsx`, `src/pages/Stats.tsx`, `src/pages/stats.css`
- Dictionary: `src/i18n/id/settings.ts`

Extra work:
- **Language:** a select in the App section ("Language": Match system / English / Bahasa Indonesia, `data-tour="settings-language"`). It saves `language` with the existing settings save, then calls `applyLanguage`.
- **Connection** (`data-tour="settings-connection"`):
  - Drop the "Your admin can lock these two fields." sentence; keep the Install ID line.
  - **Locked and `unlockable`:** below the locked note, add an "Unlock with admin password" button. It opens a `Dialog` (from ui.tsx) with a password input, a show/hide toggle and a submit button. Submit calls `api.unlockConnection(pw)`.
    - On success, the password is kept in component state only; the fields and buttons are enabled and a "Lock again" button appears.
    - Errors (`wrong_password`, `unlock_throttled`) show `errMsg` in the dialog with `role="alert"`, and the input keeps focus.
  - **Save and reset** pass the held password. After a successful save or reset, and on unmount, the password is cleared.
- **Guides section** (`data-tour="settings-guides"`, placed after App). One row per `GUIDES` entry:
  - the title with a Done check when `useDone()` has it;
  - the summary;
  - a `<details>` "Show steps" with an `<ol>` of the step titles and bodies (translated);
  - a "Start" button → `useTour().start(id)`.
- **Shortcut field row:** `data-tour="settings-shortcut"`.
- **Onboarding:** after `completeOnboarding` succeeds and navigates to `/`, call `start('welcome')` once, unless `isDone('welcome')`. Defer it with `setTimeout(…, 400)` so the library has mounted.

### Task 7: Desktop Library and Family (area `library`)

**Files:**
- `src/pages/Library.tsx`, `src/pages/Family.tsx`, `src/pages/family/*`
- `src/pages/PairingSuggestions.tsx`, `src/pages/AxisPresets.tsx`
- `src/sampleTokens.ts` (labels only)
- Dictionary: `src/i18n/id/library.ts`

Anchors: library-search, library-add, font-list and font-switch.

### Task 8: Other desktop pages (area `pages`)

**Files:**
- `src/pages/ProjectSets.tsx`, `src/pages/Organize.tsx`, `src/pages/GoogleFonts.tsx`, `src/pages/PdfFonts.tsx`
- `src/pages/Playground.tsx`, `src/pages/PlaygroundCompare.tsx`, `src/pages/Print.tsx`
- Dictionary: `src/i18n/id/pages.ts`

Anchors: sets-new and sets-list. The Project sets subtitle becomes a Hint.

### Task 9: Admin installs pages (area `installs`)

**Files:**
- `apps/admin-ui/src/pages/Overview.tsx`, `Users.tsx`, `UserDetail.tsx`, `InstallFonts.tsx`, `useFontToggle.ts`, `Requests.tsx`, `Audit.tsx`
- Dictionary: `apps/admin-ui/src/i18n/id/installs.ts`

### Task 10: Admin library, settings, shell (area `admin`)

**Files:**
- `apps/admin-ui/src/pages/Library.tsx`, `Merge.tsx`, `Names.tsx`, `Organize.tsx`, `Settings.tsx`, `Login.tsx`
- `src/ui.tsx`, `src/nav.ts`, `src/palette.tsx`, `src/toast.tsx`, `src/ErrorBoundary.tsx`, `src/main.tsx`
- Dictionary: `apps/admin-ui/src/i18n/id/admin.ts`

Extra work: every remaining explanatory paragraph on the Settings page becomes a Hint, using the same rule.

---

## Final

- **Review:** a whole-branch review against the spec.
- **Checks:**
  - `cargo test --workspace` and clippy;
  - tsc and oxlint for both UIs;
  - the i18n checker on all of `src` for both UIs, with 0 missing.
- **Browser walkthrough in mock mode:**
  - language switching in both apps;
  - the welcome tour;
  - the Add fonts guide;
  - the Folders "+";
  - unlock with `?locked=1`;
  - the admin password control and file inputs, in light and dark mode.
