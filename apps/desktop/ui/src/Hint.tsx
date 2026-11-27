// ⓘ button with a tooltip: explanatory copy that used to sit under headings and labels.
// Opens on hover, focus or click; closes on leave (with a grace to reach the tip), blur, Escape or an outside click.
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { t } from './i18n'

const GAP = 6
const EDGE = 8

export function Hint({ text, label }: { text: string; label?: string }) {
  const id = useId()
  const btn = useRef<HTMLButtonElement>(null)
  const tip = useRef<HTMLSpanElement>(null)
  const [open, setOpen] = useState(false)
  // Inside a modal <dialog> the tip must live in the dialog (top layer), elsewhere in <body>.
  const [host, setHost] = useState<Element | null>(null)
  const btnRef = useCallback((el: HTMLButtonElement | null) => {
    btn.current = el
    if (el) setHost(el.closest('dialog') ?? document.body)
  }, [])
  const leave = useRef(0)
  useEffect(() => () => window.clearTimeout(leave.current), [])

  const show = useCallback(() => (window.clearTimeout(leave.current), setOpen(true)), [])
  const hide = useCallback(() => (window.clearTimeout(leave.current), setOpen(false)), [])
  const hideSoon = useCallback(() => {
    window.clearTimeout(leave.current)
    leave.current = window.setTimeout(() => setOpen(false), 100)
  }, [])

  // Below the button, above it when there's no room, kept 8px inside the viewport.
  const place = useCallback(() => {
    const b = btn.current, el = tip.current
    if (!b || !el) return
    const r = b.getBoundingClientRect()
    const w = el.offsetWidth, h = el.offsetHeight
    let top = r.bottom + GAP
    if (top + h > innerHeight - EDGE && r.top - GAP - h >= EDGE) top = r.top - GAP - h
    top = Math.max(EDGE, Math.min(top, innerHeight - EDGE - h))
    const left = Math.max(EDGE, Math.min(r.left + r.width / 2 - w / 2, innerWidth - EDGE - w))
    el.style.transform = `translate(${Math.round(left)}px, ${Math.round(top)}px)`
  }, [])

  useLayoutEffect(() => {
    if (open) place()
  }, [open, place, text])

  useEffect(() => {
    if (!open) return
    let frame = 0
    const follow = () => (cancelAnimationFrame(frame), (frame = requestAnimationFrame(place)))
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      // Escape on the button closes only the tip, not a dialog around it.
      if (e.target === btn.current) {
        e.preventDefault()
        e.stopPropagation()
      }
      hide()
    }
    const onDown = (e: PointerEvent) => {
      const n = e.target as Node
      if (!btn.current?.contains(n) && !tip.current?.contains(n)) hide()
    }
    addEventListener('scroll', follow, true)
    addEventListener('resize', follow)
    document.addEventListener('keydown', onKey, true)
    document.addEventListener('pointerdown', onDown, true)
    return () => {
      cancelAnimationFrame(frame)
      removeEventListener('scroll', follow, true)
      removeEventListener('resize', follow)
      document.removeEventListener('keydown', onKey, true)
      document.removeEventListener('pointerdown', onDown, true)
    }
  }, [open, place, hide])

  return (
    <>
      <button
        ref={btnRef} type="button" className="hint" aria-label={label ?? t('More info')} aria-describedby={id}
        onPointerEnter={show} onPointerLeave={hideSoon} onFocus={show} onBlur={hide} onClick={show}
      >
        <svg aria-hidden="true" width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
          <circle cx="8" cy="8" r="6.75" />
          <path d="M8 7.25v4" />
          <circle cx="8" cy="4.9" r=".4" fill="currentColor" />
        </svg>
      </button>
      {/* Always in the DOM (hidden when closed) so aria-describedby always resolves. */}
      {host && createPortal(
        <span ref={tip} id={id} role="tooltip" className="hint-tip" hidden={!open} onPointerEnter={show} onPointerLeave={hideSoon}>{text}</span>,
        host,
      )}
    </>
  )
}
