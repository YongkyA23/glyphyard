import { useCallback, useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, type Folder, type OrganizeCase, type OrganizeChange, type OrganizePlan } from '../api'
import { cssStagger, dur, easeOut } from '../motion'
import { useApp } from '../state'
import { ago, Badge, basename, Check, errMsg, Num, num, plural, SelectRing, Skel, Spinner, type Tone } from '../ui'

const CASES: Record<OrganizeCase, { label: string; action: string; tone: Tone }> = {
  exact_copy: { label: 'Exact copy', action: 'Keep one, rest to Recycle Bin', tone: 'neutral' },
  different_version: { label: 'Different version', action: 'Rename with version', tone: 'accent' },
  other_format: { label: 'Same font, other format', action: 'Prefer OTF', tone: 'ok' },
  name_clash: { label: 'Name clash', action: 'Rename with foundry', tone: 'bad' },
  variable_static: { label: 'Variable + static', action: 'Group, activate one kind', tone: 'violet' },
  in_system: { label: 'Already in system', action: 'Skip', tone: 'neutral' },
  junk: { label: 'Junk', action: 'Move to quarantine', tone: 'warn' },
  tidy: { label: 'Tidy folders', action: 'Move into Foundry / Family', tone: 'accent' },
}

const str = (v: unknown) => (typeof v === 'string' ? v : '')
/** The file a change is about, shortened to "parent\name". */
function fileOf(c: OrganizeChange) {
  const a = c.action
  const short = (p: string) => p.split(/[\\/]/).slice(-2).join('\\')
  if (a.op === 'group' && Array.isArray(a.paths)) return `${basename(str(a.paths[0]))}${a.paths.length > 1 ? ` + ${a.paths.length - 1} more` : ''}`
  return short(str(a.path) || str(a.from))
}
/** Plain-language result for one change. */
function describe(c: OrganizeChange, trash: string) {
  const a = c.action
  const base = ((): string => {
    switch (a.op) {
      case 'trash': return `Moved to ${trash}. Kept: ${str(a.kept).split(/[\\/]/).slice(-2).join('\\')}`
      case 'rename': return `Renamed to ${basename(str(a.to))}`
      case 'keep_as_alternate': return `Kept as alternate. ${basename(str(a.preferred))} is used`
      case 'move_aside': return `Moved aside to ${str(a.to)}`
      case 'group': return `Grouped as one family${a.family ? ` (${str(a.family)})` : ''}`
      case 'skip': return 'Skipped'
      case 'quarantine': return 'Moved to quarantine'
      case 'move': return `Moved to ${str(a.to)}`
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
  const trash = app.platform === 'macos' ? 'Trash' : 'Recycle Bin'

  const scan = useCallback((t: boolean) => {
    setBusy(true)
    setError(null)
    api.organizePlan(t).then(
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
      if (failed.length) toast(`${plural(failed.length, 'change')} couldn’t be done, e.g. ${failed[0]}`)
      toast(`Applied ${plural(applied, 'change')}`, historyId ? {
        label: 'Undo',
        run: () => api.undoHistory(historyId).then(() => (toast('Auto-organize undone'), bump(), scan(tidy)), (e) => toast(errMsg(e))),
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
          <h1 className="h-page">Auto-organize</h1>
          <span className="muted" aria-live="polite">
            {busy && !plan ? 'Scanning…' : plan ? `Scanned ${plural(plan.scanned, 'file')} in ${plural(folders.length, 'folder')} · ${ago(plan.scannedAt)} · nothing has changed yet` : ''}
          </span>
        </div>
        <button type="button" className="btn" onClick={() => scan(tidy)} disabled={busy}>{busy ? 'Scanning…' : 'Scan again'}</button>
      </div>
      {error && <p role="alert" style={{ margin: 0, color: 'var(--bad)' }}>{error}</p>}

      <div className="case-grid">
        {caseCard('all', changes.length, 'All changes', 'Everything below')}
        {caseKeys.map((k) => caseCard(k, count(k), CASES[k].label, k === 'exact_copy' ? `Keep one, rest to ${trash}` : CASES[k].action))}
      </div>

      <div className="table-wrap">
        <table style={{ minWidth: 820 }}>
          <caption className="sr-only">Planned changes</caption>
          <thead>
            <tr><th style={{ width: 44 }}><span className="sr-only">Include</span></th><th>Case</th><th>File</th><th>What will happen</th></tr>
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
                      type="checkbox" checked={!excluded.has(c.id)} aria-label={`Include change for ${file}`} style={{ width: 16, height: 16 }}
                      onChange={() => setExcluded((s) => { const n = new Set(s); if (n.has(c.id)) n.delete(c.id); else n.add(c.id); return n })}
                    />
                  </td>
                  <td><span style={{ whiteSpace: 'nowrap' }}><Badge tone={CASES[c.case].tone}>{CASES[c.case].label}</Badge></span></td>
                  <td className="mono" style={{ fontSize: 12 }}>{file}</td>
                  <td style={{ color: 'var(--ink-2)' }}>{describe(c, trash)}</td>
                </tr>
              )
            })}
            {plan && rows.length === 0 && (
              <tr><td colSpan={4} className="muted">Nothing to change here.</td></tr>
            )}
          </tbody>
        </table>
      </div>
      {matching.length > shown && (
        <button type="button" className="btn" style={{ alignSelf: 'center' }} onClick={() => setShown((n) => n + 100)}>
          Show {Math.min(100, matching.length - shown)} more of {matching.length - shown}
        </button>
      )}

      {notes.length > 0 && (
        <details className="card notes">
          <summary>
            <span style={{ fontWeight: 500 }}>No action needed · {num(notes.length)}</span>
            <span className="muted" style={{ fontSize: 13 }}> Fonts kept as alternates, variable + static groups and fonts the system already has. Typeshelf handles these when activating; no file changes.</span>
          </summary>
          <div className="table-wrap" style={{ marginTop: 10 }}>
            <table style={{ minWidth: 760 }}>
              <thead><tr><th>Case</th><th>File</th><th>What happens</th></tr></thead>
              <tbody>
                {notes.slice(0, notesShown).map((c) => (
                  <tr key={c.id} style={{ verticalAlign: 'top' }}>
                    <td><span style={{ whiteSpace: 'nowrap' }}><Badge tone={CASES[c.case].tone}>{CASES[c.case].label}</Badge></span></td>
                    <td className="mono" style={{ fontSize: 12 }}>{fileOf(c)}</td>
                    <td style={{ color: 'var(--ink-2)' }}>{describe(c, trash)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {notes.length > notesShown && (
            <button type="button" className="btn small" style={{ marginTop: 10 }} onClick={() => setNotesShown((n) => n + 100)}>Show more</button>
          )}
        </details>
      )}

      <label className="card" style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 10, padding: 14, borderRadius: 10 }}>
        <input type="checkbox" checked={tidy} onChange={() => setTidy(!tidy)} style={{ marginTop: 2, width: 16, height: 16 }} />
        <span style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <span style={{ fontWeight: 500 }}>Also tidy folders</span>
          <span className="muted" style={{ fontSize: 13 }}>Move files into Foundry / Family / Family-Style v1.000.otf. License files move with their fonts.</span>
        </span>
      </label>

      <div className="row-between" style={{ paddingTop: 4 }}>
        <span className="muted" style={{ fontSize: 13, maxWidth: 560, lineHeight: 1.5 }}>
          Renames change the file name and the name shown in Typeshelf. The font’s internal name is never edited. You can undo this run from Settings › History.
        </span>
        <span style={{ display: 'flex', gap: 8 }}>
          <button type="button" className="btn big-ish" onClick={() => navigate(-1)}>Cancel</button>
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
                {applyState === 'applying' ? <><Spinner /> Applying…</> : applyState === 'done' ? <><Check /> Applied</> : `Apply ${num(selected)} changes`}
              </motion.span>
            </AnimatePresence>
          </button>
        </span>
      </div>
    </div>
  )
}
