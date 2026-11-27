import { useEffect, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { api, isApiError, on, type AdobeRow, type Folder, type Health, type HistoryEntry, type Settings } from '../api'
import { collapse, dur, easeOut, inView, spring } from '../motion'
import { useApp } from '../state'
import { VersionGames } from '../extras/Games'
import { refreshPlayful } from '../extras/effects'
import { ago, Check, daysUntil, Dialog, errMsg, fmtDate, fmtWhen, plural, Spinner, Switch } from '../ui'
import { ShortcutField } from '../ShortcutField'
import { setTheme } from '../theme'
import { applyLanguage, getLang, locale, resolveLang, t, tk } from '../i18n'
import { Hint } from '../Hint'
import { GUIDES } from '../tour/guides'
import { useDone } from '../tour/progress'
import { useTour } from '../tour/Tour'

/** Same text as `REVOKED` in src-tauri/src/sync.rs. */
const REVOKED = tk('This computer was removed by your admin. Save the connection again to rejoin.')

const THEMES:[Settings['theme'], string][] = [['system', tk('Match system')], ['light', tk('Light')], ['dark', tk('Dark')]]

const APP_TOGGLES: [keyof Settings, string, string][] = [
  ['startAtLogin', tk('Start at login'), tk('Fonts are ready as soon as you sign in to your computer')],
  ['startHidden', tk('Start in the tray'), tk('Open quietly without showing the window')],
  ['autoUpdate', tk('Update automatically'), tk('Install new versions when you restart the app')],
  ['deactivateOnQuit', tk('Turn off all fonts when quitting'), tk('Leaves no fonts active after Glyphyard closes')],
  ['playful', tk('Playful extras'), tk('Easter eggs and hidden games. They never change your fonts')],
]

const titleRow = { display: 'flex', alignItems: 'center', gap: 6 } as const

function Section({ id, title, hint, tour, children, tinted, action }: { id: string; title: string; hint?: string; tour?: string; children: ReactNode; tinted?: boolean; action?: ReactNode }) {
  return (
    <motion.section aria-labelledby={id} className={tinted ? 'settings-card tinted' : 'settings-card'} data-tour={tour} {...inView}>
      <div className="row-between">
        <div style={titleRow}>
          <h2 id={id} className="h-section">{title}</h2>
          {hint && <Hint text={hint} />}
        </div>
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
    if (!name.trim()) return setError(t('Enter a name.'))
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
        setError(nn ? t('Your name is locked until {date}.', { date: fmtDate(nn) }) : errMsg(err))
      } else if (isApiError(err, 'offline')) setError(t('You need a connection to your server to change your name. Try again when you’re online.'))
      else setError(errMsg(err))
    }
  }

  // The date sits in <strong>: split the translated sentence around its placeholder (t() leaves unknown ones as is).
  const [againA, againB] = next ? t('You can change it again on {date} (in {days}).', { days: plural(daysUntil(next), 'day') }).split('{date}') : []

  return (
    <Section
      id="s-profile" title={t('Your name')}
      hint={local ? t('Your name stays on this computer. If you connect to a server later, Glyphyard joins it with this name.') : t('Your admin can see every name you’ve used.')}
    >
      <form onSubmit={save} style={{ display: 'flex', flexWrap: 'wrap', gap: 10, alignItems: 'flex-end' }}>
        <label className="field-label" style={{ flex: '1 1 260px', fontSize: 13 }}>
          {local ? t('Name') : t('Name shown to your admin')}
          <input type="text" className="input" value={name} disabled={locked} onChange={(e) => (setName(e.target.value), setError(null), setSaved(false))} aria-invalid={!!error} />
        </label>
        <button type="submit" className="btn primary" disabled={locked || !name.trim() || name.trim() === app.name} style={{ height: 42 }}>{t('Save name')}</button>
      </form>
      {error && <p role="alert" style={{ margin: 0, color: 'var(--bad)', fontSize: 13 }}>{error}</p>}
      {locked ? (
        <div className="note neutral">
          <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0, marginTop: 2, color: 'var(--mute)' }}>
            <rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" />
          </svg>
          <span>
            {saved ? `${t('Name saved.')} ` : changedOn ? `${t('You changed your name on {date}.', { date: fmtDate(changedOn) })} ` : ''}
            {againA}<strong style={{ fontWeight: 600 }}>{fmtDate(next!)}</strong>{againB}
          </span>
        </div>
      ) : !local && app.registered ? (
        <div className="note warn">{t('After you save, you won’t be able to change your name again for {n} days.', { n: app.nameChangeDays })}</div>
      ) : null}
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
    <Section
      id="s-folders" title={t('Watched folders')}
      hint={t('Folders are tracked even if you move or rename them on the same drive. A missing drive shows as Offline and reconnects by itself.')}
      action={<button type="button" className="btn small" onClick={async () => { const p = await pick(); if (p) run(api.addFolder(p)) }}>{t('Add folder')}</button>}
    >
      {folders.map((f) => (
        <div key={f.id} className={f.status === 'offline' ? 'folder-box offline' : 'folder-box'}>
          <span style={{ flex: '1 1 260px', display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
            <span className="mono" style={{ fontSize: 13, overflowWrap: 'anywhere' }}>{f.path}</span>
            {f.status === 'offline' ? (
              <span style={{ fontSize: 12, color: 'var(--warn)' }}>
                {f.offlineSince
                  ? t('Offline · drive not connected since {date}. Its {n} fonts keep their tags and collections.', { n: f.fontCount, date: new Date(f.offlineSince).toLocaleDateString(locale(), { day: 'numeric', month: 'short' }) })
                  : t('Offline · drive not connected. Its {n} fonts keep their tags and collections.', { n: f.fontCount })}
              </span>
            ) : f.status === 'scanning' ? (
              <span style={{ fontSize: 12, color: 'var(--accent-ink)' }}>{t('Scanning…')}</span>
            ) : (
              <span style={{ fontSize: 12, color: 'var(--ok)' }}>{t('Watching · {fonts}', { fonts: plural(f.fontCount, 'font') })}</span>
            )}
          </span>
          {f.status === 'offline' && (
            <button type="button" className="btn small locate" onClick={async () => { const p = await pick(); if (p) run(api.locateFolder(f.id, p)) }}>{t('Locate…')}</button>
          )}
          <button type="button" className="btn small" aria-label={t('Remove {path}', { path: f.path })} onClick={() => run(api.removeFolder(f.id))}>{t('Remove')}</button>
        </div>
      ))}
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
    const timer = setTimeout(() => setJustRepaired(false), 1800)
    return () => clearTimeout(timer)
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
  const title = repaired ? t('Repaired just now') : problems.length ? t('{problems} found', { problems: plural(problems.length, 'problem') }) : t('Everything looks good')
  const color = repaired ? 'var(--accent-ink)' : problems.length ? 'var(--bad)' : 'var(--ok)'
  const detail = !h ? '' : repaired
    ? t('Removed {entries} · re-registered {fonts} · {n} fonts active', { entries: plural(h.removed, 'leftover entry', 'leftover entries'), fonts: plural(h.reregistered, 'font'), n: h.active })
    : t('Last check: {when} · {n} fonts active · {problems}', { when: fmtWhen(h.lastCheck), n: h.active, problems: plural(problems.length, 'problem') })

  return (
    <Section
      id="s-health" title={t('Activation health')}
      hint={t('Glyphyard checks this every time it starts: it removes leftover entries for fonts that no longer exist and re-registers fonts that went missing. It never needs admin rights.')}
    >
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
              {busy ? <><Spinner /> {t('Repairing…')}</> : justRepaired ? <><Check /> {t('Repaired')}</> : t('Repair now')}
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
      toast(t('Backup saved'))
    } catch (e) {
      toast(errMsg(e))
    }
  }
  const restore = async () => {
    try {
      const file = await api.pickFile([{ name: t('Glyphyard backup'), extensions: ['typeshelf'] }])
      if (!file) return
      const res = await api.backupRestore(file)
      toast(t('Restored backup from {date}', { date: fmtDate(res.at) }))
      bump()
    } catch (e) {
      toast(errMsg(e))
    }
  }
  const moving = app.serverConfigured
    ? t('Moving to a new computer? Restore there, then ask your admin to merge the two installs.')
    : t('Moving to a new computer? Restore there.')
  return (
    <Section
      id="s-backup" title={t('Backup and moving computers')}
      hint={`${t('A backup holds your collections, tags, project sets, license labels and settings. It doesn’t include the font files themselves.')} ${moving}`}
    >
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
        <button type="button" className="btn primary" onClick={backup}>{t('Back up now')}</button>
        <button type="button" className="btn" onClick={restore}>{t('Restore from backup…')}</button>
      </div>
      {last && <span className="faint" style={{ fontSize: 12 }}>{t('Last backup: {date} · {file}.', { date: fmtDate(last.at), file: last.path.split(/[\\/]/).pop() ?? '' })}</span>}
    </Section>
  )
}

