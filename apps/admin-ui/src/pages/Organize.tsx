import { useState } from 'react'
import { Link } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, errorText, type OrganizeChange, type OrganizeReport } from '../api'
import { fast, pop, row } from '../motion'
import {
  Badge, Empty, ErrorBox, fmtDate, Loaded, PageHeader, Pager, plural, Skel, Spin, Swap, Table, TableSkeleton, useConfirm, useLoad, usePage, type Tone,
} from '../ui'

const HEAD = ['Case', 'File', 'What will happen']
const SHOW_STEP = 100
const reportsSkeleton = (
  <div>{[0, 1].map((i) => <div key={i} className="report"><span className="report-name"><Skel w="60%" /></span><span className="report-found"><Skel w="50%" /></span><span className="report-last"><Skel w={90} /></span><Skel w={170} h={34} r={8} /></div>)}</div>
)

const CASES: Record<string, [string, Tone]> = {
  exact_copy: ['Exact copy', 'user'], different_version: ['Different version', 'admin'], other_format: ['Other format', 'ok'],
  name_clash: ['Name clash', 'bad'], variable_static: ['Variable + static', 'violet'], in_system: ['In system', 'outline'],
  junk: ['Junk', 'warn'], tidy: ['Tidy', 'outline'],
}
const fileOf = (c: OrganizeChange) => {
  const a = c.action
  return String(a.path ?? a.from ?? a.family ?? '')
}

export function Organize() {
  return (
    <div className="page">
      <PageHeader title="Auto-organize" sub="Clean up the shared library, or ask a user’s app to clean up their own fonts." />
      <LibraryPlan />
      <Reports />
    </div>
  )
}

