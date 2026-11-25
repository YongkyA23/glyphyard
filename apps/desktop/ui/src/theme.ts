// Light / dark theme: sets data-theme on <html> (tokens.css holds the dark palette). public/theme.js applies the
// last known choice before first paint; this keeps it in sync with the saved setting, the system preference,
// and the other window (main and quick share localStorage).
import { api, type Settings } from './api'

type Theme = Settings['theme']
const KEY = 'typeshelf.theme'
const systemDark = matchMedia('(prefers-color-scheme: dark)')

function stored(): Theme {
  try {
    const t = localStorage.getItem(KEY)
    return t === 'light' || t === 'dark' ? t : 'system'
  } catch {
    return 'system'
  }
}

let choice = stored()

/** Re-applies the theme; also call it when entering or leaving the print page, which is always light. */
export function paintTheme() {
  const t = location.pathname.startsWith('/print') ? 'light' : choice === 'system' ? (systemDark.matches ? 'dark' : 'light') : choice
  document.documentElement.dataset.theme = t
}

export function setTheme(t: Theme) {
  choice = t
  try {
    localStorage.setItem(KEY, t)
  } catch {
    // Private mode or blocked storage: the theme still applies, it just isn't remembered before the next load.
  }
  paintTheme()
}

const sync = () => api.getSettings().then((s) => setTheme(s.theme), () => {})
systemDark.addEventListener('change', paintTheme)
window.addEventListener('storage', (e) => e.key === KEY && ((choice = stored()), paintTheme()))
// The quick window is hidden, not reloaded, between uses: catch up when it (or the main window) comes back.
window.addEventListener('focus', sync)
paintTheme()
sync()
