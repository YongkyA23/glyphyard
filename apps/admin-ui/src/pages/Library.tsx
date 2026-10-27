import { useRef, useState, type ChangeEvent, type FormEvent } from 'react'
import { Link } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, errorText, type LibraryFile, type License, type Page } from '../api'
import { base, fadeRise, fast, row, spring } from '../motion'
import {
  Badge, Dialog, Empty, ErrorBox, Loaded, PageHeader, Pager, plural, Skel, Spin, Swap, Table, TableSkeleton, useConfirm, useDebounced, useLoad, usePage,
} from '../ui'

const HEAD = ['Family', 'License', 'Delivered to', 'Active on', 'Status', <span className="sr-only">Actions</span>]
type Upload = { name: string; state: 'busy' | 'done' | 'error' }

const LICENSES: [License, string][] = [['licensed', 'Licensed'], ['personal', 'Personal use'], ['client', 'Client-provided'], ['unknown', 'Unknown']]
const ACCEPT = '.otf,.ttf,.ttc,.otc,.woff,.woff2'

const deliveredText = (t: LibraryFile['delivered_to']) =>
  t === 'all' ? 'Everyone' : t === 'none' || t.length === 0 ? 'Nobody' : plural(t.length, 'user')
const kb = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1e3))} KB`)

export function Library() {
  const [q, setQ] = useState('')
  const query = useDebounced(q.trim(), 150)
  const [page, setPage] = usePage(query)
  const state = useLoad(() => api.library(query, page), [query, page])
  const [notice, setNotice] = useState('')
  const [error, setError] = useState('')
  const [uploading, setUploading] = useState(false)
  const [uploads, setUploads] = useState<Upload[]>([])
  const [confirmDialog, confirm] = useConfirm()
  const [delivering, setDelivering] = useState<LibraryFile>()
  const fileInput = useRef<HTMLInputElement>(null)

  const replace = (f: LibraryFile) => state.setData((p) => ({ ...p, items: p.items.map((r) => (r.hash === f.hash ? f : r)) }))
  const run = async (fn: () => Promise<LibraryFile>) => {
    setError('')
    try { replace(await fn()) } catch (e) { setError(errorText(e)) }
  }

  const upload = async (e: ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files?.length) return
    setUploading(true)
    setError('')
    setNotice('')
    // ponytail: one request for all files, so chips show an indeterminate bar, then all finish together.
    const names = Array.from(files, (f) => f.name)
    const mark = (state: Upload['state']) => setUploads(names.map((name) => ({ name, state })))
    mark('busy')
    try {
      const added = await api.upload(files)
      mark('done')
      setNotice(added.length ? `Added ${plural(added.length, 'font file')}.` : 'Those files are already in the library.')
      await state.reload(true)
    } catch (err) {
      mark('error')
      setError(errorText(err))
    } finally {
      setUploading(false)
      e.target.value = ''
      setTimeout(() => setUploads((u) => (u.some((x) => x.state === 'busy') ? u : [])), 2600)
    }
  }

  const block = async (f: LibraryFile) => {
    if (!f.blocked && !(await confirm(`Block ${f.family} ${f.style}? It will be turned off on every computer.`, 'Block', true))) return
    void run(() => api.block(f.hash, !f.blocked))
  }

  return (
    <div className="page gap-18">
      <PageHeader title="Shared library"
        sub={state.data && !query ? `Approved fonts you deliver to users’ apps. ${plural(state.data.total, 'file')}.` : 'Approved fonts you deliver to users’ apps.'}
        actions={
          <span className="row-8">
            <Link className="btn" to="/organize">Auto-organize</Link>
            <button type="button" className="btn btn-primary" disabled={uploading} onClick={() => fileInput.current?.click()}>
              {uploading && <Spin />}{uploading ? 'Uploading…' : 'Upload fonts'}
            </button>
            <input ref={fileInput} type="file" multiple accept={ACCEPT} hidden onChange={upload} aria-label="Font files to upload" />
          </span>
        } />
      <div className="toolbar">
        <label htmlFor="lsearch" className="sr-only">Search the shared library</label>
        <input id="lsearch" type="search" className="input grow" placeholder="Search family, style or file name" value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      <AnimatePresence>
        {uploads.length > 0 && (
          <motion.div className="upload-chips" aria-hidden="true" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0, y: -4, transition: fast }}>
            {uploads.map((u, i) => (
              <motion.span key={u.name + i} className={`upload-chip is-${u.state}`}
                initial={{ opacity: 0, y: 6, scale: 0.96 }} animate={{ opacity: 1, y: 0, scale: 1, transition: { ...spring, delay: Math.min(i, 8) * 0.04 } }}>
                <span className="row-between"><span className="ellipsis">{u.name}</span>
                  <Swap>{u.state === 'done' ? '✓' : u.state === 'error' ? 'Failed' : ''}</Swap>
                </span>
                <span className={u.state === 'busy' ? 'progress progress-run' : 'progress'}>
                  {u.state === 'busy' ? <span /> : <motion.span initial={{ scaleX: 0.4 }} animate={{ scaleX: 1 }} transition={base} />}
                </span>
              </motion.span>
            ))}
          </motion.div>
        )}
      </AnimatePresence>
      {notice && <span role="status" className="notice">{notice}</span>}
      {error && <ErrorBox message={error} />}
      <Loaded state={state} skeleton={<TableSkeleton head={HEAD} minWidth={980} rows={3} />}>{({ items: shown, total }) => (
          <>
            <Table caption="Font files in the shared library" minWidth={980} head={HEAD}>
              {shown.map((f, i) => (
                <motion.tr key={f.hash} {...row(i, shown.length)}>
                  <td>
                    <span className="stack-2">
                      <span className={f.blocked ? 'lib-name lib-blocked' : 'lib-name'}>{f.family}</span>
                      <span className="meta">{f.style} · v{f.version} · {f.filename} · {kb(f.size)}</span>
                    </span>
                  </td>
                  <td>
                    <select className={`license tone-lic-${f.license}`} value={f.license} aria-label={`License for ${f.family} ${f.style}`}
                      onChange={(e) => void run(() => api.setLicense(f.hash, e.target.value as License))}>
                      {LICENSES.map(([k, label]) => <option key={k} value={k}>{label}</option>)}
                    </select>
                  </td>
                  <td>{deliveredText(f.delivered_to)}</td>
                  <td>{plural(f.active_on, 'user')}</td>
                  <td>
                    {/* Block/unblock cross-fades the status badge. */}
                    <AnimatePresence mode="popLayout" initial={false}>
                      <motion.span key={String(f.blocked)} className="swap" initial={{ opacity: 0, scale: 0.9 }} animate={{ opacity: 1, scale: 1, transition: spring }}
                        exit={{ opacity: 0, scale: 0.9, transition: fast }}>
                        {f.blocked ? <Badge tone="bad">Blocked</Badge> : <Badge tone="ok">Published</Badge>}
                      </motion.span>
                    </AnimatePresence>
                  </td>
                  <td className="nowrap">
                    <span className="row-6 end">
                      <button type="button" className="btn btn-xs" disabled={f.blocked} onClick={() => setDelivering(f)}>Deliver…</button>
                      <button type="button" className={f.blocked ? 'btn btn-xs' : 'btn btn-xs btn-danger'} onClick={() => void block(f)}>
                        <Swap>{f.blocked ? 'Unblock' : 'Block'}</Swap>
                      </button>
                    </span>
                  </td>
                </motion.tr>
              ))}
            </Table>
            {total === 0 && <Empty>{query ? 'No fonts match.' : 'The shared library is empty. Upload font files to deliver them to users.'}</Empty>}
            <Pager label="Library pages" page={page} total={total} onPage={setPage} />
          </>
      )}</Loaded>
      <motion.div className="info-cards" variants={fadeRise} initial="hidden" animate="show">
        <div className="card pad-16 stack-6">
          <span className="strong">Delivering a font</span>
          <span className="small mute lh">Pick everyone or specific users. The file is downloaded to their app and appears in their library. Choose whether it’s also turned on.</span>
        </div>
        <div className="card card-bad pad-16 stack-6">
          <span className="strong">Blocking a font</span>
          <span className="small lh">A blocked font is turned off on every computer and can’t be turned on again, even if a user has their own copy.</span>
        </div>
      </motion.div>
      <AnimatePresence>
        {delivering && <DeliverDialog key={delivering.hash} file={delivering} onClose={() => setDelivering(undefined)}
          onDone={(f) => { replace(f); setDelivering(undefined) }} />}
      </AnimatePresence>
      {confirmDialog}
    </div>
  )
}

const pickSkeleton = <div className="stack-10" style={{ padding: '8px 0' }}>{[60, 45, 70].map((w) => <Skel key={w} w={`${w}%`} />)}</div>

function DeliverDialog({ file, onClose, onDone }: { file: LibraryFile; onClose: () => void; onDone: (f: LibraryFile) => void }) {
  const [mode, setMode] = useState<'all' | 'pick'>(Array.isArray(file.delivered_to) ? 'pick' : 'all')
  const [picked, setPicked] = useState<string[]>(Array.isArray(file.delivered_to) ? file.delivered_to : [])
  const [activate, setActivate] = useState(false)
  const [q, setQ] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const query = useDebounced(q.trim(), 150)
  const [page, setPage] = usePage(query)
  const installs = useLoad(() => (mode === 'pick' ? api.installs(query, 'all', page) : Promise.resolve<Page<never>>({ items: [], total: 0 })), [mode, query, page])

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (mode === 'pick' && picked.length === 0) return setError('Pick at least one user.')
    setBusy(true)
    setError('')
    try {
      onDone(await api.deliver(file.hash, mode === 'all' ? 'all' : picked, activate))
    } catch (err) {
      setError(errorText(err))
      setBusy(false)
    }
  }

  return (
    <Dialog labelledBy="dlg-title" onClose={onClose}>
      <form className="stack-12" onSubmit={submit} noValidate>
        <h2 id="dlg-title" className="h2">Deliver {file.family} {file.style}</h2>
        <fieldset className="fieldset-plain stack-6">
          <legend className="sr-only">Who gets it</legend>
          <label className="radio"><input type="radio" name="target" checked={mode === 'all'} onChange={() => setMode('all')} /><span>Everyone, including new installs</span></label>
          <label className="radio"><input type="radio" name="target" checked={mode === 'pick'} onChange={() => setMode('pick')} /><span>Only the users I pick</span></label>
        </fieldset>
        <AnimatePresence initial={false}>
        {mode === 'pick' && (
          <motion.div className="stack-6" initial={{ opacity: 0, y: -6 }} animate={{ opacity: 1, y: 0, transition: base }} exit={{ opacity: 0, transition: fast }}>
            <label htmlFor="dsearch" className="sr-only">Search users</label>
            <input id="dsearch" type="search" className="input input-38" placeholder="Search users" value={q} onChange={(e) => setQ(e.target.value)} />
            <div className="pick-list">
              <Loaded state={installs} label="Loading users…" skeleton={pickSkeleton}>{({ items: shown }) => {
                if (shown.length === 0) return <Empty>No users match.</Empty>
                return shown.map((u) => (
                  <label key={u.id} className="radio">
                    <input type="checkbox" className="check" checked={picked.includes(u.id)}
                      onChange={() => setPicked((p) => (p.includes(u.id) ? p.filter((x) => x !== u.id) : [...p, u.id]))} />
                    <span>{u.name} <span className="meta mono">{u.id}</span></span>
                  </label>
                ))
              }}</Loaded>
            </div>
            {installs.data && <Pager label="User pages" page={page} total={installs.data.total} onPage={setPage} />}
            <span className="meta">{plural(picked.length, 'user')} selected</span>
          </motion.div>
        )}
        </AnimatePresence>
        <label className="radio">
          <input type="checkbox" className="check" checked={activate} onChange={(e) => setActivate(e.target.checked)} />
          <span>Also turn it on for them</span>
        </label>
        {error && <span role="alert" className="form-error">{error}</span>}
        <div className="row-8 end">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn btn-primary" disabled={busy}>{busy && <Spin />}{busy ? 'Delivering…' : 'Deliver'}</button>
        </div>
      </form>
    </Dialog>
  )
}
