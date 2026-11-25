import { useState } from 'react'
import { Link } from 'react-router'
import { motion } from 'motion/react'
import { api, errorText, type FontRequest, type RequestState } from '../api'
import { row } from '../motion'
import { Chips, Empty, ErrorBox, fmtDate, Loaded, PageHeader, Pager, Table, TableSkeleton, useLoad, usePage } from '../ui'

const HEAD = ['Asked', 'Font', 'Note', 'Who', '']
const STATES: [RequestState, string][] = [['open', 'Open'], ['done', 'Done'], ['all', 'All']]

export function Requests() {
  const [state, setState] = useState<RequestState>('open')
  const [page, setPage] = usePage(state)
  const list = useLoad(() => api.fontRequests(state, page), [state, page])
  const [busy, setBusy] = useState<number | null>(null)
  const [error, setError] = useState('')

  const toggle = async (r: FontRequest) => {
    setBusy(r.id)
    setError('')
    try {
      await api.setRequestDone(r.id, !r.done_at)
      await list.reload(true)
      window.dispatchEvent(new Event('requests-changed'))
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="page gap-18">
      <PageHeader title="Font requests" sub="Fonts people asked you for from the desktop app. Add them on the Shared library page and deliver them from there." />
      <div className="toolbar">
        <Chips label="Show" options={STATES} value={state} onChange={setState} />
      </div>
      {error && <ErrorBox message={error} />}
      <Loaded state={list} skeleton={<TableSkeleton head={HEAD} minWidth={820} />}>{({ items, total }) => (
        <>
          <Table caption="Font requests" minWidth={820} head={HEAD}>
            {items.map((r, i) => (
              <motion.tr key={r.id} {...row(i, items.length)}>
                <td className="ink-2 nowrap">{fmtDate(r.at)}</td>
                <td className="strong">{r.font_name}</td>
                <td className="ink-2">{r.note ?? '—'}</td>
                <td><Link className="name-link" to={`/users/${encodeURIComponent(r.install_id)}`}>{r.install_name}</Link></td>
                <td className="nowrap">
                  <button type="button" className="btn btn-sm" disabled={busy === r.id} onClick={() => void toggle(r)}
                    aria-label={`${r.done_at ? 'Reopen' : 'Mark done'}: ${r.font_name} for ${r.install_name}`}>
                    {r.done_at ? 'Reopen' : 'Mark done'}
                  </button>
                  {r.done_at && <span className="meta"> Done {fmtDate(r.done_at)}</span>}
                </td>
              </motion.tr>
            ))}
          </Table>
          {total === 0 && <Empty>{state === 'open' ? 'No open requests.' : 'No requests yet.'}</Empty>}
          <Pager label="Font request pages" page={page} total={total} onPage={setPage} />
        </>
      )}</Loaded>
    </div>
  )
}