// The language switch remounts the page (main.tsx ByLang); the new AppSection puts focus back on the select.
let refocusLanguage = false

function AppSection() {
  const { app, toast } = useApp()
  const [s, setS] = useState<Settings | null>(null)
  useEffect(() => void api.getSettings().then(setS, (e) => toast(errMsg(e))), [toast])
  useEffect(() => {
    if (!s || !refocusLanguage) return
    refocusLanguage = false
    document.getElementById('s-language')?.focus()
  }, [s])
  const [checkMsg, setCheckMsg] = useState<{ text: string; bad?: boolean } | null>(null)
  const [checking, setChecking] = useState(false)
  const checkNow = () => {
    setChecking(true)
    setCheckMsg({ text: t('Checking…') })
    api.checkUpdate().then(
      (r) => r.status === 'upToDate'
        ? setCheckMsg({ text: t('Glyphyard is up to date ({v})', { v: r.version }) })
        : (setCheckMsg(null), api.getSettings().then(setS, (e) => toast(errMsg(e)))),
      (e) => setCheckMsg({ text: errMsg(e), bad: true }),
    ).finally(() => setChecking(false))
  }
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
  const setLanguage = (language: Settings['language']) => {
    if (!s || s.language === language) return
    setS({ ...s, language })
    api.updateSettings({ language }).then(
      (next) => {
        setS(next)
        refocusLanguage = resolveLang(next.language) !== getLang()
        applyLanguage(next.language)
      },
      (e) => (toast(errMsg(e)), setS((cur) => cur && { ...cur, language: s.language })),
    )
  }
  return (
    <motion.section aria-labelledby="s-app" className="settings-card" style={{ gap: 4 }} {...inView}>
      <h2 id="s-app" className="h-section" style={{ marginBottom: 8 }}>{t('App')}</h2>
      {APP_TOGGLES.filter(([k]) => k !== 'autoUpdate' || (s?.canUpdate && app.serverConfigured)).map(([k, label, hint]) => (
        <div key={k} className="setting-row">
          <span style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            <span style={titleRow}>{t(label) /* i18n-dynamic: APP_TOGGLES */}<Hint text={t(hint) /* i18n-dynamic */} /></span>
            {k === 'autoUpdate' && s?.updateReady && (
              <span style={{ fontSize: 12, color: 'var(--accent-ink)', display: 'flex', alignItems: 'center', gap: 8 }}>
                {s.autoUpdate ? t('Version {v} will be installed when you quit Glyphyard.', { v: s.updateReady }) : t('Version {v} is ready', { v: s.updateReady })}
                <button type="button" className="btn small" onClick={() => api.restartToUpdate().catch((e) => toast(errMsg(e)))}>{t('Restart now')}</button>
              </span>
            )}
            {k === 'autoUpdate' && s?.autoUpdate && s.updateProblem && <span role="status" style={{ fontSize: 12, color: 'var(--bad)' }}>{t(s.updateProblem /* i18n-dynamic */)}</span>}
            {k === 'autoUpdate' && (
              <span style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12 }}>
                <button type="button" className="btn small" disabled={checking} onClick={checkNow}>{t('Check for updates')}</button>
                {checkMsg && <span role="status" style={{ color: checkMsg.bad ? 'var(--bad)' : 'var(--mute)' }}>{checkMsg.text}</span>}
              </span>
            )}
          </span>
          <Switch on={!!s?.[k]} label={t(label) /* i18n-dynamic */} onClick={() => flip(k)} disabled={!s} />
        </div>
      ))}
      <div className="setting-row" style={{ flexWrap: 'wrap', padding: '8px 0' }}>
        <label htmlFor="s-language">{t('Language')}</label>
        <select
          id="s-language" className="select" data-tour="settings-language" value={s?.language ?? 'system'} disabled={!s}
          onChange={(e) => setLanguage(e.target.value as Settings['language'])}
        >
          <option value="system">{t('Match system')}</option>
          <option value="en">{t('English')}</option>
          <option value="id">{t('Bahasa Indonesia')}</option>
        </select>
      </div>
      <div className="setting-row" style={{ flexWrap: 'wrap', padding: '8px 0' }}>
        <span id="theme-label">{t('Appearance')}</span>
        <div className="chips" role="group" aria-labelledby="theme-label">
          {THEMES.map(([theme, label]) => (
            <button
              key={theme} type="button" className="chip" aria-pressed={s?.theme === theme} disabled={!s}
              onClick={() => {
                if (!s || s.theme === theme) return
                setS({ ...s, theme })
                setTheme(theme)
                api.updateSettings({ theme }).then(setS, (e) => (toast(errMsg(e)), setS((cur) => cur && { ...cur, theme: s.theme }), setTheme(s.theme)))
              }}
            >
              {t(label) /* i18n-dynamic: THEMES */}
            </button>
          ))}
        </div>
      </div>
      <div className="setting-row" data-tour="settings-shortcut" style={{ borderBottom: 0, flexWrap: 'wrap', padding: '8px 0' }}>
        <span style={titleRow}>{t('Quick activate shortcut')}<Hint text={t('Opens a small search box from anywhere')} /></span>
        <ShortcutField
          value={s?.quickShortcut ?? 'CmdOrCtrl+Alt+Space'} platform={app.platform}
          onSave={async (quickShortcut) => setS(await api.updateSettings({ quickShortcut }))}
        />
      </div>
    </motion.section>
  )
}

