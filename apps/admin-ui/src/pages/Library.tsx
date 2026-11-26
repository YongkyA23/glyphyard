import { useRef, useState, type ChangeEvent, type FormEvent } from 'react'
import { Link, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, sortAria, ApiError, errorText, type BulkAction, type LibraryFile, type LibrarySort, type License, type Page, type Sort } from '../api'
import { base, fadeRise, fast, row, spring } from '../motion'
import { toast } from '../toast'
import {
  Badge, Dialog, Empty, ErrorBox, fmtDate, Loaded, PageHeader, Pager, plural, Skel, SortHead, Spin, Swap, Table, TableSkeleton, useConfirm, useDebounced, useLoad, usePage,
} from '../ui'

const HEAD = ['Family', 'License', 'Delivered to', 'Active on', 'Added', 'Size', 'Status', <span className="sr-only">Actions</span>]
const SKELETON_HEAD = [<span key="sel" className="sr-only">Select</span>, ...HEAD]
type Upload = { name: string; state: 'busy' | 'done' | 'error' }
/** Picked files for this search: hashes on any page, or `all` = every result of the search. */
type Selection = { query: string; hashes: string[]; all: boolean }
const DONE: Record<BulkAction, string> = { deliver: 'Delivered', block: 'Blocked', unblock: 'Unblocked', delete: 'Deleted' }

const LICENSES: [License, string][] = [['licensed', 'Licensed'], ['personal', 'Personal use'], ['client', 'Client-provided'], ['unknown', 'Unknown']]
const ACCEPT = '.otf,.ttf,.ttc,.otc,.woff,.woff2'

const deliveredText = (t: LibraryFile['delivered_to']) =>
  t === 'all' ? 'Everyone' : t === 'none' || t.length === 0 ? 'Nobody' : plural(t.length, 'user')
