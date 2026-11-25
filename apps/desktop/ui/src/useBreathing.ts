import { useEffect } from 'react'
import { playfulOn, previews, refreshPlayful } from './usePreviewWheel'

const IDLE = 180_000
const PERIOD = 9000
const INPUTS = ['pointermove', 'pointerdown', 'keydown', 'wheel'] as const

/** After 3 minutes without input, variable previews on screen slowly move along their weight axis and back. One timer and one frame loop per page. */
export function useBreathing() {
  useEffect(() => {
    void refreshPlayful()
    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)')
    let last = Date.now(), frame = 0, t0 = 0
    const saved = new Map<HTMLElement, string>()
    const stop = () => {
      cancelAnimationFrame(frame)
      frame = 0
      saved.forEach((v, el) => (el.style.fontVariationSettings = v))
      saved.clear()
    }
    const run = (now: number) => {
      frame = requestAnimationFrame(run)
      const s = Math.sin(((now - t0) / PERIOD) * 2 * Math.PI) * 0.8
      previews.forEach((axis, el) => {
        const a = axis()
        const r = el.getBoundingClientRect()
        if (!a || r.bottom < 0 || r.top > innerHeight || r.width === 0) return
        if (!saved.has(el)) saved.set(el, el.style.fontVariationSettings)
        el.style.fontVariationSettings = `'wght' ${Math.round(a.default + s * (s > 0 ? a.max - a.default : a.default - a.min))}`
      })
    }
    const onInput = () => (last = Date.now(), frame && stop())
    const check = window.setInterval(() => {
      if (frame || document.hidden || reduce.matches || !playfulOn() || Date.now() - last < IDLE) return
      t0 = performance.now()
      frame = requestAnimationFrame(run)
    }, 5000)
    INPUTS.forEach((n) => window.addEventListener(n, onInput, { capture: true, passive: true }))
    return () => {
      window.clearInterval(check)
      INPUTS.forEach((n) => window.removeEventListener(n, onInput, { capture: true }))
      stop()
    }
  }, [])
}
