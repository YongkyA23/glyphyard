import { useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, type Flag, type InstallFilter } from '../api'
import { pop, press, row } from '../motion'
import { ago, Badge, Chips, Empty, fmtDate, Loaded, Table, TableSkeleton, useDebounced, useLoad, type Tone } from '../ui'

const MotionLink = motion.create(Link)
const HEAD = [<span className="sr-only">Select</span>, 'Name', 'Install ID', 'Computer', 'Active fonts', 'Last seen', 'Next name change']

const FILTERS: [InstallFilter, string][] = [
  ['all', 'All'], ['online', 'Online'], ['overrides', 'Has overrides'], ['duplicates', 'Possible duplicates'], ['stale', 'Offline 14+ days'],
]
// First matching flag is shown next to the name.
const FLAGS: [Flag, string, Tone][] = [
  ['possible_duplicate', 'Possible duplicate', 'warn'], ['new', 'New', 'ok'], ['override', 'Override', 'admin'], ['failures', 'Failures', 'bad'],
]

export function Users() {
  const [params, setParams] = useSearchParams()
  const filter = (FILTERS.find(([k]) => k === params.get('filter'))?.[0] ?? 'all') as InstallFilter
  const [q, setQ] = useState('')
  const query = useDebounced(q.trim())
  const [selected, setSelected] = useState<string[]>(() => params.getAll('select'))
  const [total, setTotal] = useState<number>()
  const state = useLoad(async () => {
    const rows = await api.installs(query, filter)
    if (!query && filter === 'all') setTotal(rows.length)
    return rows
  }, [query, filter])

  const toggle = (id: string) =>
    setSelected((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]))
  const setFilter = (f: InstallFilter) => setParams(f === 'all' ? {} : { filter: f }, { replace: true })

  return (
    <div className="page gap-18">
      <div className="page-head">
        <h1>Users</h1>
        <AnimatePresence mode="wait" initial={false}>
          {selected.length === 2 ? (
            <MotionLink key="merge" className="btn btn-primary" {...pop} {...press}
              to={`/users/merge?primary=${encodeURIComponent(selected[0])}&secondary=${encodeURIComponent(selected[1])}`}>
              Merge 2 selected installs
            </MotionLink>
          ) : (
            <motion.span key="hint" className="small faint" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0, transition: { duration: 0.1 } }}>
              {selected.length > 2 ? `${selected.length} selected · select exactly 2 to merge` : 'Select 2 installs to merge them into one person'}
            </motion.span>
          )}
        </AnimatePresence>
      </div>
      <div className="toolbar">
        <label htmlFor="usearch" className="sr-only">Search by name or install ID</label>
        <input id="usearch" type="search" className="input grow" value={q} onChange={(e) => setQ(e.target.value)}
          placeholder="Search by name, old name or install ID" />
        <Chips label="Filter installs" options={FILTERS} value={filter} onChange={setFilter} />
      </div>
      <Loaded state={state} skeleton={<TableSkeleton head={HEAD} minWidth={960} />}>{(rows) => (
        <>
          <Table caption="Installs" minWidth={960} head={HEAD}>
            {rows.map((u, i) => {
              const flag = FLAGS.find(([f]) => u.flags.includes(f))
              return (
                <motion.tr key={u.id} {...row(i, rows.length)}>
                  <td className="w-44">
                    <input type="checkbox" className="check" checked={selected.includes(u.id)} onChange={() => toggle(u.id)} aria-label={`Select ${u.name}`} />
                  </td>
                  <td>
                    <span className="stack-2">
                      <span className="row-8">
                        <Link to={`/users/${encodeURIComponent(u.id)}`} className="name-link">{u.name}</Link>
                        {flag && <Badge tone={flag[2]}>{flag[1]}</Badge>}
                      </span>
                      <span className="meta">{u.previous_names.length ? `Previously: ${u.previous_names.join(', ')}` : 'No name changes'}</span>
                    </span>
                  </td>
                  <td className="mono">{u.id}</td>
                  <td>{u.os}</td>
                  <td>{u.active_fonts}</td>
                  <td><span className="row-6"><span aria-hidden="true" className={u.online ? 'dot dot-on' : 'dot'} />{u.online ? 'Online' : ago(u.last_seen)}</span></td>
                  <td className="ink-2">{u.next_name_change ? fmtDate(u.next_name_change) : 'Any time'}</td>
                </motion.tr>
              )
            })}
          </Table>
          {rows.length === 0 && <Empty>{query || filter !== 'all' ? 'No installs match.' : 'No installs yet. They appear when the desktop app connects for the first time.'}</Empty>}
          <span className="meta">Showing {rows.length}{total !== undefined ? ` of ${total}` : ''} installs</span>
        </>
      )}</Loaded>
    </div>
  )
}
