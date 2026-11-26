// Toasts: a module-level store (so a toast survives route changes) read through useSyncExternalStore.
import { useEffect, useState, useSyncExternalStore } from 'react'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { errorText } from './api'
import { fast, spring } from './motion'
import './toast.css'

type Opts = { tone?: 'ok' | 'bad'; undo?: () => Promise<unknown> }
type Item = { id: number; text: string; tone: 'ok' | 'bad'; undo?: () => Promise<unknown> }

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
  items = items.filter((t) => t.id !== id)
  emit()
}

function ToastView({ t, reduced }: { t: Item; reduced: boolean | null }) {
  const [hover, setHover] = useState(false)
  // The timer restarts after the pointer leaves, so a toast under the pointer stays.
  useEffect(() => {
    if (hover) return
    const h = window.setTimeout(() => dismiss(t.id), t.undo ? 8000 : 5000)
    return () => clearTimeout(h)
  }, [hover, t.id, t.undo])
  const undo = async () => {
    if (!t.undo) return
    dismiss(t.id)
    try {
      await t.undo()
      toast('Undone.')
    } catch (e) {
      toast(errorText(e), { tone: 'bad' })
    }
  }
  return (
    <motion.div className={`toast${t.tone === 'bad' ? ' toast-bad' : ''}`} role={t.tone === 'bad' ? 'alert' : undefined} layout={!reduced}
      initial={reduced ? false : { opacity: 0, y: 16, scale: 0.96 }} animate={{ opacity: 1, y: 0, scale: 1, transition: spring }}
      exit={reduced ? { opacity: 0, transition: { duration: 0 } } : { opacity: 0, scale: 0.96, transition: fast }}
      onPointerEnter={() => setHover(true)} onPointerLeave={() => setHover(false)}>
      <span className="toast-text">{t.text}</span>
      {t.undo && <button type="button" className="toast-undo" onClick={() => void undo()}>Undo</button>}
    </motion.div>
  )
}

export function Toaster() {
  const list = useSyncExternalStore(subscribe, snapshot)
  const reduced = useReducedMotion()
  useEffect(() => {
    const seen = new Set<string>()
    const onRejection = (e: PromiseRejectionEvent) => {
      const m = errorText(e.reason)
      if (seen.has(m)) return
      if (seen.size >= 50) seen.clear() // ponytail: forget old messages rather than grow forever
      seen.add(m)
      toast('Something didn’t work: ' + m, { tone: 'bad' })
    }
    addEventListener('unhandledrejection', onRejection)
    return () => removeEventListener('unhandledrejection', onRejection)
  }, [])
  return (
    <div aria-live="polite" className="toasts">
      <AnimatePresence initial={false}>
        {list.map((t) => <ToastView key={t.id} t={t} reduced={reduced} />)}
      </AnimatePresence>
    </div>
  )
}
