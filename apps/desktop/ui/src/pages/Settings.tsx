import { useEffect, useState, type FormEvent, type ReactNode } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { api, isApiError, on, type AdobeRow, type Folder, type Health, type HistoryEntry, type Settings } from '../api'
import { collapse, dur, easeOut, inView, spring } from '../motion'
import { useApp } from '../state'
import { VersionGames } from '../extras/Games'
import { refreshPlayful } from '../extras/effects'
import { ago, Check, daysUntil, errMsg, fmtDate, fmtWhen, num, plural, Spinner, Switch } from '../ui'
import { ShortcutField } from '../ShortcutField'
import { setTheme } from '../theme'

/** Same text as `REVOKED` in src-tauri/src/sync.rs. */
const REVOKED = 'This computer was removed by your admin. Save the connection again to rejoin.'

const THEMES:[Settings['theme'], string][] = [['system', 'Match system'], ['light', 'Light'], ['dark', 'Dark']]

const APP_TOGGLES: [keyof Settings, string, string][] = [
  ['startAtLogin', 'Start at login', 'Fonts are ready as soon as you sign in to your computer'],
  ['startHidden', 'Start in the tray', 'Open quietly without showing the window'],
  ['autoUpdate', 'Update automatically', 'Install new versions when you restart the app'],
  ['deactivateOnQuit', 'Turn off all fonts when quitting', 'Leaves no fonts active after Typeshelf closes'],
  ['playful', 'Playful extras', 'Easter eggs and hidden games. They never change your fonts'],
]

function Section({ id, title, children, tinted, action }: { id: string; title: string; children: ReactNode; tinted?: boolean; action?: ReactNode }) {
  return (
    <motion.section aria-labelledby={id} className={tinted ? 'settings-card tinted' : 'settings-card'} {...inView}>
      <div className="row-between">
        <h2 id={id} className="h-section">{title}</h2>
        {action}
      </div>
      {children}
    </motion.section>
  )
}

function NameSection() {
  const { app, setApp } = useApp()
  const [name, setName] = useState(app.name)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  useEffect(() => setName(app.name), [app.name])

  const next = app.nextNameChange
  // Without a server the name is only kept here, so the server's once-every-N-days rule doesn't apply.
  const local = !app.serverConfigured
  const locked = !local && !!next && new Date(next).getTime() > Date.now()
  const changedOn = next ? new Date(new Date(next).getTime() - app.nameChangeDays * 86_400_000).toISOString() : null

  async function save(e: FormEvent) {
    e.preventDefault()
    if (!name.trim()) return setError('Enter a name.')
    if (name.trim() === app.name) return
    try {
      const res = await api.changeName(name.trim())
      setApp((a) => ({ ...a, ...res }))
      setError(null)
      setSaved(true)
    } catch (err) {
      if (isApiError(err, 'name_locked')) {
        const nn = typeof err.nextNameChange === 'string' ? err.nextNameChange : null
        setApp((a) => ({ ...a, nextNameChange: nn }))
        setError(nn ? `Your name is locked until ${fmtDate(nn)}.` : err.message)
      } else if (isApiError(err, 'offline')) setError('You need a connection to your server to change your name. Try again when you’re online.')
      else setError(errMsg(err))
    }
  }

  return (
    <Section id="s-profile" title="Your name">
      <form onSubmit={save} style={{ display: 'flex', flexWrap: 'wrap', gap: 10, alignItems: 'flex-end' }}>
        <label className="field-label" style={{ flex: '1 1 260px', fontSize: 13 }}>
          {local ? 'Name' : 'Name shown to your admin'}
          <input type="text" className="input" value={name} disabled={locked} onChange={(e) => (setName(e.target.value), setError(null), setSaved(false))} aria-invalid={!!error} />
        </label>
        <button type="submit" className="btn primary" disabled={locked || !name.trim() || name.trim() === app.name} style={{ height: 42 }}>Save name</button>
      </form>
      {error && <p role="alert" style={{ margin: 0, color: 'var(--bad)', fontSize: 13 }}>{error}</p>}
      {local ? (
        <span className="faint" style={{ fontSize: 12 }}>Your name stays on this computer. If you connect to a server later, Typeshelf joins it with this name.</span>
      ) : locked ? (
        <div className="note neutral">
          <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0, marginTop: 2, color: 'var(--mute)' }}>
            <rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" />
          </svg>
          <span>
            {saved ? 'Name saved. ' : changedOn ? `You changed your name on ${fmtDate(changedOn)}. ` : ''}
            You can change it again on <strong style={{ fontWeight: 600 }}>{fmtDate(next!)}</strong> (in {plural(daysUntil(next!), 'day')}).
          </span>
        </div>
      ) : app.registered ? (
        <div className="note warn">After you save, you won’t be able to change your name again for {app.nameChangeDays} days.</div>
      ) : null}
      {!local && <span className="faint" style={{ fontSize: 12 }}>Your admin can see every name you’ve used.</span>}
    </Section>
  )
}

