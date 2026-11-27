// Right-click menus: one accessible ContextMenu (arrow keys, Enter, Esc, submenus), positioned at the cursor and kept inside the window.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react'
import { createPortal } from 'react-dom'
import { AnimatePresence, motion } from 'motion/react'
import { t } from './i18n'
import { dur, easeOut } from './motion'

export type MenuItem =
  | { label: string; onSelect: () => void; disabled?: boolean; danger?: boolean; hint?: string }
  | { label: string; items: MenuItem[]; disabled?: boolean }
  | 'sep'
type Open = { x: number; y: number; items: MenuItem[]; label: string; first: boolean }

const GAP = 8
const isAction = (it: MenuItem): it is Exclude<MenuItem, 'sep'> => it !== 'sep'

/** Open with `show(event, items)` from onContextMenu (or a button's onClick); render `menu` once. */
export function useContextMenu(label = t('Actions')) {
  const [open, setOpen] = useState<Open | null>(null)
  const back = useRef<HTMLElement | null>(null)
  /** `below`: open under the element (menu buttons) instead of at the pointer. */
  const show = useCallback((e: ReactMouseEvent<Element>, items: MenuItem[], below = false) => {
    e.preventDefault()
    back.current = document.activeElement as HTMLElement | null
    let x = e.clientX, y = e.clientY
    // The Menu key / Shift+F10 fire contextmenu without a pointer position.
    if (below || (!x && !y)) {
      const r = e.currentTarget.getBoundingClientRect()
      x = r.left
      y = r.bottom + 4
    }
    setOpen({ x, y, items, label, first: below || (!e.clientX && !e.clientY) })
  }, [label])
  const close = useCallback((refocus = true) => {
    setOpen(null)
    if (refocus) back.current?.focus?.()
  }, [])
  const menu = createPortal(
    <AnimatePresence>{open && <MenuList key={`${open.x},${open.y}`} items={open.items} x={open.x} y={open.y} label={open.label} first={open.first} onDone={close} />}</AnimatePresence>,
    document.body,
  )
  return { show, close, menu, isOpen: !!open }
}

