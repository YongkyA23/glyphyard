// Quick-activate popup (its own small window, no sidebar).
import { useEffect, useState, type KeyboardEvent } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { motion } from 'motion/react'
import { api, inTauri, type Face } from '../api'
import { faceStyle } from '../fonts'
import { cssStagger, spring } from '../motion'
import { useApp } from '../state'
import { formatShortcut, Switch } from '../ui'

export function Quick() {
  const { app, activate } = useApp()
  const [q, setQ] = useState('')
  const [rows, setRows] = useState<Face[]>([])
  const [hi, setHi] = useState(0)
  const [shortcut, setShortcut] = useState('CmdOrCtrl+Alt+Space')

  // The window is hidden, not destroyed, between uses: start fresh each time it is shown.
  useEffect(() => {
    const reset = () => {
      api.getSettings().then((s) => setShortcut(s.quickShortcut), () => {})
      setQ('')
      document.getElementById('qa')?.focus()
    }
    reset()
    window.addEventListener('focus', reset)
    return () => window.removeEventListener('focus', reset)
  }, [])
  useEffect(() => {
    let stale = false
    api.quickSearch(q.trim()).then((r) => !stale && (setRows(r), setHi(0)), () => {})
    return () => void (stale = true)
  }, [q])

  async function toggle(face: Face) {
    const res = await activate(() => api.setActive([face.hash], !face.active))
    if (res?.[0]?.ok) setRows((rs) => rs.map((r) => (r.hash === face.hash ? { ...r, active: !face.active } : r)))
  }
  function close() {
    if (inTauri) getCurrentWindow().close().catch(() => {})
    else window.close()
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') return (e.preventDefault(), close())
    if (e.key === 'ArrowDown') return (e.preventDefault(), setHi((h) => Math.max(0, Math.min(rows.length - 1, h + 1))))
    if (e.key === 'ArrowUp') return (e.preventDefault(), setHi((h) => Math.max(0, h - 1)))
    if (e.key === 'Enter' && rows[hi]) return (e.preventDefault(), toggle(rows[hi]))
  }

  return (
    <div className="quick" onKeyDown={onKey}>
      <div style={{ padding: 14, borderBottom: '1px solid var(--line)' }}>
        <label htmlFor="qa" className="sr-only">Search fonts to turn on or off</label>
        <input
          id="qa" type="search" className="input quick-input" value={q} placeholder="Search fonts…" autoFocus autoComplete="off"
          role="combobox" aria-expanded aria-controls="qa-list" aria-activedescendant={rows[hi] ? `qa-${rows[hi].hash}` : undefined}
          onChange={(e) => setQ(e.target.value)}
        />
      </div>
      <div id="qa-list" role="listbox" aria-label="Fonts" style={{ flex: 1, overflowY: 'auto', padding: '6px 8px' }}>
        {rows.map((f, i) => (
          <div key={f.hash} id={`qa-${f.hash}`} role="option" aria-selected={i === hi} className="quick-row enter" style={cssStagger(i)} onMouseEnter={() => setHi(i)}>
            {i === hi && <motion.span layoutId="quick-hi" className="quick-hi" transition={spring.glide} />}
            <span style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 2 }}>
              <span className="ellipsis" style={{ fontSize: 20, ...faceStyle(f) }}>{f.fullName}</span>
              <span className="faint" style={{ fontSize: 12 }}>{f.family} · {f.isVariable ? 'Variable' : f.style}{f.active && f.source === 'admin' ? ' · Set by admin' : ''}</span>
            </span>
            <Switch on={f.active} label={`${f.active ? 'Turn off' : 'Turn on'} ${f.fullName}`} onClick={() => toggle(f)} />
          </div>
        ))}
        {rows.length === 0 && q.trim() && <p className="faint" style={{ margin: '24px 8px', textAlign: 'center' }}>No fonts match “{q}”.</p>}
      </div>
      <div className="quick-foot">
        <span>Enter turns on · Esc closes</span>
        <span>{formatShortcut(shortcut, app.platform)}</span>
      </div>
    </div>
  )
}