function FoldersSection() {
  const { toast, bump } = useApp()
  const [folders, setFolders] = useState<Folder[]>([])
  useEffect(() => void api.listFolders().then(setFolders, (e) => toast(errMsg(e))), [toast])
  useEffect(() => on('folder-status', (f) => setFolders((all) => all.map((x) => (x.id === f.id ? f : x)))), [])
  const run = (p: Promise<Folder[]>) => p.then((f) => (setFolders(f), bump()), (e) => toast(errMsg(e)))
  const pick = () => api.pickFolder().catch((e) => (toast(errMsg(e)), null))

  return (
    <Section id="s-folders" title="Watched folders" action={<button type="button" className="btn small" onClick={async () => { const p = await pick(); if (p) run(api.addFolder(p)) }}>Add folder</button>}>
      {folders.map((f) => (
        <div key={f.id} className={f.status === 'offline' ? 'folder-box offline' : 'folder-box'}>
          <span style={{ flex: '1 1 260px', display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
            <span className="mono" style={{ fontSize: 13, overflowWrap: 'anywhere' }}>{f.path}</span>
            {f.status === 'offline' ? (
              <span style={{ fontSize: 12, color: 'var(--warn)' }}>
                Offline · drive not connected{f.offlineSince ? ` since ${new Date(f.offlineSince).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })}` : ''}. Its {num(f.fontCount)} fonts keep their tags and collections.
              </span>
            ) : f.status === 'scanning' ? (
              <span style={{ fontSize: 12, color: 'var(--accent-ink)' }}>Scanning…</span>
            ) : (
              <span style={{ fontSize: 12, color: 'var(--ok)' }}>Watching · {plural(f.fontCount, 'font')}</span>
            )}
          </span>
          {f.status === 'offline' && (
            <button type="button" className="btn small locate" onClick={async () => { const p = await pick(); if (p) run(api.locateFolder(f.id, p)) }}>Locate…</button>
          )}
          <button type="button" className="btn small" aria-label={`Remove ${f.path}`} onClick={() => run(api.removeFolder(f.id))}>Remove</button>
        </div>
      ))}
      <span className="faint" style={{ fontSize: 12, lineHeight: 1.5 }}>
        Folders are tracked even if you move or rename them on the same drive. A missing drive shows as Offline and reconnects by itself.
      </span>
    </Section>
  )
}

function HealthSection() {
  const { toast, bump } = useApp()
  const [h, setH] = useState<Health | null>(null)
  const [repaired, setRepaired] = useState(false)
  const [busy, setBusy] = useState(false)
  const [justRepaired, setJustRepaired] = useState(false)
  useEffect(() => void api.health().then(setH, (e) => toast(errMsg(e))), [toast])
  useEffect(() => {
    if (!justRepaired) return
    const t = setTimeout(() => setJustRepaired(false), 1800)
    return () => clearTimeout(t)
  }, [justRepaired])
  const repair = async () => {
    setBusy(true)
    try {
      setH(await api.repairNow())
      setRepaired(true)
      setJustRepaired(true)
      bump()
    } catch (e) {
      toast(errMsg(e))
    } finally {
      setBusy(false)
    }
  }
  const problems = h?.problems ?? []
  const title = repaired ? 'Repaired just now' : problems.length ? `${plural(problems.length, 'problem')} found` : 'Everything looks good'
  const color = repaired ? 'var(--accent-ink)' : problems.length ? 'var(--bad)' : 'var(--ok)'
  const detail = !h ? '' : repaired
    ? `Removed ${plural(h.removed, 'leftover entry', 'leftover entries')} · re-registered ${plural(h.reregistered, 'font')} · ${num(h.active)} fonts active`
    : `Last check: ${fmtWhen(h.lastCheck)} · ${num(h.active)} fonts active · ${plural(problems.length, 'problem')}`

  return (
    <Section id="s-health" title="Activation health">
      <div className="row-between">
        <span style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          <span style={{ fontWeight: 500, color }}>{title}</span>
          <span className="muted" style={{ fontSize: 13 }}>{detail}</span>
        </span>
        <button type="button" className="btn" onClick={repair} disabled={busy} style={{ minWidth: 132 }}>
          <AnimatePresence mode="popLayout" initial={false}>
            <motion.span
              key={busy ? 'busy' : justRepaired ? 'done' : 'idle'} style={{ display: 'inline-flex', alignItems: 'center', gap: 8, color: justRepaired && !busy ? 'var(--ok)' : undefined }}
              initial={{ opacity: 0, scale: 0.8 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.8 }} transition={spring.pop}
            >
              {busy ? <><Spinner /> Repairing…</> : justRepaired ? <><Check /> Repaired</> : 'Repair now'}
            </motion.span>
          </AnimatePresence>
        </button>
      </div>
      <AnimatePresence initial={false}>
        {problems.length > 0 && !repaired && (
          <motion.ul key="problems" {...collapse} style={{ margin: 0, paddingLeft: 18, fontSize: 13, color: 'var(--ink-2)', lineHeight: 1.6, overflow: 'hidden' }}>
            {problems.map((p) => <li key={p.hash}>{p.message}</li>)}
          </motion.ul>
        )}
      </AnimatePresence>
      <span className="faint" style={{ fontSize: 12, lineHeight: 1.5 }}>
        Typeshelf checks this every time it starts: it removes leftover entries for fonts that no longer exist and re-registers fonts that went missing. It never needs admin rights.
      </span>
    </Section>
  )
}

function BackupSection() {
  const { app, toast, bump } = useApp()
  const [last, setLast] = useState<{ path: string; at: string } | null>(null)
  const sep = app.platform === 'windows' ? '\\' : '/'
  const backup = async () => {
    try {
      const dir = await api.pickFolder()
      if (!dir) return
      const file = `${dir.replace(/[\\/]$/, '')}${sep}typeshelf-backup-${new Date().toISOString().slice(0, 10)}.typeshelf`
      setLast(await api.backupCreate(file))
      toast('Backup saved')
    } catch (e) {
      toast(errMsg(e))
    }
  }
  const restore = async () => {
    try {
      const file = await api.pickFile([{ name: 'Typeshelf backup', extensions: ['typeshelf'] }])
      if (!file) return
      const res = await api.backupRestore(file)
      toast(`Restored backup from ${fmtDate(res.at)}`)
      bump()
    } catch (e) {
      toast(errMsg(e))
    }
  }
  return (
    <Section id="s-backup" title="Backup and moving computers">
      <span className="muted" style={{ fontSize: 13, lineHeight: 1.5 }}>
        A backup holds your collections, tags, project sets, license labels and settings. It doesn’t include the font files themselves.
      </span>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
        <button type="button" className="btn primary" onClick={backup}>Back up now</button>
        <button type="button" className="btn" onClick={restore}>Restore from backup…</button>
      </div>
      <span className="faint" style={{ fontSize: 12 }}>
        {last && `Last backup: ${fmtDate(last.at)} · ${last.path.split(/[\\/]/).pop()}. `}
        Moving to a new computer? Restore there{app.serverConfigured ? ', then ask your admin to merge the two installs' : ''}.
      </span>
    </Section>
  )
}

function AppSection() {
  const { app, toast } = useApp()
  const [s, setS] = useState<Settings | null>(null)
  useEffect(() => void api.getSettings().then(setS, (e) => toast(errMsg(e))), [toast])
  const flip = async (k: keyof Settings) => {
    if (!s) return
    const patch = { [k]: !s[k] } as Partial<Settings>
    setS({ ...s, ...patch })
    api.updateSettings(patch).then(
      (next) => (setS(next), k === 'playful' && refreshPlayful()),
      // Only this switch goes back: another one may have saved meanwhile.
      (e) => (toast(errMsg(e)), setS((cur) => cur && { ...cur, [k]: s[k] })),
    )
  }
  return (
    <motion.section aria-labelledby="s-app" className="settings-card" style={{ gap: 4 }} {...inView}>
      <h2 id="s-app" className="h-section" style={{ marginBottom: 8 }}>App</h2>
      {APP_TOGGLES.filter(([k]) => k !== 'autoUpdate' || (s?.canUpdate && app.serverConfigured)).map(([k, label, hint]) => (
        <div key={k} className="setting-row">
          <span style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            <span>{label}</span><span className="faint" style={{ fontSize: 12 }}>{hint}</span>
            {k === 'autoUpdate' && s?.autoUpdate && s.updateReady && <span style={{ fontSize: 12, color: 'var(--accent-ink)' }}>Version {s.updateReady} will be installed when you quit Typeshelf.</span>}
            {k === 'autoUpdate' && s?.autoUpdate && s.updateProblem && <span role="status" style={{ fontSize: 12, color: 'var(--bad)' }}>{s.updateProblem}</span>}
          </span>
          <Switch on={!!s?.[k]} label={label} onClick={() => flip(k)} disabled={!s} />
        </div>
      ))}
      <div className="setting-row" style={{ flexWrap: 'wrap', padding: '8px 0' }}>
        <span id="theme-label">Appearance</span>
        <div className="chips" role="group" aria-labelledby="theme-label">
          {THEMES.map(([t, label]) => (
            <button
              key={t} type="button" className="chip" aria-pressed={s?.theme === t} disabled={!s}
              onClick={() => {
                if (!s || s.theme === t) return
                setS({ ...s, theme: t })
                setTheme(t)
                api.updateSettings({ theme: t }).then(setS, (e) => (toast(errMsg(e)), setS((cur) => cur && { ...cur, theme: s.theme }), setTheme(s.theme)))
              }}
            >
              {label}
            </button>
          ))}
        </div>
      </div>
      <div className="setting-row" style={{ borderBottom: 0, flexWrap: 'wrap', padding: '8px 0' }}>
        <span style={{ display: 'flex', flexDirection: 'column', gap: 2 }}><span>Quick activate shortcut</span><span className="faint" style={{ fontSize: 12 }}>Opens a small search box from anywhere</span></span>
        <ShortcutField
          value={s?.quickShortcut ?? 'CmdOrCtrl+Alt+Space'} platform={app.platform}
          onSave={async (quickShortcut) => setS(await api.updateSettings({ quickShortcut }))}
        />
      </div>
    </motion.section>
  )
}

/** Presets for the library's sample text picker: edit in place, reorder, remove, add. */
function SampleTextsSection() {
  const { toast } = useApp()
  const [list, setList] = useState<string[] | null>(null)
  const [draft, setDraft] = useState('')
  useEffect(() => void api.getSettings().then((s) => setList(s.sampleTexts ?? []), (e) => toast(errMsg(e))), [toast])
  if (!list) return null
  const save = (next: string[]) => {
    const clean = [...new Set(next.map((t) => t.trim()).filter(Boolean))]
    setList(clean)
    api.updateSettings({ sampleTexts: clean }).then((s) => setList(s.sampleTexts), (e) => (toast(errMsg(e)), setList(list)))
  }
  const move = (i: number, d: number) => {
    const next = [...list]
    ;[next[i], next[i + d]] = [next[i + d], next[i]]
    save(next)
  }
  return (
    <Section id="s-samples" title="Sample texts">
      <span className="muted" style={{ fontSize: 13 }}>Presets for the sample text in the library. Press Enter there to pick one.</span>
      <ul className="preset-list">
        <AnimatePresence initial={false}>
          {list.map((t, i) => (
            <motion.li key={t} layout initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: 'auto' }} exit={{ opacity: 0, height: 0 }} transition={{ ...spring.glide, opacity: { duration: dur.fast } }}>
              <input
                className="input" defaultValue={t} aria-label={`Sample text ${i + 1}`}
                onBlur={(e) => {
                  const v = e.target.value.trim()
                  if (v && v !== t) save(list.map((x, j) => (j === i ? v : x)))
                  else e.target.value = t
                }}
                onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
              />
              <button type="button" className="btn small icon-btn" aria-label={`Move “${t}” up`} disabled={i === 0} onClick={() => move(i, -1)}>↑</button>
              <button type="button" className="btn small icon-btn" aria-label={`Move “${t}” down`} disabled={i === list.length - 1} onClick={() => move(i, 1)}>↓</button>
              <button type="button" className="btn small icon-btn" aria-label={`Remove “${t}”`} onClick={() => save(list.filter((_, j) => j !== i))}>✕</button>
            </motion.li>
          ))}
        </AnimatePresence>
      </ul>
      <form style={{ display: 'flex', gap: 8 }} onSubmit={(e) => (e.preventDefault(), draft.trim() && (save([...list, draft]), setDraft('')))}>
        <label htmlFor="new-sample" className="sr-only">New sample text</label>
        <input id="new-sample" className="input" style={{ flex: 1 }} placeholder="New sample text" value={draft} onChange={(e) => setDraft(e.target.value)} />
        <button type="submit" className="btn" disabled={!draft.trim()}>Add</button>
      </form>
    </Section>
  )
}

