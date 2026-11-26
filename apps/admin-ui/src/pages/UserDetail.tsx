import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, errorText, type InstallDetail, type TimelineEvent, type TimelineFilter } from '../api'
import { dropIn, fadeRise, row } from '../motion'
import { toast } from '../toast'
import { InstallFonts } from './InstallFonts'
import { useFontToggle } from './useFontToggle'
import {
  ago, Badge, byLabel, byTone, Chips, downloadCsv, Empty, ErrorBox, fmtDate, fmtShort, firstName, Loaded, PageHeader, Pager, plural, Skel, Spin, Swap,
  useConfirm, useLoad, usePage,
} from '../ui'

const eventsSkeleton = (
  <div>
    {[70, 55, 80, 45].map((w) => (
      <div key={w} className="event">
        <span className="event-when"><Skel w={80} /></span><span className="event-text"><Skel w={`${w}%`} /></span><Skel w={64} h={20} r={10} />
      </div>
    ))}
  </div>
)
const detailSkeleton = (
  <div className="stack-18">
    <span className="stack-8">
      <Skel w={240} h={28} r={8} /><Skel w="45%" />
      <span className="pills">{[90, 80, 110, 120, 110].map((w, i) => <Skel key={i} w={w} h={22} r={11} />)}</span>
    </span>
    <div className="split">
      <div className="card pad-20 grow-3 stack-12"><Skel w={100} h={16} />{eventsSkeleton}</div>
      <div className="side">
        <div className="card pad-18 stack-14"><Skel w={130} h={16} /><Skel h={38} r={8} />{[60, 50, 70].map((w) => <Skel key={w} w={`${w}%`} h={18} />)}</div>
        <div className="card pad-18 stack-10"><Skel w={110} h={16} /><Skel /><Skel /></div>
      </div>
    </div>
  </div>
)

const KINDS: [TimelineFilter, string][] = [['all', 'All'], ['names', 'Names'], ['fonts', 'Fonts'], ['overrides', 'Overrides'], ['failures', 'Failures']]

export function UserDetail() {
  const id = useParams().id!
  const state = useLoad(() => api.install(id), [id], 10_000)
  return (
    <div className="page">
      <Loaded state={state} skeleton={detailSkeleton}>{(u) => <Detail u={u} reload={() => state.reload(true)} />}</Loaded>
      {state.data === undefined && <Link to="/users" className="back">← Users</Link>}
    </div>
  )
}


