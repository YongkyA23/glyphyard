// Command palette (Ctrl/⌘+K) and global keyboard shortcuts.
import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { useNavigate } from 'react-router'
import { AnimatePresence } from 'motion/react'
import { api } from './api'
import { Dialog, useDebounced, useLoad } from './ui'
import { isMac, isTyping, NAV, OPEN_PALETTE } from './nav'
import './palette.css'

type Item = { id: string; group: 'Pages' | 'Users' | 'Fonts'; label: string; hint?: string; to: string }
const GROUPS = ['Pages', 'Users', 'Fonts'] as const

export function Palette() {
  const [open, setOpen] = useState(false)
  const [help, setHelp] = useState(false)
  const from = useRef<HTMLElement | null>(null)
  const show = () => { from.current = document.activeElement as HTMLElement | null; setOpen(true) }
  const close = () => { setOpen(false); from.current?.focus?.() }
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        if (open) close()
        else if (!document.querySelector('dialog[open]')) show() // not over another dialog
      } else if (!e.ctrlKey && !e.metaKey && !e.altKey && !isTyping(e.target)) {
        if (e.key === '/') {
          const s = document.querySelector<HTMLInputElement>('.main input[type=search]')
          if (s) { e.preventDefault(); s.focus() }
        } else if (e.key === '?') { e.preventDefault(); setHelp(true) }
      }
    }
    const onOpen = () => { if (!open && !document.querySelector('dialog[open]')) show() }
    window.addEventListener('keydown', onKey)
    window.addEventListener(OPEN_PALETTE, onOpen)
    return () => { window.removeEventListener('keydown', onKey); window.removeEventListener(OPEN_PALETTE, onOpen) }
  })
  const keys: [string, string][] = [[isMac ? '⌘ K' : 'Ctrl K', 'Search everything'], ['/', 'Focus the page search'], ['?', 'Show this help'], ['Esc', 'Close a dialog']]
  return (
    <AnimatePresence>
      {open && <PaletteDialog key="palette" onClose={close} />}
      {help && (
        <Dialog key="help" small labelledBy="help-title" onClose={() => setHelp(false)}>
          <div className="stack-18">
            <h2 id="help-title" className="pal-title">Keyboard shortcuts</h2>
            <dl className="pal-keys">
              {keys.map(([k, d]) => <div key={k}><dt><kbd>{k}</kbd></dt><dd>{d}</dd></div>)}
            </dl>
            <div className="row-8 end"><button type="button" autoFocus className="btn" onClick={() => setHelp(false)}>Close</button></div>
          </div>
        </Dialog>
      )}
    </AnimatePresence>
  )
}

function PaletteDialog({ onClose }: { onClose: () => void }) {
  const navigate = useNavigate()
  const [q, setQ] = useState('')
  const [sel, setSel] = useState(0)
  const dq = useDebounced(q.trim(), 150)
  const found = useLoad(async () => {
    if (!dq) return { users: [], fonts: [] }
    const [u, l] = await Promise.all([api.installs(dq, 'all', 1), api.library(dq, 1, 12)])
    return { users: u.items.slice(0, 5), fonts: l.items }
  }, [dq])
  const needle = q.trim().toLowerCase()
  const items: Item[] = []
  if (needle) {
    for (const [to, label] of NAV) if (label.toLowerCase().includes(needle)) items.push({ id: `pal-p${items.length}`, group: 'Pages', label, to })
    // Results belong to the settled query; hide them while the box has moved on.
    if (dq === q.trim() && found.data) {
      for (const u of found.data.users) items.push({ id: `pal-u${items.length}`, group: 'Users', label: u.name, hint: u.os, to: `/users/${encodeURIComponent(u.id)}` })
      const seen = new Set<string>()
      for (const f of found.data.fonts) {
        if (seen.has(f.family) || seen.size >= 5) continue
        seen.add(f.family)
        items.push({ id: `pal-f${items.length}`, group: 'Fonts', label: f.family, hint: f.style, to: `/library?q=${encodeURIComponent(f.family)}` })
      }
    }
  } else for (const [to, label] of NAV) items.push({ id: `pal-p${items.length}`, group: 'Pages', label, to })
  const at = Math.min(sel, Math.max(items.length - 1, 0))
  const go = (it?: Item) => { if (it) { onClose(); navigate(it.to) } }
  const onKey = (e: KeyboardEvent) => {
    if (e.nativeEvent.isComposing) return
    const n = items.length
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      if (n) setSel((at + (e.key === 'ArrowDown' ? 1 : -1) + n) % n)
    } else if (e.key === 'Enter') { e.preventDefault(); go(items[at]) }
  }
  const activeId = items[at]?.id
  useEffect(() => { document.getElementById(activeId ?? '')?.scrollIntoView({ block: 'nearest' }) }, [activeId])
  return (
    <Dialog onClose={onClose} labelledBy="pal-label">
      <div className="pal">
        <label id="pal-label" className="sr-only" htmlFor="pal-input">Search pages, users and fonts</label>
        <input id="pal-input" className="input pal-input" type="text" autoComplete="off" spellCheck={false} placeholder="Search pages, users and fonts"
          role="combobox" aria-expanded="true" aria-controls="pal-list" aria-autocomplete="list" aria-activedescendant={items[at]?.id}
          value={q} onChange={(e) => { setQ(e.target.value); setSel(0) }} onKeyDown={onKey} />
        <div id="pal-list" role="listbox" aria-label="Results" className="pal-list">
          {GROUPS.map((g) => {
            const rows = items.filter((i) => i.group === g)
            return rows.length > 0 && (
              <div key={g} role="group" aria-labelledby={`pal-g-${g}`}>
                <div id={`pal-g-${g}`} className="pal-group">{g}</div>
                {rows.map((it) => (
                  <div key={it.id} id={it.id} role="option" aria-selected={items[at]?.id === it.id} className="pal-item"
                    onMouseMove={() => setSel(items.indexOf(it))} onMouseDown={(e) => e.preventDefault()} onClick={() => go(it)}>
                    <span>{it.label}</span>{it.hint && <span className="pal-hint">{it.hint}</span>}
                  </div>
                ))}
              </div>
            )
          })}
        </div>
        <p className="pal-note" role="status">
          {found.error ? `Search failed: ${found.error}` : needle && !items.length ? (found.loading || dq !== q.trim() ? 'Searching…' : 'No matches') : ''}
        </p>
      </div>
    </Dialog>
  )
}
