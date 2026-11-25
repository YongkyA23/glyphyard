// Shared pieces: data hook, layout, sidebar, small components, formatting.
import { Fragment, useCallback, useEffect, useRef, useState, type DependencyList, type ReactNode } from 'react'
import { Link, NavLink, useLocation, useNavigate, useOutlet } from 'react-router'
import { animate, AnimatePresence, motion, useAnimate, useReducedMotion } from 'motion/react'
import { api, errorText, PAGE_SIZE, type By } from './api'
import { easeOut, fast, page, shakeOpts, shakeX, softSpring, spring, stagger } from './motion'
import { ErrorBoundary } from './ErrorBoundary'

/* ---------- data ---------- */

export type Load<T> = {
  data?: T; error?: string; loading: boolean; at?: Date
  reload: (quiet?: boolean) => Promise<void>
  setData: (f: (d: T) => T) => void
}

/** Runs `load` when deps change; with `pollMs`, refreshes quietly while the tab is visible. */
export function useLoad<T>(load: () => Promise<T>, deps: DependencyList, pollMs = 0): Load<T> {
  const [state, setState] = useState<{ data?: T; error?: string; loading: boolean; at?: Date }>({ loading: true })
  const run = useCallback(load, deps)
  const seq = useRef(0)
  const reload = useCallback(async (quiet = false) => {
    const n = ++seq.current
    if (!quiet) setState((s) => ({ ...s, loading: true, error: undefined }))
    try {
      const data = await run()
      if (n === seq.current) setState({ data, loading: false, at: new Date() })
    } catch (e) {
      if (n === seq.current) setState((s) => ({ ...s, loading: false, error: errorText(e) }))
    }
  }, [run])
  useEffect(() => { void reload() }, [reload])
  useEffect(() => {
    if (!pollMs) return
    const tick = () => { if (document.visibilityState === 'visible') void reload(true) }
    const timer = setInterval(tick, pollMs)
    document.addEventListener('visibilitychange', tick)
    return () => { clearInterval(timer); document.removeEventListener('visibilitychange', tick) }
  }, [reload, pollMs])
  const setData = useCallback((f: (d: T) => T) =>
    setState((s) => (s.data === undefined ? s : { ...s, data: f(s.data) })), [])
  return { ...state, reload, setData }
}

/** Value that settles `ms` after the last change (for search boxes). */
export function useDebounced<T>(value: T, ms = 250): T {
  const [v, setV] = useState(value)
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms)
    return () => clearTimeout(t)
  }, [value, ms])
  return v
}

/** 1-based page number that snaps back to 1 whenever `key` changes (a new search or filter), without a second fetch. */
export function usePage(key: string): [number, (n: number) => void] {
  const [s, setS] = useState({ key, page: 1 })
  const setPage = useCallback((page: number) => setS({ key, page }), [key])
  return [s.key === key ? s.page : 1, setPage]
}

/**
 * Loading / error / empty states around a loaded value. While loading it shows `skeleton`
 * (shaped like the final layout); once loaded, motion children using variants stagger in.
 */
export function Loaded<T>({ state, children, label = 'Loading…', skeleton }: {
  state: Load<T>; children: (d: T) => ReactNode; label?: string; skeleton?: ReactNode
}) {
  if (state.data === undefined) {
    if (state.error) return <ErrorBox message={state.error} retry={() => void state.reload()} />
    return (
      <div className="skeleton" role="status">
        <span className="sr-only">{label}</span>
        {skeleton ?? <span className="stack-10"><Skel w="60%" /><Skel w="85%" /><Skel w="40%" /></span>}
      </div>
    )
  }
  return (
    <>
      {state.error && <ErrorBox message={`Couldn’t refresh: ${state.error}`} retry={() => void state.reload()} />}
      {/* display: contents keeps the page's flex/gap layout; this element only orchestrates the stagger */}
      <motion.div style={{ display: 'contents' }} initial="hidden" animate="show" variants={stagger(0.05)}>
        {children(state.data)}
      </motion.div>
    </>
  )
}

/** Shimmer placeholder block. */
export const Skel = ({ w = '100%', h = 12, r }: { w?: number | string; h?: number; r?: number }) =>
  <span aria-hidden="true" className="skel" style={{ width: w, height: h, borderRadius: r }} />

