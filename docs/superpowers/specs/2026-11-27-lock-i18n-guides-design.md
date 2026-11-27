# Connection unlock password, tooltips, ID/EN, guided tours — design

## Why

From the user (2026-11-27), looking at the desktop app and the admin web:

1. The admin can already lock Settings › Connection (`allow_connection_edit`). They want a **password** that lets someone change the server address and install key on a locked computer, enforced by the backend. Who types it: **the admin, at the user's computer**, e.g. after the install key changed. It **must work offline**, so the app keeps the password's hash and gets the new one when the admin changes it.
2. A **"+" button on the Folders header** in the desktop sidebar, to add folders from there.
3. **Remove the small explanatory sub-texts** under titles, labels and switches (page subtitles, card explanations, switch hints, the "Your admin can lock these two fields." line, the admin "App updates" paragraph). Put them in **tooltips** instead. Applies to the desktop app and the admin web. Also **fix the CSS of the admin "App updates" file inputs** (they render as tall, unstyled native boxes).
4. **Indonesian and English** UI in both apps.
5. **Walkthrough and spotlight tutorials** in the desktop app, e.g. "add fonts".
6. A **guide library in desktop Settings** listing the guides and their steps.

The user asked for everything to be built end to end, "no bugs, optimized, best practice", and opted out of further questions. The calls below that the user did not make are marked *(my call)*.

## Goals

### 1. Connection unlock password

- Admin web › Settings, next to "Let users change the server address and key": an **unlock password** the owner can set, change or remove. The page shows whether one is set and never shows the password or its hash.
- The server stores an **Argon2 PHC hash** only, and sends it to every install in the policy (`Policy.connection_password_hash`), so changing it reaches computers through the usual policy push.
- Desktop, connection locked and a hash present: Settings › Connection shows **"Unlock with admin password"**. A correct password enables the two fields and the three buttons for this visit to the page. Leaving the page, saving successfully, or pressing "Lock again" locks it again.
- **The desktop backend enforces it**: `save_connection` and `reset_connection` take the password and verify it in Rust against the stored hash whenever the policy is locked. The UI unlock alone grants nothing.
- **Brute-force limit** *(my call)*: 5 wrong passwords lock unlocking for 5 minutes. The counter is persisted, so restarting the app doesn't reset it. A correct password resets it.
- Locked with no password set: same as today, no unlock button.
- When the app registers with a different server, that server's policy (and its hash, or none) replaces the old one.

### 2. Folders "+"

A `+` button on the Folders header opens a menu with **Add fonts… (F)**, **Add a folder (copied)… (D)** and **Watch a folder… (W)**, reusing the existing `add.*` actions and shortcuts. It mirrors the Collections header button.

### 3. Tooltips instead of sub-texts

- **Moves to a tooltip**: static explanatory copy that never changes. That covers page subtitles, card explanations, switch and field hints, and footnotes like "Your admin can lock these two fields."
- **Stays as text**: anything dynamic or actionable. That covers status lines ("Connected · last sync 1 min ago", "Everything looks good · Last check …"), validation errors, warnings (`.note warn`), empty states, counts, the Install ID / version line, and confirmations.
- **The tooltip control**: a small ⓘ button next to the heading or label. It opens on hover, on keyboard focus and on tap or click, and closes with Escape and on outside click. It uses `role="tooltip"` with `aria-describedby`, stays inside the viewport, and respects reduced motion. A bare `title` attribute isn't enough, because it can't be reached by keyboard or touch.

### 4. Indonesian / English

- **The lookup**: `t('English source text', vars?)`, with the English string itself as the key. Missing Indonesian falls back to English *(my call: no key renaming across ~10k lines, and the English text stays readable in the code)*. `{name}` placeholders handle interpolation.
- **Dictionaries**: one file per area under `src/i18n/id/`, merged with `import.meta.glob`, so parallel work never edits one shared file.
- **Desktop setting**: Settings › App › Language: Match system / English / Bahasa Indonesia, stored in the engine `Settings.language`. It's mirrored to localStorage so the first paint is already in the right language, and the tray menu is relabelled too.
- **Admin web**: a language switcher in the sidebar footer and on the sign-in page, stored in localStorage.
- **Formatting**: dates and numbers follow the language (`en-GB` / `en-US` vs `id-ID`). `plural()` translates its words; Indonesian has no plural form.
- **Errors**: `errMsg()` passes the backend's English message through `t()`, so known messages are translated. Dynamic messages stay English *(my call)*.
- **Out of scope**: the setup installer (`apps/setup`). It's a separate vanilla-JS app; add it later if wanted.

### 5. Guided tours (desktop)

- **The tour**: a spotlight over a `[data-tour="…"]` element with a popover showing a title, body, "n / N", Back, Next or Done, and Skip.
  - It navigates to a step's `route` first, then waits up to 1.5 s for the target.
  - A missing target (for example, no fonts yet) shows the popover centred instead.
  - It follows the target through scroll and resize.
  - Keyboard: Esc closes, ←/→ move between steps, focus stays in the popover and returns to where it was afterwards.
  - It respects reduced motion.
- **Welcome walkthrough**: starts once, right after onboarding finishes.
- **Guides**:
  - Welcome tour
  - Add fonts
  - Turn fonts on and off
  - Collections
  - Project sets
  - Quick activate
  - Connect to your team's server

### 6. Guide library (desktop Settings)

- A "Guides" card in Settings lists every guide: its title, a summary, and an expandable list of its steps as plain text, so the steps can be read without running the tour.
- Each guide has **Start** and a **Done** check once it's been finished. Completion is kept in localStorage.

## Non-goals

- Tours in the admin web.
- Translating free-text server or engine messages that contain runtime values.
- Server-side enforcement of a desktop's local settings. The server can't stop a computer from editing its own database. Someone with local admin rights on the machine is outside this threat model.

## Anti-goals

- The unlock password or its hash must never be logged, never be shown in the admin UI, and never be returned by `GET /settings` or `PUT /settings`.
- Locking must never brick a computer: with no password set, behaviour is exactly as today.
- Old desktops on a new server and new desktops on an old server keep working (`#[serde(default)]`, unknown fields ignored).
- No new npm or cargo crates. `argon2` is already in `Cargo.lock` (server); the engine adds the same version.

## Approaches considered

- **Verification**: server-checked unlock vs. **offline hash in the policy (chosen, the user's requirement)**. The cost is that the hash sits on every computer and could be brute-forced offline. Mitigations: Argon2id, an 8-character minimum, and the owner-only endpoint. The admin UI recommends a long passphrase.
- **i18n**: an i18n library (a new dependency) vs. hand-written key-based catalogues vs. **English-as-key `t()` (chosen)**: smallest diff, no dependency, and English never goes missing.
- **Tooltip**: `title` attributes (not keyboard or touch accessible) vs. the CSS-only hover pattern (clips at edges) vs. **a small React ⓘ button with viewport-clamped fixed positioning (chosen)**.
- **Tours**: a tour library (new dependency) vs. **a small built-in engine using `data-tour` anchors (chosen)**.
- **Re-render on language change**: React context through every component vs. **a module store with `useSyncExternalStore`, plus a `key={lang}` remount of the routed content (chosen)**. Switching language is rare, and a remount guarantees that nothing keeps stale strings.
