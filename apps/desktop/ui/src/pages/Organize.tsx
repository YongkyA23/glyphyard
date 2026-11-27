import { useCallback, useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, type Folder, type OrganizeCase, type OrganizeChange, type OrganizePlan } from '../api'
import { Hint } from '../Hint'
import { t, tk } from '../i18n'
import { cssStagger, dur, easeOut } from '../motion'
import { useApp } from '../state'
import { ago, Badge, basename, Check, errMsg, Num, plural, SelectRing, Skel, Spinner, type Tone } from '../ui'

const CASES: Record<OrganizeCase, { label: string; action: string; tone: Tone }> = {
  exact_copy: { label: tk('Exact copy'), action: tk('Keep one, rest to {trash}'), tone: 'neutral' },
  different_version: { label: tk('Different version'), action: tk('Rename with version'), tone: 'accent' },
  other_format: { label: tk('Same font, other format'), action: tk('Prefer OTF'), tone: 'ok' },
  name_clash: { label: tk('Name clash'), action: tk('Rename with foundry'), tone: 'bad' },
  variable_static: { label: tk('Variable + static'), action: tk('Group, activate one kind'), tone: 'violet' },
  in_system: { label: tk('Already in system'), action: tk('Skip'), tone: 'neutral' },
  junk: { label: tk('Junk'), action: tk('Move to quarantine'), tone: 'warn' },
  tidy: { label: tk('Tidy folders'), action: tk('Move into Foundry / Family'), tone: 'accent' },
}

const str = (v: unknown) => (typeof v === 'string' ? v : '')
/** The file a change is about, shortened to "parent\name". */
function fileOf(c: OrganizeChange) {
  const a = c.action
  const short = (p: string) => p.split(/[\\/]/).slice(-2).join('\\')
  if (a.op === 'group' && Array.isArray(a.paths)) return a.paths.length > 1 ? t('{name} + {n} more', { name: basename(str(a.paths[0])), n: a.paths.length - 1 }) : basename(str(a.paths[0]))
  return short(str(a.path) || str(a.from))
}
/** Plain-language result for one change. */
function describe(c: OrganizeChange, trash: string) {
  const a = c.action
  const base = ((): string => {
    switch (a.op) {
      case 'trash': return t('Moved to {where}. Kept: {kept}', { where: trash, kept: str(a.kept).split(/[\\/]/).slice(-2).join('\\') })
      case 'rename': return t('Renamed to {name}', { name: basename(str(a.to)) })
      case 'keep_as_alternate': return t('Kept as alternate. {name} is used', { name: basename(str(a.preferred)) })
      case 'move_aside': return t('Moved aside to {to}', { to: str(a.to) })
      case 'group': return a.family ? t('Grouped as one family ({family})', { family: str(a.family) }) : t('Grouped as one family')
      case 'skip': return t('Skipped')
      case 'quarantine': return t('Moved to quarantine')
      case 'move': return t('Moved to {where}', { where: str(a.to) })
      default: return a.op
    }
  })()
  return c.note ? `${base} · ${c.note}` : base
}

