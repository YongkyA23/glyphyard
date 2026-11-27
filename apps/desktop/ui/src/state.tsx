// App-wide state: AppState, event subscriptions, toasts and the Adobe pairing prompt.
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, on, type ActivationResult, type AppState, type Events } from './api'
import { pop, spring } from './motion'
import { andList, Dialog, errMsg, plural, Skel } from './ui'
import { applyLanguage } from './i18n'

type Toast = { id: number; msg: string; action?: { label: string; run: () => void } }
type Ctx = {
  app: AppState
  setApp: (fn: (a: AppState) => AppState) => void
  /** Bumped on `library-changed`; add to effect deps to refetch. */
  libVersion: number
  bump: () => void
  toast: (msg: string, action?: Toast['action']) => void
  /** Runs an activation and reports failures. Returns null if it failed. */
  activate: (fn: () => Promise<ActivationResult[]>, opts?: { quiet?: boolean }) => Promise<ActivationResult[] | null>
  adminChange: (Events['admin-change'] & { at: string }) | null
  dismissAdmin: () => void
  organizeCount: number | null
  setOrganizeCount: (n: number) => void
}

const AppContext = createContext<Ctx | null>(null)
export const useApp = () => useContext(AppContext)!

const ADOBE_LABEL = { photoshop: 'Photoshop', illustrator: 'Illustrator', indesign: 'InDesign' }

