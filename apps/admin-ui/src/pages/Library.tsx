import { useRef, useState, type ChangeEvent, type FormEvent } from 'react'
import { Link, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, sortAria, ApiError, type BulkAction, type LibraryFile, type LibrarySort, type License, type Page, type Sort } from '../api'
import { base, fast, row, spring } from '../motion'
import { toast } from '../toast'
import { Hint } from '../Hint'
import { t, tk } from '../i18n'
import {
  Badge, Chips, Dialog, Empty, ErrorBox, errMsg, Loaded, PageHeader, Pager, plural, Skel, SortHead, Spin, Swap, Table, TableSkeleton, useConfirm, useDebounced, useLoad, usePage,
} from '../ui'

// The two columns that need an explanation carry a Hint; the rest are plain labels.
const head = () => [
  t('Family'), t('License'),
  <>{t('Delivered to')} <Hint text={t('Pick everyone or specific users. The file is downloaded to their app and appears in their library. Choose whether it’s also turned on.')} /></>,
  t('Active on'),
  <>{t('Status')} <Hint text={t('A blocked font is turned off on every computer and can’t be turned on again, even if a user has their own copy.')} /></>,
  <span key="act" className="sr-only">{t('Actions')}</span>,
]
type Upload = { name: string; state: 'busy' | 'done' | 'error' }
/** Picked files for this search: hashes on any page, or `all` = every result of the search. */
type Selection = { query: string; hashes: string[]; all: boolean }
const doneText = (a: BulkAction, files: string) =>
  a === 'deliver' ? t('Delivered {files}.', { files }) : a === 'block' ? t('Blocked {files}.', { files })
    : a === 'unblock' ? t('Unblocked {files}.', { files }) : t('Deleted {files}.', { files })

const LICENSES: [License, string][] = [['licensed', tk('Licensed')], ['personal', tk('Personal use')], ['client', tk('Client-provided')], ['unknown', tk('Unknown')]]
const ACCEPT = '.otf,.ttf,.ttc,.otc,.woff,.woff2'

const SORTS: [LibrarySort, string][] = [['family', tk('Family A–Z')], ['uploaded', tk('Newest')], ['size', tk('Largest')]]
const deliveredText = (to: LibraryFile['delivered_to']) =>
  to === 'all' ? t('Everyone') : to === 'none' || to.length === 0 ? t('Nobody') : plural(to.length, 'user')
