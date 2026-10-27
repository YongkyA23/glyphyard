// Main window layout: DeskSidebar + routed page.
import { useEffect, useRef, useState } from 'react'
import { Link, useLocation, useOutlet, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, on, type Collection, type Family, type Folder, type License, type ProjectSet } from './api'
import { route, spring } from './motion'
import { useApp } from './state'
import { ago, initials, LICENSE_LABEL, Num } from './ui'

/** The highlight behind the current sidebar entry; glides between entries via the shared layoutId. */
const Hi = ({ id }: { id: string }) => <motion.span layoutId={id} className="side-hi" transition={spring.glide} />

const LIC_DOT: Record<License, string> = { licensed: 'var(--ok-dot)', personal: 'var(--accent)', client: 'var(--violet-dot)', unknown: 'var(--amber)' }
const LIC_NAME: Record<License, string> = { ...LICENSE_LABEL, unknown: 'Unknown' }

function Sidebar() {
  const { app, libVersion, organizeCount, setOrganizeCount } = useApp()
  const [families, setFamilies] = useState<Family[] | null>(null)
  const [systemCount, setSystemCount] = useState<number | null>(null)
  const [folders, setFolders] = useState<Folder[]>([])
  const [collections, setCollections] = useState<Collection[]>([])
  const [sets, setSets] = useState<ProjectSet[]>([])
  const [params] = useSearchParams()
  const { pathname } = useLocation()
  const current = (to: string) => (to === '/' ? pathname === '/' || pathname.startsWith('/family/') : pathname.startsWith(to))

  useEffect(() => {
    api.listFamilies({}).then(setFamilies, () => {})
    api.listFamilies({ filter: 'system' }).then((f) => setSystemCount(f.length), () => {})
    api.listFolders().then(setFolders, () => {})
    api.listCollections().then(setCollections, () => {})
    api.listProjectSets().then(setSets, () => {})
  }, [libVersion])
  useEffect(() => on('folder-status', (f) => setFolders((all) => all.map((x) => (x.id === f.id ? f : x)))), [])
  // ponytail: one plan at startup for the badge; the Auto-organize page refreshes it. Needs a cheap count command if planning gets slow.
  useEffect(() => {
    if (organizeCount === null) api.organizePlan(false).then((p) => setOrganizeCount(p.changes.length), () => {})
  }, [organizeCount, setOrganizeCount])

  const nav: [string, string, number | null][] = [
    ['/', 'Library', families ? families.length : null],
    ['/playground', 'Playground', null],
    ['/sets', 'Project sets', sets.length || null],
    ['/google', 'Google Fonts', null],
    ['/organize', 'Auto-organize', organizeCount || null],
    ['/pdf', 'Fonts in a PDF', null],
    ['/settings', 'Settings', null],
  ]
  const licCount = (l: License) => families?.filter((f) => f.license === l).length ?? 0
  const sel = (k: string, v: string | number) => params.get(k) === String(v)
  const c = app.connection
  const sync =
    c.status === 'connected'
      ? { color: 'var(--ok)', text: `Synced · ${ago(c.lastSync)}` }
      : c.status === 'offline'
        ? { color: 'var(--warn)', text: 'Offline · will sync later' }
        : { color: 'var(--bad)', text: 'Can’t reach server' }

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
        {folders.map((f) => (
          <Link key={f.id} to={`/?folder=${f.id}`} className="side-item" aria-current={sel('folder', f.id) ? 'true' : undefined} title={f.path}>
            {sel('folder', f.id) && <Hi id="side-item-hi" />}
            <span aria-hidden="true" className={`dot ${f.status}`} style={{ background: f.status === 'offline' ? 'var(--amber)' : f.status === 'scanning' ? 'var(--accent)' : 'var(--ok-dot)' }} />
            <span className="ellipsis" style={{ flex: 1 }}>{f.path}</span>
            {f.status === 'offline' ? (
              <span className="side-count" style={{ color: 'var(--warn)' }}>Offline</span>
            ) : f.status === 'scanning' ? (
              <span className="side-count">Scanning…</span>
            ) : (
              <span className="side-count"><Num value={f.fontCount} /></span>
            )}
          </Link>
        ))}
        {!!systemCount && (
          <Link to="/?system=1" className="side-item" aria-current={sel('system', 1) ? 'true' : undefined} title="Fonts installed in the operating system (always on)">
            {sel('system', 1) && <Hi id="side-item-hi" />}
            <span aria-hidden="true" className="dot" style={{ background: 'var(--faint)' }} />
            <span className="ellipsis" style={{ flex: 1 }}>System fonts</span>
            <span className="side-count"><Num value={systemCount} /></span>
          </Link>
        )}
      </div>
      <div className="side-group">
        <span className="side-head">Collections</span>
        {collections.map((col) => (
          <Link key={col.id} to={`/?collection=${col.id}`} className="side-item" aria-current={sel('collection', col.id) ? 'true' : undefined}>
            {sel('collection', col.id) && <Hi id="side-item-hi" />}
            <span style={{ flex: 1, paddingLeft: col.parentId ? 12 : 0 }}>{col.name}</span>
            <span className="side-count"><Num value={col.count} /></span>
          </Link>
        ))}
      </div>
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
    </nav>
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
    </div>
  )
}
