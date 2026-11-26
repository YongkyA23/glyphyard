import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, errorText, type FontState, type InstallDetail, type TimelineEvent, type TimelineFilter } from '../api'
import { dropIn, fadeRise, row } from '../motion'
import { toast } from '../toast'
import {
  ago, Badge, byLabel, byTone, Chips, downloadCsv, Empty, ErrorBox, fmtDate, fmtShort, firstName, Loaded, PageHeader, Pager, plural, Skel, Spin, Swap, Toggle,
  useConfirm, useDebounced, useLoad, usePage,
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
const fontsSkeleton = <div className="stack-10">{[60, 45, 70].map((w) => <Skel key={w} w={`${w}%`} h={18} />)}</div>

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
          <Fonts u={u} who={who} />
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

/** A font toggle is only a request: the desktop applies it later. `pending` holds the requested state per font until a poll confirms it or PENDING_MS passes. */
const PENDING_MS = 120_000

function useFontToggle(installId: string) {
  const [error, setError] = useState('')
  const [sent, setSent] = useState<Record<string, boolean>>({})
  const [shakes, setShakes] = useState<Record<string, number>>({})
  const [pending, setPending] = useState<Record<string, boolean>>({})
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>())
  const drop = useCallback((hash: string) => {
    clearTimeout(timers.current.get(hash))
    timers.current.delete(hash)
    setPending((p) => { if (!(hash in p)) return p; const { [hash]: _, ...rest } = p; return rest })
  }, [])
  useEffect(() => { const t = timers.current; return () => { t.forEach(clearTimeout); t.clear() } }, [])
  /** `announce`: toast text for the request, with an Undo that sends the opposite request. */
  const set = async (hash: string, active: boolean, announce?: string) => {
    if (timers.current.has(hash)) return
    setError('')
    setPending((p) => ({ ...p, [hash]: active }))
    timers.current.set(hash, setTimeout(() => drop(hash), PENDING_MS))
    try {
      await api.setFont(installId, hash, active)
      setSent((s) => ({ ...s, [hash]: true }))
      if (announce) toast(announce, { undo: () => { drop(hash); return set(hash, !active) } })
    } catch (e) {
      drop(hash)
      setShakes((s) => ({ ...s, [hash]: (s[hash] ?? 0) + 1 }))
      setError(errorText(e))
    }
  }
  return { set, error, sent, shakes, pending, drop }
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

const sourceText = (f: FontState, who: string) =>
  f.overridden ? `Turned on by admin · turned off by ${who}`
    : f.source === 'admin' ? 'Turned on by admin'
    : f.source === 'project_set' ? 'Project set'
    : f.source === 'library' ? 'Shared library'
    : f.active ? `Turned on by ${who}` : `Turned off by ${who}`

function Fonts({ u, who }: { u: InstallDetail; who: string }) {
  const [q, setQ] = useState('')
  const query = useDebounced(q.trim(), 150)
  const [page, setPage] = usePage(query)
  const state = useLoad(() => api.fonts(u.id, query, page), [u.id, query, page], 10_000)
  const { set, error, shakes, pending, drop } = useFontToggle(u.id)
  // A poll that reports the requested state ends the wait.
  useEffect(() => {
    state.data?.items.forEach((f) => { if (pending[f.hash] === f.active) drop(f.hash) })
  }, [state.data, pending, drop])
  return (
    <motion.section aria-labelledby="af" className="card pad-18 stack-10" variants={fadeRise}>
      <div className="row-between">
        <h2 id="af" className="h2 h2-sm">Active fonts · {u.active_fonts}</h2>
        <Link to="/library" className="small">Turn on fonts…</Link>
      </div>
      <label htmlFor="fsearch" className="sr-only">Search this user's fonts</label>
      <input id="fsearch" type="search" className="input input-38" placeholder={`Search ${who}’s fonts`} value={q} onChange={(e) => setQ(e.target.value)} />
      {error && <span role="alert" className="form-error">{error}</span>}
      <Loaded state={state} skeleton={fontsSkeleton}>{({ items, total }) => {
        // Active first, like the server; toggling re-sorts this page so rows glide to their new place.
        const shown = items.map((f) => (f.hash in pending ? { ...f, active: pending[f.hash] } : f)).sort((a, b) => Number(b.active) - Number(a.active))
        return <>
      {total === 0 && <Empty>{query ? 'No fonts match.' : 'No fonts reported by this computer yet.'}</Empty>}
      <AnimatePresence>
      {shown.map((f, i) => {
        const name = `${f.family} ${f.style}`
        return (
          <motion.div key={f.hash} className="font-row" {...row(i, shown.length)}>
            <span className="font-info">
              <span className="font-name ellipsis" style={{ fontFamily: `'${f.family}', var(--font-ui)` }}>{name}</span>
              <span className="meta">{f.hash in pending ? 'Waiting for this computer' : sourceText(f, who)}</span>
            </span>
            <Toggle on={f.active} label={`${f.active ? 'Turn off' : 'Turn on'} ${name} for ${who}`} onChange={() => set(f.hash, !f.active, `Turning ${name} ${f.active ? 'off' : 'on'} on ${who}’s computer.`)} disabled={f.hash in pending}
              shakeKey={shakes[f.hash]} />
          </motion.div>
        )
      })}
      </AnimatePresence>
      <Pager label="Font pages" page={page} total={total} onPage={setPage} />
        </>
      }}</Loaded>
      <span className="meta">Changes reach {who}’s computer within seconds while online.</span>
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
      alert(errorText(e))
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
      alert(errorText(e))
    } finally {
      setBusy(false)
    }
  }
  return <button type="button" className="btn" disabled={busy} onClick={run}>{busy && <Spin />}{busy ? 'Exporting…' : 'Export CSV'}</button>
}
