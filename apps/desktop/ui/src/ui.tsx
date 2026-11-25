// Small shared pieces: switches, chips, badges, dialog, formatting.
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { animate, AnimatePresence, motion, useMotionValue, useReducedMotion, useTransform } from 'motion/react'
import { api, type Collection, type Family, type FontCategory, type License } from './api'
import { dur, easeOut, pop, spring } from './motion'

export const LICENSE_LABEL: Record<License, string> = {
  licensed: 'Licensed', personal: 'Personal use', client: 'Client-provided', unknown: 'Unknown license',
}

export const CATEGORY_LABEL: Record<FontCategory, string> = {
  serif: 'Serif', sans: 'Sans', display: 'Display', handwriting: 'Handwriting', mono: 'Mono',
}

export const num = (n: number) => n.toLocaleString('en-US')
export const plural = (n: number, one: string, many = one + 's') => `${num(n)} ${n === 1 ? one : many}`
export const basename = (p: string) => p.split(/[\\/]/).pop() ?? p
export const dirname = (p: string) => p.slice(0, Math.max(p.lastIndexOf('\\'), p.lastIndexOf('/')))
export const initials = (name: string) => name.trim().split(/\s+/).slice(0, 2).map((w) => w[0]?.toUpperCase()).join('') || '?'

