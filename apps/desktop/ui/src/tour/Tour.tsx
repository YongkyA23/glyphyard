// Spotlight tours: dims the app, cuts a hole over a [data-tour] element and shows the step's popover next to it.
// A step whose target doesn't turn up within 1.5 s (empty library, another page) shows the popover centred instead.
import { createContext, useCallback, useContext, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate } from 'react-router'
import { t, useLang } from '../i18n'
import { GUIDES, type Guide, type GuideId, type Step } from './guides'
import { markDone } from './progress'
import './tour.css'

type Ctx = { start(id: GuideId): void; active: GuideId | null }
const TourContext = createContext<Ctx>({ start: () => {}, active: null })

// oxlint-disable-next-line react/only-export-components -- the hook belongs with its provider
export const useTour = () => useContext(TourContext)

export function TourProvider({ children }: { children: ReactNode }) {
  const [active, setActive] = useState<GuideId | null>(null)
  const start = useCallback((id: GuideId) => setActive(id), [])
  const close = useCallback(() => setActive(null), [])
  const value = useMemo(() => ({ start, active }), [start, active])
  const guide = GUIDES.find((g) => g.id === active)
  return (
    <TourContext.Provider value={value}>
      {children}
      {guide && <TourRun key={guide.id} guide={guide} onClose={close} />}
    </TourContext.Provider>
  )
}

type Box = { x: number; y: number; w: number; h: number }
const PAD = 6 // spotlight padding around the target
const GAP = 12 // spotlight to popover
const EDGE = 12 // popover to viewport edge
const WAIT_MS = 1500
const POLL_MS = 100

const find = (name: string): HTMLElement | null => {
  const el = document.querySelector<HTMLElement>(`[data-tour="${name}"]`)
  if (!el) return null
  const r = el.getBoundingClientRect()
  return r.width || r.height ? el : null
}
const boxOf = (el: HTMLElement): Box => {
  const r = el.getBoundingClientRect()
  return { x: r.left - PAD, y: r.top - PAD, w: r.width + PAD * 2, h: r.height + PAD * 2 }
}
const sameBox = (a: Box | null, b: Box | null) => a === b || (!!a && !!b && a.x === b.x && a.y === b.y && a.w === b.w && a.h === b.h)

type Side = 'top' | 'bottom' | 'left' | 'right'
const OPPOSITE: Record<Side, Side> = { top: 'bottom', bottom: 'top', left: 'right', right: 'left' }

/** Popover position next to the spotlight: the step's side if it fits, else the opposite, else any; centred when nothing fits. */
function placePopover(box: Box | null, w: number, h: number, placement: Step['placement']) {
  const vw = innerWidth, vh = innerHeight
  const clamp = (x: number, y: number) => ({ x: Math.max(EDGE, Math.min(x, vw - EDGE - w)), y: Math.max(EDGE, Math.min(y, vh - EDGE - h)) })
  if (!box) return clamp((vw - w) / 2, (vh - h) / 2)
  const fits: Record<Side, boolean> = {
    bottom: box.y + box.h + GAP + h <= vh - EDGE, top: box.y - GAP - h >= EDGE,
    right: box.x + box.w + GAP + w <= vw - EDGE, left: box.x - GAP - w >= EDGE,
  }
  const first: Side = placement && placement !== 'auto' ? placement : 'bottom'
  const side = [first, OPPOSITE[first], ...(['bottom', 'top', 'right', 'left'] as Side[])].find((s) => fits[s])
  const cx = box.x + box.w / 2 - w / 2, cy = box.y + box.h / 2 - h / 2
  switch (side) {
    case 'bottom': return clamp(cx, box.y + box.h + GAP)
    case 'top': return clamp(cx, box.y - GAP - h)
    case 'right': return clamp(box.x + box.w + GAP, cy)
    case 'left': return clamp(box.x - GAP - w, cy)
    default: return clamp((vw - w) / 2, (vh - h) / 2)
  }
}