/** Table-shaped skeleton: same header as the real table, shimmer cells. */
export function TableSkeleton({ head, minWidth, rows = 5, inset }: { head: ReactNode[]; minWidth: number; rows?: number; inset?: boolean }) {
  const widths = ['70%', '55%', '80%', '45%', '65%']
  return (
    <Table caption="Loading" minWidth={minWidth} head={head} inset={inset}>
      {Array.from({ length: rows }, (_, r) => (
        <tr key={r}>{head.map((_, c) => <td key={c}><Skel w={widths[(r + c) % widths.length]} /></td>)}</tr>
      ))}
    </Table>
  )
}

export function ErrorBox({ message, retry }: { message: string; retry?: () => void }) {
  return (
    <div className="error-box" role="alert">
      <span>{message}</span>
      {retry && <button type="button" className="btn btn-sm" onClick={retry}>Try again</button>}
    </div>
  )
}

export const Empty = ({ children }: { children: ReactNode }) => <p className="empty">{children}</p>

/* ---------- layout ---------- */

export function Layout() {
  const outlet = useOutlet()
  const { pathname, key } = useLocation()
  return (
    <div className="shell">
      <Sidebar />
      <main className="main">
        {/* The exiting route keeps its own outlet element, so it renders the old page while fading out. */}
        <AnimatePresence mode="wait">
          <motion.div key={pathname} {...page}><ErrorBoundary resetKey={key} sidebar>{outlet}</ErrorBoundary></motion.div>
        </AnimatePresence>
      </main>
    </div>
  )
}

function Sidebar() {
  const navigate = useNavigate()
  const me = useLoad(() => api.me(), [])
  const counts = useLoad(async () => {
    const [o, lib] = await Promise.all([api.overview(), api.library('', 1, 1)])
    return { users: o.installs, library: lib.total, requests: o.attention.requests }
  }, [])
  // The Requests page announces changes so the open count stays current.
  const reloadCounts = counts.reload
  useEffect(() => {
    const on = () => void reloadCounts(true)
    window.addEventListener('requests-changed', on)
    return () => window.removeEventListener('requests-changed', on)
  }, [reloadCounts])
  const nav: [string, string, number | undefined][] = [
    ['/', 'Overview', undefined],
    ['/users', 'Users', counts.data?.users],
    ['/names', 'Name history', undefined],
    ['/library', 'Shared library', counts.data?.library],
    ['/organize', 'Auto-organize', undefined],
    ['/requests', 'Font requests', counts.data?.requests],
    ['/settings', 'Settings', undefined],
    ['/audit', 'Audit log', undefined],
  ]
  const signOut = async () => {
    try { await api.logout() } finally { navigate('/login') }
  }
  return (
    <nav aria-label="Admin" className="sidebar">
      <div className="brand"><span className="brand-name">Typeshelf</span><span className="brand-sub">Admin</span></div>
      <div className="nav-list">
        {nav.map(([to, label, count]) => (
          <NavLink key={to} to={to} end={to === '/'} className="nav-item">
            {({ isActive }) => <>
              {isActive && <motion.span layoutId="nav-active" className="nav-pill" transition={spring} />}
              <span>{label}</span>
              {count !== undefined && <span className="nav-count fade-in">{count}</span>}
            </>}
          </NavLink>
        ))}
      </div>
      <div className="account">
        <span className="account-email">{me.data?.email ?? '…'}</span>
        <button type="button" className="link-btn account-out" onClick={signOut}>Sign out</button>
      </div>
    </nav>
  )
}

export function PageHeader({ title, sub, actions, back, align = 'center', children }: {
  title: ReactNode; sub?: ReactNode; actions?: ReactNode; back?: boolean; align?: 'center' | 'start' | 'baseline'; children?: ReactNode
}) {
  return (
    <>
      {back && <Link to="/users" className="back">← Users</Link>}
      <div className="page-head" style={{ alignItems: align === 'start' ? 'flex-start' : align }}>
        <div className={children ? 'stack-8' : 'stack-4'}>
          <h1>{title}</h1>
          {sub && <span className="sub">{sub}</span>}
          {children}
        </div>
        {actions}
      </div>
    </>
  )
}

/* ---------- small components ---------- */

