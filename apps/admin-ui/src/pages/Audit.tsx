import { useState } from 'react'
import { motion } from 'motion/react'
import { api } from '../api'
import { t, tk } from '../i18n'
import { row } from '../motion'
import { Empty, fmtShort, Loaded, PageHeader, Pager, Table, TableSkeleton, useLoad, usePage } from '../ui'

// Actions added later show their code until they get a label here.
const LABELS: Record<string, string> = {
  sign_in: tk('Signed in'), sign_in_failed: tk('Sign-in locked after failed attempts'),
  settings_changed: tk('Changed settings'), install_key_regenerated: tk('Regenerated the install key'), install_key_set: tk('Set a custom install key'),
  admin_added: tk('Added an admin'), admin_removed: tk('Removed an admin'), role_changed: tk('Changed a role'),
  password_reset: tk('Reset a password'), password_changed: tk('Changed their password'),
  font_uploaded: tk('Uploaded a font'), license_changed: tk('Changed a license'), font_blocked: tk('Blocked a font'), font_unblocked: tk('Unblocked a font'),
  font_delivered: tk('Delivered a font'), organize_applied: tk('Applied auto-organize'), font_on: tk('Turned a font on remotely'),
  font_off: tk('Turned a font off remotely'), name_limit_reset: tk('Reset the name-change limit'), organize_requested: tk('Asked an install to organize'),
  installs_merged: tk('Merged installs'), install_split: tk('Split an install back out'),
  install_revoked: tk('Removed a user and all their computers'), computer_removed: tk('Removed a computer'), font_undelivered: tk('Stopped delivering a font'), font_deleted: tk('Deleted a font'),
  bulk_deliver: tk('Delivered fonts in bulk'), bulk_block: tk('Blocked fonts in bulk'), bulk_unblock: tk('Unblocked fonts in bulk'), bulk_delete: tk('Deleted fonts in bulk'),
  fonts_bulk_on: tk('Turned fonts on in bulk'), fonts_bulk_off: tk('Turned fonts off in bulk'), fonts_bulk_block: tk('Blocked fonts in bulk'),
  font_request_done: tk('Marked a font request done'), font_request_reopened: tk('Reopened a font request'),
  release_uploaded: tk('Uploaded an app release'),
  connection_password_set: tk('Unlock password set'), connection_password_cleared: tk('Unlock password removed'),
}
const label = (action: string) => (LABELS[action] ? t(LABELS[action]) : action) // i18n-dynamic

export function Audit() {
  const [admin, setAdmin] = useState('')
  const [action, setAction] = useState('')
  const [page, setPage] = usePage(`${admin}|${action}`)
  const state = useLoad(() => api.audit(admin, action, page), [admin, action, page])
  const HEAD = [t('When'), t('Admin'), t('Action'), t('Target'), t('Details')]
  const admins = state.data?.admins ?? []
  const actions = state.data?.actions ?? []

  return (
    <div className="page gap-18">
      <PageHeader title={t('Audit log')} sub={t('What each admin changed, and when. Passwords and keys are never recorded.')} />
      <div className="toolbar">
        <label className="inline-label">{t('Admin')}
          <select className="input" value={admin} onChange={(e) => setAdmin(e.target.value)}>
            <option value="">{t('Everyone')}</option>
            {admins.map((a) => <option key={a} value={a}>{a}</option>)}
          </select>
        </label>
        <label className="inline-label">{t('Action')}
          <select className="input" value={action} onChange={(e) => setAction(e.target.value)}>
            <option value="">{t('All actions')}</option>
            {actions.map((a) => <option key={a} value={a}>{label(a)}</option>)}
          </select>
        </label>
      </div>
      <Loaded state={state} skeleton={<TableSkeleton head={HEAD} minWidth={820} />}>{({ items, total }) => (
        <>
          <Table caption={t('Audit log')} minWidth={820} head={HEAD}>
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
          {total === 0 && (admin || action ? <Empty>{t('Nothing matches these filters.')}</Empty> : <Empty title={t('Nothing has been recorded yet')} />)}
          <Pager label={t('Audit log pages')} page={page} total={total} onPage={setPage} />
        </>
      )}</Loaded>
    </div>
  )
}
