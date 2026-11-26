import { Link } from 'react-router'
import { useState } from 'react'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { api, type Overview as Data, type TrendDay } from '../api'
import '../trend.css'
import { dropIn, easeOut, fadeRise, lift, row, stagger } from '../motion'
import { ago, Badge, byLabel, byTone, CountUp, Empty, fmtTime, Loaded, plural, Skel, useLoad } from '../ui'

const MotionLink = motion.create(Link)

const skeleton = (
  <div className="stack-18">
    <div className="stats">
      {Array.from({ length: 5 }, (_, i) => (
        <div key={i} className="card stat"><Skel w="55%" /><Skel w={56} h={30} r={8} /><Skel w="75%" h={10} /></div>
      ))}
    </div>
    <div className="card pad-20 stack-14"><Skel w={120} h={16} /><Skel h={128} r={8} /></div>
    <div className="split">
      <div className="card pad-20 grow-2 stack-18"><Skel w={140} h={16} />{[80, 65, 72, 58].map((w) => <Skel key={w} w={`${w}%`} />)}</div>
      <div className="card pad-20 grow-1 stack-14"><Skel w={130} h={16} /><Skel h={52} r={10} /><Skel h={52} r={10} /><Skel w={120} h={16} /><Skel h={6} /></div>
    </div>
  </div>
)
export function Overview() {
  const state = useLoad(() => api.overview(), [], 10_000)
  return (
    <div className="page">
      <div className="page-head" style={{ alignItems: 'baseline' }}>
        <h1>Overview</h1>
        {state.at && <span className="sub small fade-in">Updated live · {fmtTime(state.at)}</span>}
      </div>
      <Loaded state={state} skeleton={skeleton}>{(o) => <Body o={o} />}</Loaded>
    </div>
  )
}

function Body({ o }: { o: Data }) {
  const stats = [
    { label: 'Installs', value: o.installs, note: `${o.os.windows} Windows · ${o.os.macos} macOS`, to: '/users' },
    { label: 'Online now', value: o.online, note: 'Seen in the last 5 minutes', to: '/users?filter=online', tone: 'ok' },
    { label: 'Name changes', value: o.name_changes_month, note: 'This month', to: '/names' },
    { label: 'User overrides', value: o.overrides_week, note: 'Admin fonts turned off this week', to: '/users?filter=overrides', tone: 'accent' },
    { label: 'Activation failures', value: o.failures_week, note: 'Last 7 days', to: '/users', tone: 'bad' },
  ]
  const a = o.attention
  const attention = [
    { n: a.overrides, title: plural(a.overrides, 'user override'), detail: 'Fonts you turned on were turned off by users', to: '/users?filter=overrides', tone: 'admin' },
    { n: a.failures, title: plural(a.failures, 'activation failure'), detail: 'Fonts that couldn’t be turned on', to: '/users', tone: 'bad' },
    { n: a.possible_duplicates, title: plural(a.possible_duplicates, 'possible duplicate install'), detail: 'Same name on a new computer · merge?', to: '/users?filter=duplicates', tone: 'warn' },
    { n: a.requests, title: plural(a.requests, 'open font request'), detail: 'Fonts people asked you for', to: '/requests', tone: 'ok' },
    { n: a.stale, title:`${plural(a.stale, 'install')} offline for 14+ days`, detail: 'Changes will wait until they reconnect', to: '/users?filter=stale', tone: 'user' },
  ].filter((x) => x.n > 0)
  const max = Math.max(o.installs, ...o.top_fonts.map((t) => t.users), 1)

  return (
    <>
      <motion.div className="stats" variants={stagger(0.05)}>
        {stats.map((s) => (
          <MotionLink key={s.label} to={s.to} className="card stat" variants={fadeRise} {...lift}>
            <span className="stat-label">{s.label}</span>
            <span className={`stat-value ${s.tone ? 'ink-' + s.tone : ''}`}><CountUp value={s.value} /></span>
            <span className="stat-note">{s.note}</span>
          </MotionLink>
        ))}
      </motion.div>
      <motion.section aria-labelledby="trend" className="card pad-20" variants={fadeRise}>
        <h2 id="trend" className="h2 mb-10">Last 30 days</h2>
        <TrendChart days={o.trend} />
      </motion.section>
      <div className="split">
        <motion.section aria-labelledby="act" className="card pad-20 grow-2" variants={fadeRise}>
          <h2 id="act" className="h2 mb-10">Recent activity</h2>
          {o.recent.length === 0 && <Empty><span className="strong">No activity yet</span><br />Events appear here as desktop apps report in.</Empty>}
          {/* New events from polling mount with "hidden" → slide down in at the top; layout pushes the rest down. */}
          <AnimatePresence>
          {o.recent.map((r, i) => (
            <motion.div key={r.id} className="activity" {...row(i, o.recent.length, dropIn)}>
              <Badge tone={byTone[r.by]} className="badge-who">{byLabel[r.by]}</Badge>
              <span className="activity-text">
                <span>{r.text}</span>
                <span className="meta">{ago(r.at)}</span>
              </span>
              <Link to={`/users/${encodeURIComponent(r.install_id)}`} className="small nowrap" aria-label={`View ${r.install_name}`}>View</Link>
            </motion.div>
          ))}
          </AnimatePresence>
        </motion.section>
        <motion.section aria-labelledby="att" className="card pad-20 grow-1 stack-10" variants={fadeRise}>
          <h2 id="att" className="h2 mb-6">Needs attention</h2>
          {attention.length === 0 && <Empty>Nothing needs attention.</Empty>}
          {attention.map((x) => (
            <MotionLink key={x.tone} to={x.to} className={`attention tone-${x.tone}`} {...lift}>
              <span className="strong">{x.title}</span>
              <span className="small">{x.detail}</span>
            </MotionLink>
          ))}
          <h2 className="h2 mt-12">Most used fonts</h2>
          {o.top_fonts.length === 0 && <Empty>No active fonts reported yet.</Empty>}
          {o.top_fonts.map((t) => (
            <div key={t.family} className="stack-4">
              <span className="bar-label"><span>{t.family}</span><span className="mute">{plural(t.users, 'user')}</span></span>
              <span aria-hidden="true" className="bar">
                {/* Full-width fill scaled to the value: grows from 0 on load, tweens on each poll. */}
                <motion.span initial={{ scaleX: 0 }} animate={{ scaleX: t.users / max }} transition={{ duration: 0.7, ease: easeOut, delay: 0.2 }} />
              </span>
            </div>
          ))}
        </motion.section>
      </div>
    </>
  )
}

