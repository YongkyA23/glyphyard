// Inline (i) button with a tooltip: opens on hover, focus or click; closes on leave, blur, Escape or an outside click.
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { t } from './i18n'

export function Hint({ text, label }: { text: string; label?: string }) {
  const id = useId()
  const btn = useRef<HTMLButtonElement>(null)
  const tip = useRef<HTMLSpanElement>(null)
  const timer = useRef<number>(undefined)
  const [open, setOpen] = useState(false)

  const show = () => { window.clearTimeout(timer.current); setOpen(true) }
  const hideSoon = () => { window.clearTimeout(timer.current); timer.current = window.setTimeout(() => setOpen(false), 100) }

  useLayoutEffect(() => {
    if (!open) return
    const place = () => {
      const b = btn.current!.getBoundingClientRect()
      const w = tip.current!.offsetWidth
      const h = tip.current!.offsetHeight
      const left = Math.max(8, Math.min(b.left + b.width / 2 - w / 2, window.innerWidth - w - 8))
      const below = b.bottom + 6
      const top = below + h > window.innerHeight - 8 && b.top - 6 - h >= 8 ? b.top - 6 - h : below
      const el = tip.current!
      el.style.left = left + 'px'
      el.style.top = top + 'px'
      el.style.visibility = 'visible'
    }
    place()
    window.addEventListener('resize', place)
    window.addEventListener('scroll', place, true)
    return () => { window.removeEventListener('resize', place); window.removeEventListener('scroll', place, true) }
  }, [open, text])

  useEffect(() => {
    if (!open) return
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') setOpen(false) }
    const down = (e: PointerEvent) => {
      const n = e.target as Node
      if (!btn.current?.contains(n) && !tip.current?.contains(n)) setOpen(false)
    }
    document.addEventListener('keydown', key)
    document.addEventListener('pointerdown', down)
    return () => { document.removeEventListener('keydown', key); document.removeEventListener('pointerdown', down) }
  }, [open])
  useEffect(() => () => window.clearTimeout(timer.current), [])

  return (
    <>
      <button ref={btn} type="button" className="hint hint-btn" aria-label={label ?? t('More info')} aria-describedby={open ? id : undefined}
        aria-expanded={open} onPointerEnter={show} onPointerLeave={hideSoon} onFocus={show} onBlur={hideSoon}
        onClick={show}>
        <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" strokeWidth="1.4" /><path d="M8 7.2v3.6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" /><circle cx="8" cy="5" r="0.9" fill="currentColor" /></svg>
      </button>
      {open && createPortal(
        <span ref={tip} id={id} role="tooltip" className="hint-tip" onPointerEnter={show} onPointerLeave={hideSoon}
          style={{ visibility: 'hidden' }}>{text}</span>,
        document.body,
      )}
    </>
  )
}