function MenuList({ items, x, y, altX, label, first, onDone, onBack, onEnter }: {
  items: MenuItem[]; x: number; y: number; altX?: number; label: string
  /** Opened from the keyboard: start on the first item. */ first?: boolean
  /** Close every level; `refocus` = give focus back to where it was. */ onDone: (refocus?: boolean) => void
  /** Submenus only: close this level. */ onBack?: () => void
  /** Submenus only: the pointer reached it (keeps it open). */ onEnter?: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  const [pos, setPos] = useState({ left: x, top: y })
  const [sub, setSub] = useState<{ i: number; x: number; y: number; altX: number; first: boolean } | null>(null)
  const enabled = items.map((it, i) => (isAction(it) && !it.disabled ? i : -1)).filter((i) => i >= 0)
  const [active, setActive] = useState(first ? (enabled[0] ?? -1) : -1)
  // Moving diagonally from an item to its submenu crosses other items; close the submenu only if the pointer lingers there.
  const closeTimer = useRef(0)
  useEffect(() => () => clearTimeout(closeTimer.current), [])

  // Keep the whole menu inside the window; a submenu flips to the left of its parent when there's no room.
  useLayoutEffect(() => {
    const r = ref.current!.getBoundingClientRect()
    let left = x
    if (left + r.width > innerWidth - GAP) left = altX !== undefined ? altX - r.width : innerWidth - GAP - r.width
    setPos({ left: Math.max(GAP, left), top: Math.max(GAP, Math.min(y, innerHeight - GAP - r.height)) })
    if (!first) ref.current!.focus()
  }, [x, y, altX, first])

  // Root level: clicks outside, scrolling, resizing or leaving the window close the menu.
  useEffect(() => {
    if (onBack) return
    const down = (e: PointerEvent) => !(e.target as Element).closest?.('.ctx-menu') && onDone(false)
    const away = () => onDone(false)
    addEventListener('pointerdown', down, true)
    addEventListener('resize', away)
    addEventListener('blur', away)
    addEventListener('wheel', away, { passive: true })
    return () => {
      removeEventListener('pointerdown', down, true)
      removeEventListener('resize', away)
      removeEventListener('blur', away)
      removeEventListener('wheel', away)
    }
  }, [onBack, onDone])

  useEffect(() => {
    if (active >= 0) (ref.current?.querySelector(`[data-i="${active}"]`) as HTMLElement | null)?.focus()
  }, [active])

  const openSub = (i: number, kb = false) => {
    const el = ref.current?.querySelector(`[data-i="${i}"]`)
    if (!el) return
    const r = el.getBoundingClientRect()
    setSub({ i, x: r.right - 4, y: r.top - 5, altX: r.left + 4, first: kb })
  }
  const run = (i: number) => {
    const it = items[i]
    if (!isAction(it) || it.disabled) return
    if ('items' in it) return openSub(i, true)
    onDone()
    it.onSelect()
  }
  const move = (d: number) => {
    if (!enabled.length) return
    setSub(null)
    const at = enabled.indexOf(active)
    setActive(enabled[at < 0 ? (d > 0 ? 0 : enabled.length - 1) : (at + d + enabled.length) % enabled.length])
  }

  return (
    <>
      <motion.div
        ref={ref} role="menu" aria-label={label} tabIndex={-1} className="ctx-menu" style={{ left: pos.left, top: pos.top }}
        initial={{ opacity: 0, scale: 0.96, y: -4 }} animate={{ opacity: 1, scale: 1, y: 0 }} exit={{ opacity: 0, scale: 0.98, pointerEvents: 'none', transition: { duration: dur.fast } }}
        transition={{ duration: dur.fast, ease: easeOut }}
        onContextMenu={(e) => e.preventDefault()}
        onPointerEnter={onEnter}
        onKeyDown={(e) => {
          e.stopPropagation()
          const k = e.key
          if (k === 'ArrowDown') (e.preventDefault(), move(1))
          else if (k === 'ArrowUp') (e.preventDefault(), move(-1))
          else if (k === 'Home') (e.preventDefault(), setActive(enabled[0] ?? -1))
          else if (k === 'End') (e.preventDefault(), setActive(enabled.at(-1) ?? -1))
          else if (k === 'Enter' || k === ' ') (e.preventDefault(), active >= 0 && run(active))
          else if (k === 'ArrowRight') (e.preventDefault(), active >= 0 && 'items' in (items[active] as object) && openSub(active, true))
          else if (k === 'ArrowLeft' && onBack) (e.preventDefault(), onBack())
          else if (k === 'Escape') (e.preventDefault(), onBack ? onBack() : onDone())
          else if (k === 'Tab') (e.preventDefault(), onDone())
        }}
      >
        {items.map((it, i) =>
          it === 'sep' ? (
            <div key={`sep-${i}`} role="separator" className="ctx-sep" />
          ) : (
            <button
              key={`${i}-${it.label}`} type="button" role="menuitem" data-i={i} tabIndex={-1} disabled={it.disabled}
              aria-haspopup={'items' in it ? 'menu' : undefined} aria-expanded={'items' in it ? sub?.i === i : undefined}
              className={'danger' in it && it.danger ? 'ctx-item danger' : 'ctx-item'}
              onPointerEnter={() => {
                if (it.disabled) return
                setActive(i)
                clearTimeout(closeTimer.current)
                if ('items' in it) openSub(i)
                else if (sub) closeTimer.current = window.setTimeout(() => setSub(null), 300)
              }}
              onClick={() => run(i)}
            >
              <span className="ellipsis" style={{ flex: 1 }}>{it.label}</span>
              {'hint' in it && it.hint && <span className="ctx-hint">{it.hint}</span>}
              {'items' in it && <span aria-hidden="true" className="ctx-hint">›</span>}
            </button>
          ),
        )}
      </motion.div>
      <AnimatePresence>
        {sub && (() => {
          const it = items[sub.i] as { label: string; items: MenuItem[] }
          return (
            <MenuList
              key={sub.i} items={it.items} x={sub.x} y={sub.y} altX={sub.altX} label={it.label} first={sub.first} onDone={onDone}
              onEnter={() => clearTimeout(closeTimer.current)}
              onBack={() => (setSub(null), setActive(sub.i), (ref.current?.querySelector(`[data-i="${sub.i}"]`) as HTMLElement | null)?.focus())}
            />
          )
        })()}
      </AnimatePresence>
    </>
  )
}
