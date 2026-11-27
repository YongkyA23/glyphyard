import { useState } from 'react'
import { Link } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, type OrganizeChange, type OrganizeReport } from '../api'
import { Hint } from '../Hint'
import { t, tk } from '../i18n'
import { fast, pop, row } from '../motion'
import { toast } from '../toast'
import {
  Badge, Empty, ErrorBox, errMsg, fmtDate, Loaded, PageHeader, Pager, plural, Skel, Spin, Swap, Table, TableSkeleton, useConfirm, useLoad, usePage, type Tone,
} from '../ui'

const head = () => [t('Case'), t('File'), t('What will happen')]
const SHOW_STEP = 100
const reportsSkeleton = (
  <div>{[0, 1].map((i) => <div key={i} className="report"><span className="report-name"><Skel w="60%" /></span><span className="report-found"><Skel w="50%" /></span><span className="report-last"><Skel w={90} /></span><Skel w={170} h={34} r={8} /></div>)}</div>
)

const CASES: Record<string, [string, Tone]> = {
  exact_copy: [tk('Exact copy'), 'user'], different_version: [tk('Different version'), 'admin'], other_format: [tk('Other format'), 'ok'],
  name_clash: [tk('Name clash'), 'bad'], variable_static: [tk('Variable + static'), 'violet'], in_system: [tk('In system'), 'outline'],
  junk: [tk('Junk'), 'warn'], tidy: [tk('Tidy'), 'outline'],
}
const fileOf = (c: OrganizeChange) => {
  const a = c.action
  return String(a.path ?? a.from ?? a.family ?? '')
}

export function Organize() {
  return (
    <div className="page">
      <PageHeader title={t('Auto-organize')} sub={t('Clean up the shared library, or ask a user’s app to clean up their own fonts.')} />
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
  const HEAD = head()

  const apply = async () => {
    if (!(await confirm(t('Apply {changes} to the shared library? Files that are removed leave the library and are kept in the server’s quarantine folder.', { changes: plural(changes.length, 'change') }), t('Apply'), true))) return
    setBusy(true)
    setError('')
    try {
      const n = (await api.applyOrganize()).applied
      setApplied(n)
      toast(t('Applied {changes}.', { changes: plural(n, 'change') }))
      await state.reload(true)
    } catch (e) {
      setError(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <section aria-labelledby="lib" className="card pad-20 stack-14">
      <div className="page-head">
        <div className="stack-2">
          <div className="title-row"><h2 id="lib" className="h2">{t('Shared library')}</h2><Hint text={t('Removed files leave the shared library and are kept in the server’s quarantine folder; they are not sent to users. Renamed fonts keep their internal names.')} /></div>
          <span className="small mute">
            <Swap>{state.data
              ? applied !== undefined
                ? t('Applied {applied} · {more} suggested', { applied: plural(applied, 'change'), more: plural(changes.length, 'more change') })
                : t('Scanned {files} · {changes} suggested · nothing changed yet', { files: plural(state.data.files, 'file'), changes: plural(changes.length, 'change') })
              : t('Scanning…')}</Swap>
          </span>
        </div>
        {/* Apply → progress → success */}
        <AnimatePresence mode="wait" initial={false}>
          {applied !== undefined && changes.length === 0 ? (
            <motion.span key="done" className="btn btn-done" {...pop}>{t('Applied ✓')}</motion.span>
          ) : (
            <motion.button key="apply" type="button" className="btn btn-primary" disabled={busy || changes.length === 0} onClick={apply}
              exit={{ opacity: 0, scale: 0.96, transition: fast }}>
              {busy && <Spin />}<Swap>{busy ? t('Applying…') : t('Apply {changes}', { changes: plural(changes.length, 'change') })}</Swap>
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
        ? <Empty>{t('Nothing to clean up. The shared library is tidy.')}</Empty>
        : (
          <>
            <Table inset caption={t('Suggested changes to the shared library')} minWidth={760} head={HEAD}>
              {changes.slice(0, limit).map((c, i, shown) => {
                const [label, tone] = CASES[c.case] ?? [c.case, 'outline']
                return (
                  <motion.tr key={i} {...row(i, shown.length)}>
                    <td><Badge tone={tone} className="nowrap">{t(label /* i18n-dynamic */)}</Badge></td>
                    <td className="mono">{fileOf(c)}</td>
                    <td className="ink-2">{c.note}</td>
                  </motion.tr>
                )
              })}
            </Table>
            {changes.length > limit && (
              <span className="row-between">
                <span className="meta">{t('Showing {limit} of {total}', { limit, total: changes.length })}</span>
                <button type="button" className="btn btn-sm" onClick={() => setLimit((n) => n + SHOW_STEP)}>
                  {t('Show {n} more', { n: Math.min(SHOW_STEP, changes.length - limit) })}
                </button>
              </span>
            )}
          </>
        )}</Loaded>
      {confirmDialog}
    </section>
  )
}

const found = (r: OrganizeReport) =>
  [plural(r.duplicates, 'duplicate'), plural(r.version_conflicts, 'version conflict'), plural(r.name_clashes, 'name clash', 'name clashes')]
    .filter((s) => !s.startsWith('0 ')).join(' · ') || t('No duplicates')

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
      setError(errMsg(e))
    }
  }

  return (
    <section aria-labelledby="usr" className="card pad-20 stack-10">
      <div className="title-row"><h2 id="usr" className="h2">{t('Users’ computers')}</h2><Hint text={t('Duplicates reported by each app. A request shows the user a preview, and nothing changes until they confirm.')} /></div>
      {error && <ErrorBox message={error} />}
      <Loaded state={state} skeleton={reportsSkeleton}>{({ items: reports, total }) => (
        <div className="stack-10">
          <div>
          {total === 0 && <Empty>{t('No reports yet. Apps send one after they scan their fonts.')}</Empty>}
          {reports.map((r, i) => {
            const clean = r.duplicates + r.version_conflicts + r.name_clashes === 0
            const s = asked[r.install_id]
            return (
              <motion.div key={r.install_id} className="report" {...row(i, reports.length)}>
                <Link to={`/users/${encodeURIComponent(r.install_id)}`} className="name-link report-name">{r.name}</Link>
                <span className="report-found small mute">{found(r)}</span>
                <span className="report-last meta">{r.last_organized ? t('Organized {date}', { date: fmtDate(r.last_organized) }) : t('Never organized')}</span>
                <button type="button" className={clean || s ? 'btn btn-sm btn-muted' : 'btn btn-sm btn-outline'} disabled={clean || !!s}
                  onClick={() => request(r.install_id)}>
                  {s === 'busy' && <Spin />}
                  <Swap>{clean ? t('Nothing to do') : s === 'done' ? t('Requested · waiting for user') : s === 'busy' ? t('Sending…') : t('Request auto-organize')}</Swap>
                </button>
              </motion.div>
            )
          })}
          </div>
          <Pager label={t('Report pages')} page={page} total={total} onPage={setPage} />
        </div>
      )}</Loaded>
    </section>
  )
}