export const fmtDate = (iso: string) => new Date(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' })
const hm = (d: Date) => d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })
export function fmtWhen(iso: string) {
  const d = new Date(iso)
  return d.toDateString() === new Date().toDateString() ? `today ${hm(d)}` : `${fmtDate(iso)}, ${hm(d)}`
}
export function ago(iso: string | null) {
  if (!iso) return 'never'
  const s = (Date.now() - new Date(iso).getTime()) / 1000
  if (s < 45) return 'just now'
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))} min ago`
  if (new Date(iso).toDateString() === new Date().toDateString()) return 'today'
  const days = Math.max(1, Math.round(s / 86400))
  if (days < 14) return days === 1 ? 'yesterday' : `${days} days ago`
  if (days < 60) return `${Math.round(days / 7)} weeks ago`
  return fmtDate(iso)
}
export const daysUntil = (iso: string) => Math.max(0, Math.ceil((new Date(iso).getTime() - Date.now()) / 86_400_000))

/** "CmdOrCtrl+Alt+Space" → "Ctrl + Alt + Space" (or the Mac names). */
export function formatShortcut(s: string, platform: 'windows' | 'macos') {
  const mac = platform === 'macos'
  return s
    .replace(/CmdOrCtrl|CommandOrControl/gi, mac ? 'Cmd' : 'Ctrl')
    .replace(/\bAlt\b/i, mac ? 'Option' : 'Alt')
    .split('+')
    .join(' + ')
}

/** Collections in tree order (each parent followed by its sub-collections) with their depth. */
export function collectionTree(all: Collection[]): { c: Collection; depth: number }[] {
  const ids = new Set(all.map((c) => c.id))
  const walk = (parent: number | null, depth: number): { c: Collection; depth: number }[] =>
    all.filter((c) => (parent === null ? c.parentId === null || !ids.has(c.parentId) : c.parentId === parent)).flatMap((c) => [{ c, depth }, ...walk(c.id, depth + 1)])
  return walk(null, 0)
}

/** "A", "A and B", "A, B and C" */
export function andList(items: string[]) {
  return items.length <= 1 ? items.join('') : `${items.slice(0, -1).join(', ')} and ${items.at(-1)}`
}

export function Switch({ on, mixed, label, onClick, disabled, title }: { on: boolean; mixed?: boolean; label: string; onClick: () => void; disabled?: boolean; title?: string }) {
  return (
    <button type="button" className="switch" aria-pressed={mixed ? 'mixed' : on} aria-label={label} onClick={onClick} disabled={disabled} title={title}>
      <motion.span initial={false} animate={{ x: mixed ? 9 : on ? 18 : 0 }} transition={spring.pop} />
    </button>
  )
}

export function Chip({ pressed, onClick, children, className = 'chip' }: { pressed: boolean; onClick: () => void; children: ReactNode; className?: string }) {
  return (
    <motion.button type="button" className={className} aria-pressed={pressed} onClick={onClick} whileTap={{ scale: 0.94 }} transition={spring.pop}>
      {children}
    </motion.button>
  )
}

/** A number that tweens to its new value (counts up on mount). */
export function Num({ value }: { value: number }) {
  const mv = useMotionValue(0)
  const text = useTransform(mv, (v) => num(Math.round(v)))
  const reduce = useReducedMotion()
  useEffect(() => {
    if (reduce) return void mv.set(value)
    const c = animate(mv, value, { duration: dur.count, ease: easeOut })
    return () => c.stop()
  }, [value, reduce, mv])
  return <motion.span style={{ fontVariantNumeric: 'tabular-nums' }}>{text}</motion.span>
}

export const Spinner = () => (
  <svg aria-hidden="true" className="spinner" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
    <path d="M12 3a9 9 0 1 0 9 9" />
  </svg>
)

/** Check mark that draws itself. */
export const Check = () => (
  <svg aria-hidden="true" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
    <motion.path d="M5 12.5l4.5 4.5L19 7" initial={{ pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: dur.slow, ease: easeOut }} />
  </svg>
)

/** Ring on the selected .select-card; glides between cards that share `group`. */
export const SelectRing = ({ group }: { group: string }) => <motion.span aria-hidden="true" layoutId={group} className="select-ring" transition={spring.glide} />

/** Shimmer placeholder block. */
export const Skel = ({ w = '100%', h = 12, r }: { w?: number | string; h?: number; r?: number }) => (
  <span aria-hidden="true" className="skel" style={{ width: w, height: h, borderRadius: r }} />
)

export type Tone = 'ok' | 'accent' | 'violet' | 'warn' | 'bad' | 'neutral'
export const Badge = ({ tone, children }: { tone: Tone; children: ReactNode }) => <span className={`badge ${tone}`}>{children}</span>
const LIC_TONE: Record<License, Tone> = { licensed: 'ok', personal: 'accent', client: 'violet', unknown: 'warn' }
export const LicenseBadge = ({ license }: { license: License }) => <Badge tone={LIC_TONE[license]}>{LICENSE_LABEL[license]}</Badge>

/** Native modal <dialog>; Esc and the backdrop close it. Stays shown until the panel's exit animation ends. */
export function Dialog({ open, onClose, title, children, wide }: { open: boolean; onClose: () => void; title: string; children: ReactNode; wide?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null)
  const [shown, setShown] = useState(open)
  if (open && !shown) setShown(true)
  useEffect(() => {
    const d = ref.current
    if (!d) return
    if (shown && !d.open) d.showModal()
    if (!shown && d.open) d.close()
  }, [shown])
  return (
    <dialog
      ref={ref}
      className={`${wide ? 'dialog wide' : 'dialog'}${shown && !open ? ' closing' : ''}`}
      aria-labelledby="dialog-title"
      onCancel={(e) => (e.preventDefault(), onClose())}
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <AnimatePresence onExitComplete={() => setShown(false)}>
        {open && (
          <motion.div className="dialog-body" {...pop} transition={spring.panel}>
            <h2 id="dialog-title">{title}</h2>
            {children}
          </motion.div>
        )}
      </AnimatePresence>
    </dialog>
  )
}

/** Dialog that asks for one line of text (names). Submits trimmed, non-empty values only. */
export function PromptDialog({ open, title, label, initial = '', confirm, onClose, onSubmit, children }: {
  open: boolean; title: string; label: string; initial?: string; confirm: string; onClose: () => void; onSubmit: (value: string) => void; children?: ReactNode
}) {
  return (
    <Dialog open={open} onClose={onClose} title={title}>
      <PromptForm {...{ label, initial, confirm, onClose, onSubmit, children }} />
    </Dialog>
  )
}
/** Mounts each time the dialog opens, so the field starts at `initial` with the text selected. */
function PromptForm({ label, initial, confirm, onClose, onSubmit, children }: {
  label: string; initial: string; confirm: string; onClose: () => void; onSubmit: (value: string) => void; children?: ReactNode
}) {
  const [value, setValue] = useState(initial)
  return (
    <form style={{ display: 'contents' }} onSubmit={(e) => (e.preventDefault(), value.trim() && onSubmit(value.trim()))}>
      <label className="field-label">
        {label}
        <input className="input" value={value} onChange={(e) => setValue(e.target.value)} autoFocus onFocus={(e) => e.target.select()} />
      </label>
      <div className="dialog-actions">
        {children}
        <button type="button" className="btn" onClick={onClose}>Cancel</button>
        <button type="submit" className="btn primary" disabled={!value.trim()}>{confirm}</button>
      </div>
    </form>
  )
}

/** Unknown props aside, an ApiError message or a generic fallback. */
export const errMsg = (e: unknown) =>
  e && typeof e === 'object' && 'message' in e && typeof (e as { message: unknown }).message === 'string'
    ? (e as { message: string }).message
    : 'Something went wrong.'

/** Page numbers to show: first, last, and two around the current one, with gaps as null. */
export function pageList(current: number, last: number): (number | null)[] {
  const keep = new Set([1, last, current - 1, current, current + 1].filter((p) => p >= 1 && p <= last))
  const sorted = [...keep].sort((a, b) => a - b)
  return sorted.flatMap((p, i) => (i > 0 && p - sorted[i - 1] > 1 ? [null, p] : [p]))
}

/** Typing in a field, or a dialog or menu is open: single-key shortcuts stay quiet. */
export const keysBusy = (e: KeyboardEvent) =>
  !!(e.target as Element).closest?.('input:not([type=checkbox]):not([type=radio]), textarea, select, [contenteditable=""], [contenteditable="true"]') ||
  !!document.querySelector('dialog[open], .ctx-menu')

/** True once the element has come near the viewport (it never goes back to false). */
export function useSeen<T extends Element>(margin = '200px') {
  const ref = useRef<T>(null)
  const [seen, setSeen] = useState(false)
  useEffect(() => {
    const el = ref.current
    if (!el || seen) return
    const io = new IntersectionObserver((es) => es.some((e) => e.isIntersecting) && setSeen(true), { rootMargin: margin })
    io.observe(el)
    return () => io.disconnect()
  }, [seen, margin])
  return [ref, seen] as const
}

/** "1–30 of 1,950" plus numbered page buttons. */
export function Pager({ page, size, total, onPage }: { page: number; size: number; total: number; onPage: (p: number) => void }) {
  const pages = Math.max(1, Math.ceil(total / size))
  if (!total) return null
  return (
    <nav aria-label="Pages" className="pager">
      <span className="faint" role="status" style={{ fontSize: 12 }}>
        {num((page - 1) * size + 1)}–{num(Math.min(page * size, total))} of {num(total)}
      </span>
      {pages > 1 && (
        <span className="pager-buttons">
          <button type="button" className="btn small" disabled={page === 1} onClick={() => onPage(page - 1)}>‹ Prev</button>
          {pageList(page, pages).map((p, i) =>
            p === null ? (
              <span key={`gap-${i}`} className="faint" aria-hidden="true">…</span>
            ) : (
              <button
                key={p} type="button" className={p === page ? 'btn small primary' : 'btn small'} aria-current={p === page ? 'page' : undefined}
                aria-label={`Page ${p}`} onClick={() => onPage(p)}
              >{p}</button>
            ),
          )}
          <button type="button" className="btn small" disabled={page === pages} onClick={() => onPage(page + 1)}>Next ›</button>
        </span>
      )}
    </nav>
  )
}

/** Search-as-you-type family picker: asks the backend for at most 30 matches instead of listing the library. */
export function FamilyPicker({ id, label, value, onChange }: { id: string; label: string; value?: Family; onChange: (key: string) => void }) {
  const [q, setQ] = useState('')
  const [open, setOpen] = useState(false)
  const [hits, setHits] = useState<Family[]>([])
  const [hi, setHi] = useState(0)
  useEffect(() => {
    if (!open) return
    const t = setTimeout(() => api.familyPage({ search: q.trim() || undefined, sort: 'name' }, 0, 30).then((p) => (setHits(p.items), setHi(0)), () => {}), 120)
    return () => clearTimeout(t)
  }, [q, open])
  const choose = (f?: Family) => {
    if (f) onChange(f.key)
    setOpen(false)
  }
  return (
    <label htmlFor={id} className="field-label picker">
      {label}
      <input
        id={id} className="input" style={{ minWidth: 200 }} role="combobox" aria-expanded={open} aria-controls={`${id}-list`} aria-autocomplete="list"
        aria-activedescendant={open && hits[hi] ? `${id}-${hi}` : undefined}
        placeholder={value?.family ?? 'Search fonts'} value={open ? q : (value?.family ?? '')}
        onFocus={() => (setQ(''), setOpen(true))} onBlur={() => setOpen(false)} onChange={(e) => setQ(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown') (e.preventDefault(), setHi((i) => Math.min(i + 1, hits.length - 1)))
          else if (e.key === 'ArrowUp') (e.preventDefault(), setHi((i) => Math.max(i - 1, 0)))
          else if (e.key === 'Enter') (e.preventDefault(), choose(hits[hi]))
          else if (e.key === 'Escape') setOpen(false)
        }}
      />
      {open && (
        <ul id={`${id}-list`} role="listbox" className="picker-list">
          {hits.map((f, i) => (
            <li key={f.key} id={`${id}-${i}`} role="option" aria-selected={i === hi} onMouseDown={(e) => (e.preventDefault(), choose(f))}>{f.family}</li>
          ))}
          {!hits.length && <li className="faint" aria-disabled="true">No fonts match</li>}
        </ul>
      )}
    </label>
  )
}