const kb = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1e3))} KB`)

export function Library() {
  const [params] = useSearchParams()
  const urlQ = params.get('q') ?? ''
  const [q, setQ] = useState(urlQ)
  // The palette can change ?q= while this page stays mounted.
  const [lastUrlQ, setLastUrlQ] = useState(urlQ)
  if (urlQ !== lastUrlQ) { setLastUrlQ(urlQ); setQ(urlQ) }
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
  const HEAD = head()
  const SKELETON_HEAD = [<span key="sel" className="sr-only">{t('Select')}</span>, ...HEAD]
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
      const files = plural(r.changed, 'file')
      const done = action === 'deliver' && Array.isArray(target) && target.length === 0 ? t('Removed delivery of {files}.', { files }) : doneText(action, files)
      toast(done + (r.skipped ? ' ' + (r.skipped === 1 ? t('Skipped 1 that was already done, blocked or gone.') : t('Skipped {n} that were already done, blocked or gone.', { n: r.skipped })) : ''))
      if (r.failed) setError(t('{files} couldn’t be deleted. Check the server log, then try again.', { files: plural(r.failed, 'file') }))
    } catch (e) {
      setError(errMsg(e))
    } finally {
      // Some files may have changed even when the request failed part way.
      clearSelection()
      await state.reload(true)
    }
  }
  const bulkAsk = async (action: 'block' | 'delete') => {
    const n = plural(selectedCount, 'file')
    const text = action === 'block'
      ? t('Block {files}? They will be turned off on every computer.', { files: n })
      : t('Delete {files} from the library? The files move to the server’s quarantine folder. Blocked files are skipped: unblock them first. Computers that already have them keep their copies.', { files: n })
    if (await confirm(text, action === 'block' ? t('Block') : t('Delete'), true)) await bulk(action)
  }
  const remove = async (f: LibraryFile) => {
    const blockedText = t('{name} is blocked. Unblock it first, then delete it.', { name: `${f.family} ${f.style}` })
    if (f.blocked) return setError(blockedText)
    const text = t('Delete {name} from the library? The file moves to the server’s quarantine folder. Computers that already have it keep their copy.', { name: `${f.family} ${f.style}` })
    if (!(await confirm(text, t('Delete'), true))) return
    setError('')
    try {
      await api.deleteFile(f.hash)
      toast(t('Deleted {name}.', { name: `${f.family} ${f.style}` }))
      setSel({ ...selection, hashes: selection.hashes.filter((h) => h !== f.hash) })
      await state.reload(true)
    } catch (e) {
      setError(e instanceof ApiError && e.code === 'blocked' ? blockedText : errMsg(e))
    }
  }

  const replace = (f: LibraryFile) => state.setData((p) => ({ ...p, items: p.items.map((r) => (r.hash === f.hash ? f : r)) }))
  const run = async (fn: () => Promise<LibraryFile>, done?: (f: LibraryFile) => void) => {
    setError('')
    try {
      const f = await fn()
      replace(f)
      done?.(f)
    } catch (e) { setError(errMsg(e)) }
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
      toast(added.length ? t('Added {files}.', { files: plural(added.length, 'font file') }) : t('Those files are already in the library.'))
      await state.reload(true)
    } catch (err) {
      mark('error')
      setError(errMsg(err))
    } finally {
      setUploading(false)
      e.target.value = ''
      setTimeout(() => setUploads((u) => (u.some((x) => x.state === 'busy') ? u : [])), 2600)
    }
  }

  const block = async (f: LibraryFile) => {
    if (!f.blocked && !(await confirm(t('Block {name}? It will be turned off on every computer.', { name: `${f.family} ${f.style}` }), t('Block'), true))) return
    const blocked = !f.blocked
    void run(() => api.block(f.hash, blocked), () =>
      toast(blocked ? t('Blocked {name}.', { name: `${f.family} ${f.style}` }) : t('Unblocked {name}.', { name: `${f.family} ${f.style}` }), { undo: () => api.block(f.hash, !blocked).then(replace) }))
  }

  return (
    <div className="page gap-18">
      <PageHeader title={t('Shared library')}
        sub={t('Approved fonts you deliver to users’ apps.')}
        actions={
          <span className="row-8">
            <Link className="btn" to="/organize">{t('Auto-organize')}</Link>
            <button type="button" className="btn btn-primary" disabled={uploading} onClick={() => fileInput.current?.click()}>
              {uploading && <Spin />}{uploading ? t('Uploading…') : t('Upload fonts')}
            </button>
            <input ref={fileInput} type="file" multiple accept={ACCEPT} hidden onChange={upload} aria-label={t('Font files to upload')} />
          </span>
        }>
        {state.data && !query && <span className="sub">{plural(state.data.total, 'file')}</span>}
      </PageHeader>
      <div className="toolbar">
        <label htmlFor="lsearch" className="sr-only">{t('Search the shared library')}</label>
        <input id="lsearch" type="search" className="input grow" placeholder={t('Search family, style or file name')} value={q} onChange={(e) => setQ(e.target.value)} />
        <Chips label={t('Sort')} options={SORTS.map(([k, label]) => [k, t(label /* i18n-dynamic */)] as [LibrarySort, string])} value={sort?.key ?? 'family'}
          onChange={(k) => setSort({ key: k, dir: k === 'family' ? 'asc' : 'desc' })} />
      </div>
      <AnimatePresence>
        {uploads.length > 0 && (
          <motion.div className="upload-chips" aria-hidden="true" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0, y: -4, transition: fast }}>
            {uploads.map((u, i) => (
              <motion.span key={u.name + i} className={`upload-chip is-${u.state}`}
                initial={{ opacity: 0, y: 6, scale: 0.96 }} animate={{ opacity: 1, y: 0, scale: 1, transition: { ...spring, delay: Math.min(i, 8) * 0.04 } }}>
                <span className="row-between"><span className="ellipsis">{u.name}</span>
                  <Swap>{u.state === 'done' ? '✓' : u.state === 'error' ? t('Failed') : ''}</Swap>
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
        <div className="toolbar" role="group" aria-label={t('Selected files')}>
          <span className="meta">
            {selection.all ? t('All {results} selected', { results: plural(total, 'result') }) : t('{files} selected', { files: plural(selectedCount, 'file') })}
          </span>
          {!selection.all && state.data && total > state.data.items.length && state.data.items.every((f) => isSelected(f.hash)) && (
            <button type="button" className="link-btn small" onClick={() => setSel({ query, hashes: [], all: true })}>{t('Select all {total} results', { total })}</button>
          )}
          <button type="button" className="link-btn small" onClick={clearSelection}>{t('Clear selection')}</button>
          <span className="row-6">
            <button type="button" className="btn btn-xs" onClick={() => setDelivering('selected')}>{t('Deliver…')}</button>
            <button type="button" className="btn btn-xs" onClick={() => void bulkAsk('block')}>{t('Block')}</button>
            <button type="button" className="btn btn-xs" onClick={() => void bulk('unblock')}>{t('Unblock')}</button>
            <button type="button" className="btn btn-xs btn-danger" onClick={() => void bulkAsk('delete')}>{t('Delete')}</button>
          </span>
        </div>
      )}
      <Loaded state={state} skeleton={<TableSkeleton head={SKELETON_HEAD} minWidth={1020} rows={3} />}>{({ items: shown, total }) => (
          <>
            <Table caption={t('Font files in the shared library')} minWidth={1020} head={[
              <input key="sel" type="checkbox" className="check" aria-label={t('Select every file on this page')}
                checked={shown.length > 0 && shown.every((f) => isSelected(f.hash))} onChange={() => togglePage(shown)} />,
              { node: <SortHead label={t('Family')} k="family" sort={sort} onSort={setSort} defaultDir="asc" />, sort: sortAria(sort, 'family') },
              ...HEAD.slice(1, 4),
              ...HEAD.slice(4),
            ]}>
              {shown.map((f, i) => (
                <motion.tr key={f.hash} {...row(i, shown.length)}>
                  <td>
                    <input type="checkbox" className="check" aria-label={t('Select {name}', { name: `${f.family} ${f.style}` })} checked={isSelected(f.hash)} onChange={() => toggle(f.hash)} />
                  </td>
                  <td>
                    <span className="stack-2">
                      <span className={f.blocked ? 'lib-name lib-blocked' : 'lib-name'}>{f.family}</span>
                      <span className="meta">{f.style} · v{f.version} · {f.filename} · {kb(f.size)}</span>
                    </span>
                  </td>
                  <td>
                    <select className={`license tone-lic-${f.license}`} value={f.license} aria-label={t('License for {name}', { name: `${f.family} ${f.style}` })}
                      onChange={(e) => void run(() => api.setLicense(f.hash, e.target.value as License),
                        (u) => toast(t('License set to {license} for {name}.', { license: t(LICENSES.find(([k]) => k === u.license)?.[1] ?? u.license /* i18n-dynamic */), name: `${u.family} ${u.style}` })))}>
                      {LICENSES.map(([k, label]) => <option key={k} value={k}>{t(label /* i18n-dynamic */)}</option>)}
                    </select>
                  </td>
                  <td>{deliveredText(f.delivered_to)}</td>
                  <td>{plural(f.active_on, 'user')}</td>
                  <td>
                    {/* Block/unblock cross-fades the status badge. */}
                    <AnimatePresence mode="popLayout" initial={false}>
                      <motion.span key={String(f.blocked)} className="swap" initial={{ opacity: 0, scale: 0.9 }} animate={{ opacity: 1, scale: 1, transition: spring }}
                        exit={{ opacity: 0, scale: 0.9, transition: fast }}>
                        {f.blocked ? <Badge tone="bad">{t('Blocked')}</Badge> : <Badge tone="ok">{t('Published')}</Badge>}
                      </motion.span>
                    </AnimatePresence>
                  </td>
                  <td className="nowrap">
                    <span className="row-6 end">
                      <button type="button" className="btn btn-xs" disabled={f.blocked} onClick={() => setDelivering(f)}>{t('Deliver…')}</button>
                      <button type="button" className={f.blocked ? 'btn btn-xs' : 'btn btn-xs btn-danger'} onClick={() => void block(f)}>
                        <Swap>{f.blocked ? t('Unblock') : t('Block')}</Swap>
                      </button>
                      <button type="button" className="btn btn-xs btn-danger" aria-label={t('Delete {name}', { name: `${f.family} ${f.style}` })} onClick={() => void remove(f)}>{t('Delete')}</button>
                    </span>
                  </td>
                </motion.tr>
              ))}
            </Table>
            {total === 0 && (query
              ? <Empty>{t('No fonts match.')}</Empty>
              : <Empty title={t('Your shared library is empty')}
                  action={<button type="button" className="btn btn-primary" disabled={uploading} onClick={() => fileInput.current?.click()}>{t('Upload fonts')}</button>}>
                  {t('Upload the fonts your team should have.')}
                </Empty>)}
            <Pager label={t('Library pages')} page={page} total={total} onPage={setPage} />
          </>
      )}</Loaded>
      <AnimatePresence>
        {delivering === 'selected' && (
          <DeliverDialog key="selected" title={t('Deliver {files}', { files: plural(selectedCount, 'file') })} onClose={() => setDelivering(undefined)}
            onSubmit={async (target, activate) => { setDelivering(undefined); await bulk('deliver', target, activate) }} />
        )}
        {delivering && delivering !== 'selected' && (
          <DeliverDialog key={delivering.hash} title={t('Deliver {name}', { name: `${delivering.family} ${delivering.style}` })} current={delivering.delivered_to}
            onClose={() => setDelivering(undefined)}
            onSubmit={async (target, activate) => {
              replace(await api.deliver(delivering.hash, target, activate))
              toast(target === 'all' ? t('Delivering {name} to everyone.', { name: `${delivering.family} ${delivering.style}` }) : t('Delivering {name} to {users}.', { name: `${delivering.family} ${delivering.style}`, users: plural(target.length, 'user') }))
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
      setError(errMsg(err))
      setBusy(false)
    }
  }

  return (
    <Dialog labelledBy="dlg-title" onClose={onClose}>
      <form className="stack-12" onSubmit={submit} noValidate>
        <h2 id="dlg-title" className="h2">{title}</h2>
        <fieldset className="fieldset-plain stack-6">
          <legend className="sr-only">{t('Who gets it')}</legend>
          <label className="radio"><input type="radio" name="target" checked={mode === 'all'} onChange={() => setMode('all')} /><span>{t('Everyone, including new installs')}</span></label>
          <label className="radio"><input type="radio" name="target" checked={mode === 'pick'} onChange={() => setMode('pick')} /><span>{t('Only the users I pick')}</span></label>
        </fieldset>
        <AnimatePresence initial={false}>
        {mode === 'pick' && (
          <motion.div className="stack-6" initial={{ opacity: 0, y: -6 }} animate={{ opacity: 1, y: 0, transition: base }} exit={{ opacity: 0, transition: fast }}>
            <label htmlFor="dsearch" className="sr-only">{t('Search users')}</label>
            <input id="dsearch" type="search" className="input input-38" placeholder={t('Search users')} value={q} onChange={(e) => setQ(e.target.value)} />
            <div className="pick-list">
              <Loaded state={installs} label={t('Loading users…')} skeleton={pickSkeleton}>{({ items: shown }) => {
                if (shown.length === 0) return <Empty>{t('No users match.')}</Empty>
                return shown.map((u) => (
                  <label key={u.id} className="radio">
                    <input type="checkbox" className="check" checked={picked.includes(u.id)}
                      onChange={() => setPicked((p) => (p.includes(u.id) ? p.filter((x) => x !== u.id) : [...p, u.id]))} />
                    <span>{u.name} <span className="meta mono">{u.id}</span></span>
                  </label>
                ))
              }}</Loaded>
            </div>
            {installs.data && <Pager label={t('User pages')} page={page} total={installs.data.total} onPage={setPage} />}
            <span className="meta">{picked.length ? t('{users} selected', { users: plural(picked.length, 'user') }) : t('Nobody selected: it stays in the library but goes to no one.')}</span>
          </motion.div>
        )}
        </AnimatePresence>
        <label className="radio">
          <input type="checkbox" className="check" checked={activate} onChange={(e) => setActivate(e.target.checked)} />
          <span>{t('Also turn it on for them')}</span>
        </label>
        <span className="small mute lh">{t('These users replace the current list. People you leave out stop getting it, including on new installs, but computers that already have the file keep it.')}</span>
        {error && <span role="alert" className="form-error">{error}</span>}
        <div className="row-8 end">
          <button type="button" className="btn" onClick={onClose}>{t('Cancel')}</button>
          <button type="submit" className="btn btn-primary" disabled={busy || !mode}>{busy && <Spin />}{busy ? t('Delivering…') : t('Deliver')}</button>
        </div>
      </form>
    </Dialog>
  )
}
