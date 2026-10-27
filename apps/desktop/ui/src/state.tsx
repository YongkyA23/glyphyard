// App-wide state: AppState, event subscriptions, toasts, the unknown-license confirm and the Adobe pairing prompt.
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, isApiError, on, type ActivationResult, type AppState, type Events } from './api'
import { pop, spring } from './motion'
import { andList, Dialog, errMsg, plural, Skel } from './ui'

type Toast = { id: number; msg: string; action?: { label: string; run: () => void } }
type Ctx = {
  app: AppState
  setApp: (fn: (a: AppState) => AppState) => void
  /** Bumped on `library-changed`; add to effect deps to refetch. */
  libVersion: number
  bump: () => void
  toast: (msg: string, action?: Toast['action']) => void
  /** Runs an activation, asking first when the backend rejects with `unknown_license`. Returns null if cancelled/failed. */
  activate: (fn: (confirm: boolean) => Promise<ActivationResult[]>, opts?: { quiet?: boolean }) => Promise<ActivationResult[] | null>
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
  const [confirm, setConfirm] = useState<{ families: string[]; resolve: (ok: boolean) => void } | null>(null)
  const [adminChange, setAdminChange] = useState<Ctx['adminChange']>(null)
  const [pair, setPair] = useState<Events['adobe-pair-request'] | null>(null)
  const [organizeCount, setOrganizeCount] = useState<number | null>(null)
  const toastId = useRef(0)
  const navigate = useNavigate()

  const load = useCallback(() => {
    setBootError(null)
    api.getAppState().then(setAppState, (e) => setBootError(errMsg(e)))
  }, [])
  useEffect(load, [load])

  const setApp = useCallback((fn: (a: AppState) => AppState) => setAppState((a) => (a ? fn(a) : a)), [])
  const bump = useCallback(() => setLibVersion((v) => v + 1), [])
  const dismissToast = useCallback((id: number) => setToasts((ts) => ts.filter((t) => t.id !== id)), [])
  const toast = useCallback((msg: string, action?: Toast['action']) => {
    const id = ++toastId.current
    setToasts((ts) => [...ts.slice(-2), { id, msg, action }]) // ponytail: newest 3 stack; older ones drop off
    window.setTimeout(() => dismissToast(id), action ? 9000 : 5000)
  }, [dismissToast])

  const activate = useCallback<Ctx['activate']>(
    async (fn, opts) => {
      const report = (res: ActivationResult[]) => {
        const failed = res.filter((r) => !r.ok)
        if (failed.length && !opts?.quiet)
          toast(`${plural(failed.length, 'font')} couldn’t be changed. ${failed[0].message ?? ''}`.trim())
        return res
      }
      try {
        return report(await fn(false))
      } catch (e) {
        if (!isApiError(e, 'unknown_license')) {
          toast(errMsg(e))
          return null
        }
        const families = Array.isArray(e.families) ? (e.families as string[]) : []
        const ok = await new Promise<boolean>((resolve) => setConfirm({ families, resolve }))
        setConfirm(null)
        if (!ok) return null
        try {
          return report(await fn(true))
        } catch (e2) {
          toast(errMsg(e2))
          return null
        }
      }
    },
    [toast],
  )

  useEffect(() => {
    const offs = [
      on('connection-changed', (connection) => setApp((a) => ({ ...a, connection }))),
      on('name-changed', ({ name, nextNameChange }) => setApp((a) => ({ ...a, name, nextNameChange }))),
      on('library-changed', bump),
    ]
    if (main)
      offs.push(
        on('admin-change', (p) => (setAdminChange({ ...p, at: new Date().toISOString() }), bump())),
        on('organize-requested', () => navigate('/organize')),
        on('adobe-pair-request', setPair),
        on('adobe-activated', ({ app: a, document, activated, missing }) =>
          toast(
            `${ADOBE_LABEL[a]} · ${document}: turned on ${plural(activated.length, 'font')}` +
              (missing.length ? `, ${missing.length} not in your library (${andList(missing.slice(0, 3))})` : ''),
          ),
        ),
      )
    return () => offs.forEach((off) => off())
  }, [main, bump, navigate, setApp, toast])

  if (bootError)
    return (
      <div className="center-screen">
        <p role="alert">Typeshelf couldn’t start: {bootError}</p>
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

      <Dialog open={!!confirm} onClose={() => confirm?.resolve(false)} title="Turn on fonts with an unknown license?">
        <p>
          {andList(confirm?.families ?? [])} {(confirm?.families.length ?? 0) > 1 ? 'have' : 'has'} no license label yet. Make sure you’re allowed
          to use {(confirm?.families.length ?? 0) > 1 ? 'them' : 'it'} before turning {(confirm?.families.length ?? 0) > 1 ? 'them' : 'it'} on.
        </p>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => confirm?.resolve(false)}>Cancel</button>
          <button type="button" className="btn primary" onClick={() => confirm?.resolve(true)} autoFocus>Turn on anyway</button>
        </div>
      </Dialog>

      <Dialog open={!!pair} onClose={() => respondPair(false)} title={`Allow ${pair ? ADOBE_LABEL[pair.app] : ''} to turn on fonts automatically?`}>
        <p>
          {pair && `${ADOBE_LABEL[pair.app]} ${pair.appVersion}`} wants to connect to Typeshelf. When you open a document, it will turn on the fonts
          the document uses from your library. Only allow this if you just installed the plug-in.
        </p>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => respondPair(false)}>Don’t allow</button>
          <button type="button" className="btn primary" onClick={() => respondPair(true)}>Allow</button>
        </div>
      </Dialog>

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
