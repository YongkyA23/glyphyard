// Usage statistics, badges and the "Year in type" card. Everything here is computed on this computer.
import { useCallback, useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { api, type Badge, type Family, type UsageStats } from '../api'
import { familyStyle } from '../fonts'
import { cssStagger } from '../motion'
import { useApp } from '../state'
import { Chip, errMsg, fmtDate, plural, Skel } from '../ui'
import { t } from '../i18n'
import { Hint } from '../Hint'
import './stats.css'

const titleRow = { display: 'flex', alignItems: 'center', gap: 6 } as const

type Data = { month: UsageStats; year: UsageStats; badges: Badge[]; faces: Map<string, Family> }

export function Stats() {
  const { app, libVersion } = useApp()
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
    <div style={titleRow}>
      <h1 className="h-page">{t('Stats')}</h1>
      <Hint
        text={app.serverConfigured
          ? t('How you use your fonts. These statistics are worked out on this computer and are not sent to your server.')
          : t('How you use your fonts. Counted on this computer only, never sent anywhere.')}
      />
    </div>
  )
  if (error)
    return (
      <div className="page" style={{ gap: 20 }}>
        {header}
        <p role="alert" style={{ margin: 0, color: 'var(--bad)' }}>{t('Couldn’t load your stats: {error}', { error })}</p>
        <div><button type="button" className="btn" onClick={load}>{t('Try again')}</button></div>
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
            <div style={titleRow}>
              <h2 id="most-used" className="stats-h">{t('Most used')}</h2>
              <Hint text={t('Ranked by the number of days a family was turned on.')} />
            </div>
            <div className="chips" role="group" aria-label={t('Period')}>
              <Chip pressed={period === 'month'} onClick={() => setPeriod('month')}>{t('This month')}</Chip>
              <Chip pressed={period === 'year'} onClick={() => setPeriod('year')}>{t('This year')}</Chip>
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
              {period === 'month'
                ? t('Nothing turned on this month yet. Fonts you turn on show up here.')
                : t('Nothing turned on this year yet. Fonts you turn on show up here.')}
            </p>
          )}
          {stats.recordingSince && <p className="faint stats-foot">{t('Counting since {date}.', { date: fmtDate(stats.recordingSince) })}</p>}
        </section>

        <section className="card stats-card" aria-labelledby="never-used">
          <h2 id="never-used" className="stats-h">{t('Never used')}</h2>
          {data.year.neverUsedNote ? (
            <p className="muted stats-empty">{t(data.year.neverUsedNote) /* i18n-dynamic: engine text, crates/engine/src/stats.rs */}</p>
          ) : unused.length ? (
            <>
              <span className="muted">
                {data.year.recordingSince && new Date(data.year.recordingSince).getTime() > yearAgo
                  ? t('{families} not turned on since {date}, when Glyphyard started counting.', { families: plural(unused.length, 'family', 'families'), date: fmtDate(data.year.recordingSince) })
                  : t('{families} not turned on in the last 12 months.', { families: plural(unused.length, 'family', 'families') })}
              </span>
              <p className="stats-names">
                {unused.length > 24
                  ? t('{names} and {n} more', { names: unused.slice(0, 24).map((f) => f.family).join(', '), n: unused.length - 24 })
                  : unused.map((f) => f.family).join(', ')}
              </p>
              <div>
                <button type="button" className="btn" onClick={() => navigate('/', { state: { select: unused.map((f) => f.key) } })}>
                  {unused.length === 1 ? t('Select it in the Library') : t('Select all {n} in the Library', { n: unused.length })}
                </button>
              </div>
            </>
          ) : (
            <p className="muted stats-empty">{t('Every family was turned on at least once in the last 12 months.')}</p>
          )}
        </section>

        <section className="card stats-card" aria-labelledby="badges">
          <h2 id="badges" className="stats-h">{t('Badges')}</h2>
          <ul className="stats-badges">
            {data.badges.map((b) => (
              <li key={b.id} className={b.earnedAt ? 'earned' : undefined}>
                <span className="stats-badge-name">{t(b.name) /* i18n-dynamic: engine badges, crates/engine/src/stats.rs */}</span>
                <span className="muted">{t(b.description) /* i18n-dynamic */}</span>
                <span className="faint">{b.earnedAt ? t('Earned {date}', { date: fmtDate(b.earnedAt) }) : t('Not yet')}</span>
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
  // A string, so t() doesn't format the year as a number (2.026 in Indonesian).
  const y = String(new Date(year.since).getUTCFullYear())
  return (
    <section className={featured ? 'card stats-wrapped featured' : 'card stats-wrapped'} aria-labelledby="year-in-type">
      <h2 id="year-in-type" className="stats-h">{t('Your {year} in type', { year: y })}</h2>
      {fav ? (
        <>
          <p className="stats-fav">
            <span className="muted">{t('Favourite family')}</span>
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
        <p className="muted stats-empty">{t('No fonts turned on in {year} yet. Your year in type fills in as you work.', { year: y })}</p>
      )}
      <span className="muted">{t('{added} added in {year} · {total} in your library', { added: plural(year.addedInPeriod, 'font'), year: y, total: plural(year.totalFonts, 'font') })}</span>
    </section>
  )
}