function Detail({ u, reload }: { u: InstallDetail; reload: () => void }) {
  const [busy, setBusy] = useState<'' | 'reset' | 'organize'>('')
  const [done, setDone] = useState<Record<string, string>>({})
  const [error, setError] = useState('')
  const who = firstName(u.name)
  const history = [...u.name_history].sort((a, b) => b.from.localeCompare(a.from))
  const previous = history[1]

  const act = async (key: 'reset' | 'organize', run: () => Promise<unknown>, doneText: string) => {
    setBusy(key)
    setError('')
    try {
      await run()
      setDone((d) => ({ ...d, [key]: doneText }))
      toast(key === 'reset' ? `Reset ${who}’s name limit.` : `Asked ${who} to auto-organize their fonts.`)
      reload()
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy('')
    }
  }

  return (
    <>
      <PageHeader back align="start" title={u.name}
        sub={<>
          {previous && <>Previously “{previous.name}” until {fmtDate(previous.to ?? previous.from)} · </>}
          {u.next_name_change ? `next name change allowed ${fmtDate(u.next_name_change)}` : 'can change name any time'}
        </>}
        actions={
          <div className="actions">
            <button type="button" className="btn" disabled={busy !== '' || !u.next_name_change}
              onClick={() => act('reset', () => api.resetNameLimit(u.id), 'Name limit reset')}>
              {busy === 'reset' && <Spin />}<Swap>{done.reset ?? (busy === 'reset' ? 'Resetting…' : 'Reset name limit')}</Swap>
            </button>
            <button type="button" className="btn" disabled={busy !== '' || !!done.organize}
              onClick={() => act('organize', () => api.requestOrganize(u.id), 'Requested · waiting for user')}>
              {busy === 'organize' && <Spin />}<Swap>{done.organize ?? (busy === 'organize' ? 'Sending…' : 'Request auto-organize')}</Swap>
            </button>
            <Link className="btn" to={`/users?select=${encodeURIComponent(u.id)}`}>Merge with another install</Link>
            <ExportCsv u={u} />
            <RemoveComputer u={u} />
          </div>
        }>
          <span className="pills">
            <Badge tone={u.online ? 'ok' : 'outline'}>{u.online ? '● Online' : `Last seen ${ago(u.last_seen).toLowerCase()}`}</Badge>
            <Badge tone="outline">{u.os}</Badge>
            <Badge tone="outline">Glyphyard {u.app_version}</Badge>
            <Badge tone="outline" className="mono">{u.id}</Badge>
            <Badge tone="outline">First seen {fmtDate(u.first_seen)}</Badge>
          </span>
        </PageHeader>
      {error && <ErrorBox message={error} />}

      <div className="split">
        <Timeline id={u.id} />
        <div className="side">
          {u.overrides.length > 0 && <Overrides u={u} who={who} />}
          <InstallFonts u={u} who={who} />
          <motion.section aria-labelledby="nh" className="card pad-18 stack-8" variants={fadeRise}>
            <h2 id="nh" className="h2 h2-sm mb-4">Name history</h2>
            {history.map((n) => (
              <div key={n.from} className="kv">
                <span>{n.name}</span><span className="mute">{fmtDate(n.from)} – {n.to ? fmtDate(n.to) : 'now'}</span>
              </div>
            ))}
          </motion.section>
          {u.merged_from.length > 0 && <Merged ids={u.merged_from} reload={reload} />}
        </div>
      </div>
    </>
  )
}

function Timeline({ id }: { id: string }) {
  const [kind, setKind] = useState<TimelineFilter>('all')
  const [page, setPage] = usePage(kind)
  const state = useLoad(() => api.timeline(id, kind, page), [id, kind, page], 10_000)
  return (
    <motion.section aria-labelledby="tl" className="card pad-20 grow-3 stack-12" variants={fadeRise}>
      <div className="page-head">
        <h2 id="tl" className="h2">Timeline</h2>
        <Chips small label="Filter timeline" options={KINDS} value={kind} onChange={setKind} />
      </div>
      <Loaded state={state} skeleton={eventsSkeleton}>{({ items: events, total }) => (
        <div className="stack-12">
          {events.length === 0 && <Empty>No events{kind === 'all' ? ' yet' : ' of this kind'}.</Empty>}
          {/* Filter changes fade rows out/in and slide the rest; polled events drop in at the top. */}
          <div>
          <AnimatePresence>
          {events.map((e, i) => (
            <motion.div key={e.id} className={e.kind === 'override' ? 'event event-override' : 'event'} {...row(i, events.length, dropIn)}>
              <span className="event-when">{fmtShort(e.at)}</span>
              <span className="event-text">
                <span>{e.text}</span>
                {e.kind === 'override' && <span className="small ink-accent">Override: this font was turned on by an admin</span>}
              </span>
              <Badge tone={byTone[e.by]} className="badge-who">{byLabel[e.by]}</Badge>
            </motion.div>
          ))}
          </AnimatePresence>
          </div>
          <Pager label="Timeline pages" page={page} total={total} onPage={setPage} />
        </div>
      )}</Loaded>
    </motion.section>
  )
}