const dayLabel = (d: string) => new Date(`${d}T00:00:00Z`).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', timeZone: 'UTC' })
const dayText = (d: TrendDay) =>
  `${dayLabel(d.day)}: ${plural(d.activity, 'event')}, ${plural(d.failures, 'failure')}, ${plural(d.new_installs, 'new computer')}`

function TrendChart({ days }: { days: TrendDay[] }) {
  const reduce = useReducedMotion()
  const [on, setOn] = useState<number | null>(null)
  const max = Math.max(...days.map((d) => d.activity), 1)
  const sum = (k: 'activity' | 'failures' | 'new_installs') => days.reduce((n, d) => n + d[k], 0)
  const mid = days[Math.floor(days.length / 2)]
  return (
    <div onMouseLeave={() => setOn(null)}>
      <div className="trend-plot" role="group" aria-label="Daily activity for the last 30 days">
        {days.map((d, i) => {
          const h = (d.activity / max) * 100
          const f = (Math.min(d.failures, d.activity) / max) * 100
          const t = reduce ? { duration: 0 } : { duration: 0.6, ease: easeOut, delay: i * 0.012 }
          return (
            <div key={d.day} className={`trend-col ${on === i ? 'on' : ''}`} tabIndex={0} role="img" aria-label={dayText(d)}
              onMouseEnter={() => setOn(i)} onFocus={() => setOn(i)} onBlur={() => setOn(null)}>
              <span className={`trend-dot ${d.new_installs ? '' : 'none'}`} aria-hidden="true" />
              <svg className="trend-bar" viewBox="0 0 10 100" preserveAspectRatio="none">
                <title>{dayText(d)}</title>
                {/* Keys are dates, so polls tween heights instead of remounting. */}
                <motion.rect x={0} width={10} fill="var(--accent)" initial={reduce ? false : { y: 100, height: 0 }} animate={{ y: 100 - h, height: h }} transition={t} />
                <motion.rect x={0} width={10} fill="var(--bad)" initial={reduce ? false : { y: 100, height: 0 }} animate={{ y: 100 - f, height: f }} transition={t} />
              </svg>
            </div>
          )
        })}
      </div>
      <div className="trend-axis" aria-hidden="true"><span>{dayLabel(days[0].day)}</span><span>{dayLabel(mid.day)}</span><span>Today</span></div>
      <p className="trend-tip" aria-hidden="true">{on === null ? '' : dayText(days[on])}</p>
      <div className="trend-legend">
        <span><i />{plural(sum('activity'), 'event')}</span>
        <span><i className="bad" />{plural(sum('failures'), 'failure')}</span>
        <span><i className="ok" />{plural(sum('new_installs'), 'new computer')}</span>
      </div>
    </div>
  )
}
