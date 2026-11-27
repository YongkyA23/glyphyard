import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, errorText, type Computer, type InstallDetail, type TimelineEvent, type TimelineFilter } from '../api'
import { Hint } from '../Hint'
import { t, tk } from '../i18n'
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

const KINDS: [TimelineFilter, string][] = [['all', tk('All')], ['names', tk('Names')], ['fonts', tk('Fonts')], ['overrides', tk('Overrides')], ['failures', tk('Failures')]]

export function UserDetail() {
  const id = useParams().id!
  const state = useLoad(() => api.install(id), [id], 10_000)
  return (
    <div className="page">
      <Loaded state={state} skeleton={detailSkeleton}>{(u) => <Detail u={u} reload={() => state.reload(true)} />}</Loaded>
      {state.data === undefined && <Link to="/users" className="back">{t('← Users')}</Link>}
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
      toast(key === 'reset' ? t('Reset {name}’s name limit.', { name: who }) : t('Asked {name} to auto-organize their fonts.', { name: who }))
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
          {previous && <>{t('Previously “{name}” until {date}', { name: previous.name, date: fmtDate(previous.to ?? previous.from) })} · </>}
          {u.next_name_change ? t('next name change allowed {date}', { date: fmtDate(u.next_name_change) }) : t('can change name any time')}
        </>}
        actions={
          <div className="actions">
            <button type="button" className="btn" disabled={busy !== '' || !u.next_name_change}
              onClick={() => act('reset', () => api.resetNameLimit(u.id), t('Name limit reset'))}>
              {busy === 'reset' && <Spin />}<Swap>{done.reset ?? (busy === 'reset' ? t('Resetting…') : t('Reset name limit'))}</Swap>
            </button>
            <button type="button" className="btn" disabled={busy !== '' || !!done.organize}
              onClick={() => act('organize', () => api.requestOrganize(u.id), t('Requested · waiting for user'))}>
              {busy === 'organize' && <Spin />}<Swap>{done.organize ?? (busy === 'organize' ? t('Sending…') : t('Request auto-organize'))}</Swap>
            </button>
            <Link className="btn" to={`/users?select=${encodeURIComponent(u.id)}`}>{t('Merge with another install')}</Link>
            <ExportCsv u={u} />
            <RemovePerson u={u} />
          </div>
        }>
          <span className="pills">
            <Badge tone={u.online ? 'ok' : 'outline'}>{u.online ? t('● Online') : t('Last seen {when}', { when: ago(u.last_seen).toLowerCase() })}</Badge>
            <Badge tone="outline">{u.os}</Badge>
            <Badge tone="outline">Glyphyard {u.app_version}</Badge>
            <Badge tone="outline" className="mono">{u.id}</Badge>
            <Badge tone="outline">{t('First seen {date}', { date: fmtDate(u.first_seen) })}</Badge>
          </span>
        </PageHeader>
      {error && <ErrorBox message={error} />}

      <div className="split">
        <Timeline id={u.id} />
        <div className="side">
          {u.overrides.length > 0 && <Overrides u={u} who={who} />}
          <InstallFonts u={u} who={who} />
          <motion.section aria-labelledby="nh" className="card pad-18 stack-8" variants={fadeRise}>
            <h2 id="nh" className="h2 h2-sm mb-4">{t('Name history')}</h2>
            {history.map((n) => (
              <div key={n.from} className="kv">
                <span>{n.name}</span><span className="mute">{fmtDate(n.from)} – {n.to ? fmtDate(n.to) : t('now')}</span>
              </div>
            ))}
          </motion.section>
          <Computers u={u} reload={reload} />
        </div>
      </div>
    </>
  )
}

function Timeline({ id }: { id: string }) {
  const [kind, setKind] = useState<TimelineFilter>('all')
  const [page, setPage] = usePage(kind)
  const state = useLoad(() => api.timeline(id, kind, page), [id, kind, page], 10_000)
  const kinds = KINDS.map(([k, label]): [TimelineFilter, string] => [k, t(label)]) // i18n-dynamic
  return (
    <motion.section aria-labelledby="tl" className="card pad-20 grow-3 stack-12" variants={fadeRise}>
      <div className="page-head">
        <h2 id="tl" className="h2">{t('Timeline')}</h2>
        <Chips small label={t('Filter timeline')} options={kinds} value={kind} onChange={setKind} />
      </div>
      <Loaded state={state} skeleton={eventsSkeleton}>{({ items: events, total }) => (
        <div className="stack-12">
          {events.length === 0 && <Empty>{kind === 'all' ? t('No events yet.') : t('No events of this kind.')}</Empty>}
          {/* Filter changes fade rows out/in and slide the rest; polled events drop in at the top. */}
          <div>
          <AnimatePresence>
          {events.map((e, i) => (
            <motion.div key={e.id} className={e.kind === 'override' ? 'event event-override' : 'event'} {...row(i, events.length, dropIn)}>
              <span className="event-when">{fmtShort(e.at)}</span>
              <span className="event-text">
                <span>{e.text}</span>
                {e.kind === 'override' && <span className="small ink-accent">{t('Override: this font was turned on by an admin')}</span>}
              </span>
              <Badge tone={byTone[e.by]} className="badge-who">{t(byLabel[e.by])}{/* i18n-dynamic */}</Badge>
            </motion.div>
          ))}
          </AnimatePresence>
          </div>
          <Pager label={t('Timeline pages')} page={page} total={total} onPage={setPage} />
        </div>
      )}</Loaded>
    </motion.section>
  )
}

