// Theme (system/light/dark) and studio brand colour. Both are applied as attributes/properties on <html>.
export type ThemePref = 'system' | 'light' | 'dark'

const KEY = 'admin-theme'
const root = document.documentElement
const dark = matchMedia('(prefers-color-scheme: dark)')
let brand = ''

export function getThemePref(): ThemePref {
  try {
    const v = localStorage.getItem(KEY)
    if (v === 'light' || v === 'dark') return v
  } catch { /* storage blocked */ }
  return 'system'
}

function apply() {
  const p = getThemePref()
  root.dataset.theme = p === 'system' ? (dark.matches ? 'dark' : 'light') : p
  applyBrand(brand)
}

export function setThemePref(p: ThemePref): void {
  try { localStorage.setItem(KEY, p) } catch { /* storage blocked */ }
  apply()
}

export function initTheme(): void {
  apply()
  dark.addEventListener('change', () => { if (getThemePref() === 'system') apply() })
}

const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16))
const lum = ([r, g, b]: number[]) => {
  const [R, G, B] = [r, g, b].map((c) => { const s = c / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4 })
  return 0.2126 * R + 0.7152 * G + 0.0722 * B
}

/** Black or white, whichever has the higher WCAG contrast on `hex` (#rrggbb). */
export function onAccent(hex: string): '#000' | '#fff' {
  const L = lum(rgb(hex))
  return (L + 0.05) / 0.05 >= 1.05 / (L + 0.05) ? '#000' : '#fff'
}

/** Sets the studio colour on <html> ('' clears it). In dark mode the CSS lightens the accent, so the text colour is checked against that. */
export function applyBrand(color: string): void {
  brand = /^#[0-9a-f]{6}$/i.test(color) ? color : ''
  if (!brand) {
    root.style.removeProperty('--brand')
    root.style.removeProperty('--on-accent')
    return
  }
  // ponytail: sRGB mix approximates the CSS oklab mix; the black/white pick only differs near the luminance boundary
  const accent = root.dataset.theme === 'dark'
    ? '#' + rgb(brand).map((c) => Math.round(c * 0.65 + 255 * 0.35).toString(16).padStart(2, '0')).join('')
    : brand
  root.style.setProperty('--brand', brand)
  root.style.setProperty('--on-accent', onAccent(accent))
}

if (import.meta.env.DEV) {
  console.assert(onAccent('#ffff00') === '#000' && onAccent('#2547d0') === '#fff' && onAccent('#777777') === '#000')
}