export function Organize() {
  const { app, toast, bump, setOrganizeCount } = useApp()
  const navigate = useNavigate()
  const [tidy, setTidy] = useState(false)
  const [plan, setPlan] = useState<OrganizePlan | null>(null)
  const [folders, setFolders] = useState<Folder[]>([])
  const [filter, setFilter] = useState<OrganizeCase | 'all'>('all')
  const [excluded, setExcluded] = useState<Set<number>>(new Set())
  const [busy, setBusy] = useState(false)
  const [applyState, setApplyState] = useState<'idle' | 'applying' | 'done'>('idle')
  const [error, setError] = useState<string | null>(null)
  const trash = app.platform === 'macos' ? t('Trash') : t('Recycle Bin')

  const scan = useCallback((withTidy: boolean) => {
    setBusy(true)
    setError(null)
    api.organizePlan(withTidy).then(
      (p) => (setPlan(p), setOrganizeCount(p.changes.length), setBusy(false)),
      (e) => (setError(errMsg(e)), setBusy(false)),
    )
  }, [setOrganizeCount])
  useEffect(() => scan(tidy), [tidy, scan])
  useEffect(() => void api.listFolders().then(setFolders, () => {}), [])

  const changes = plan?.changes ?? []
  const count = (k: OrganizeCase) => changes.filter((c) => c.case === k).length
  // Only cases that have work in this plan get a card.
  const caseKeys = (Object.keys(CASES) as OrganizeCase[]).filter((k) => count(k) > 0)
  const notes = plan?.notes ?? []
  const [notesShown, setNotesShown] = useState(100)
  const matching = changes.filter((c) => filter === 'all' || c.case === filter)
  // Big plans render 100 rows at a time.
  const [shown, setShown] = useState(100)
  useEffect(() => setShown(100), [filter, plan])
  const rows = matching.slice(0, shown)
  const selected = changes.filter((c) => !excluded.has(c.id)).length

  async function apply() {
    setBusy(true)
    setApplyState('applying')
    try {
      const { historyId, applied, failed } = await api.organizeApply(tidy, [...excluded].filter((id) => changes.some((c) => c.id === id)))
      setApplyState('done')
      setTimeout(() => setApplyState('idle'), 1600)
      if (failed.length) toast(t('{changes} couldn’t be done, e.g. {first}', { changes: plural(failed.length, 'change'), first: failed[0] }))
      toast(t('Applied {changes}', { changes: plural(applied, 'change') }), historyId ? {
        label: t('Undo'),
        run: () => api.undoHistory(historyId).then(() => (toast(t('Auto-organize undone')), bump(), scan(tidy)), (e) => toast(errMsg(e))),
      } : undefined)
      setExcluded(new Set())
      bump()
      scan(tidy)
    } catch (e) {
      toast(errMsg(e))
      setBusy(false)
      setApplyState('idle')
    }
  }

  const caseCard = (key: OrganizeCase | 'all', n: number, label: string, action: string) => (
    <button key={key} type="button" className="select-card compact" aria-pressed={filter === key} onClick={() => setFilter(key)}>
      {filter === key && <SelectRing group="case-ring" />}
      <span style={{ fontSize: 22, fontWeight: 600 }}><Num value={n} /></span>
      <span style={{ fontWeight: 500, fontSize: 13 }}>{label}</span>
      <span className="faint" style={{ fontSize: 12 }}>{action}</span>
    </button>
  )

  return (
    <div className="page" style={{ gap: 18 }}>
      <div className="row-between">
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <h1 className="h-page">{t('Auto-organize')}</h1>
          <span className="muted" aria-live="polite">
            {busy && !plan ? t('Scanning…') : plan ? t('Scanned {files} in {folders} · {when} · nothing has changed yet', { files: plural(plan.scanned, 'file'), folders: plural(folders.length, 'folder'), when: ago(plan.scannedAt) }) : ''}
          </span>
        </div>
        <button type="button" className="btn" onClick={() => scan(tidy)} disabled={busy}>{busy ? t('Scanning…') : t('Scan again')}</button>
      </div>
      {error && <p role="alert" style={{ margin: 0, color: 'var(--bad)' }}>{error}</p>}

      <div className="case-grid">
        {caseCard('all', changes.length, t('All changes'), t('Everything below'))}
        {caseKeys.map((k) => caseCard(k, count(k), t(CASES[k].label /* i18n-dynamic */), t(CASES[k].action, { trash })))}
      </div>

      <div className="table-wrap">
        <table style={{ minWidth: 820 }}>
          <caption className="sr-only">{t('Planned changes')}</caption>
          <thead>
            <tr><th style={{ width: 44 }}><span className="sr-only">{t('Include')}</span></th><th>{t('Case')}</th><th>{t('File')}</th><th>{t('What will happen')}</th></tr>
          </thead>
          {/* Keyed by filter so a new filter staggers its rows in. */}
          <tbody key={filter}>
            {!plan && [0, 1, 2, 3].map((i) => (
              <tr key={i} aria-hidden="true">{[16, '40%', '60%', '80%'].map((w, j) => <td key={j}><Skel w={w} h={j ? 12 : 16} /></td>)}</tr>
            ))}
            {rows.map((c, i) => {
              const file = fileOf(c)
              return (
                <tr key={c.id} className="enter" style={{ verticalAlign: 'top', ...cssStagger(i) }}>
                  <td>
                    <input
                      type="checkbox" checked={!excluded.has(c.id)} aria-label={t('Include change for {file}', { file })} style={{ width: 16, height: 16 }}
                      onChange={() => setExcluded((s) => { const n = new Set(s); if (n.has(c.id)) n.delete(c.id); else n.add(c.id); return n })}
                    />
                  </td>
                  <td><span style={{ whiteSpace: 'nowrap' }}><Badge tone={CASES[c.case].tone}>{t(CASES[c.case].label /* i18n-dynamic */)}</Badge></span></td>
                  <td className="mono" style={{ fontSize: 12 }}>{file}</td>
                  <td style={{ color: 'var(--ink-2)' }}>{describe(c, trash)}</td>
                </tr>
              )
            })}
            {plan && rows.length === 0 && (
              <tr><td colSpan={4} className="muted">{t('Nothing to change here.')}</td></tr>
            )}
          </tbody>
        </table>
      </div>
      {matching.length > shown && (
        <button type="button" className="btn" style={{ alignSelf: 'center' }} onClick={() => setShown((n) => n + 100)}>
          {t('Show {n} more of {total}', { n: Math.min(100, matching.length - shown), total: matching.length - shown })}
        </button>
      )}

      {notes.length > 0 && (
        <details className="card notes">
          <summary>
            <span style={{ fontWeight: 500 }}>{t('No action needed · {n}', { n: notes.length })}</span>
            <span className="muted" style={{ fontSize: 13 }}> {t('Fonts kept as alternates, variable + static groups and fonts the system already has. Glyphyard handles these when activating; no file changes.')}</span>
          </summary>
          <div className="table-wrap" style={{ marginTop: 10 }}>
            <table style={{ minWidth: 760 }}>
              <thead><tr><th>{t('Case')}</th><th>{t('File')}</th><th>{t('What happens')}</th></tr></thead>
              <tbody>
                {notes.slice(0, notesShown).map((c) => (
                  <tr key={c.id} style={{ verticalAlign: 'top' }}>
                    <td><span style={{ whiteSpace: 'nowrap' }}><Badge tone={CASES[c.case].tone}>{t(CASES[c.case].label /* i18n-dynamic */)}</Badge></span></td>
                    <td className="mono" style={{ fontSize: 12 }}>{fileOf(c)}</td>
                    <td style={{ color: 'var(--ink-2)' }}>{describe(c, trash)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {notes.length > notesShown && (
            <button type="button" className="btn small" style={{ marginTop: 10 }} onClick={() => setNotesShown((n) => n + 100)}>{t('Show more')}</button>
          )}
        </details>
      )}

      <label className="card" style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 10, padding: 14, borderRadius: 10 }}>
        <input type="checkbox" checked={tidy} onChange={() => setTidy(!tidy)} style={{ marginTop: 2, width: 16, height: 16 }} />
        <span style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <span style={{ fontWeight: 500 }}>{t('Also tidy folders')}</span>
          <span className="muted" style={{ fontSize: 13 }}>{t('Move files into Foundry / Family / Family-Style v1.000.otf. License files move with their fonts.')}</span>
        </span>
      </label>

      <div className="row-between" style={{ paddingTop: 4 }}>
        <span className="muted" style={{ fontSize: 13 }}>
          {t('About renames')} <Hint text={t('Renames change the file name and the name shown in Glyphyard. The font’s internal name is never edited. You can undo this run from Settings › History.')} />
        </span>
        <span style={{ display: 'flex', gap: 8 }}>
          <button type="button" className="btn big-ish" onClick={() => navigate(-1)}>{t('Cancel')}</button>
          <button
            type="button" className={`btn big-ish apply-btn ${applyState === 'done' ? 'installed' : 'primary'}`} onClick={apply}
            disabled={applyState !== 'idle' || busy || selected === 0} aria-busy={applyState === 'applying'}
          >
            {applyState === 'applying' && <motion.span className="apply-progress" initial={{ scaleX: 0 }} animate={{ scaleX: 0.9 }} transition={{ duration: 1.2, ease: easeOut }} />}
            <AnimatePresence mode="popLayout" initial={false}>
              <motion.span
                key={applyState} style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}
                initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -10 }} transition={{ duration: dur.base, ease: easeOut }}
              >
                {applyState === 'applying' ? <><Spinner /> {t('Applying…')}</> : applyState === 'done' ? <><Check /> {t('Applied')}</> : t('Apply {n} changes', { n: selected })}
              </motion.span>
            </AnimatePresence>
          </button>
        </span>
      </div>
    </div>
  )
}
