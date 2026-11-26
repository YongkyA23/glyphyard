import { useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, errorText, type InstallDetail } from '../api'
import { fadeRise, pop, press, softSpring } from '../motion'
import { toast } from '../toast'
import { ago, ErrorBox, fmtDate, Loaded, PageHeader, plural, Skel, Spin, Swap, useLoad } from '../ui'

const MotionLink = motion.create(Link)
// The two installs arrive from opposite sides.
const fromSide = (dx: number) => ({ hidden: { opacity: 0, x: dx }, show: { opacity: 1, x: 0, transition: softSpring } })
const skeletonCard = (
  <div className="card merge-card"><Skel w="50%" /><Skel w={180} h={22} r={6} /><Skel w={120} /><Skel w="70%" /><Skel w="60%" /></div>
)
const skeleton = (
  <div className="stack-18">
    <div className="merge-cards">{skeletonCard}{skeletonCard}</div>
    <div className="card pad-18 stack-10"><Skel w={240} h={16} /><Skel w="40%" /><Skel w="40%" /></div>
  </div>
)

export function Merge() {
  const [params] = useSearchParams()
  const a = params.get('primary') ?? ''
  const b = params.get('secondary') ?? ''
  const state = useLoad(async () => {
    if (!a || !b || a === b) throw new Error('Pick two different installs on the Users page to merge them.')
    const pair = await Promise.all([api.install(a), api.install(b)])
    // The older install always stays the main record.
    return pair.sort((x, y) => x.first_seen.localeCompare(y.first_seen)) as [InstallDetail, InstallDetail]
  }, [a, b])
  return (
    <div className="page">
      <PageHeader back title="Merge two installs into one person"
        sub="Use this when someone reinstalls or moves to a new computer. Their history and name limit carry over." />
      <Loaded state={state} skeleton={skeleton}>{([older, newer]) => <Form older={older} newer={newer} />}</Loaded>
    </div>
  )
}

function Form({ older, newer }: { older: InstallDetail; newer: InstallDetail }) {
  const [keep, setKeep] = useState<'primary' | 'secondary'>('primary')
  const [status, setStatus] = useState<'idle' | 'busy' | 'done'>('idle')
  const [error, setError] = useState('')
  const keepName = keep === 'primary' ? older.name : newer.name

  const merge = async () => {
    setStatus('busy')
    setError('')
    try {
      await api.merge(older.id, newer.id, keep)
      setStatus('done')
      toast(`Merged into ${keepName}.`)
    } catch (e) {
      setError(errorText(e))
      setStatus('idle')
    }
  }

  const card = (u: InstallDetail, main: boolean) => (
    <motion.div className={main ? 'card merge-card merge-main' : 'card merge-card'} variants={fromSide(main ? -28 : 28)}>
      <span className="small strong mute">{main ? 'Older install · stays the main record' : 'Newer install · will be retired'}</span>
      <span className="merge-name">{u.name}</span>
      <span className="mono ink-2">{u.id}</span>
      <span className="stack-4 small ink-2">
        <span>{u.os}</span>
        <span>First seen {fmtDate(u.first_seen)} · {u.online ? 'online now' : ago(u.last_seen).toLowerCase()}</span>
        <span>{plural(u.active_fonts, 'active font')}</span>
        <span>{plural(u.name_history.length, 'name')} used</span>
      </span>
    </motion.div>
  )

  const after = [
    `One person named “${keepName}” with both computers listed`,
    `Name history combined: ${plural(older.name_history.length + newer.name_history.length, 'entry', 'entries')}, nothing deleted`,
    `Name limit carries over: ${older.next_name_change ? `next change allowed ${fmtDate(older.next_name_change)}` : 'name can be changed any time'}`,
    'Fonts you deliver to this person from now on reach both computers',
  ]

  return (
    <>
      <motion.div className="merge-cards" variants={{ show: { transition: { staggerChildren: 0.06 } } }}>{card(older, true)}{card(newer, false)}</motion.div>
      <motion.fieldset className="card pad-18 stack-10 fieldset" variants={fadeRise}>
        <legend>Which name should this person keep?</legend>
        {([['primary', older, 'older'], ['secondary', newer, 'newer']] as const).map(([k, u, age]) => (
          <label key={k} className="radio">
            <input type="radio" name="keep" checked={keep === k} onChange={() => setKeep(k)} disabled={status !== 'idle'} />
            <span>{u.name} (from the {age} install)</span>
          </label>
        ))}
      </motion.fieldset>
      <motion.section aria-labelledby="after" className="card pad-18 stack-8" variants={fadeRise}>
        <h2 id="after" className="h2 h2-sm mb-4">After merging</h2>
        {/* Picking the other name cross-fades the line that mentions it. */}
        {after.map((t, i) => (
          <span key={i} className="check-line"><span aria-hidden="true" className="ink-ok">✓</span><span><Swap>{t}</Swap></span></span>
        ))}
      </motion.section>
      {error && <ErrorBox message={error} />}
      <div className="page-head">
        <span className="small ink-warn" role="status">
          <Swap>{status === 'done' ? 'Merged. You can split them again from this person’s page within 30 days.' : 'Both computers keep working while you merge.'}</Swap>
        </span>
        <AnimatePresence mode="wait" initial={false}>
          {status === 'done' ? (
            <motion.span key="done" className="row-8" {...pop}>
              <span className="btn btn-lg btn-done">Merged ✓</span>
              <MotionLink className="btn btn-lg btn-primary" to={`/users/${encodeURIComponent(older.id)}`} {...press}>Open {keepName}</MotionLink>
            </motion.span>
          ) : (
            <motion.span key="ask" className="row-8" exit={{ opacity: 0, scale: 0.96, transition: { duration: 0.12 } }}>
              <Link className="btn btn-lg" to="/users">Cancel</Link>
              <button type="button" className="btn btn-lg btn-primary" disabled={status === 'busy'} onClick={merge}>
                {status === 'busy' && <Spin />}{status === 'busy' ? 'Merging…' : 'Merge installs'}
              </button>
            </motion.span>
          )}
        </AnimatePresence>
      </div>
    </>
  )
}
