// Ctrl+wheel resizes a preview, Shift+wheel moves a variable font along its weight axis (visual only, reset when the pointer leaves the row).
import { useEffect, useRef, useState, type CSSProperties } from 'react'
import { api, type Axis } from './api'

// ponytail: refreshed whenever a page with previews opens; a change in Settings shows up on the next page visit.
let playful = true
export const playfulOn = () => playful
export const refreshPlayful = () => api.getSettings().then((s) => void (playful = (s as { playful?: boolean }).playful !== false), () => {})

const axesCache = new Map<string, Axis[] | Promise<void>>()
/** Axes of a family, loaded once with the call the family page uses. Undefined while loading. */
export function familyAxes(key: string): Axis[] | undefined {
  const hit = axesCache.get(key)
  if (Array.isArray(hit)) return hit
  if (!hit) axesCache.set(key, api.getFamily(key).then((f) => void axesCache.set(key, f.axes), () => void axesCache.delete(key)))
  return undefined
}

type Source = { famKey?: string; axes?: Axis[]; variable: boolean }
const wghtOf = (s: Source) => (s.axes ?? (s.famKey ? familyAxes(s.famKey) : undefined))?.find((a) => a.tag === 'wght')

/** Every mounted preview that can change weight; the breathing loop reads this. */
export const previews = new Map<HTMLElement, () => Axis | undefined>()

const ROW = '.font-row, .font-card, .face-row'
export const wheelStep = 2
const weightStep = 25

export function usePreviewWheel(src: Source & { onSize?: (delta: number) => void }) {
  const ref = useRef<HTMLDivElement>(null)
  const [weight, setWeight] = useState<number | null>(null)
  const cur = useRef(src)
  useEffect(() => void (cur.current = src))
  const w = useRef<number | null>(null)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const row = el.closest(ROW) ?? el
    const reset = () => { if (w.current !== null) setWeight((w.current = null)) }
    const onWheel = (e: WheelEvent) => {
      if (!playful) return
      const d = e.deltaY || e.deltaX // Shift turns the wheel into a horizontal one on some systems
      if (!d) return
      const up = d < 0
      if (e.ctrlKey && cur.current.onSize) {
        e.preventDefault() // not the webview zoom
        cur.current.onSize(up ? wheelStep : -wheelStep)
      } else if (e.shiftKey && cur.current.variable) {
        e.preventDefault()
        const a = wghtOf(cur.current)
        if (!a) return // still loading (or no weight axis): the next notch works
        const next = Math.min(a.max, Math.max(a.min, (w.current ?? a.default) + (up ? weightStep : -weightStep)))
        setWeight((w.current = next))
      }
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    row.addEventListener('pointerleave', reset)
    previews.set(el, () => (cur.current.variable ? wghtOf(cur.current) : undefined))
    return () => {
      el.removeEventListener('wheel', onWheel)
      row.removeEventListener('pointerleave', reset)
      previews.delete(el)
    }
  }, [])
  const style: CSSProperties = weight === null ? {} : { fontVariationSettings: `'wght' ${weight}` }
  return [ref, style] as const
}