/** Every guided tour: read its steps here, or start it. */
function GuidesSection() {
  const { start } = useTour()
  const done = useDone()
  return (
    <Section id="s-guides" title={t('Guides')} tour="settings-guides">
      <div>
        {GUIDES.map((g) => (
          <div key={g.id} className="setting-row" style={{ alignItems: 'flex-start', padding: '10px 0' }}>
            <span style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
              <span style={{ ...titleRow, fontWeight: 500 }}>
                {t(g.title) /* i18n-dynamic: guides.ts */}
                {done.has(g.id) && <span style={{ ...titleRow, gap: 4, color: 'var(--ok)', fontSize: 12, fontWeight: 400 }}><Check /> {t('Done')}</span>}
              </span>
              <span className="muted" style={{ fontSize: 13 }}>{t(g.summary) /* i18n-dynamic */}</span>
              <details>
                <summary className="faint" style={{ fontSize: 12, cursor: 'pointer', width: 'fit-content' }}>{t('Show steps')}</summary>
                <ol style={{ margin: '6px 0 0', paddingLeft: 18, fontSize: 13, lineHeight: 1.55, color: 'var(--ink-2)' }}>
                  {g.steps.map((step, i) => (
                    <li key={i}>
                      <strong style={{ fontWeight: 600 }}>{t(step.title) /* i18n-dynamic */}</strong>{' '}
                      {t(step.body) /* i18n-dynamic */}
                    </li>
                  ))}
                </ol>
              </details>
            </span>
            <button
              type="button" className="btn small" onClick={() => start(g.id)}
              aria-label={t('Start {guide}', { guide: t(g.title) /* i18n-dynamic */ })}
            >
              {t('Start')}
            </button>
          </div>
        ))}
      </div>
    </Section>
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
    const clean = [...new Set(next.map((x) => x.trim()).filter(Boolean))]
    setList(clean)
    api.updateSettings({ sampleTexts: clean }).then((s) => setList(s.sampleTexts), (e) => (toast(errMsg(e)), setList(list)))
  }
  const move = (i: number, d: number) => {
    const next = [...list]
    ;[next[i], next[i + d]] = [next[i + d], next[i]]
    save(next)
  }
  return (
    <Section id="s-samples" title={t('Sample texts')} hint={t('Presets for the sample text in the library. Press Enter there to pick one.')}>
      <ul className="preset-list">
        <AnimatePresence initial={false}>
          {list.map((text, i) => (
            <motion.li key={text} layout initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: 'auto' }} exit={{ opacity: 0, height: 0 }} transition={{ ...spring.glide, opacity: { duration: dur.fast } }}>
              <input
                className="input" defaultValue={text} aria-label={t('Sample text {n}', { n: i + 1 })}
                onBlur={(e) => {
                  const v = e.target.value.trim()
                  if (v && v !== text) save(list.map((x, j) => (j === i ? v : x)))
                  else e.target.value = text
                }}
                onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
              />
              <button type="button" className="btn small icon-btn" aria-label={t('Move “{text}” up', { text })} disabled={i === 0} onClick={() => move(i, -1)}>↑</button>
              <button type="button" className="btn small icon-btn" aria-label={t('Move “{text}” down', { text })} disabled={i === list.length - 1} onClick={() => move(i, 1)}>↓</button>
              <button type="button" className="btn small icon-btn" aria-label={t('Remove “{text}”', { text })} onClick={() => save(list.filter((_, j) => j !== i))}>✕</button>
            </motion.li>
          ))}
        </AnimatePresence>
      </ul>
      <form style={{ display: 'flex', gap: 8 }} onSubmit={(e) => (e.preventDefault(), draft.trim() && (save([...list, draft]), setDraft('')))}>
        <label htmlFor="new-sample" className="sr-only">{t('New sample text')}</label>
        <input id="new-sample" className="input" style={{ flex: 1 }} placeholder={t('New sample text')} value={draft} onChange={(e) => setDraft(e.target.value)} />
        <button type="submit" className="btn" disabled={!draft.trim()}>{t('Add')}</button>
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
    <Section id="s-adobe" title={t('Adobe auto-activation')} hint={t('When you open a document, missing fonts are turned on automatically.')}>
      <div>
        {rows.map((r) => {
          const found = r.versions.length > 0
          const [status, color] = r.installed
            ? [r.paired ? t('Installed · connected') : t('Installed · waiting to connect'), 'var(--ok)']
            : [found ? t('Not installed') : t('App not found'), 'var(--faint)']
          return (
            <div key={r.app} className="setting-row" style={{ minHeight: 44 }}>
              <span>{r.label}{found && ` ${r.versions.join(', ')}`}</span>
              <span style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <span style={{ fontSize: 12, color, transition: `color ${dur.base}s` }}>{status}</span>
                <button type="button" className="btn small" style={{ fontSize: 13 }} disabled={busy === r.app || (!found && !r.installed)} onClick={() => act(r)}>
                  {r.installed ? t('Remove') : t('Install')}
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
  const undo = (id: number) => api.undoHistory(id).then((h) => (setRows(h), bump(), toast(t('Undone'))), (e) => toast(errMsg(e)))
  return (
    <Section id="s-history" title={t('History')}>
      {rows.length === 0 && <span className="faint" style={{ fontSize: 13 }}>{t('Nothing to undo yet.')}</span>}
      <div>
        {rows.map((h) => (
          <div key={h.id} className="setting-row" style={{ minHeight: 44, flexWrap: 'wrap' }}>
            <span style={{ display: 'flex', flexDirection: 'column', gap: 2 }}><span>{h.summary}</span><span className="faint" style={{ fontSize: 12 }}>{fmtWhen(h.at)}</span></span>
            <button type="button" className="btn small" style={{ fontSize: 13 }} disabled={h.undone} onClick={() => undo(h.id)}>{h.undone ? t('Undone') : t('Undo')}</button>
          </div>
        ))}
      </div>
    </Section>
  )
}

/** Asks for the admin unlock password; hands it up only once the backend accepted it. */
function UnlockForm({ onUnlocked, onCancel }: { onUnlocked: (password: string) => void; onCancel: () => void }) {
  const [pw, setPw] = useState('')
  const [show, setShow] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const input = useRef<HTMLInputElement>(null)
  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!pw || busy) return
    setBusy(true)
    setError(null)
    try {
      await api.unlockConnection(pw)
      onUnlocked(pw)
    } catch (err) {
      setError(errMsg(err))
      input.current?.focus()
      input.current?.select()
    } finally {
      setBusy(false)
    }
  }
  return (
    <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div style={{ display: 'flex', gap: 8, alignItems: 'flex-end' }}>
        <label className="field-label" style={{ flex: 1, fontSize: 13 }}>
          {t('Admin password')}
          <input
            ref={input} type={show ? 'text' : 'password'} className="input" value={pw} autoComplete="off" spellCheck={false}
            aria-invalid={!!error} aria-describedby={error ? 'unlock-error' : undefined}
            onChange={(e) => (setPw(e.target.value), setError(null))}
          />
        </label>
        <button type="button" className="btn" style={{ height: 42 }} aria-label={t('Show password')} aria-pressed={show} onClick={() => setShow(!show)}>
          {show ? t('Hide') : t('Show')}
        </button>
      </div>
      {error && <p id="unlock-error" role="alert" style={{ color: 'var(--bad)', fontSize: 13, margin: 0 }}>{error}</p>}
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={onCancel}>{t('Cancel')}</button>
        <button type="submit" className="btn primary" disabled={!pw || busy}>{busy ? t('Unlocking…') : t('Unlock')}</button>
      </div>
    </form>
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
  // The accepted unlock password, in this component's state only: gone after a save or reset, Lock again, or leaving the page.
  const [password, setPassword] = useState<string | null>(null)
  const [unlocking, setUnlocking] = useState(false)
  // The admin removed this computer: saving the same address and key joins again (the shell's REVOKED text).
  const removed = c.status === 'error' && c.error === REVOKED
  const dirty = server !== c.serverUrl || key !== c.installKey
  const locked = !c.editable && password === null
  const local = !app.serverConfigured

  const status = result ?? (
    local
      ? { color: 'var(--mute)', label: t('Local library'), detail: t('no server is set up. Add your team’s server address and install key to connect.') }
      : c.status === 'connected'
      ? { color: 'var(--ok)', label: t('Connected'), detail: t('last sync {when}', { when: ago(c.lastSync) }) }
      : c.status === 'offline'
        ? { color: 'var(--warn)', label: t('Offline'), detail: t('changes will sync when the connection is back') }
        : { color: 'var(--bad)', label: t('Can’t connect'), detail: c.error ? t(c.error) /* i18n-dynamic: engine text */ : t('check the server address and install key') }
  )
  const run = async (fn: () => Promise<void>) => {
    setBusy(true)
    try {
      await fn()
    } catch (e) {
      // The held password is no good any more: the fields lock again.
      const passwordProblem = ['wrong_password', 'unlock_unavailable', 'connection_locked'].some((code) => isApiError(e, code))
      if (passwordProblem) setPassword(null)
      setResult({ color: 'var(--bad)', label: passwordProblem ? t('Can’t save') : t('Can’t connect'), detail: errMsg(e) })
    } finally {
      setBusy(false)
    }
  }
  const apply = (next: typeof c) => {
    setApp((a) => ({ ...a, connection: next, serverConfigured: !!next.serverUrl.trim() }))
    setServer(next.serverUrl)
    setKey(next.installKey)
  }
  const lockAgain = () => {
    setPassword(null)
    setServer(c.serverUrl)
    setKey(c.installKey)
    setResult(null)
  }
  const test = () => run(async () => {
    const r = await api.testConnection(server.trim(), key.trim())
    setResult(r.ok ? { color: 'var(--ok)', label: t('Test passed'), detail: t('the server accepted this address and key') } : { color: 'var(--bad)', label: t('Can’t connect'), detail: r.error ? t(r.error) /* i18n-dynamic: engine text */ : t('check the server address and install key') })
  })
  const save = () => run(async () => {
    const next = await api.saveConnection(server.trim(), key.trim(), password ?? undefined)
    setPassword(null)
    apply(next)
    setResult(next.status === 'connected' ? { color: 'var(--accent-ink)', label: t('Reconnected'), detail: t('using the new server and key') } : null)
  })
  const reset = () => run(async () => {
    const next = await api.resetConnection(password ?? undefined)
    setPassword(null)
    apply(next)
    setResult(null)
  })

  return (
    <Section id="s-server" title={t('Connection')} tour="settings-connection" tinted>
      <span style={{ fontSize: 13 }} aria-live="polite">
        <AnimatePresence mode="popLayout" initial={false}>
          <motion.span key={status.label} style={{ color: status.color, display: 'inline-block' }} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: dur.base, ease: easeOut }}>
            <motion.span style={{ display: 'inline-block' }} initial={{ scale: 0.3 }} animate={{ scale: 1 }} transition={spring.pop}>●</motion.span> {status.label}
          </motion.span>
        </AnimatePresence>{' '}
        · {status.detail}
      </span>
      {!c.editable && (
        <div className="note neutral" aria-live="polite">
          {password === null
            ? t('Your admin has locked these settings. Ask them if you need to connect to a different server.')
            : t('Unlocked for this visit. Saving or leaving this page locks these settings again.')}
        </div>
      )}
      {/* One button for both, so focus comes back to it when the unlock dialog closes. */}
      {!c.editable && (c.unlockable || password !== null) && (
        <div>
          <button type="button" className="btn small" onClick={() => (password === null ? setUnlocking(true) : lockAgain())}>
            {password === null ? t('Unlock with admin password') : t('Lock again')}
          </button>
        </div>
      )}
      <label className="field-label" style={{ fontSize: 13 }}>
        {t('Server address')}
        <input type="url" className="input mono" value={server} disabled={locked} placeholder="https://fonts.example.com" onChange={(e) => (setServer(e.target.value), setResult(null))} />
      </label>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10, alignItems: 'flex-end' }}>
        <label className="field-label" style={{ flex: '1 1 260px', fontSize: 13 }}>
          {t('Install key')}
          <input type={showKey ? 'text' : 'password'} className="input mono" value={key} disabled={locked} autoComplete="off" spellCheck={false} onChange={(e) => (setKey(e.target.value), setResult(null))} />
        </label>
        <button type="button" className="btn" style={{ height: 42 }} aria-pressed={showKey} onClick={() => setShowKey(!showKey)}>{showKey ? t('Hide') : t('Show')}</button>
      </div>
      <AnimatePresence initial={false}>
        {dirty && !local && (
          <motion.div key="dirty" {...collapse} style={{ overflow: 'hidden' }}>
            <div className="note warn">
              {t('If you connect to a different server, you start there as a new install. Your fonts, collections and tags stay on this computer, but your name history stays on the old server.')}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
        <button type="button" className="btn" onClick={test} disabled={locked || busy}>{t('Test connection')}</button>
        <button type="button" className="btn primary" onClick={save} disabled={locked || busy || (!dirty && !removed)}>{local ? t('Connect') : t('Save and reconnect')}</button>
        <button type="button" className="btn link" onClick={reset} disabled={locked || busy}>{t('Use installer defaults')}</button>
      </div>
      <span className="faint" style={{ fontSize: 12 }}>
        {t('Install ID')} <span className="mono">{c.installId}</span> · <VersionGames version={app.appVersion} /> · {app.platform === 'macos' ? 'macOS' : 'Windows'}.
      </span>
      <Dialog open={unlocking} onClose={() => setUnlocking(false)} title={t('Unlock connection settings')}>
        {unlocking && <UnlockForm onCancel={() => setUnlocking(false)} onUnlocked={(pw) => (setPassword(pw), setUnlocking(false))} />}
      </Dialog>
    </Section>
  )
}

export function SettingsPage() {
  return (
    <div className="page" style={{ padding: '24px 32px 48px' }}>
      <div style={{ maxWidth: 760, display: 'flex', flexDirection: 'column', gap: 20 }}>
        <h1 className="h-page">{t('Settings')}</h1>
        <NameSection />
        <FoldersSection />
        <HealthSection />
        <BackupSection />
        <AppSection />
        <GuidesSection />
        <SampleTextsSection />
        <AdobeSection />
        <HistorySection />
        <ConnectionSection />
      </div>
    </div>
  )
}
