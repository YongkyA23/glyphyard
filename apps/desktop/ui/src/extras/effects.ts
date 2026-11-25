// Shared bits of the easter eggs: the "Playful extras" check, the 5-second interface font swap, random families.
import { useRef } from 'react'
import { api, type Family, type ListFamiliesArgs } from '../api'

/** The last value read; null until the first read. Lets the listeners stay idle while the setting is off. */
let known: boolean | null = null
/** Read when an egg fires (rare), so turning the setting off takes effect at once. A missing value means on. */
export const playful = () => api.getSettings().then((s) => (known = s.playful !== false), () => (known = false))
/** Known to be off: listeners return before doing any work. Refreshed by `refreshPlayful` (Extras: route change, window focus). */
export const playfulOff = () => known === false
export const refreshPlayful = () => void playful()

export const reducedMotion = () => matchMedia('(prefers-reduced-motion: reduce)').matches

export type EggFont = 'comic' | 'papyrus' | 'wingdings'
let timer = 0
/** Sets the interface chrome (not previews: they set their own font-family) in `kind` for 5 seconds. extras.css keys on the attribute. */
export function swapFont(kind: EggFont) {
  clearTimeout(timer)
  document.documentElement.dataset.eggFont = kind
  timer = window.setTimeout(clearFont, 5000)
}
export function clearFont() {
  if (document.documentElement.dataset.eggFont === undefined) return
  clearTimeout(timer)
  delete document.documentElement.dataset.eggFont
}

/** Calls `fn` on the `n`th click in a row, each within 600 ms of the last. Idle while the setting is known to be off,
 *  unless `always` (the Settings page's own egg, where the switch can be turned back on without a route change). */
export function useTaps(n: number, fn: () => void, always = false) {
  const t = useRef({ count: 0, at: 0 })
  return () => {
    if (!always && playfulOff()) return
    const c = t.current
    const now = Date.now()
    c.count = now - c.at < 600 ? c.count + 1 : 1
    c.at = now
    if (c.count < n) return
    c.count = 0
    fn()
  }
}

/** `n` random library families with two small requests (the total, then one page at a random offset). */
export async function randomFamilies(n: number): Promise<Family[]> {
  const q: ListFamiliesArgs = { sort: 'random', seed: Math.floor(Math.random() * 2 ** 31) }
  const { total } = await api.familyPage(q, 0, 1)
  // ponytail: pinned families sort first, so a random offset (not offset 0) keeps them from always showing up.
  return (await api.familyPage(q, Math.floor(Math.random() * Math.max(1, total - n + 1)), n)).items
}

export const pick = <T>(xs: T[]) => xs[Math.floor(Math.random() * xs.length)]