/** Switch with a spring knob. Bump `shakeKey` to shake it (e.g. after a failed request snapped it back). */
export function Toggle({ on, label, onChange, disabled, shakeKey }: {
  on: boolean; label: string; onChange: () => void; disabled?: boolean; shakeKey?: number
}) {
  const [scope, animateEl] = useAnimate<HTMLButtonElement>()
  const reduce = useReducedMotion()
  useEffect(() => {
    if (shakeKey && !reduce) void animateEl(scope.current, { x: shakeX }, shakeOpts)
  }, [shakeKey, reduce, animateEl, scope])
  return (
    <motion.button ref={scope} type="button" className="toggle" aria-pressed={on} aria-label={label} onClick={onChange} disabled={disabled}
      whileTap={disabled ? undefined : { scale: 0.94 }}>
      <motion.span className="toggle-knob" initial={false} animate={{ x: on ? 18 : 0 }} transition={spring} />
    </motion.button>
  )
}

/** Cross-fades a short label when it changes (button text, status text). */
export function Swap({ children }: { children: string }) {
  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <motion.span key={children} className="swap" initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0, transition: fast }}
        exit={{ opacity: 0, y: -6, transition: fast }}>
        {children}
      </motion.span>
    </AnimatePresence>
  )
}

/** Number that counts up from 0 on mount and tweens to each new value. */
export function CountUp({ value }: { value: number }) {
  const ref = useRef<HTMLSpanElement>(null)
  const shown = useRef(0)
  const reduce = useReducedMotion()
  useEffect(() => {
    const el = ref.current!
    if (reduce) { el.textContent = String(value); shown.current = value; return }
    const a = animate(shown.current, value, {
      duration: shown.current === 0 ? 0.9 : 0.5, ease: easeOut,
      onUpdate: (v) => { shown.current = v; el.textContent = String(Math.round(v)) },
    })
    return () => a.stop()
  }, [value, reduce])
  // The effect writes the text; React only ever renders the starting 0.
  return <span ref={ref}>0</span>
}

/** Small spinner for busy buttons. */
export const Spin = () => <span aria-hidden="true" className="spin" />

/** Modal <dialog> that springs open and fades out. Render inside <AnimatePresence>; close by unmounting. */
export function Dialog({ onClose, labelledBy, small, children }: { onClose: () => void; labelledBy: string; small?: boolean; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => { const d = ref.current; if (d && !d.open) d.showModal() }, [])
  return (
    <motion.dialog ref={ref} className={small ? 'dialog dialog-sm' : 'dialog'} aria-labelledby={labelledBy}
      // Esc: keep it open so the exit animation can play; the parent unmounts it.
      onCancel={(e) => { e.preventDefault(); onClose() }}
      initial={{ opacity: 0, scale: 0.94, y: 12, '--backdrop': 0 }}
      animate={{ opacity: 1, scale: 1, y: 0, '--backdrop': 1, transition: softSpring }}
      exit={{ opacity: 0, scale: 0.97, y: 6, '--backdrop': 0, transition: fast }}>
      {children}
    </motion.dialog>
  )
}

/** Promise-based replacement for window.confirm with an animated dialog. Render `dialog` somewhere in the tree. */
export function useConfirm() {
  const [req, setReq] = useState<{ text: string; ok: string; danger: boolean; resolve: (v: boolean) => void }>()
  const ask = useCallback((text: string, ok = 'OK', danger = false) =>
    new Promise<boolean>((resolve) => setReq({ text, ok, danger, resolve })), [])
  const answer = (v: boolean) => { req?.resolve(v); setReq(undefined) }
  const dialog = (
    <AnimatePresence>
      {req && (
        <Dialog key="confirm" small labelledBy="confirm-text" onClose={() => answer(false)}>
          <div className="stack-18">
            <p id="confirm-text" className="confirm-text">{req.text}</p>
            <div className="row-8 end">
              <button type="button" className="btn" onClick={() => answer(false)}>Cancel</button>
              <button type="button" autoFocus className={req.danger ? 'btn btn-danger' : 'btn btn-primary'} onClick={() => answer(true)}>{req.ok}</button>
            </div>
          </div>
        </Dialog>
      )}
    </AnimatePresence>
  )
  return [dialog, ask] as const
}

export type Tone = 'user' | 'admin' | 'system' | 'ok' | 'bad' | 'violet' | 'warn' | 'outline'
export const Badge = ({ tone, children, className = '' }: { tone: Tone; children: ReactNode; className?: string }) =>
  <span className={`badge tone-${tone} ${className}`}>{children}</span>