function Overrides({ u, who }: { u: InstallDetail; who: string }) {
  const { set, error, sent } = useFontToggle(u.id)
  return (
    <motion.section aria-labelledby="ov" className="card card-accent pad-18 stack-10" variants={fadeRise}>
      <h2 id="ov" className="h2 h2-sm ink-accent">User overrides</h2>
      {u.overrides.map((o) => {
        const done = sent[o.font.hash]
        return (
          <motion.div key={o.font.hash} className="override" variants={fadeRise}>
            <span className="stack-2">
              <span className="font-name" style={{ fontFamily: `'${o.font.family}', var(--font-ui)` }}>{o.font.family} {o.font.style}</span>
              <span className="meta mute">
                <Swap>{done ? `Sent · ${who} will see it turned on again` : `You turned it on ${fmtDate(o.admin_at)} · ${who} turned it off ${fmtDate(o.user_at)}`}</Swap>
              </span>
            </span>
            <motion.button layout type="button" className="btn btn-primary btn-sm" disabled={done} onClick={() => set(o.font.hash, true)}>
              <Swap>{done ? 'Sent' : 'Turn on again'}</Swap>
            </motion.button>
          </motion.div>
        )
      })}
      {error && <span role="alert" className="form-error">{error}</span>}
    </motion.section>
  )
}

function Merged({ ids, reload }: { ids: string[]; reload: () => void }) {
  const [error, setError] = useState('')
  const split = async (id: string) => {
    if (!confirm(`Split install ${id} back out into its own person?`)) return
    setError('')
    try { await api.split(id); reload() } catch (e) { setError(errorText(e)) }
  }
  return (
    <motion.section aria-labelledby="mg" className="card pad-18 stack-8" variants={fadeRise}>
      <h2 id="mg" className="h2 h2-sm mb-4">Merged installs</h2>
      {ids.map((id) => (
        <div key={id} className="kv">
          <span className="mono">{id}</span>
          <button type="button" className="btn btn-sm" onClick={() => split(id)}>Split</button>
        </div>
      ))}
      <span className="meta">You can split a merge within 30 days.</span>
      {error && <span role="alert" className="form-error">{error}</span>}
    </motion.section>
  )
}

function RemoveComputer({ u }: { u: InstallDetail }) {
  const navigate = useNavigate()
  const [confirmDialog, confirm] = useConfirm()
  const [busy, setBusy] = useState(false)
  const run = async () => {
    const merged = u.merged_from.length ? ` and the ${plural(u.merged_from.length, 'computer')} merged into it` : ''
    const text = `Remove ${u.name}’s computer${merged}? It is disconnected at once and disappears from your lists. Its name history stays in the export. If Glyphyard is set up on it again, it shows up as a new user.`
    if (!(await confirm(text, 'Remove', true))) return
    setBusy(true)
    try {
      await api.removeInstall(u.id)
      toast(`Removed ${u.name}’s computer.`)
      navigate('/users')
    } catch (e) {
      toast(errorText(e), { tone: 'bad' })
      setBusy(false)
    }
  }
  return <>
    <button type="button" className="btn btn-danger" disabled={busy} onClick={run}>{busy && <Spin />}{busy ? 'Removing…' : 'Remove this computer'}</button>
    {confirmDialog}
  </>
}

function ExportCsv({ u }: { u: InstallDetail }) {
  const [busy, setBusy] = useState(false)
  const run = async () => {
    setBusy(true)
    try {
      // The export is complete: walk every page at the server's maximum page size.
      const events: TimelineEvent[] = []
      for (let page = 1; ; page++) {
        const p = await api.timeline(u.id, 'all', page, 500)
        events.push(...p.items)
        if (p.items.length === 0 || events.length >= p.total) break
      }
      downloadCsv(`${u.name} timeline.csv`, [['Date', 'By', 'Kind', 'Event'], ...events.map((e) => [e.at, e.by, e.kind, e.text])])
    } catch (e) {
      toast(errorText(e), { tone: 'bad' })
    } finally {
      setBusy(false)
    }
  }
  return <button type="button" className="btn" disabled={busy} onClick={run}>{busy && <Spin />}{busy ? 'Exporting…' : 'Export CSV'}</button>
}