function AdobeSection() {
  const { toast } = useApp()
  const [rows, setRows] = useState<AdobeRow[]>([])
  const [busy, setBusy] = useState<string | null>(null)
  useEffect(() => void api.adobeStatus().then(setRows, (e) => toast(errMsg(e))), [toast])
  const act = async (r: AdobeRow) => {
    setBusy(r.app)
    try {
      const next = await (r.installed ? api.adobeRemove(r.app) : api.adobeInstall(r.app))
      setRows((rs) => rs.map((x) => (x.app === next.app ? next : x)))
    } catch (e) {
      toast(errMsg(e))
    } finally {
      setBusy(null)
    }
  }
  return (
    <Section id="s-adobe" title="Adobe auto-activation">
      <span className="muted" style={{ fontSize: 13 }}>When you open a document, missing fonts are turned on automatically.</span>
      <div>
        {rows.map((r) => {
          const found = r.versions.length > 0
          const [status, color] = r.installed
            ? [r.paired ? 'Installed · connected' : 'Installed · waiting to connect', 'var(--ok)']
            : [found ? 'Not installed' : 'App not found', 'var(--faint)']
          return (
            <div key={r.app} className="setting-row" style={{ minHeight: 44 }}>
              <span>{r.label}{found && ` ${r.versions.join(', ')}`}</span>
              <span style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <span style={{ fontSize: 12, color, transition: `color ${dur.base}s` }}>{status}</span>
                <button type="button" className="btn small" style={{ fontSize: 13 }} disabled={busy === r.app || (!found && !r.installed)} onClick={() => act(r)}>
                  {r.installed ? 'Remove' : 'Install'}
                </button>
              </span>
            </div>
          )
        })}
      </div>
    </Section>
  )
}

