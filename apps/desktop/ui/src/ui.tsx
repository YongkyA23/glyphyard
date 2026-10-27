// Small shared pieces: switches, chips, badges, dialog, formatting.
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { animate, AnimatePresence, motion, useMotionValue, useReducedMotion, useTransform } from 'motion/react'
import type { License } from './api'
import { dur, easeOut, pop, spring } from './motion'

export const LICENSE_LABEL: Record<License, string> = {
  licensed: 'Licensed', personal: 'Personal use', client: 'Client-provided', unknown: 'Unknown license',
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

/** "A", "A and B", "A, B and C" */
export function andList(items: string[]) {
  return items.length <= 1 ? items.join('') : `${items.slice(0, -1).join(', ')} and ${items.at(-1)}`
}

export function Switch({ on, mixed, label, onClick, disabled }: { on: boolean; mixed?: boolean; label: string; onClick: () => void; disabled?: boolean }) {
  return (
    <button type="button" className="switch" aria-pressed={mixed ? 'mixed' : on} aria-label={label} onClick={onClick} disabled={disabled}>
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

/** Unknown props aside, an ApiError message or a generic fallback. */
export const errMsg = (e: unknown) =>
  e && typeof e === 'object' && 'message' in e && typeof (e as { message: unknown }).message === 'string'
    ? (e as { message: string }).message
    : 'Something went wrong.'
