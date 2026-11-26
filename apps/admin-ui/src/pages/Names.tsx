import { useState } from 'react'
import { Link } from 'react-router'
import { motion } from 'motion/react'
import { api, type NameChangeBy } from '../api'
import { row } from '../motion'
import { Badge, Empty, fmtDate, Loaded, PageHeader, Pager, Table, TableSkeleton, useDebounced, useLoad, usePage, type Tone } from '../ui'

const HEAD = ['Date', 'Old name', 'New name', 'Install', 'Changed by', 'Next change allowed']

const BY: Record<NameChangeBy, [string, Tone]> = {
  user: ['User', 'user'], first: ['First name', 'ok'], merge: ['Merge', 'warn'], admin_reset: ['Admin reset', 'admin'],
}
const PERIODS = [['90', 'Last 90 days'], ['month', 'This month'], ['year', 'This year'], ['all', 'All time']] as const
type Period = (typeof PERIODS)[number][0]

function since(p: Period) {
  const now = new Date()
  if (p === 'all') return ''
  if (p === 'month') return new Date(now.getFullYear(), now.getMonth(), 1).toISOString()
  if (p === 'year') return new Date(now.getFullYear(), 0, 1).toISOString()
  return new Date(now.getTime() - 90 * 864e5).toISOString()
}

export function Names() {
  const [q, setQ] = useState('')
  const [period, setPeriod] = useState<Period>('90')
  const query = useDebounced(q.trim(), 150)
  const from = since(period).slice(0, 10) // date only, so the value is stable between renders
  const [page, setPage] = usePage(`${query}|${from}`)
  const state = useLoad(() => api.nameChanges(query, from, page), [query, from, page])

  return (
    <div className="page gap-18">
      <PageHeader title="Name history" sub="Every name every install has used. Nothing is ever overwritten."
        actions={<a className="btn btn-primary" href={api.nameChangesCsvUrl(query, from)} download="name-history.csv">Export CSV</a>} />
      <div className="toolbar">
        <label htmlFor="nsearch" className="sr-only">Search names</label>
        <input id="nsearch" type="search" className="input grow" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search old or new names" />
        <label className="inline-label">Period
          <select className="input" value={period} onChange={(e) => setPeriod(e.target.value as Period)}>
            {PERIODS.map(([k, label]) => <option key={k} value={k}>{label}</option>)}
          </select>
        </label>
      </div>
      <Loaded state={state} skeleton={<TableSkeleton head={HEAD} minWidth={900} />}>{({ items: rows, total }) => (
        <>
          <Table caption="Name changes" minWidth={900} head={HEAD}>
            {rows.map((r, i) => (
              <motion.tr key={`${r.at}-${i}`} {...row(i, rows.length)}>
                <td className="ink-2 nowrap">{fmtDate(r.at)}</td>
                <td className="mute">{r.old_name ?? '—'}</td>
                <td><Link className="name-link" to={`/users/${encodeURIComponent(r.install_ids[0])}`}>{r.new_name}</Link></td>
                <td className="mono">{r.install_ids.join(' + ')}</td>
                <td><Badge tone={BY[r.by][1]}>{BY[r.by][0]}</Badge></td>
                <td className="ink-2">{r.next_allowed ? fmtDate(r.next_allowed) : r.by === 'merge' ? 'Kept from older install' : 'Any time'}</td>
              </motion.tr>
            ))}
          </Table>
          {total === 0 && (query ? <Empty>No names match.</Empty> : <Empty title="No name changes in this period" />)}
          <Pager label="Name history pages" page={page} total={total} onPage={setPage} />
        </>
      )}</Loaded>
      <span className="meta">“First name” rows are the name entered at first launch. Merged installs show both install IDs.</span>
    </div>
  )
}
