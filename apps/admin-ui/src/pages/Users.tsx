import { useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, sortAria, type Flag, type InstallFilter, type InstallSort, type Sort } from '../api'
import { t, tk } from '../i18n'
import { pop, press, row } from '../motion'
import { ago, Badge, Chips, Empty, fmtDate, Loaded, Pager, SortHead, Table, TableSkeleton, useDebounced, useLoad, usePage, type Tone } from '../ui'

const MotionLink = motion.create(Link)

const FILTERS: [InstallFilter, string][] = [
  ['all', tk('All')], ['online', tk('Online')], ['overrides', tk('Has overrides')], ['duplicates', tk('Possible duplicates')], ['stale', tk('Offline 14+ days')],
]
// First matching flag is shown next to the name.
const FLAGS: [Flag, string, Tone][] = [
  ['possible_duplicate', tk('Possible duplicate'), 'warn'], ['new', tk('New'), 'ok'], ['override', tk('Override'), 'admin'], ['failures', tk('Failures'), 'bad'],
]

export function Users() {
  const [params, setParams] = useSearchParams()
  const filter = (FILTERS.find(([k]) => k === params.get('filter'))?.[0] ?? 'all') as InstallFilter
  const [q, setQ] = useState('')
  const query = useDebounced(q.trim(), 150)
  const [sort, setSort] = useState<Sort<InstallSort>>(null)
  const [page, setPage] = usePage(`${query}|${filter}|${sort?.key}|${sort?.dir}`)
  const [selected, setSelected] = useState<string[]>(() => params.getAll('select'))
  const state = useLoad(() => api.installs(query, filter, page, sort), [query, filter, page, sort])
  const HEAD = [<span key="sel" className="sr-only">{t('Select')}</span>, t('Name'), t('Install ID'), t('Computer'), t('Active fonts'), t('Last seen'), t('Next name change')]
  const head = [
    HEAD[0],
    { node: <SortHead label={t('Name')} k="name" sort={sort} onSort={setSort} defaultDir="asc" />, sort: sortAria(sort, 'name') },
    HEAD[2], HEAD[3],
    { node: <SortHead label={t('Active fonts')} k="fonts" sort={sort} onSort={setSort} defaultDir="desc" />, sort: sortAria(sort, 'fonts') },
    { node: <SortHead label={t('Last seen')} k="last_seen" sort={sort} onSort={setSort} defaultDir="desc" />, sort: sortAria(sort, 'last_seen') },
    HEAD[6],
  ]
  const filters = FILTERS.map(([k, label]): [InstallFilter, string] => [k, t(label)]) // i18n-dynamic

  const toggle = (id: string) =>
    setSelected((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]))
  const setFilter = (f: InstallFilter) => setParams(f === 'all' ? {} : { filter: f }, { replace: true })

  return (
    <div className="page gap-18">
      <div className="page-head">
        <h1>{t('Users')}</h1>
        <AnimatePresence mode="wait" initial={false}>
          {selected.length === 2 ? (
            <MotionLink key="merge" className="btn btn-primary" {...pop} {...press}
              to={`/users/merge?primary=${encodeURIComponent(selected[0])}&secondary=${encodeURIComponent(selected[1])}`}>
              {t('Merge 2 selected installs')}
            </MotionLink>
          ) : (
            <motion.span key="hint" className="small faint" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0, transition: { duration: 0.1 } }}>
              {selected.length > 2 ? t('{n} selected · select exactly 2 to merge', { n: selected.length }) : t('Select 2 installs to merge them into one person')}
            </motion.span>
          )}
        </AnimatePresence>
      </div>
      <div className="toolbar">
        <label htmlFor="usearch" className="sr-only">{t('Search by name or install ID')}</label>
        <input id="usearch" type="search" className="input grow" value={q} onChange={(e) => setQ(e.target.value)}
          placeholder={t('Search by name, old name or install ID')} />
        <Chips label={t('Filter installs')} options={filters} value={filter} onChange={setFilter} />
      </div>
      <Loaded state={state} skeleton={<TableSkeleton head={HEAD} minWidth={960} />}>{({ items: rows, total }) => (
        <>
          <Table caption={t('Installs')} minWidth={960} head={head}>
            {rows.map((u, i) => {
              const flag = FLAGS.find(([f]) => u.flags.includes(f))
              return (
                <motion.tr key={u.id} {...row(i, rows.length)}>
                  <td className="w-44">
                    <input type="checkbox" className="check" checked={selected.includes(u.id)} onChange={() => toggle(u.id)} aria-label={t('Select {name}', { name: u.name })} />
                  </td>
                  <td>
                    <span className="stack-2">
                      <span className="row-8">
                        <Link to={`/users/${encodeURIComponent(u.id)}`} className="name-link">{u.name}</Link>
                        {flag && <Badge tone={flag[2]}>{t(flag[1])}{/* i18n-dynamic */}</Badge>}
                      </span>
                      <span className="meta">{u.previous_names.length ? t('Previously: {names}', { names: u.previous_names.join(', ') }) : t('No name changes')}</span>
                    </span>
                  </td>
                  <td className="mono">{u.id}</td>
                  <td>{u.os}</td>
                  <td>{u.active_fonts}</td>
                  <td><span className="row-6"><span aria-hidden="true" className={u.online ? 'dot dot-on' : 'dot'} />{u.online ? t('Online') : ago(u.last_seen)}</span></td>
                  <td className="ink-2">{u.next_name_change ? fmtDate(u.next_name_change) : t('Any time')}</td>
                </motion.tr>
              )
            })}
          </Table>
          {total === 0 && (query || filter !== 'all'
            ? <Empty>{t('No installs match.')}</Empty>
            : <Empty title={t('No computers yet')} action={<Link className="btn btn-primary" to="/settings">{t('Show install key')}</Link>}>
                {t('Install Glyphyard on a computer and enter this server’s address and install key.')}
              </Empty>)}
          <Pager label={t('Install pages')} page={page} total={total} onPage={setPage} />
        </>
      )}</Loaded>
    </div>
  )
}
