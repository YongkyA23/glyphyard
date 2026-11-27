// Toasts: a module-level store (so a toast survives route changes) read through useSyncExternalStore.
import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { t, useLang } from './i18n'
import { errMsg } from './ui'
import { fast, spring } from './motion'
import './toast.css'

type Opts = { tone?: 'ok' | 'bad'; undo?: () => Promise<string | void> }
type Item = { id: number; text: string; tone: 'ok' | 'bad'; undo?: () => Promise<string | void> }

const MAX = 3
let items: Item[] = []
let nextId = 1
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())
const subscribe = (l: () => void) => (listeners.add(l), () => void listeners.delete(l))
const snapshot = () => items

// oxlint-disable-next-line react/only-export-components -- the store function lives beside its component by design
export function toast(text: string, opts: Opts = {}): void {
  items = [...items, { id: nextId++, text, tone: opts.tone ?? 'ok', undo: opts.undo }].slice(-MAX)
  emit()
}
const dismiss = (id: number) => {
  items = items.filter((x) => x.id !== id)
  emit()
}

function ToastView({ item, reduced }: { item: Item; reduced: boolean | null }) {
  const [hover, setHover] = useState(false)
  // Hovering pauses the timer; leaving resumes with the time that was left.
  const [focus, setFocus] = useState(false)
  const left = useRef(item.undo || item.tone === 'bad' ? 8000 : 5000)
  useEffect(() => {
    if (hover || focus) return
    const start = Date.now()
    const h = window.setTimeout(() => dismiss(item.id), left.current)
    return () => { clearTimeout(h); left.current = Math.max(0, left.current - (Date.now() - start)) }
  }, [hover, focus, item.id])
  const undo = async () => {
    if (!item.undo) return
    dismiss(item.id)
    try {
      toast((await item.undo()) || t('Undone.'))
    } catch (e) {
      toast(errMsg(e), { tone: 'bad' })
    }
  }
  return (
    <motion.div className={`toast${item.tone === 'bad' ? ' toast-bad' : ''}`} layout={!reduced}
      initial={reduced ? false : { opacity: 0, y: 16, scale: 0.96 }} animate={{ opacity: 1, y: 0, scale: 1, transition: spring }}
      exit={reduced ? { opacity: 0, transition: { duration: 0 } } : { opacity: 0, scale: 0.96, transition: fast }}
      onPointerEnter={() => setHover(true)} onPointerLeave={() => setHover(false)}
      onFocus={() => setFocus(true)} onBlur={() => setFocus(false)}>
      {item.tone === 'bad' && <span className="toast-flag" aria-hidden="true">!</span>}
      <span className="toast-text">{item.text}</span>
      {item.undo && <button type="button" className="toast-undo" onClick={() => void undo()}>{t('Undo')}</button>}
    </motion.div>
  )
}

export function Toaster() {
  useLang() // sits outside the language remount, so follow the language here
  const list = useSyncExternalStore(subscribe, snapshot)
  const reduced = useReducedMotion()
  useEffect(() => {
    const seen = new Set<string>()
    const onRejection = (e: PromiseRejectionEvent) => {
      const m = errMsg(e.reason)
      if (seen.has(m)) return
      if (seen.size >= 50) seen.clear() // ponytail: forget old messages rather than grow forever
      seen.add(m)
      toast(t('Something didn’t work: {message}', { message: m }), { tone: 'bad' })
    }
    addEventListener('unhandledrejection', onRejection)
    return () => removeEventListener('unhandledrejection', onRejection)
  }, [])
  return (
    <div aria-live="polite" className="toasts">
      <AnimatePresence initial={false}>
        {list.map((item) => <ToastView key={item.id} item={item} reduced={reduced} />)}
      </AnimatePresence>
    </div>
  )
}
