// Main window layout: DeskSidebar + routed page, plus the window-wide font file drop (import).
import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react'
import { Link, useLocation, useNavigate, useOutlet, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import {
  api, inTauri, on, type ActivationResult, type Collection, type Folder, type FolderNode, type LibraryCounts, type License, type ProjectSet, type Tag,
} from './api'
import { useAddFonts } from './fontActions'
import { useContextMenu, type MenuItem } from './menu'
import { dur, easeOut, pop, route, spring } from './motion'
import { useApp } from './state'
import { ago, basename, collectionTree, Dialog, errMsg, initials, keysBusy, LICENSE_LABEL, Num, plural, PromptDialog, Spinner } from './ui'

/** The highlight behind the current sidebar entry; glides between entries via the shared layoutId. */
const Hi = ({ id }: { id: string }) => <motion.span layoutId={id} className="side-hi" transition={spring.glide} />

const LIC_DOT: Record<License, string> = { licensed: 'var(--ok-dot)', personal: 'var(--accent)', client: 'var(--violet-dot)', unknown: 'var(--amber)' }
const LIC_NAME: Record<License, string> = { ...LICENSE_LABEL, unknown: 'Unknown' }

/** Inside `dir` (not `dir` itself), for either path separator. */
const isUnder = (path: string | null, dir: string) => !!path && (path.startsWith(dir + '\\') || path.startsWith(dir + '/'))
const countHidden = (nodes: FolderNode[]): number => nodes.reduce((n, x) => n + (x.hidden ? 1 : 0) + countHidden(x.children), 0)

type NodeMenu = (e: ReactMouseEvent<Element>, node: FolderNode, folder?: Folder) => void

/** Like FontBase: a filled dot when everything inside is on, a ring in the "on" colour when some of it is,
 *  an empty ring when none is. `extra` adds a folder state (offline / scanning). */
function OnDot({ on, total, extra }: { on: number; total: number; extra?: string }) {
  const state = on <= 0 || total <= 0 ? 'none' : on >= total ? 'all' : 'some'
  const label = state === 'all' ? 'All on' : state === 'some' ? `${on} of ${total} on` : 'None on'
  return (
    <span className={`on-dot ${state}${extra ? ` ${extra}` : ''}`} title={label}>
      <span className="sr-only">{label}</span>
    </span>
  )
}

/** A folder and its subfolders; opens by itself when the current folder is inside it. Hidden ones show only with `showHidden`. */
function TreeNode({ node, depth, folder, showHidden, onMenu }: { node: FolderNode; depth: number; folder?: Folder; showHidden: boolean; onMenu: NodeMenu }) {
  const [params] = useSearchParams()
  const selected = params.get('path')
  const [open, setOpen] = useState(() => isUnder(selected, node.path))
  useEffect(() => void (isUnder(selected, node.path) && setOpen(true)), [selected, node.path])
  const here = selected === node.path
  const status = folder?.status
  const kids = node.children.filter((c) => showHidden || !c.hidden)
  return (
    <>
      <div className={node.hidden ? 'tree-row is-hidden' : 'tree-row'} style={{ paddingLeft: depth * 12 }}>
        {kids.length ? (
          <button type="button" className="tree-caret" aria-expanded={open} aria-label={`${open ? 'Collapse' : 'Expand'} ${node.name}`} onClick={() => setOpen((o) => !o)}>
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M3 1.5 6.5 5 3 8.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" /></svg>
          </button>
        ) : <span className="tree-caret" aria-hidden="true" />}
        <Link
          to={`/?path=${encodeURIComponent(node.path)}`} className="side-item" aria-current={here ? 'true' : undefined} title={node.path}
          onContextMenu={(e) => onMenu(e, node, folder)}
        >
          {here && <Hi id="side-item-hi" />}
          <OnDot on={node.activeCount} total={node.fontCount} extra={status === 'offline' || status === 'scanning' ? status : undefined} />
          <span className="ellipsis" style={{ flex: 1 }}>{node.name}</span>
          {node.hidden && <span className="tree-flag" title="Its fonts are left out of the library">HIDDEN</span>}
          {node.autoActivate && <span className="tree-auto" title="New fonts here are turned on automatically">AUTO</span>}
          {status === 'offline' ? (
            <span className="side-count" style={{ color: 'var(--warn)' }}>Offline</span>
          ) : status === 'scanning' ? (
            <span className="side-count">Scanning…</span>
          ) : (
            <span className="side-count"><Num value={node.fontCount} /></span>
          )}
        </Link>
      </div>
      {open && kids.map((c) => <TreeNode key={c.path} node={c} depth={depth + 1} showHidden={showHidden} onMenu={onMenu} />)}
    </>
  )
}

const JSON_FILES = [{ name: 'Collections (JSON)', extensions: ['json'] }]

type Prompt = { title: string; label: string; initial?: string; confirm: string; run: (value: string) => Promise<unknown> }
type Confirm = { title: string; body: string; action: string; run: () => Promise<unknown> }

function Sidebar() {
  const { app, libVersion, organizeCount, setOrganizeCount, activate, toast, bump } = useApp()
  const [counts, setCounts] = useState<LibraryCounts | null>(null)
  const [folders, setFolders] = useState<Folder[]>([])
  const [tree, setTree] = useState<FolderNode[]>([])
  const [collections, setCollections] = useState<Collection[]>([])
  const [tags, setTags] = useState<Tag[]>([])
  const [sets, setSets] = useState<ProjectSet[]>([])
  const [showHidden, setShowHidden] = useState(false)
  const [prompt, setPrompt] = useState<Prompt | null>(null)
  const [confirm, setConfirm] = useState<Confirm | null>(null)
  const [busy, setBusy] = useState(false)
  const [params] = useSearchParams()
  const { pathname } = useLocation()
  const navigate = useNavigate()
  const { show, menu } = useContextMenu('Sidebar actions')
  const current = (to: string) => (to === '/' ? pathname === '/' || pathname.startsWith('/family/') : pathname.startsWith(to))
  const bin = app.platform === 'macos' ? 'Trash' : 'Recycle Bin'
  const add = useAddFonts()

  useEffect(() => {
    api.libraryCounts().then(setCounts, () => {})
    api.listFolders().then(setFolders, () => {})
    api.folderTree().then(setTree, () => {})
    api.listCollections().then(setCollections, () => {})
    api.listTags().then(setTags, () => {})
    api.listProjectSets().then(setSets, () => {})
  }, [libVersion])
  useEffect(() => on('folder-status', (f) => setFolders((all) => all.map((x) => (x.id === f.id ? f : x)))), [])
  // ponytail: one plan at startup for the badge; the Auto-organize page refreshes it. Needs a cheap count command if planning gets slow.
  useEffect(() => {
    if (organizeCount === null) api.organizePlan(false).then((p) => setOrganizeCount(p.changes.length), () => {})
  }, [organizeCount, setOrganizeCount])

  const nav: [string, string, number | null][] = [
    ['/', 'Library', counts ? counts.library : null],
    ['/playground', 'Playground', null],
    ['/sets', 'Project sets', sets.length || null],
    ['/google', 'Google Fonts', null],
    ['/organize', 'Auto-organize', organizeCount || null],
    ['/pdf', 'Fonts in a document', null],
    ['/stats', 'Stats', null],
    ['/settings', 'Settings', null],
  ]
  const licCount = (l: License) => counts?.licenses[l] ?? 0
  const systemCount = counts?.system ?? 0
  const sel = (k: string, v: string | number) => params.get(k) === String(v)
  const c = app.connection
  const sync =
    c.status === 'connected'
      ? { color: 'var(--ok)', text: `Synced · ${ago(c.lastSync)}` }
      : c.status === 'offline'
        ? { color: 'var(--warn)', text: 'Offline · will sync later' }
        : { color: 'var(--bad)', text: 'Can’t reach server' }

  const fail = (e: unknown) => toast(errMsg(e))
  const saveCollections = (p: Promise<Collection[]>) => p.then((cs) => (setCollections(cs), bump()), fail)
  /** Activation, then a refresh and a one-line result. */
  async function turn(fn: () => Promise<ActivationResult[]>, done: string) {
    const res = await activate(fn)
    bump()
    if (res) toast(`${done} · ${plural(res.filter((r) => r.ok).length, 'font')}`)
  }
  const newCollection = () =>
    setPrompt({ title: 'New collection', label: 'Name', confirm: 'Create', run: (name) => saveCollections(api.createCollection(name, null)) })

  // FontBase shortcuts: F add fonts, D add a folder (copied), W watch a folder, C new collection.
  const keys = useRef({ add, newCollection })
  keys.current = { add, newCollection }
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey || e.repeat || keysBusy(e)) return
      const { add, newCollection } = keys.current
      const run = ({ f: add.addFonts, d: add.addFolder, w: add.watchFolder, c: newCollection } as Record<string, () => unknown>)[e.key.toLowerCase()]
      if (run) (e.preventDefault(), run())
    }
    addEventListener('keydown', onKey)
    return () => removeEventListener('keydown', onKey)
  }, [])

  async function exportCollections() {
    try {
      const dest = await api.pickSaveFile('typeshelf-collections.json', JSON_FILES)
      if (!dest) return
      const r = await api.exportCollections(dest)
      toast(`Exported ${plural(r.collections, 'collection')} to ${basename(dest)}`, { label: 'Show', run: () => void api.showInFolder(dest).catch(fail) })
    } catch (e) {
      fail(e)
    }
  }
  async function importCollections() {
    try {
      const path = await api.pickFile(JSON_FILES)
      if (!path) return
      const r = await api.importCollections(path)
      bump()
      toast(
        `Imported ${plural(r.collections, 'collection')} with ${plural(r.families, 'font family', 'font families')}` +
          (r.missing ? ` · ${plural(r.missing, 'font')} not in your library` : ''),
      )
    } catch (e) {
      fail(e)
    }
  }

  async function exportZip(col: Collection) {
    const dest = await api.pickSaveFile(`${col.name.replace(/[\\/:*?"<>|]/g, '-')}.zip`, [{ name: 'ZIP archive', extensions: ['zip'] }]).catch((e) => (fail(e), null))
    if (!dest) return
    try {
      const r = await api.exportCollection(col.id, dest)
      toast(`Exported ${plural(r.files, 'font file')} to ${basename(r.path)}`, { label: 'Show', run: () => void api.showInFolder(r.path).catch(fail) })
    } catch (e) {
      fail(e)
    }
  }

  function collectionMenu(e: ReactMouseEvent<Element>, col: Collection) {
    const items: MenuItem[] = [
      { label: 'Activate all', disabled: !col.count, onSelect: () => turn(() => api.setCollectionActive(col.id, true), `${col.name} turned on`) },
      { label: 'Deactivate all', disabled: !col.count, onSelect: () => turn(() => api.setCollectionActive(col.id, false), `${col.name} turned off`) },
      'sep',
      { label: 'Export as ZIP…', disabled: !col.count, onSelect: () => exportZip(col) },
      { label: 'Print / Save as PDF…', disabled: !col.count, onSelect: () => navigate(`/print/collection/${col.id}`) },
    ]
    if (!col.builtin)
      items.push(
        'sep',
        {
          label: 'Add sub-collection…',
          onSelect: () => setPrompt({ title: `New collection in ${col.name}`, label: 'Name', confirm: 'Create', run: (name) => saveCollections(api.createCollection(name, col.id)) }),
        },
        {
          label: 'Rename…',
          onSelect: () => setPrompt({ title: 'Rename collection', label: 'Name', initial: col.name, confirm: 'Rename', run: (name) => saveCollections(api.renameCollection(col.id, name)) }),
        },
        {
          label: 'Delete…', danger: true,
          onSelect: () => setConfirm({
            title: `Delete “${col.name}”?`, body: 'The fonts stay in your library; only the collection goes.', action: 'Delete collection',
            run: async () => {
              await saveCollections(api.deleteCollection(col.id))
              if (sel('collection', col.id)) navigate('/')
            },
          }),
        },
      )
    show(e, items)
  }

  // Order and wording follow FontBase's folder menu.
  const folderMenu: NodeMenu = (e, node, folder) => {
    const items: MenuItem[] = [
      { label: 'Activate', disabled: !node.fontCount, onSelect: () => turn(() => api.setFolderActive(node.path, true), `${node.name} turned on`) },
      { label: 'Deactivate', disabled: !node.activeCount, onSelect: () => turn(() => api.setFolderActive(node.path, false), `${node.name} turned off`) },
      {
        label: `Autoactivate: ${node.autoActivate ? 'Disable' : 'Enable'}`,
        onSelect: () => turn(() => api.setFolderAuto(node.path, !node.autoActivate), node.autoActivate ? 'Autoactivate off' : 'Autoactivate on'),
      },
      'sep',
      {
        label: node.hidden ? 'Show folder' : 'Hide folder',
        onSelect: () => api.setFolderHidden(node.path, !node.hidden).then((t) => {
          setTree(t)
          bump()
          toast(node.hidden ? `${node.name} is back in the library` : `${node.name} is hidden; its fonts are left out of the library`)
        }, fail),
      },
      { label: 'Show in folder', onSelect: () => void api.showInFolder(node.path).catch(fail) },
      'sep',
    ]
    if (folder)
      items.push({
        label: 'Remove from Typeshelf',
        onSelect: () => api.removeFolder(folder.id).then((fs) => {
          setFolders(fs)
          bump()
          toast(`Removed ${node.name} from Typeshelf. The files are still on disk.`)
          if (sel('path', node.path) || isUnder(params.get('path'), node.path)) navigate('/')
        }, fail),
      })
    items.push({
      label: 'Delete…', danger: true,
      onSelect: () => setConfirm({
        title: `Delete “${node.name}”?`,
        body: `The folder and everything in it goes to the ${bin}: ${plural(node.fontCount, 'font')} and any other files. Its fonts are turned off and leave your library.`,
        action: `Move to ${bin}`,
        run: async () => {
          const r = await api.deleteFolder(node.path)
          toast(`Moved ${node.name} to the ${bin}${r.failed.length ? ` · ${plural(r.failed.length, 'file')} couldn’t be moved` : ''}`)
          if (sel('path', node.path) || isUnder(params.get('path'), node.path)) navigate('/')
          bump()
        },
      }),
    })
    show(e, items)
  }

  async function runConfirm() {
    if (!confirm) return
    setBusy(true)
    try {
      await confirm.run()
      setConfirm(null)
    } catch (e) {
      fail(e)
    } finally {
      setBusy(false)
    }
  }

  const hiddenCount = countHidden(tree)
  return (
    <nav aria-label="Main" className="sidebar">
      <div className="side-group">
        {nav.map(([to, label, count]) => (
          <Link key={to} to={to} className="side-nav" aria-current={current(to) ? 'page' : undefined}>
            {current(to) && <Hi id="side-nav-hi" />}
            <span>{label}</span>
            <span className="side-count">{count !== null && <Num value={count} />}</span>
          </Link>
        ))}
      </div>
      <div className="side-group">
        <span className="side-head">Folders</span>
        {folders.map((f) => {
          const node = tree.find((n) => n.path === f.path) ?? { path: f.path, name: f.path, fontCount: f.fontCount, activeCount: 0, autoActivate: false, hidden: false, children: [] }
          return node.hidden && !showHidden ? null : <TreeNode key={f.id} node={node} depth={0} folder={f} showHidden={showHidden} onMenu={folderMenu} />
        })}
        {!!systemCount && (
          <Link to="/?system=1" className="side-item" aria-current={sel('system', 1) ? 'true' : undefined} title="Fonts installed in the operating system (always on)">
            {sel('system', 1) && <Hi id="side-item-hi" />}
            <OnDot on={1} total={1} />
            <span className="ellipsis" style={{ flex: 1 }}>System fonts</span>
            <span className="side-count"><Num value={systemCount} /></span>
          </Link>
        )}
        {hiddenCount > 0 && (
          <button type="button" className="side-item side-toggle" aria-pressed={showHidden} onClick={() => setShowHidden((s) => !s)}>
            <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
              <path d="M1.5 8s2.4-4.5 6.5-4.5S14.5 8 14.5 8s-2.4 4.5-6.5 4.5S1.5 8 1.5 8Z" /><circle cx="8" cy="8" r="2" />
              {!showHidden && <path d="M2.5 13.5l11-11" />}
            </svg>
            <span style={{ flex: 1 }}>{showHidden ? 'Hide hidden folders' : 'Show hidden folders'}</span>
            <span className="side-count">{hiddenCount}</span>
          </button>
        )}
      </div>
      <div className="side-group">
        <div className="side-head-row">
          <span className="side-head">Collections</span>
          <span style={{ display: 'flex', gap: 2 }}>
            <button
              type="button" className="side-add" aria-label="Collection options" title="Import or export collections" aria-haspopup="menu"
              onClick={(e) => show(e, [
                { label: 'Export collections…', onSelect: exportCollections },
                { label: 'Import collections…', onSelect: importCollections },
              ], true)}
            >⋯</button>
            <button type="button" className="side-add" aria-label="New collection" title="New collection (C)" onClick={newCollection}>+</button>
          </span>
        </div>
        {collectionTree(collections).map(({ c: col, depth }) => (
          <Link
            key={col.id} to={`/?collection=${col.id}`} className="side-item" aria-current={sel('collection', col.id) ? 'true' : undefined}
            data-drop-collection={col.builtin === 'recent' ? undefined : col.id} data-drop-name={col.name}
            style={{ paddingLeft: 10 + depth * 14 }} onContextMenu={(e) => collectionMenu(e, col)}
          >
            {sel('collection', col.id) && <Hi id="side-item-hi" />}
            {depth > 0 && <span aria-hidden="true" className="coll-branch" />}
            <OnDot on={col.active} total={col.count} />
            <span className="ellipsis" style={{ flex: 1 }}>{col.name}</span>
            <span className="side-count"><Num value={col.count} /></span>
          </Link>
        ))}
      </div>
      {tags.length > 0 && (
        <div className="side-group">
          <span className="side-head">Tags</span>
          {tags.map((t) => (
            <Link key={t.name} to={`/?tag=${encodeURIComponent(t.name)}`} className="side-item" aria-current={sel('tag', t.name) ? 'true' : undefined}>
              {sel('tag', t.name) && <Hi id="side-item-hi" />}
              <span aria-hidden="true" className="faint" style={{ width: 10, textAlign: 'center' }}>#</span>
              <span className="ellipsis" style={{ flex: 1 }}>{t.name}</span>
              <span className="side-count"><Num value={t.count} /></span>
            </Link>
          ))}
        </div>
      )}
      <div className="side-group">
        <span className="side-head">Licenses</span>
        {(Object.keys(LIC_NAME) as License[]).map((l) => (
          <Link key={l} to={`/?license=${l}`} className="side-item" aria-current={sel('license', l) ? 'true' : undefined}>
            {sel('license', l) && <Hi id="side-item-hi" />}
            <span aria-hidden="true" className="lic-dot" style={{ background: LIC_DOT[l] }} />
            <span style={{ flex: 1 }}>{LIC_NAME[l]}</span>
            <span className="side-count"><Num value={licCount(l)} /></span>
          </Link>
        ))}
      </div>
      <Link to="/settings" className="user-chip">
        <span aria-hidden="true" className="avatar">{initials(app.name)}</span>
        <span style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
          <span className="ellipsis" style={{ fontWeight: 500, fontSize: 13 }}>{app.name}</span>
          <span className="sync-text" style={{ fontSize: 12, color: sync.color }}>{sync.text}</span>
        </span>
      </Link>

      {menu}
      <PromptDialog
        open={!!prompt} title={prompt?.title ?? ''} label={prompt?.label ?? ''} initial={prompt?.initial} confirm={prompt?.confirm ?? 'OK'}
        onClose={() => setPrompt(null)} onSubmit={(v) => (prompt?.run(v), setPrompt(null))}
      />
      <Dialog open={!!confirm} onClose={() => !busy && setConfirm(null)} title={confirm?.title ?? ''}>
        <p>{confirm?.body}</p>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => setConfirm(null)} disabled={busy}>Cancel</button>
          <button type="button" className="btn primary danger-fill" onClick={runConfirm} disabled={busy} aria-busy={busy}>{busy && <Spinner />}{confirm?.action}</button>
        </div>
      </Dialog>
    </nav>
  )
}

