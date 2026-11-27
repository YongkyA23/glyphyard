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
  // vw/vh make a resize re-render even when the box didn't change (the centred popover and hole move).
  const [view, setView] = useState<{ index: number; box: Box | null; vw: number; vh: number } | null>(null)
  const navigated = useRef(-1)
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
      if (back?.isConnected) return back.focus({ preventScroll: true })
      // The tour changed page and the element is gone: start from the page content instead.
      const main = document.querySelector<HTMLElement>('main')
      if (!main) return
      if (!main.hasAttribute('tabindex')) main.tabIndex = -1
      main.focus({ preventScroll: true })
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

  // Go to the step's page once per step. useNavigate changes identity on every route change, so without the
  // guard a redirect (e.g. / → /onboarding) would bounce back and forth.
  useEffect(() => {
    if (navigated.current === index) return
    navigated.current = index
    if (step.route && location.pathname !== step.route) navigate(step.route)
  }, [index, step, navigate])

  // Find the step's target, then follow it through scroll, resize and re-renders.
  useEffect(() => {
    const target = step.target
    let el: HTMLElement | null = null
    let frame = 0
    let poll = 0
    let ro: ResizeObserver | null = null
    const show = (box: Box | null) => {
      const vw = innerWidth, vh = innerHeight
      setView((v) => (v?.index === index && v.vw === vw && v.vh === vh && sameBox(v.box, box) ? v : { index, box, vw, vh }))
    }
    const measure = () => {
      frame = 0
      if (target && !el?.isConnected) {
        // The page re-rendered the anchor, or it turned up after the wait: follow the new element.
        el = find(target)
        ro?.disconnect()
        if (el) ro?.observe(el)
      }
      show(el ? boxOf(el) : null)
    }
    const schedule = () => void (frame ||= requestAnimationFrame(measure))
    const lock = (found: HTMLElement | null) => {
      el = found
      ro = new ResizeObserver(schedule)
      if (el) {
        // Centre it unless it's already in view; one taller than the window shows its top.
        const r = el.getBoundingClientRect()
        if (r.top < 0 || r.bottom > innerHeight || r.left < 0 || r.right > innerWidth)
          el.scrollIntoView({ block: r.height > innerHeight ? 'start' : 'center', inline: 'nearest' })
        ro.observe(el)
      }
      addEventListener('scroll', schedule, true)
      addEventListener('resize', schedule)
      measure() // now, not on the next frame: a window without frames (hidden) still gets its popover
    }
    const startedAt = performance.now()
    const look = () => {
      const found = target ? find(target) : null
      if (!target || found || performance.now() - startedAt >= WAIT_MS) lock(found)
      else poll = window.setTimeout(look, POLL_MS)
    }
    poll = window.setTimeout(look, 0)
    return () => {
      window.clearTimeout(poll)
      cancelAnimationFrame(frame)
      ro?.disconnect()
      removeEventListener('scroll', schedule, true)
      removeEventListener('resize', schedule)
    }
  }, [index, step])

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

  // While the next target is looked for, the spotlight stays where it was. No target: a zero-size hole in the middle dims everything.
  const spot = view?.box ?? null
  const hole = spot ?? { x: innerWidth / 2, y: innerHeight / 2, w: 0, h: 0 }
  return createPortal(
    <div className="tour">
      <div className="tour-block" onPointerDown={(e) => e.preventDefault()} />
      <div
        className="tour-spot" aria-hidden="true"
        style={{ width: hole.w, height: hole.h, transform: `translate(${hole.x}px, ${hole.y}px)`, borderRadius: spot ? 10 : 0 }}
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