/** `main` = the library window. The quick-activate window skips the global listeners. */
export function AppProvider({ main, children }: { main: boolean; children: ReactNode }) {
  const [app, setAppState] = useState<AppState | null>(null)
  const [bootError, setBootError] = useState<string | null>(null)
  const [libVersion, setLibVersion] = useState(0)
  const [toasts, setToasts] = useState<Toast[]>([])
  const [adminChange, setAdminChange] = useState<Ctx['adminChange']>(null)
  const [pair, setPair] = useState<Events['adobe-pair-request'] | null>(null)
  const [organizeCount, setOrganizeCount] = useState<number | null>(null)
  // Big on/off batches report progress; the pill hides shortly after the last chunk.
  const [progress, setProgress] = useState<Events['activation-progress'] | null>(null)
  useEffect(() => {
    if (!main) return
    let hide = 0
    const off = on('activation-progress', (p) => {
      window.clearTimeout(hide)
      setProgress(p)
      if (p.done >= p.total) hide = window.setTimeout(() => setProgress(null), 900)
    })
    return () => (off(), window.clearTimeout(hide))
  }, [main])
  const toastId = useRef(0)
  const indesignNoted = useRef(0)
  const navigate = useNavigate()

  const load = useCallback(() => {
    setBootError(null)
    api.getAppState().then(setAppState, (e) => setBootError(errMsg(e)))
    api.getSettings().then((s) => applyLanguage(s.language), () => {})
  }, [])
  useEffect(load, [load])

  // Admin branding: app.css derives the accent palette from --brand.
  const brand = app?.brandColor ?? ''
  useEffect(() => {
    const root = document.documentElement
    // The value comes from the server and goes into the stylesheet: only a plain hex colour gets through.
    if (!/^#[0-9a-f]{6}$/i.test(brand)) return
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(brand.slice(i, i + 2), 16))
    // ponytail: perceived brightness in three bands picks readable text; a WCAG contrast calculation if a brand colour ever reads badly.
    const bright = (r * 299 + g * 587 + b * 114) / 1000
    root.style.setProperty('--brand', brand)
    root.dataset.brand = bright > 160 ? 'light' : bright < 70 ? 'dark' : 'mid'
    return () => (root.style.removeProperty('--brand'), void delete root.dataset.brand)
  }, [brand])

  const setApp = useCallback((fn: (a: AppState) => AppState) => setAppState((a) => (a ? fn(a) : a)), [])
  const bump = useCallback(() => setLibVersion((v) => v + 1), [])
  const dismissToast = useCallback((id: number) => setToasts((ts) => ts.filter((t) => t.id !== id)), [])
  const toast = useCallback((msg: string, action?: Toast['action']) => {
    const id = ++toastId.current
    setToasts((ts) => [...ts.slice(-2), { id, msg, action }]) // ponytail: newest 3 stack; older ones drop off
    window.setTimeout(() => dismissToast(id), action ? 9000 : 5000)
  }, [dismissToast])
  // Fonts turn on at once and register with the OS in the background; the ones that failed are already
  // switched back off by then, so only say why.
  useEffect(() => {
    if (!main) return
    return on('activation-done', ({ failed }) => {
      if (failed.length) toast(`${plural(failed.length, 'font')} couldn’t be turned on and ${failed.length === 1 ? 'was' : 'were'} switched back off. ${failed[0]}`)
    })
  }, [main, toast])

  // A promise nobody handled: say so once per distinct message instead of failing silently.
  useEffect(() => {
    const seen = new Set<string>()
    const onRejection = (e: PromiseRejectionEvent) => {
      const msg = errMsg(e.reason)
      if (seen.has(msg)) return
      if (seen.size >= 50) seen.clear() // ponytail: forget old messages rather than grow forever
      seen.add(msg)
      toast(`Something didn’t work: ${msg}`)
    }
    addEventListener('unhandledrejection', onRejection)
    return () => removeEventListener('unhandledrejection', onRejection)
  }, [toast])

  const activate = useCallback<Ctx['activate']>(
    async (fn, opts) => {
      const report = (res: ActivationResult[]) => {
        const failed = res.filter((r) => !r.ok)
        if (failed.length && !opts?.quiet)
          toast(`${plural(failed.length, 'font')} couldn’t be changed. ${failed[0].message ?? ''}`.trim())
        // InDesign only sees font changes after a restart; say so at most every 10 minutes.
        if (res.some((r) => r.ok) && Date.now() - indesignNoted.current > 600_000)
          api.indesignRunning().then((open) => {
            if (!open) return
            indesignNoted.current = Date.now()
            toast('InDesign is open: restart it to see these font changes. Photoshop and Illustrator pick them up right away.')
          }, () => {})
        return res
      }
      try {
        return report(await fn())
      } catch (e) {
        toast(errMsg(e))
        return null
      }
    },
    [toast],
  )

  useEffect(() => {
    const offs = [
      on('connection-changed', (connection) => {
        setApp((a) => ({ ...a, connection, serverConfigured: !!connection.serverUrl.trim() }))
        // The shell sends this after a policy change too, and branding and the name rule come with the policy.
        void api.getAppState().then(({ brandName, brandColor, brandLogo, nameChangeDays }) => setApp((a) => ({ ...a, brandName, brandColor, brandLogo, nameChangeDays })), () => {})
      }),
      // An empty name means the server refused the one chosen offline: back to onboarding to pick another.
      // The shell sends a name only once it is registered (after joining, or a change made on the server).
      on('name-changed', ({ name, nextNameChange }) => setApp((a) => ({ ...a, name, nextNameChange, onboarded: !!name, registered: !!name }))),
      on('library-changed', bump),
    ]
    if (main)
      offs.push(
        on('admin-change', (p) => (setAdminChange({ ...p, at: new Date().toISOString() }), bump())),
        on('organize-requested', () => navigate('/organize')),
        on('adobe-pair-request', setPair),
        on('badge-earned', (b) => toast(`Badge earned: ${b.name}. ${b.description}`, { label: 'See badges', run: () => navigate('/stats') })),
        on('events-dropped', () => toast('Your Glyphyard server didn’t accept some updates from this computer. It may need updating: tell your admin.')),
        on('adobe-activated', ({ app: a, document, activated, missing }) =>
          toast(
            `${ADOBE_LABEL[a]} · ${document}: turned on ${plural(activated.length, 'font')}` +
              (missing.length ? `, ${missing.length} not in your library (${andList(missing.slice(0, 3))})` : '') +
              (a === 'indesign' && activated.length ? '. Restart InDesign to use them.' : ''),
          ),
        ),
      )
    return () => offs.forEach((off) => off())
  }, [main, bump, navigate, setApp, toast])

  if (bootError)
    return (
      <div className="center-screen">
        <p role="alert">Glyphyard couldn’t start: {bootError}</p>
        <button type="button" className="btn" onClick={load}>Try again</button>
      </div>
    )
  if (!app) return main ? <BootSkeleton /> : <div className="center-screen" aria-busy="true" />

  const respondPair = (allow: boolean) => {
    if (pair) api.adobePairRespond(pair.requestId, allow).catch((e) => toast(errMsg(e)))
    setPair(null)
  }

  return (
    <AppContext.Provider
      value={{ app, setApp, libVersion, bump, toast, activate, adminChange, dismissAdmin: () => setAdminChange(null), organizeCount, setOrganizeCount }}
    >
      {children}

      <Dialog open={!!pair} onClose={() => respondPair(false)} title={`Allow ${pair ? ADOBE_LABEL[pair.app] : ''} to turn on fonts automatically?`}>
        <p>
          {pair && `${ADOBE_LABEL[pair.app]} ${pair.appVersion}`} wants to connect to Glyphyard. When you open a document, it will turn on the fonts
          the document uses from your library. Only allow this if you just installed the plug-in.
        </p>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => respondPair(false)}>Don’t allow</button>
          <button type="button" className="btn primary" onClick={() => respondPair(true)}>Allow</button>
        </div>
      </Dialog>

      <AnimatePresence>
        {progress && (
          <motion.div key="progress" className="progress-pill" role="status" {...pop} initial={{ opacity: 0, y: -12 }} transition={spring.pop}>
            <span>{progress.active ? 'Turning on' : 'Turning off'} fonts… {progress.done.toLocaleString('en-US')} / {progress.total.toLocaleString('en-US')}</span>
            <span className="progress-track" aria-hidden="true">
              <motion.span className="progress-fill" animate={{ scaleX: progress.done / Math.max(1, progress.total) }} transition={spring.glide} />
            </span>
          </motion.div>
        )}
      </AnimatePresence>

      <div className="toast-region" role="status" aria-live="polite">
        <AnimatePresence initial={false} mode="popLayout">
          {toasts.map((t) => (
            <motion.div key={t.id} layout className="toast" {...pop} initial={{ opacity: 0, y: 24, scale: 0.94 }} transition={spring.pop}>
              <span>{t.msg}</span>
              {t.action && (
                <button type="button" onClick={() => (t.action!.run(), dismissToast(t.id))}>{t.action.label}</button>
              )}
              <button type="button" aria-label="Dismiss" className="toast-x" onClick={() => dismissToast(t.id)}>✕</button>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </AppContext.Provider>
  )
}

/** First paint of the main window while AppState loads: sidebar + library rows in shimmer. */
function BootSkeleton() {
  return (
    <div className="shell" aria-busy="true">
      <div className="shell-side">
        <div className="sidebar">
          <div className="side-group">{[70, 60, 80, 70, 85, 75, 55].map((w, i) => <span key={i} className="side-nav"><Skel w={`${w}%`} /></span>)}</div>
          <div className="side-group">{[0, 1].map((i) => <span key={i} className="side-item"><Skel w="80%" h={10} /></span>)}</div>
        </div>
      </div>
      <main className="shell-main"><div className="page" style={{ gap: 24 }}>{[0, 1, 2, 3, 4].map((i) => <Skel key={i} h={44} r={8} />)}</div></main>
    </div>
  )
}