function TourRun({ guide, onClose }: { guide: Guide; onClose: () => void }) {
  useLang() // re-render the open popover when the language changes
  const navigate = useNavigate()
  const titleId = useId(), bodyId = useId()
  const [index, setIndex] = useState(0)
  // null box = centred; ready = target found or given up on (the popover is hidden while waiting).
  const [view, setView] = useState<{ index: number; box: Box | null } | null>(null)
  const pop = useRef<HTMLDivElement>(null)
  const next = useRef<HTMLButtonElement>(null)
  const step = guide.steps[index]
  const last = index === guide.steps.length - 1
  const ready = view?.index === index

  // While running: the app behind is inert (no focus, no clicks, hidden from screen readers); focus returns afterwards.
  useEffect(() => {
    const back = document.activeElement as HTMLElement | null
    const root = document.getElementById('root')
    if (root) root.inert = true
    return () => {
      if (root) root.inert = false
      if (back?.isConnected) back.focus({ preventScroll: true })
    }
  }, [])

  const finish = useCallback(() => (markDone(guide.id), onClose()), [guide.id, onClose])
  const go = useCallback((d: number) => setIndex((i) => Math.max(0, Math.min(guide.steps.length - 1, i + d))), [guide.steps.length])

  // Keys while running: Esc closes, ←/→ step, Tab stays inside the popover. Nothing reaches the app's own shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      e.stopPropagation()
      if (e.key === 'Escape') return e.preventDefault(), onClose()
      if (e.key === 'ArrowRight') return e.preventDefault(), go(1)
      if (e.key === 'ArrowLeft') return e.preventDefault(), go(-1)
      if (e.key !== 'Tab' || !pop.current) return
      const items = [...pop.current.querySelectorAll<HTMLElement>('button:not(:disabled)')]
      if (!items.length) return e.preventDefault()
      const at = items.indexOf(document.activeElement as HTMLElement)
      if (e.shiftKey ? at <= 0 : at === items.length - 1 || at === -1) {
        e.preventDefault()
        items[e.shiftKey ? items.length - 1 : 0].focus()
      }
    }
    addEventListener('keydown', onKey, true)
    return () => removeEventListener('keydown', onKey, true)
  }, [onClose, go])

  // Find the step's target (navigating to its page first), then follow it through scroll, resize and re-renders.
  useEffect(() => {
    if (step.route && location.pathname !== step.route) navigate(step.route)
    const target = step.target
    let el: HTMLElement | null = null
    let frame = 0
    let poll = 0
    let ro: ResizeObserver | null = null
    const show = (box: Box | null) => setView((v) => (v?.index === index && sameBox(v.box, box) ? v : { index, box }))
    const measure = () => {
      frame = 0
      if (target && !el?.isConnected) el = find(target) // the page re-rendered the anchor
      show(el ? boxOf(el) : null)
    }
    const schedule = () => void (frame ||= requestAnimationFrame(measure))
    const lock = (found: HTMLElement | null) => {
      el = found
      if (el) {
        // Centre it unless it's already in view; one taller than the window shows its top.
        const r = el.getBoundingClientRect()
        if (r.top < 0 || r.bottom > innerHeight || r.left < 0 || r.right > innerWidth)
          el.scrollIntoView({ block: r.height > innerHeight ? 'start' : 'center', inline: 'nearest' })
        ro = new ResizeObserver(schedule)
        ro.observe(el)
      }
      addEventListener('scroll', schedule, true)
      addEventListener('resize', schedule)
      schedule()
    }
    if (!target) lock(null)
    else {
      const startedAt = performance.now()
      const look = () => {
        const found = find(target)
        if (found || performance.now() - startedAt >= WAIT_MS) lock(found)
        else poll = window.setTimeout(look, POLL_MS)
      }
      poll = window.setTimeout(look, 0)
    }
    return () => {
      window.clearTimeout(poll)
      cancelAnimationFrame(frame)
      ro?.disconnect()
      removeEventListener('scroll', schedule, true)
      removeEventListener('resize', schedule)
    }
  }, [index, step, navigate])

  // Place the popover after each layout change, and move focus to Next when a step appears.
  const box = ready ? view.box : null
  useLayoutEffect(() => {
    const p = pop.current
    if (!p || !ready) return
    const { x, y } = placePopover(box, p.offsetWidth, p.offsetHeight, step.placement)
    p.style.translate = `${Math.round(x)}px ${Math.round(y)}px` // `translate`, so the entry animation can use transform
  })
  useEffect(() => {
    if (ready) next.current?.focus({ preventScroll: true })
  }, [ready, index])

  // No target: a zero-size hole in the middle dims the whole window.
  const hole = box ?? { x: innerWidth / 2, y: innerHeight / 2, w: 0, h: 0 }
  return createPortal(
    <div className="tour">
      <div className="tour-block" onPointerDown={(e) => e.preventDefault()} />
      <div
        className="tour-spot" aria-hidden="true"
        style={{ width: hole.w, height: hole.h, transform: `translate(${hole.x}px, ${hole.y}px)`, borderRadius: box ? 10 : 0 }}
      />
      <div ref={pop} className="tour-pop" role="dialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={bodyId} hidden={!ready}>
        <h2 id={titleId}>{t(step.title) /* i18n-dynamic: guide text is marked with tk() */}</h2>
        <p id={bodyId}>{t(step.body) /* i18n-dynamic */}</p>
        <div className="tour-foot">
          <span className="tour-count">
            <span aria-hidden="true">{index + 1} / {guide.steps.length}</span>
            <span className="sr-only">{t('Step {n} of {total}', { n: index + 1, total: guide.steps.length })}</span>
          </span>
          <button type="button" className="btn small link" onClick={onClose}>{t('Skip')}</button>
          {index > 0 && <button type="button" className="btn small" onClick={() => go(-1)}>{t('Back')}</button>}
          <button ref={next} type="button" className="btn small primary" onClick={() => (last ? finish() : go(1))}>{last ? t('Done') : t('Next')}</button>
        </div>
      </div>
    </div>,
    document.body,
  )
}