export const byTone: Record<By, Tone> = { user: 'user', admin: 'admin', system: 'system' }
export const byLabel: Record<By, string> = { user: 'User', admin: 'Admin', system: 'System' }

export function Chips<K extends string>({ options, value, onChange, small, label }: {
  options: [K, string][]; value: K; onChange: (k: K) => void; small?: boolean; label: string
}) {
  return (
    <span className="chips" role="group" aria-label={label}>
      {options.map(([k, text]) => (
        <button key={k} type="button" className={small ? 'chip chip-sm' : 'chip'} aria-pressed={value === k} onClick={() => onChange(k)}>
          {text}
        </button>
      ))}
    </span>
  )
}

const num = (n: number) => n.toLocaleString('en-US')

/** Numbered pager: "1–50 of 1,234", Prev/Next, and first, last and current ±1 with gaps. `page` is 1-based. */
export function Pager({ page, total, size = PAGE_SIZE, onPage, label = 'Pages' }: {
  page: number; total: number; size?: number; onPage: (n: number) => void; label?: string
}) {
  if (total === 0) return null
  const pages = Math.ceil(total / size)
  const nums = [...new Set([1, page - 1, page, page + 1, pages])].filter((n) => n >= 1 && n <= pages).sort((a, b) => a - b)
  return (
    <nav className="pager" aria-label={label}>
      <span className="meta">{num(Math.min(total, (page - 1) * size + 1))}–{num(Math.min(total, page * size))} of {num(total)}</span>
      {pages > 1 && (
        <span className="pager-pages">
          <button type="button" className="btn btn-xs" disabled={page <= 1} onClick={() => onPage(page - 1)}>Prev</button>
          {nums.map((n, i) => (
            <Fragment key={n}>
              {i > 0 && n - nums[i - 1] > 1 && <span className="pager-gap" aria-hidden="true">…</span>}
              <button type="button" className="btn btn-xs pager-num" aria-current={n === page ? 'page' : undefined}
                aria-label={`Page ${n}`} onClick={() => onPage(n)}>{n}</button>
            </Fragment>
          ))}
          <button type="button" className="btn btn-xs" disabled={page >= pages} onClick={() => onPage(page + 1)}>Next</button>
        </span>
      )}
    </nav>
  )
}

export function Table({ caption, minWidth, head, children, inset }: {
  caption: string; minWidth: number; head: ReactNode[]; children: ReactNode; inset?: boolean
}) {
  return (
    <div className={inset ? 'table-wrap table-inset' : 'table-wrap'}>
      <table style={{ minWidth }}>
        <caption className="sr-only">{caption}</caption>
        <thead><tr>{head.map((h, i) => <th key={i}>{h}</th>)}</tr></thead>
        <tbody><AnimatePresence>{children}</AnimatePresence></tbody>
      </table>
    </div>
  )
}

/* ---------- formatting ---------- */

const d = (iso: string) => new Date(iso)
export const fmtDate = (iso: string) => d(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' })
export const fmtShort = (iso: string) =>
  d(iso).toLocaleString('en-GB', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })
export const fmtTime = (date: Date) =>
  date.toLocaleString('en-GB', { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' })

export function ago(iso: string) {
  const t = d(iso)
  const mins = Math.round((Date.now() - t.getTime()) / 60000)
  if (mins < 1) return 'Just now'
  if (mins < 60) return `${mins} min ago`
  if (mins < 24 * 60) return `${Math.round(mins / 60)} h ago`
  const yesterday = new Date(); yesterday.setDate(yesterday.getDate() - 1)
  const hm = t.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })
  if (t.toDateString() === yesterday.toDateString()) return `Yesterday, ${hm}`
  const days = Math.round(mins / 1440)
  if (days < 30) return `${days} days ago`
  return fmtDate(iso)
}

export const plural = (n: number, one: string, many = one + 's') => `${n} ${n === 1 ? one : many}`
export const firstName = (name: string) => name.trim().split(/\s+/)[0] || name

export function downloadCsv(filename: string, rows: (string | number)[][]) {
  const cell = (v: string | number) => `"${String(v).replace(/"/g, '""')}"`
  const blob = new Blob([rows.map((r) => r.map(cell).join(',')).join('\r\n')], { type: 'text/csv' })
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = filename
  a.click()
  URL.revokeObjectURL(a.href)
}