function HistorySection() {
  const { toast, bump, libVersion } = useApp()
  const [rows, setRows] = useState<HistoryEntry[]>([])
  useEffect(() => void api.listHistory().then(setRows, (e) => toast(errMsg(e))), [toast, libVersion])
  const undo = (id: number) => api.undoHistory(id).then((h) => (setRows(h), bump(), toast('Undone')), (e) => toast(errMsg(e)))
  return (
    <Section id="s-history" title="History">
      {rows.length === 0 && <span className="faint" style={{ fontSize: 13 }}>Nothing to undo yet.</span>}
      <div>
        {rows.map((h) => (
          <div key={h.id} className="setting-row" style={{ minHeight: 44, flexWrap: 'wrap' }}>
            <span style={{ display: 'flex', flexDirection: 'column', gap: 2 }}><span>{h.summary}</span><span className="faint" style={{ fontSize: 12 }}>{fmtWhen(h.at)}</span></span>
            <button type="button" className="btn small" style={{ fontSize: 13 }} disabled={h.undone} onClick={() => undo(h.id)}>{h.undone ? 'Undone' : 'Undo'}</button>
          </div>
        ))}
      </div>
    </Section>
  )
}

function ConnectionSection() {
  const { app, setApp } = useApp()
  const c = app.connection
  const [server, setServer] = useState(c.serverUrl)
  const [key, setKey] = useState(c.installKey)
  const [showKey, setShowKey] = useState(false)
  const [result, setResult] = useState<{ color: string; label: string; detail: string } | null>(null)
  const [busy, setBusy] = useState(false)
  // The admin removed this computer: saving the same address and key joins again (the shell's REVOKED text).
  const removed = c.status === 'error' && c.error === REVOKED
  const dirty = server !== c.serverUrl || key !== c.installKey
  const locked = !c.editable
  const local = !app.serverConfigured

  const status = result ?? (
    local
      ? { color: 'var(--mute)', label: 'Local library', detail: 'no server is set up. Add your team’s server address and install key to connect.' }
      : c.status === 'connected'
      ? { color: 'var(--ok)', label: 'Connected', detail: `last sync ${ago(c.lastSync)}` }
      : c.status === 'offline'
        ? { color: 'var(--warn)', label: 'Offline', detail: 'changes will sync when the connection is back' }
        : { color: 'var(--bad)', label: 'Can’t connect', detail: c.error ?? 'check the server address and install key' }
  )
  const run = async (fn: () => Promise<void>) => {
    setBusy(true)
    try {
      await fn()
    } catch (e) {
      setResult({ color: 'var(--bad)', label: 'Can’t connect', detail: errMsg(e) })
    } finally {
      setBusy(false)
    }
  }
  const apply = (next: typeof c) => {
    setApp((a) => ({ ...a, connection: next, serverConfigured: !!next.serverUrl.trim() }))
    setServer(next.serverUrl)
    setKey(next.installKey)
  }
  const test = () => run(async () => {
    const r = await api.testConnection(server.trim(), key.trim())
    setResult(r.ok ? { color: 'var(--ok)', label: 'Test passed', detail: 'the server accepted this address and key' } : { color: 'var(--bad)', label: 'Can’t connect', detail: r.error ?? 'check the server address and install key' })
  })
  const save = () => run(async () => {
    const next = await api.saveConnection(server.trim(), key.trim())
    apply(next)
    setResult(next.status === 'connected' ? { color: 'var(--accent-ink)', label: 'Reconnected', detail: 'using the new server and key' } : null)
  })
  const reset = () => run(async () => {
    apply(await api.resetConnection())
    setResult(null)
  })

  return (
    <Section id="s-server" title="Connection" tinted>
      <span style={{ fontSize: 13 }} aria-live="polite">
        <AnimatePresence mode="popLayout" initial={false}>
          <motion.span key={status.label} style={{ color: status.color, display: 'inline-block' }} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: dur.base, ease: easeOut }}>
            <motion.span style={{ display: 'inline-block' }} initial={{ scale: 0.3 }} animate={{ scale: 1 }} transition={spring.pop}>●</motion.span> {status.label}
          </motion.span>
        </AnimatePresence>{' '}
        · {status.detail}
      </span>
      {locked && <div className="note neutral">Your admin has locked these settings. Ask them if you need to connect to a different server.</div>}
      <label className="field-label" style={{ fontSize: 13 }}>
        Server address
        <input type="url" className="input mono" value={server} disabled={locked} placeholder="https://fonts.example.com" onChange={(e) => (setServer(e.target.value), setResult(null))} />
      </label>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10, alignItems: 'flex-end' }}>
        <label className="field-label" style={{ flex: '1 1 260px', fontSize: 13 }}>
          Install key
          <input type={showKey ? 'text' : 'password'} className="input mono" value={key} disabled={locked} autoComplete="off" spellCheck={false} onChange={(e) => (setKey(e.target.value), setResult(null))} />
        </label>
        <button type="button" className="btn" style={{ height: 42 }} aria-pressed={showKey} onClick={() => setShowKey(!showKey)}>{showKey ? 'Hide' : 'Show'}</button>
      </div>
      <AnimatePresence initial={false}>
        {dirty && !local && (
          <motion.div key="dirty" {...collapse} style={{ overflow: 'hidden' }}>
            <div className="note warn">
              If you connect to a different server, you start there as a new install. Your fonts, collections and tags stay on this computer, but your name history stays on the old server.
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
        <button type="button" className="btn" onClick={test} disabled={locked || busy}>Test connection</button>
        <button type="button" className="btn primary" onClick={save} disabled={locked || busy || (!dirty && !removed)}>{local ? 'Connect' : 'Save and reconnect'}</button>
        <button type="button" className="btn link" onClick={reset} disabled={locked || busy}>Use installer defaults</button>
      </div>
      <span className="faint" style={{ fontSize: 12 }}>
        Install ID <span className="mono">{c.installId}</span> · <VersionGames version={app.appVersion} /> · {app.platform === 'macos' ? 'macOS' : 'Windows'}.{local ? '' : ' Your admin can lock these two fields.'}
      </span>
    </Section>
  )
}

export function SettingsPage() {
  return (
    <div className="page" style={{ padding: '24px 32px 48px' }}>
      <div style={{ maxWidth: 760, display: 'flex', flexDirection: 'column', gap: 20 }}>
        <h1 className="h-page">Settings</h1>
        <NameSection />
        <FoldersSection />
        <HealthSection />
        <BackupSection />
        <AppSection />
        <SampleTextsSection />
        <AdobeSection />
        <HistorySection />
        <ConnectionSection />
      </div>
    </div>
  )
}