/** Dropping font files or folders anywhere on the window imports them (the PDF page takes its own drops). */
function FileDrop() {
  const { pathname } = useLocation()
  const [over, setOver] = useState(false)
  const here = useRef(pathname)
  here.current = pathname
  const { importPaths } = useAddFonts()
  const run = useRef(importPaths)
  run.current = importPaths

  useEffect(() => {
    if (inTauri) {
      const un = getCurrentWebview().onDragDropEvent((e) => {
        if (here.current === '/pdf') return setOver(false)
        const t = e.payload.type
        if (t === 'enter' || t === 'over') setOver(true)
        else if (t === 'leave') setOver(false)
        else if (t === 'drop') {
          setOver(false)
          run.current(e.payload.paths)
        }
      })
      return () => void un.then((f) => f())
    }
    // Browser (mock): HTML5 file drops. Browsers hide real paths, so file names stand in.
    let depth = 0
    const files = (e: DragEvent) => !!e.dataTransfer?.types.includes('Files') && here.current !== '/pdf'
    const enter = (e: DragEvent) => {
      if (!files(e)) return
      e.preventDefault()
      depth++
      setOver(true)
    }
    const dragover = (e: DragEvent) => void (files(e) && e.preventDefault())
    const leave = (e: DragEvent) => {
      if (files(e) && --depth <= 0) (depth = 0, setOver(false))
    }
    const drop = (e: DragEvent) => {
      if (!files(e)) return
      e.preventDefault()
      depth = 0
      setOver(false)
      run.current([...(e.dataTransfer?.files ?? [])].map((f) => f.name))
    }
    addEventListener('dragenter', enter)
    addEventListener('dragover', dragover)
    addEventListener('dragleave', leave)
    addEventListener('drop', drop)
    return () => {
      removeEventListener('dragenter', enter)
      removeEventListener('dragover', dragover)
      removeEventListener('dragleave', leave)
      removeEventListener('drop', drop)
    }
  }, [])

  return (
    <AnimatePresence>
      {over && (
        <motion.div className="drop-overlay" aria-hidden="true" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: dur.base, ease: easeOut }}>
          <motion.div className="drop-card" {...pop} transition={spring.panel}>
            <svg width="40" height="40" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 4v11M7 10l5 5 5-5M5 19h14" />
            </svg>
            <strong style={{ fontSize: 18 }}>Drop to add fonts</strong>
            <span className="muted" style={{ fontSize: 13, textAlign: 'center' }}>Font files and folders are copied into “Imported fonts” and added to your library.</span>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

export function Shell() {
  const outlet = useOutlet()
  const { pathname } = useLocation()
  const main = useRef<HTMLElement>(null)
  // The content column is the scroller now, so each page starts at its top.
  // (Block body: newer Chromium's scrollTo returns a Promise, which React would take as a cleanup.)
  useEffect(() => {
    main.current?.scrollTo(0, 0)
  }, [pathname])
  return (
    <div className="shell">
      <div className="shell-side">
        <Sidebar />
      </div>
      <main className="shell-main" ref={main}>
        <AnimatePresence mode="popLayout" initial={false}>
          <motion.div key={pathname} className="route" {...route}>{outlet}</motion.div>
        </AnimatePresence>
      </main>
      <FileDrop />
    </div>
  )
}
