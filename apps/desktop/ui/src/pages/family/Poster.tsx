// Hold Space on the family page (focus not on a control or text field) for a full-window poster of the family.
import { useEffect, useState, type CSSProperties } from 'react'
import { createPortal } from 'react-dom'
import { keysBusy } from '../../ui'
import './glyph-tools.css'

/** Space activates focused controls, so a press on one (fields, dialogs and menus are `keysBusy`) leaves the poster shut. */
const onControl = (t: EventTarget | null) =>
  t instanceof Element && !!t.closest('a[href], button, input, summary, [role="button"], [role="tab"], [role="menuitem"], [role="checkbox"], [role="radio"], [role="switch"], [role="option"], [role="slider"]')

export function Poster({ family, sample, style }: { family: string; sample: string; style: CSSProperties }) {
  const [open, setOpen] = useState(false)
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.key === 'Escape') return setOpen(false)
      if (e.key !== ' ' || e.ctrlKey || e.altKey || e.metaKey || e.defaultPrevented || keysBusy(e) || onControl(e.target)) return
      e.preventDefault() // no page scroll
      setOpen(true)
    }
    const up = (e: KeyboardEvent) => e.key === ' ' && setOpen(false)
    const off = () => setOpen(false)
    addEventListener('keydown', down)
    addEventListener('keyup', up)
    addEventListener('blur', off)
    return () => (removeEventListener('keydown', down), removeEventListener('keyup', up), removeEventListener('blur', off))
  }, [])
  if (!open) return null
  return createPortal(
    <div className="poster" role="img" aria-label={`${family} poster: ${sample}`}>
      <h2 className="poster-name" style={style}>{family}</h2>
      <p className="poster-sample" style={style}>{sample}</p>
      <span className="poster-hint">Release Space or press Esc to close</span>
    </div>,
    document.body,
  )
}