function LibraryPlan() {
  const state = useLoad(async () => {
    const [plan, lib] = await Promise.all([api.organizePlan(), api.library('', 1, 1)])
    return { changes: plan.changes, files: lib.total }
  }, [])
  const [limit, setLimit] = useState(SHOW_STEP)
  const [applied, setApplied] = useState<number>()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [confirmDialog, confirm] = useConfirm()
  const changes = state.data?.changes ?? []

  const apply = async () => {
    if (!(await confirm(`Apply ${plural(changes.length, 'change')} to the shared library?`, 'Apply'))) return
    setBusy(true)
    setError('')
    try {
      setApplied((await api.applyOrganize()).applied)
      await state.reload(true)
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <section aria-labelledby="lib" className="card pad-20 stack-14">
      <div className="page-head">
        <div className="stack-2">
          <h2 id="lib" className="h2">Shared library</h2>
          <span className="small mute">
            <Swap>{state.data
              ? applied !== undefined
                ? `Applied ${plural(applied, 'change')} · ${plural(changes.length, 'more change')} suggested`
                : `Scanned ${plural(state.data.files, 'file')} · ${plural(changes.length, 'change')} suggested · nothing changed yet`
              : 'Scanning…'}</Swap>
          </span>
        </div>
        {/* Apply → progress → success */}
        <AnimatePresence mode="wait" initial={false}>
          {applied !== undefined && changes.length === 0 ? (
            <motion.span key="done" className="btn btn-done" {...pop}>Applied ✓</motion.span>
          ) : (
            <motion.button key="apply" type="button" className="btn btn-primary" disabled={busy || changes.length === 0} onClick={apply}
              exit={{ opacity: 0, scale: 0.96, transition: fast }}>
              {busy && <Spin />}<Swap>{busy ? 'Applying…' : `Apply ${plural(changes.length, 'change')}`}</Swap>
            </motion.button>
          )}
        </AnimatePresence>
      </div>
      <AnimatePresence>
        {busy && (
          <motion.span className="progress progress-run" aria-hidden="true" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
            <span />
          </motion.span>
        )}
      </AnimatePresence>
      {error && <ErrorBox message={error} />}
      <Loaded state={state} skeleton={<TableSkeleton inset head={HEAD} minWidth={760} rows={2} />}>{({ changes }) => changes.length === 0
        ? <Empty>Nothing to clean up. The shared library is tidy.</Empty>
        : (
          <>
            <Table inset caption="Suggested changes to the shared library" minWidth={760} head={HEAD}>
              {changes.slice(0, limit).map((c, i, shown) => {
                const [label, tone] = CASES[c.case] ?? [c.case, 'outline']
                return (
                  <motion.tr key={i} {...row(i, shown.length)}>
                    <td><Badge tone={tone} className="nowrap">{label}</Badge></td>
                    <td className="mono">{fileOf(c)}</td>
                    <td className="ink-2">{c.note}</td>
                  </motion.tr>
                )
              })}
            </Table>
            {changes.length > limit && (
              <span className="row-between">
                <span className="meta">Showing {limit} of {changes.length}</span>
                <button type="button" className="btn btn-sm" onClick={() => setLimit((n) => n + SHOW_STEP)}>
                  Show {Math.min(SHOW_STEP, changes.length - limit)} more
                </button>
              </span>
            )}
          </>
        )}</Loaded>
      <span className="meta">Users receive the cleaned-up files on their next sync. Renamed fonts keep their internal names.</span>
      {confirmDialog}
    </section>
  )
}

const found = (r: OrganizeReport) =>
  [plural(r.duplicates, 'duplicate'), plural(r.version_conflicts, 'version conflict'), plural(r.name_clashes, 'name clash', 'name clashes')]
    .filter((s) => !s.startsWith('0 ')).join(' · ') || 'No duplicates'

function Reports() {
  const [page, setPage] = usePage('')
  const state = useLoad(() => api.organizeReports(page), [page])
  const [asked, setAsked] = useState<Record<string, 'busy' | 'done'>>({})
  const [error, setError] = useState('')

  const request = async (id: string) => {
    setAsked((a) => ({ ...a, [id]: 'busy' }))
    setError('')
    try {
      await api.requestOrganize(id)
      setAsked((a) => ({ ...a, [id]: 'done' }))
    } catch (e) {
      setAsked(({ [id]: _, ...rest }) => rest)
      setError(errorText(e))
    }
  }

  return (
    <section aria-labelledby="usr" className="card pad-20 stack-10">
      <div className="stack-2">
        <h2 id="usr" className="h2">Users’ computers</h2>
        <span className="small mute">Duplicates reported by each app. A request shows the user a preview, and nothing changes until they confirm.</span>
      </div>
      {error && <ErrorBox message={error} />}
      <Loaded state={state} skeleton={reportsSkeleton}>{({ items: reports, total }) => (
        <div className="stack-10">
          <div>
          {total === 0 && <Empty>No reports yet. Apps send one after they scan their fonts.</Empty>}
          {reports.map((r, i) => {
            const clean = r.duplicates + r.version_conflicts + r.name_clashes === 0
            const s = asked[r.install_id]
            return (
              <motion.div key={r.install_id} className="report" {...row(i, reports.length)}>
                <Link to={`/users/${encodeURIComponent(r.install_id)}`} className="name-link report-name">{r.name}</Link>
                <span className="report-found small mute">{found(r)}</span>
                <span className="report-last meta">{r.last_organized ? `Organized ${fmtDate(r.last_organized)}` : 'Never organized'}</span>
                <button type="button" className={clean || s ? 'btn btn-sm btn-muted' : 'btn btn-sm btn-outline'} disabled={clean || !!s}
                  onClick={() => request(r.install_id)}>
                  {s === 'busy' && <Spin />}
                  <Swap>{clean ? 'Nothing to do' : s === 'done' ? 'Requested · waiting for user' : s === 'busy' ? 'Sending…' : 'Request auto-organize'}</Swap>
                </button>
              </motion.div>
            )
          })}
          </div>
          <Pager label="Report pages" page={page} total={total} onPage={setPage} />
        </div>
      )}</Loaded>
    </section>
  )
}
