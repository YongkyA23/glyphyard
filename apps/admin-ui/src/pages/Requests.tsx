import { useState } from 'react'
import { Link } from 'react-router'
import { motion } from 'motion/react'
import { api, sortAria, errorText, type FontRequest, type RequestSort, type RequestState, type Sort } from '../api'
import { t, tk } from '../i18n'
import { row } from '../motion'
import { toast } from '../toast'
import { Chips, Empty, ErrorBox, fmtDate, Loaded, PageHeader, Pager, SortHead, Table, TableSkeleton, useLoad, usePage } from '../ui'

const STATES: [RequestState, string][] = [['open', tk('Open')], ['done', tk('Done')], ['all', tk('All')]]

export function Requests() {
  const [state, setState] = useState<RequestState>('open')
  const [sort, setSort] = useState<Sort<RequestSort>>(null)
  const [page, setPage] = usePage(`${state}|${sort?.key}|${sort?.dir}`)
  const list = useLoad(() => api.fontRequests(state, page, sort), [state, page, sort])
  const HEAD = [t('Asked'), t('Font'), t('Note'), t('Who'), '']
  const head = [
    { node: <SortHead label={t('Asked')} k="at" sort={sort} onSort={setSort} defaultDir="desc" />, sort: sortAria(sort, 'at') },
    { node: <SortHead label={t('Font')} k="font" sort={sort} onSort={setSort} defaultDir="asc" />, sort: sortAria(sort, 'font') },
    ...HEAD.slice(2),
  ]
  const states = STATES.map(([k, label]): [RequestState, string] => [k, t(label)]) // i18n-dynamic
  const [busy, setBusy] = useState<number | null>(null)
  const [error, setError] = useState('')

  const toggle = async (r: FontRequest) => {
    setBusy(r.id)
    setError('')
    try {
      const done = !r.done_at
      await api.setRequestDone(r.id, done)
      await list.reload(true)
      window.dispatchEvent(new Event('requests-changed'))
      toast(done ? t('Marked “{font}” done.', { font: r.font_name }) : t('Reopened “{font}”.', { font: r.font_name }), {
        undo: () => api.setRequestDone(r.id, !done).then(() => { void list.reload(true); window.dispatchEvent(new Event('requests-changed')) }),
      })
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="page gap-18">
      <PageHeader title={t('Font requests')} sub={t('Fonts people asked you for from the desktop app. Add them on the Shared library page and deliver them from there.')} />
      <div className="toolbar">
        <Chips label={t('Show')} options={states} value={state} onChange={setState} />
      </div>
      {error && <ErrorBox message={error} />}
      <Loaded state={list} skeleton={<TableSkeleton head={HEAD} minWidth={820} />}>{({ items, total }) => (
        <>
          <Table caption={t('Font requests')} minWidth={820} head={head}>
            {items.map((r, i) => (
              <motion.tr key={r.id} {...row(i, items.length)}>
                <td className="ink-2 nowrap">{fmtDate(r.at)}</td>
                <td className="strong">{r.font_name}</td>
                <td className="ink-2">{r.note ?? '—'}</td>
                <td><Link className="name-link" to={`/users/${encodeURIComponent(r.install_id)}`}>{r.install_name}</Link></td>
                <td className="nowrap">
                  <button type="button" className="btn btn-sm" disabled={busy === r.id} onClick={() => void toggle(r)}
                    aria-label={r.done_at ? t('Reopen: {font} for {name}', { font: r.font_name, name: r.install_name }) : t('Mark done: {font} for {name}', { font: r.font_name, name: r.install_name })}>
                    {r.done_at ? t('Reopen') : t('Mark done')}
                  </button>
                  {r.done_at && <span className="meta"> {t('Done {date}', { date: fmtDate(r.done_at) })}</span>}
                </td>
              </motion.tr>
            ))}
          </Table>
          {total === 0 && (state === 'open'
            ? <Empty title={t('No open requests')}>{t('When someone asks for a font in the desktop app, it shows up here.')}</Empty>
            : <Empty>{t('No requests yet.')}</Empty>)}
          <Pager label={t('Font request pages')} page={page} total={total} onPage={setPage} />
        </>
      )}</Loaded>
    </div>
  )
}