function Overrides({ u, who }: { u: InstallDetail; who: string }) {
  const { set, error, sent } = useFontToggle(u.id)
  return (
    <motion.section aria-labelledby="ov" className="card card-accent pad-18 stack-10" variants={fadeRise}>
      <h2 id="ov" className="h2 h2-sm ink-accent">{t('User overrides')}</h2>
      {u.overrides.map((o) => {
        const done = sent[o.font.hash]
        return (
          <motion.div key={o.font.hash} className="override" variants={fadeRise}>
            <span className="stack-2">
              <span className="font-name" style={{ fontFamily: `'${o.font.family}', var(--font-ui)` }}>{o.font.family} {o.font.style}</span>
              <span className="meta mute">
                <Swap>{done ? t('Sent · {name} will see it turned on again', { name: who }) : t('You turned it on {admin} · {name} turned it off {user}', { admin: fmtDate(o.admin_at), name: who, user: fmtDate(o.user_at) })}</Swap>
              </span>
            </span>
            <motion.button layout type="button" className="btn btn-primary btn-sm" disabled={done} onClick={() => set(o.font.hash, true)}>
              <Swap>{done ? t('Sent') : t('Turn on again')}</Swap>
            </motion.button>
          </motion.div>
        )
      })}
      {error && <span role="alert" className="form-error">{error}</span>}
    </motion.section>
  )
}

/** A merge can be split within 30 days (the server refuses later ones). */
const canSplit = (c: Computer) => c.merged_at !== null && Date.now() - Date.parse(c.merged_at) < 30 * 86_400_000

/** The person's computers: split a recent merge, or remove one computer while the person stays. */
function Computers({ u, reload }: { u: InstallDetail; reload: () => void }) {
  const navigate = useNavigate()
  const [confirmDialog, confirm] = useConfirm()
  const [busy, setBusy] = useState('')
  const [error, setError] = useState('')
  const several = u.computers.length > 1

  const run = async (c: Computer, work: () => Promise<void>) => {
    setBusy(c.id)
    setError('')
    try { await work() } catch (e) { setError(errorText(e)) } finally { setBusy('') }
  }
  const split = async (c: Computer) => {
    if (!(await confirm(t('Split computer {id} back out into its own user? It gets its own name back.', { id: c.id }), t('Split')))) return
    await run(c, async () => { await api.split(c.id); toast(t('Split computer {id} back out.', { id: c.id })); reload() })
  }
  const remove = async (c: Computer) => {
    const text = c.id === u.id
      ? t('Remove computer {id}? It is disconnected at once. {name} keeps their other computers and their name, and from now on is listed under the next one. Its name history stays in the export.', { id: c.id, name: u.name })
      : t('Remove computer {id} from {name}? It is disconnected at once and disappears from your lists. {name} keeps their other computers. Its name history stays in the export.', { id: c.id, name: u.name })
    if (!(await confirm(text, t('Remove'), true))) return
    await run(c, async () => {
      const { person } = await api.removeComputer(u.id, c.id)
      toast(t('Removed a computer from {name}.', { name: u.name }))
      if (person === u.id) reload()
      else navigate(`/users/${encodeURIComponent(person)}`, { replace: true })
    })
  }

  return (
    <motion.section aria-labelledby="pc" className="card pad-18 stack-8" variants={fadeRise}>
      <div className="title-row mb-4"><h2 id="pc" className="h2 h2-sm">{t('Computers')}</h2><Hint text={t('Every computer of this user shares one name. You can split a merge within 30 days.')} /></div>
      {u.computers.map((c) => (
        <div key={c.id} className="kv computer">
          <span className="stack-2">
            <span className="row-6"><span aria-hidden="true" className={c.online ? 'dot dot-on' : 'dot'} />{c.os} · Glyphyard {c.app_version}</span>
            <span className="meta"><span className="mono">{c.id}</span> · {c.online ? t('Online') : t('Last seen {when}', { when: ago(c.last_seen).toLowerCase() })}</span>
          </span>
          <span className="row-6">
            {canSplit(c) && (
              <button type="button" className="btn btn-sm" disabled={busy !== ''} onClick={() => split(c)}>{t('Split')}</button>
            )}
            {several && (
              <button type="button" className="btn btn-sm btn-danger" disabled={busy !== ''} onClick={() => remove(c)}>
                {busy === c.id && <Spin />}{t('Remove this computer')}
              </button>
            )}
          </span>
        </div>
      ))}
      {error && <span role="alert" className="form-error">{error}</span>}
      {confirmDialog}
    </motion.section>
  )
}

/** Removes the whole person: every one of their computers. */
function RemovePerson({ u }: { u: InstallDetail }) {
  const navigate = useNavigate()
  const [confirmDialog, confirm] = useConfirm()
  const [busy, setBusy] = useState(false)
  const several = u.computers.length > 1
  const run = async () => {
    const text = several
      ? t('Remove {name} and all {computers}? They are disconnected at once and disappear from your lists. The name history stays in the export. If Glyphyard is set up on one of them again, it shows up as a new user.', { name: u.name, computers: plural(u.computers.length, 'computer') })
      : t('Remove {name}’s computer? It is disconnected at once and disappears from your lists. Its name history stays in the export. If Glyphyard is set up on it again, it shows up as a new user.', { name: u.name })
    if (!(await confirm(text, t('Remove'), true))) return
    setBusy(true)
    try {
      await api.removeInstall(u.id)
      toast(t('Removed {name}.', { name: u.name }))
      navigate('/users')
    } catch (e) {
      toast(errorText(e), { tone: 'bad' })
      setBusy(false)
    }
  }
  return <>
    <button type="button" className="btn btn-danger" disabled={busy} onClick={run}>
      {busy && <Spin />}{busy ? t('Removing…') : several ? t('Remove user and all computers') : t('Remove user')}
    </button>
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
  return <button type="button" className="btn" disabled={busy} onClick={run}>{busy && <Spin />}{busy ? t('Exporting…') : t('Export CSV')}</button>
}
