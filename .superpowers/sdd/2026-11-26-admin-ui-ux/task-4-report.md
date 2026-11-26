# Task 4 report
Implemented: palette.tsx (Palette, global keydown: Ctrl/Cmd+K toggle, `/` focus .main search, `?` help dialog; combobox/listbox palette with Pages/Users/Fonts, 150ms debounce, useLoad newest-wins, wrap arrows, Enter/Esc, focus restored to prior element), palette.css, nav.ts (NAV, isTyping, isMac, OPEN_PALETTE), ui.tsx (Layout mounts Palette; Sidebar uses NAV + "Search Ctrl K" button).
Checks: tsc clean; oxlint 19 warnings (unchanged).
Deviation: NAV and isTyping live in nav.ts (not exported from ui.tsx/palette.tsx) because exporting non-components from those files adds only-export-components warnings, breaking the <=19 cap.
Notes: `.sr-only` has no CSS definition in repo (pre-existing); scoped copy added under `.pal`. Browser verification left to controller.

## Fix
Ctrl/Cmd+K and the sidebar open event now skip opening when any `dialog[open]` exists (help dialog included); Ctrl+K with the palette open still closes it (checked first). Removed `.pal .sr-only` from palette.css.
Checks: `bunx tsc --noEmit -p tsconfig.app.json` clean; `bunx oxlint` 19 warnings.
