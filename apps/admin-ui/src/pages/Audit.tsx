import { useState } from 'react'
import { motion } from 'motion/react'
import { api } from '../api'
import { row } from '../motion'
import { Empty, fmtShort, Loaded, PageHeader, Pager, Table, TableSkeleton, useLoad, usePage } from '../ui'

const HEAD = ['When', 'Admin', 'Action', 'Target', 'Details']

// Actions added later show their code until they get a label here.
const LABELS: Record<string, string> = {
  sign_in: 'Signed in', sign_in_failed: 'Sign-in locked after failed attempts',
  settings_changed: 'Changed settings', install_key_regenerated: 'Regenerated the install key',
  admin_added: 'Added an admin', admin_removed: 'Removed an admin', role_changed: 'Changed a role',
  password_reset: 'Reset a password', password_changed: 'Changed their password',
  font_uploaded: 'Uploaded a font', license_changed: 'Changed a license', font_blocked: 'Blocked a font', font_unblocked: 'Unblocked a font',
  font_delivered: 'Delivered a font', organize_applied: 'Applied auto-organize', font_on: 'Turned a font on remotely',
  font_off: 'Turned a font off remotely', name_limit_reset: 'Reset the name-change limit', organize_requested: 'Asked an install to organize',
  installs_merged: 'Merged installs', install_split: 'Split an install back out',
  install_revoked: 'Removed a computer', font_undelivered: 'Stopped delivering a font', font_deleted: 'Deleted a font',
  bulk_deliver: 'Delivered fonts in bulk', bulk_block: 'Blocked fonts in bulk', bulk_unblock: 'Unblocked fonts in bulk', bulk_delete: 'Deleted fonts in bulk',
  font_request_done: 'Marked a font request done', font_request_reopened: 'Reopened a font request',
}
const label = (action: string) => LABELS[action] ?? action

export function Audit() {
  const [admin, setAdmin] = useState('')
  const [action, setAction] = useState('')
  const [page, setPage] = usePage(`${admin}|${action}`)
  const state = useLoad(() => api.audit(admin, action, page), [admin, action, page])
  const admins = state.data?.admins ?? []
  const actions = state.data?.actions ?? []

  return (
    <div className="page gap-18">
      <PageHeader title="Audit log" sub="What each admin changed, and when. Passwords and keys are never recorded." />
      <div className="toolbar">
        <label className="inline-label">Admin
          <select className="input" value={admin} onChange={(e) => setAdmin(e.target.value)}>
            <option value="">Everyone</option>
            {admins.map((a) => <option key={a} value={a}>{a}</option>)}
          </select>
        </label>
        <label className="inline-label">Action
          <select className="input" value={action} onChange={(e) => setAction(e.target.value)}>
            <option value="">All actions</option>
            {actions.map((a) => <option key={a} value={a}>{label(a)}</option>)}
          </select>
        </label>
      </div>
      <Loaded state={state} skeleton={<TableSkeleton head={HEAD} minWidth={820} />}>{({ items, total }) => (
        <>
          <Table caption="Audit log" minWidth={820} head={HEAD}>
            {items.map((e, i) => (
              <motion.tr key={e.id} {...row(i, items.length)}>
                <td className="ink-2 nowrap">{fmtShort(e.at)}</td>
                <td>{e.admin}</td>
                <td>{label(e.action)}</td>
                <td className="mono">{e.target || '—'}</td>
                <td className="ink-2">{e.detail || '—'}</td>
              </motion.tr>
            ))}
          </Table>
          {total === 0 && <Empty>{admin || action ? 'Nothing matches these filters.' : 'Nothing has been recorded yet.'}</Empty>}
          <Pager label="Audit log pages" page={page} total={total} onPage={setPage} />
        </>
      )}</Loaded>
    </div>
  )
}