const kb = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1e3))} KB`)

export function Library() {
  const [params] = useSearchParams()
  const [q, setQ] = useState(params.get('q') ?? '')
  const query = useDebounced(q.trim(), 150)
  const [sort, setSort] = useState<Sort<LibrarySort>>(null)
  const [page, setPage] = usePage(`${query}|${sort?.key}|${sort?.dir}`)
  const state = useLoad(() => api.library(query, page, undefined, sort), [query, page, sort])
  const [error, setError] = useState('')
  const [uploading, setUploading] = useState(false)
  const [uploads, setUploads] = useState<Upload[]>([])
  const [confirmDialog, confirm] = useConfirm()
  const [delivering, setDelivering] = useState<LibraryFile | 'selected'>()
  const fileInput = useRef<HTMLInputElement>(null)
  // A new search starts with nothing selected.
  const [sel, setSel] = useState<Selection>({ query, hashes: [], all: false })
  const selection = sel.query === query ? sel : { query, hashes: [], all: false }
  const total = state.data?.total ?? 0
  const selectedCount = selection.all ? total : selection.hashes.length
  const isSelected = (hash: string) => selection.all || selection.hashes.includes(hash)
  const clearSelection = () => setSel({ query, hashes: [], all: false })
  const toggle = (hash: string) => setSel({
    query, all: false,
    // Unticking one row while "all results" is on keeps the rest of this page.
    hashes: selection.all ? (state.data?.items ?? []).map((f) => f.hash).filter((h) => h !== hash)
      : selection.hashes.includes(hash) ? selection.hashes.filter((h) => h !== hash) : [...selection.hashes, hash],
  })
  const togglePage = (shown: LibraryFile[]) => {
    const page = shown.map((f) => f.hash)
    const allOn = page.every(isSelected)
    setSel({ query, all: false, hashes: allOn ? (selection.all ? [] : selection.hashes.filter((h) => !page.includes(h))) : [...new Set([...selection.hashes, ...page])] })
  }

  const bulk = async (action: BulkAction, target?: 'all' | string[], activate?: boolean) => {
    setError('')
    try {
      const r = await api.bulk(action, selection.all ? { q: query } : { hashes: selection.hashes }, target, activate)
      const done = action === 'deliver' && Array.isArray(target) && target.length === 0 ? 'Removed delivery of' : DONE[action]
      toast(`${done} ${plural(r.changed, 'file')}.`
        + (r.skipped ? ` Skipped ${r.skipped} that ${r.skipped === 1 ? 'was' : 'were'} already done, blocked or gone.` : ''))
      if (r.failed) setError(`${plural(r.failed, 'file')} couldn’t be deleted. Check the server log, then try again.`)
    } catch (e) {
      setError(errorText(e))
    } finally {
      // Some files may have changed even when the request failed part way.
      clearSelection()
      await state.reload(true)
    }
  }
  const bulkAsk = async (action: 'block' | 'delete') => {
    const n = plural(selectedCount, 'file')
    const text = action === 'block'
      ? `Block ${n}? They will be turned off on every computer.`
      : `Delete ${n} from the library? The files move to the server’s quarantine folder. Blocked files are skipped: unblock them first. Computers that already have them keep their copies.`
    if (await confirm(text, action === 'block' ? 'Block' : 'Delete', true)) await bulk(action)
  }
  const remove = async (f: LibraryFile) => {
    const blockedText = `${f.family} ${f.style} is blocked. Unblock it first, then delete it.`
    if (f.blocked) return setError(blockedText)
    const text = `Delete ${f.family} ${f.style} from the library? The file moves to the server’s quarantine folder. Computers that already have it keep their copy.`
    if (!(await confirm(text, 'Delete', true))) return
    setError('')
    try {
      await api.deleteFile(f.hash)
      toast(`Deleted ${f.family} ${f.style}.`)
      setSel({ ...selection, hashes: selection.hashes.filter((h) => h !== f.hash) })
      await state.reload(true)
    } catch (e) {
      setError(e instanceof ApiError && e.code === 'blocked' ? blockedText : errorText(e))
    }
  }

  const replace = (f: LibraryFile) => state.setData((p) => ({ ...p, items: p.items.map((r) => (r.hash === f.hash ? f : r)) }))
  const run = async (fn: () => Promise<LibraryFile>, done?: (f: LibraryFile) => void) => {
    setError('')
    try {
      const f = await fn()
      replace(f)
      done?.(f)
    } catch (e) { setError(errorText(e)) }
  }

  const upload = async (e: ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files?.length) return
    setUploading(true)
    setError('')
    // ponytail: one request for all files, so chips show an indeterminate bar, then all finish together.
    const names = Array.from(files, (f) => f.name)
    const mark = (state: Upload['state']) => setUploads(names.map((name) => ({ name, state })))
    mark('busy')
    try {
      const added = await api.upload(files)
      mark('done')
      toast(added.length ? `Added ${plural(added.length, 'font file')}.` : 'Those files are already in the library.')
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
    const blocked = !f.blocked
    void run(() => api.block(f.hash, blocked), () =>
      toast(`${blocked ? 'Blocked' : 'Unblocked'} ${f.family} ${f.style}.`, { undo: () => api.block(f.hash, !blocked).then(replace) }))
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
      {error && <ErrorBox message={error} />}
      {selectedCount > 0 && (
        <div className="toolbar" role="group" aria-label="Selected files">
          <span className="meta">
            {selection.all ? `All ${plural(total, 'result')} selected` : `${plural(selectedCount, 'file')} selected`}
          </span>
          {!selection.all && state.data && total > state.data.items.length && state.data.items.every((f) => isSelected(f.hash)) && (
            <button type="button" className="link-btn small" onClick={() => setSel({ query, hashes: [], all: true })}>Select all {total} results</button>
          )}
          <button type="button" className="link-btn small" onClick={clearSelection}>Clear selection</button>
          <span className="row-6">
            <button type="button" className="btn btn-xs" onClick={() => setDelivering('selected')}>Deliver…</button>
            <button type="button" className="btn btn-xs" onClick={() => void bulkAsk('block')}>Block</button>
            <button type="button" className="btn btn-xs" onClick={() => void bulk('unblock')}>Unblock</button>
            <button type="button" className="btn btn-xs btn-danger" onClick={() => void bulkAsk('delete')}>Delete</button>
          </span>
        </div>
      )}
      <Loaded state={state} skeleton={<TableSkeleton head={SKELETON_HEAD} minWidth={1020} rows={3} />}>{({ items: shown, total }) => (
          <>
            <Table caption="Font files in the shared library" minWidth={1180} head={[
              <input key="sel" type="checkbox" className="check" aria-label="Select every file on this page"
                checked={shown.length > 0 && shown.every((f) => isSelected(f.hash))} onChange={() => togglePage(shown)} />,
              { node: <SortHead label="Family" k="family" sort={sort} onSort={setSort} defaultDir="asc" />, sort: sortAria(sort, 'family') },
              ...HEAD.slice(1, 4),
              { node: <SortHead label="Added" k="uploaded" sort={sort} onSort={setSort} defaultDir="desc" />, sort: sortAria(sort, 'uploaded') },
              { node: <SortHead label="Size" k="size" sort={sort} onSort={setSort} defaultDir="desc" />, sort: sortAria(sort, 'size') },
              ...HEAD.slice(6),
            ]}>
              {shown.map((f, i) => (
                <motion.tr key={f.hash} {...row(i, shown.length)}>
                  <td>
                    <input type="checkbox" className="check" aria-label={`Select ${f.family} ${f.style}`} checked={isSelected(f.hash)} onChange={() => toggle(f.hash)} />
                  </td>
                  <td>
                    <span className="stack-2">
                      <span className={f.blocked ? 'lib-name lib-blocked' : 'lib-name'}>{f.family}</span>
                      <span className="meta">{f.style} · v{f.version} · {f.filename}</span>
                    </span>
                  </td>
                  <td>
                    <select className={`license tone-lic-${f.license}`} value={f.license} aria-label={`License for ${f.family} ${f.style}`}
                      onChange={(e) => void run(() => api.setLicense(f.hash, e.target.value as License),
                        (u) => toast(`License set to ${LICENSES.find(([k]) => k === u.license)?.[1] ?? u.license} for ${u.family} ${u.style}.`))}>
                      {LICENSES.map(([k, label]) => <option key={k} value={k}>{label}</option>)}
                    </select>
                  </td>
                  <td>{deliveredText(f.delivered_to)}</td>
                  <td>{plural(f.active_on, 'user')}</td>
                  <td className="ink-2 nowrap">{fmtDate(f.uploaded_at)}</td>
                  <td className="ink-2 nowrap">{kb(f.size)}</td>
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
                      <button type="button" className="btn btn-xs btn-danger" aria-label={`Delete ${f.family} ${f.style}`} onClick={() => void remove(f)}>Delete</button>
                    </span>
                  </td>
                </motion.tr>
              ))}
            </Table>
            {total === 0 && (query
              ? <Empty>No fonts match.</Empty>
              : <Empty title="Your shared library is empty"
                  action={<button type="button" className="btn btn-primary" disabled={uploading} onClick={() => fileInput.current?.click()}>Upload fonts</button>}>
                  Upload the fonts your team should have.
                </Empty>)}
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
        {delivering === 'selected' && (
          <DeliverDialog key="selected" title={`Deliver ${plural(selectedCount, 'file')}`} onClose={() => setDelivering(undefined)}
            onSubmit={async (target, activate) => { setDelivering(undefined); await bulk('deliver', target, activate) }} />
        )}
        {delivering && delivering !== 'selected' && (
          <DeliverDialog key={delivering.hash} title={`Deliver ${delivering.family} ${delivering.style}`} current={delivering.delivered_to}
            onClose={() => setDelivering(undefined)}
            onSubmit={async (target, activate) => {
              replace(await api.deliver(delivering.hash, target, activate))
              toast(`Delivering ${delivering.family} ${delivering.style} to ${target === 'all' ? 'everyone' : plural(target.length, 'user')}.`)
              setDelivering(undefined)
            }} />
        )}
      </AnimatePresence>
      {confirmDialog}
    </div>
  )
}

const pickSkeleton = <div className="stack-10" style={{ padding: '8px 0' }}>{[60, 45, 70].map((w) => <Skel key={w} w={`${w}%`} />)}</div>

/** Sets who gets the file(s): the ticked users replace the current recipients. Without `current` (several files)
 * nothing is preselected, so one click cannot send every selected file to everyone. */
function DeliverDialog({ title, current, onClose, onSubmit }: {
  title: string; current?: LibraryFile['delivered_to']; onClose: () => void; onSubmit: (target: 'all' | string[], activate: boolean) => Promise<void>
}) {
  // Delivered to nobody ('none') preselects nothing, like several files.
  const [mode, setMode] = useState<'all' | 'pick' | undefined>(current === 'all' ? 'all' : Array.isArray(current) ? 'pick' : undefined)
  const [picked, setPicked] = useState<string[]>(Array.isArray(current) ? current : [])
  const [activate, setActivate] = useState(false)
  const [q, setQ] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const query = useDebounced(q.trim(), 150)
  const [page, setPage] = usePage(query)
  const installs = useLoad(() => (mode === 'pick' ? api.installs(query, 'all', page) : Promise.resolve<Page<never>>({ items: [], total: 0 })), [mode, query, page])

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      await onSubmit(mode === 'all' ? 'all' : picked, activate)
    } catch (err) {
      setError(errorText(err))
      setBusy(false)
    }
  }

  return (
    <Dialog labelledBy="dlg-title" onClose={onClose}>
      <form className="stack-12" onSubmit={submit} noValidate>
        <h2 id="dlg-title" className="h2">{title}</h2>
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
            <span className="meta">{picked.length ? `${plural(picked.length, 'user')} selected` : 'Nobody selected: it stays in the library but goes to no one.'}</span>
          </motion.div>
        )}
        </AnimatePresence>
        <label className="radio">
          <input type="checkbox" className="check" checked={activate} onChange={(e) => setActivate(e.target.checked)} />
          <span>Also turn it on for them</span>
        </label>
        <span className="small mute lh">These users replace the current list. People you leave out stop getting it, including on new installs, but computers that already have the file keep it.</span>
        {error && <span role="alert" className="form-error">{error}</span>}
        <div className="row-8 end">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn btn-primary" disabled={busy || !mode}>{busy && <Spin />}{busy ? 'Delivering…' : 'Deliver'}</button>
        </div>
      </form>
    </Dialog>
  )
}
