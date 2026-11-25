// Usage statistics, badges and the "Year in type" card. Everything here is computed on this computer.
import { useCallback, useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { api, type Badge, type Family, type UsageStats } from '../api'
import { familyStyle } from '../fonts'
import { cssStagger } from '../motion'
import { useApp } from '../state'
import { Chip, errMsg, fmtDate, num, plural, Skel } from '../ui'
import './stats.css'

type Data = { month: UsageStats; year: UsageStats; badges: Badge[]; faces: Map<string, Family> }

export function Stats() {
  const { libVersion } = useApp()
  const navigate = useNavigate()
  const [data, setData] = useState<Data | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [period, setPeriod] = useState<'month' | 'year'>('month')

  const [december] = useState(() => new Date().getMonth() === 11)
  const [yearAgo] = useState(() => Date.now() - 365 * 86_400_000)
  const [attempt, setAttempt] = useState(0)

  useEffect(() => {
    let live = true
    ;(async () => {
      const [month, year, badges] = await Promise.all([api.usageStats('month'), api.usageStats('year'), api.badges()])
      const keys = [...new Set([...month.mostUsed, ...year.mostUsed].map((f) => f.key))]
      const fams = keys.length ? await api.listFamilies({ keys }) : []
      return { month, year, badges, faces: new Map(fams.map((f) => [f.key, f])) }
    })().then(
      (d) => live && (setData(d), setError(null)),
      (e) => live && setError(errMsg(e)),
    )
    return () => void (live = false)
  }, [libVersion, attempt])
  const load = useCallback(() => (setError(null), setData(null), setAttempt((n) => n + 1)), [])

  const header = (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <h1 className="h-page">Stats</h1>
      <span className="muted">How you use your fonts. Counted on this computer only, never sent anywhere.</span>
    </div>
  )
  if (error)
    return (
      <div className="page" style={{ gap: 20 }}>
        {header}
        <p role="alert" style={{ margin: 0, color: 'var(--bad)' }}>Couldn’t load your stats: {error}</p>
        <div><button type="button" className="btn" onClick={load}>Try again</button></div>
      </div>
    )
  if (!data)
    return (
      <div className="page" style={{ gap: 20 }} aria-busy="true">
        {header}
        <div className="stats-grid">
          {[0, 1, 2].map((i) => <div key={i} className="card stats-card"><Skel w="40%" h={16} /><Skel h={12} /><Skel w="70%" h={12} /></div>)}
        </div>
      </div>
    )

  const stats = period === 'month' ? data.month : data.year
  const wrapped = <YearInType year={data.year} faces={data.faces} featured={december} />
  const unused = data.year.neverUsed

  return (
    <div className="page" style={{ gap: 20 }}>
      {header}
      {december && wrapped}
      <div className="stats-grid">
        <section className="card stats-card" aria-labelledby="most-used">
          <div className="row-between">
            <h2 id="most-used" className="stats-h">Most used</h2>
            <div className="chips" role="group" aria-label="Period">
              <Chip pressed={period === 'month'} onClick={() => setPeriod('month')}>This month</Chip>
              <Chip pressed={period === 'year'} onClick={() => setPeriod('year')}>This year</Chip>
            </div>
          </div>
          {stats.mostUsed.length ? (
            <ol className="stats-list">
              {stats.mostUsed.map((f, i) => (
                <li key={f.key} className="enter" style={cssStagger(i)}>
                  <span className="stats-name" style={familyStyle(data.faces.get(f.key))}>{f.family}</span>
                  <span className="faint">{plural(f.days, 'day')}</span>
                </li>
              ))}
            </ol>
          ) : (
            <p className="muted stats-empty">
              Nothing turned on {period === 'month' ? 'this month' : 'this year'} yet. Fonts you turn on show up here.
            </p>
          )}
          <p className="faint stats-foot">
            Ranked by the number of days a family was turned on.
            {stats.recordingSince && ` Counting since ${fmtDate(stats.recordingSince)}.`}
          </p>
        </section>

        <section className="card stats-card" aria-labelledby="never-used">
          <h2 id="never-used" className="stats-h">Never used</h2>
          {data.year.neverUsedNote ? (
            <p className="muted stats-empty">{data.year.neverUsedNote}</p>
          ) : unused.length ? (
            <>
              <span className="muted">
                {plural(unused.length, 'family', 'families')} not turned on{' '}
                {data.year.recordingSince && new Date(data.year.recordingSince).getTime() > yearAgo
                  ? `since ${fmtDate(data.year.recordingSince)}, when Typeshelf started counting.`
                  : 'in the last 12 months.'}
              </span>
              <p className="stats-names">
                {unused.slice(0, 24).map((f) => f.family).join(', ')}
                {unused.length > 24 && ` and ${num(unused.length - 24)} more`}
              </p>
              <div>
                <button type="button" className="btn" onClick={() => navigate('/', { state: { select: unused.map((f) => f.key) } })}>
                  Select {unused.length === 1 ? 'it' : `all ${num(unused.length)}`} in the Library
                </button>
              </div>
            </>
          ) : (
            <p className="muted stats-empty">Every family was turned on at least once in the last 12 months.</p>
          )}
        </section>

        <section className="card stats-card" aria-labelledby="badges">
          <h2 id="badges" className="stats-h">Badges</h2>
          <ul className="stats-badges">
            {data.badges.map((b) => (
              <li key={b.id} className={b.earnedAt ? 'earned' : undefined}>
                <span className="stats-badge-name">{b.name}</span>
                <span className="muted">{b.description}</span>
                <span className="faint">{b.earnedAt ? `Earned ${fmtDate(b.earnedAt)}` : 'Not yet'}</span>
              </li>
            ))}
          </ul>
        </section>
      </div>
      {!december && wrapped}
    </div>
  )
}

/** The "Wrapped": top five families of the year, each in its own font. Featured at the top in December. */
function YearInType({ year, faces, featured }: { year: UsageStats; faces: Map<string, Family>; featured: boolean }) {
  const top = year.mostUsed.slice(0, 5)
  const fav = top[0]
  const y = new Date(year.since).getUTCFullYear()
  return (
    <section className={featured ? 'card stats-wrapped featured' : 'card stats-wrapped'} aria-labelledby="year-in-type">
      <h2 id="year-in-type" className="stats-h">Your {y} in type</h2>
      {fav ? (
        <>
          <p className="stats-fav">
            <span className="muted">Favourite family</span>
            <span style={{ ...familyStyle(faces.get(fav.key)), fontSize: 40, lineHeight: 1.15 }}>{fav.family}</span>
          </p>
          <ol className="stats-top">
            {top.map((f, i) => (
              <li key={f.key} className="enter" style={cssStagger(i)}>
                <span className="faint">{i + 1}</span>
                <span style={{ ...familyStyle(faces.get(f.key)), fontSize: 22 }}>{f.family}</span>
                <span className="faint">{plural(f.days, 'day')}</span>
              </li>
            ))}
          </ol>
        </>
      ) : (
        <p className="muted stats-empty">No fonts turned on in {y} yet. Your year in type fills in as you work.</p>
      )}
      <span className="muted">{plural(year.addedInPeriod, 'font')} added in {y} · {plural(year.totalFonts, 'font')} in your library</span>
    </section>
  )
}
