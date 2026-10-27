import { memo, useEffect, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import {
  api, type ActivationResult, type Family, type FamilyFilter, type FamilyPage, type FamilySort, type Folder, type FolderNode, type Health, type LibraryCounts,
  type License, type ListFamiliesArgs,
} from '../api'
import { familyStyle } from '../fonts'
import { collapse, dur, easeOut, rise, STAGGER_CAP, staggerDelay } from '../motion'
import { useApp } from '../state'
import { ago, andList, Badge, Chip, errMsg, fmtWhen, LICENSE_LABEL, LicenseBadge, num, Pager, plural, Skel, Switch, useSeen } from '../ui'

const FILTERS: [FamilyFilter, string][] = [
  ['all', 'All'], ['active', 'Active'], ['inactive', 'Inactive'], ['admin', 'Set by admin'], ['unknown_license', 'Unknown license'], ['duplicates', 'Duplicates'],
]
const SORTS: [FamilySort, string][] = [['name', 'Name A–Z'], ['name_desc', 'Name Z–A'], ['date', 'Recently added'], ['foundry', 'Foundry']]
const ADMIN_VERB = { activated: 'turned on', deactivated: 'turned off', blocked: 'blocked', delivered: 'added' }
const DEFAULT_SAMPLE = 'Sphinx of black quartz, judge my vow'
/** Families per page; previews load only for rows that scroll into view. */
const PAGE = 50

const RowSkeleton = ({ size }: { size: number }) => (
  <div className="font-row" aria-hidden="true">
    <Skel w={40} h={22} r={11} />
    <div style={{ width: 200, display: 'flex', flexDirection: 'column', gap: 6 }}>
      <Skel w="70%" h={14} /><Skel w="90%" h={10} /><Skel w={70} h={16} r={10} />
    </div>
    <div style={{ flex: '1 1 300px' }}><Skel w="80%" h={Math.round(size * 0.8)} /></div>
  </div>
)

function findNode(nodes: FolderNode[], path: string): FolderNode | null {
  for (const n of nodes) {
    if (n.path === path) return n
    const hit = findNode(n.children, path)
    if (hit) return hit
  }
  return null
}

type RowProps = { f: Family; i: number; first: boolean; sample: string; size: number; error?: string; onToggle: (f: Family) => void }

/** One family. The font file is only requested once the row is near the viewport. */
const FamilyRow = memo(function FamilyRow({ f, i, first, sample, size, error, onToggle }: RowProps) {
  const [ref, seen] = useSeen<HTMLDivElement>()
  const on = f.activeState !== 'none'
  return (
    <motion.div
      ref={ref} className="font-row"
      initial={first && i < STAGGER_CAP ? rise.initial : false} animate={rise.animate}
      transition={{ duration: dur.slow, ease: easeOut, delay: first ? staggerDelay(i) : 0 }}
    >
      <Switch
        on={on} mixed={f.activeState === 'some'} disabled={f.system} onClick={() => onToggle(f)}
        label={f.system ? `${f.family} is a system font, always on` : `${f.activeState === 'all' ? 'Deactivate' : 'Activate'} ${f.family}`}
      />
      <div style={{ width: 200, display: 'flex', flexDirection: 'column', gap: 4 }}>
        <Link to={`/family/${encodeURIComponent(f.key)}`} className="plain-link" style={{ fontWeight: 500 }}>{f.family}</Link>
        <span className="faint" style={{ fontSize: 12 }}>
          {plural(f.styles, 'style')} · {f.foundry}{f.isVariable && ' · variable'}
        </span>
        <span style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
          <LicenseBadge license={f.license} />
          {f.system && <Badge tone="neutral">System</Badge>}
          {f.adminSet && <Badge tone="accent">Set by admin</Badge>}
          {f.hasDuplicates && <Link to="/organize" className="badge warn" style={{ textDecoration: 'none' }}>Duplicates</Link>}
        </span>
      </div>
      <div className="sample" style={{ ...(seen ? familyStyle(f) : {}), fontSize: size, color: on ? 'var(--ink)' : 'var(--disabled)' }}>{sample}</div>
      {error && <p role="alert" className="row-error">{error}</p>}
    </motion.div>
  )
})

/** Turn a whole folder (and its subfolders) on or off, and auto-activate what lands in it later. */
function FolderBar({ node, onDone }: { node: FolderNode; onDone: () => void }) {
  const { activate, toast } = useApp()
  const [busy, setBusy] = useState(false)
  async function run(fn: (confirm: boolean) => Promise<ActivationResult[]>) {
    setBusy(true)
    await activate(fn)
    setBusy(false)
    onDone()
  }
  return (
    <div className="folder-bar">
      <span className="faint" style={{ fontSize: 13 }}>{num(node.activeCount)} of {plural(node.fontCount, 'font')} on</span>
      <button type="button" className="btn small" disabled={busy || node.activeCount === node.fontCount} onClick={() => run((c) => api.setFolderActive(node.path, true, c))}>Turn all on</button>
      <button type="button" className="btn small" disabled={busy || node.activeCount === 0} onClick={() => run((c) => api.setFolderActive(node.path, false, c))}>Turn all off</button>
      <label className="check">
        <input type="checkbox" checked={node.autoActivate} disabled={busy} onChange={(e) => run((c) => api.setFolderAuto(node.path, e.target.checked, c))} />
        Auto-activate new fonts in this folder
      </label>
      <button type="button" className="btn small" onClick={() => api.showInFolder(node.path).catch((e) => toast(errMsg(e)))}>Show in folder</button>
    </div>
  )
}

export function Library() {
  const { libVersion, activate, adminChange, dismissAdmin } = useApp()
  const [params, setParams] = useSearchParams()
  const [search, setSearch] = useState('')
  const [filter, setFilter] = useState<FamilyFilter>('all')
  const [sort, setSort] = useState<FamilySort>('name')
  const [sample, setSample] = useState(DEFAULT_SAMPLE)
  const [size, setSize] = useState(32)
  const [page, setPage] = useState(1)
  const [data, setData] = useState<FamilyPage | null>(null)
  const [counts, setCounts] = useState<LibraryCounts | null>(null)
  const [folders, setFolders] = useState<Folder[]>([])
  const [node, setNode] = useState<FolderNode | null>(null)
  const [health, setHealth] = useState<Health | null>(null)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [reload, setReload] = useState(0)
  const top = useRef<HTMLDivElement>(null)
  // Rows stagger in only on the first load; later pages and filters just appear.
  const firstLoad = useRef(true)
  useEffect(() => void (data && (firstLoad.current = false)), [data])

  const path = params.get('path'), collectionId = params.get('collection'), license = params.get('license') as License | null
  const system = params.get('system') === '1'
  const scopeKey = [search.trim(), filter, sort, path, collectionId, license, system].join('|')

  useEffect(() => {
    api.getSettings().then((s) => s.sampleText && setSample(s.sampleText), () => {})
    api.health().then(setHealth, () => {})
  }, [])
  useEffect(() => {
    api.libraryCounts().then(setCounts, () => {})
    api.listFolders().then(setFolders, () => {})
  }, [libVersion, reload])
  useEffect(() => {
    if (!path) return setNode(null)
    api.folderTree().then((t) => setNode(findNode(t, path)), () => setNode(null))
  }, [path, libVersion, reload])
  // A new search or scope starts at page 1.
  useEffect(() => setPage(1), [scopeKey])
  useEffect(() => {
    const args: ListFamiliesArgs = { search: search.trim() || undefined, filter: system ? 'system' : filter, sort }
    if (path) args.path = path
    if (collectionId) args.collectionId = Number(collectionId)
    if (license) args.license = license
    const t = setTimeout(() => api.familyPage(args, (page - 1) * PAGE, PAGE).then(setData, () => setData({ items: [], total: 0 })), search ? 150 : 0)
    return () => clearTimeout(t)
  }, [scopeKey, page, libVersion, reload]) // eslint-disable-line react-hooks/exhaustive-deps

  async function toggle(f: Family) {
    const active = f.activeState !== 'all'
    const res = await activate((confirm) => api.setActive(f.faces.map((x) => x.hash), active, confirm), { quiet: true })
    const failed = res?.filter((r) => !r.ok) ?? []
    setErrors((e) => {
      const { [f.key]: _, ...rest } = e
      return failed.length ? { ...rest, [f.key]: failed.map((r) => r.message ?? r.error).join(' ') } : rest
    })
    setReload((n) => n + 1)
  }
  function go(p: number) {
    setPage(p)
    top.current?.scrollIntoView({ block: 'start' })
  }

  const scope = system ? 'System fonts · always on' : path ? `Folder: ${path}`
    : collectionId ? 'Collection' : license ? `License: ${LICENSE_LABEL[license]}` : null
  const offline = folders.filter((f) => f.status === 'offline').length
  const adminFamilies = adminChange ? [...new Set(adminChange.fonts.map((f) => f.family))] : []
  const rows = data?.items ?? null
  const first = firstLoad.current

  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: '100%' }}>
      <div className="toolbar" ref={top}>
        <label htmlFor="search" className="sr-only">Search fonts</label>
        <input
          id="search" type="search" className="input" style={{ flex: '1 1 220px' }} value={search} onChange={(e) => setSearch(e.target.value)}
          placeholder={counts ? `Search ${num(counts.library)} fonts` : 'Search fonts'}
        />
        <label htmlFor="sample" className="sr-only">Sample text</label>
        <input
          id="sample" type="text" className="input" style={{ flex: '2 1 260px' }} value={sample} onChange={(e) => setSample(e.target.value)}
          onBlur={() => api.updateSettings({ sampleText: sample }).catch(() => {})}
        />
        <label className="muted" style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13 }}>
          Size
          <input type="range" min={16} max={72} value={size} onChange={(e) => setSize(Number(e.target.value))} style={{ width: 120 }} />
          <span style={{ width: 36 }}>{size}px</span>
        </label>
        <label className="sr-only" htmlFor="sort">Sort</label>
        <select id="sort" className="select" value={sort} onChange={(e) => setSort(e.target.value as FamilySort)} style={{ width: 'auto' }}>
          {SORTS.map(([k, label]) => <option key={k} value={k}>{label}</option>)}
        </select>
      </div>

      <div className="chips" style={{ padding: '12px 24px' }}>
        {!system && FILTERS.map(([k, label]) => (
          <Chip key={k} pressed={filter === k} onClick={() => setFilter(k)}>{label}</Chip>
        ))}
        {scope && (
          <Chip pressed onClick={() => setParams({})}>
            <span className="ellipsis" style={{ maxWidth: 260 }}>{scope}</span> <span aria-hidden="true">✕</span><span className="sr-only">(clear)</span>
          </Chip>
        )}
      </div>
      {node && <FolderBar node={node} onDone={() => setReload((n) => n + 1)} />}

      <AnimatePresence initial={false}>
      {adminChange && (
        <motion.div key="admin" {...collapse} style={{ overflow: 'hidden' }}>
        <div role="status" className="banner" style={{ margin: '4px 24px 8px' }}>
          <span style={{ fontWeight: 500, color: 'var(--accent-ink)' }}>
            Your admin {ADMIN_VERB[adminChange.action]} {plural(adminChange.fonts.length, 'font')} {adminChange.action === 'delivered' ? 'to your library' : 'for you'}
          </span>
          <span style={{ color: 'var(--banner-ink)', flex: '1 1 200px' }}>
            {andList(adminFamilies)} · {ago(adminChange.at)}.{adminChange.action === 'activated' && ' You can turn any of them off.'}
          </span>
          <button type="button" className="btn small" style={{ borderColor: 'var(--accent-line)' }} onClick={dismissAdmin}>Got it</button>
        </div>
        </motion.div>
      )}
      </AnimatePresence>

      <div style={{ display: 'flex', flexDirection: 'column', padding: '0 24px 24px', position: 'relative' }}>
        {rows === null && [0, 1, 2, 3, 4, 5].map((i) => <RowSkeleton key={i} size={size} />)}
        {rows?.length === 0 && <motion.p className="faint" style={{ padding: '24px 0' }} initial={{ opacity: 0 }} animate={{ opacity: 1 }}>No fonts match.</motion.p>}
        {rows?.map((f, i) => (
          <FamilyRow key={f.key} f={f} i={i} first={first} sample={sample} size={size} error={errors[f.key]} onToggle={toggle} />
        ))}
        {data && <div style={{ paddingTop: 16 }}><Pager page={page} size={PAGE} total={data.total} onPage={go} /></div>}
      </div>

      <div className="statusbar">
        {counts && <span>{num(counts.active)} active</span>}
        {counts && <span>{plural(counts.library, 'font')}</span>}
        <span>
          {folders.length - offline} {folders.length - offline === 1 ? 'folder' : 'folders'} watching{offline ? ` · ${offline} offline` : ''}
        </span>
        {health && (
          <span>
            Last activation check: {fmtWhen(health.lastCheck)},{' '}
            {health.problems.length ? <Link to="/settings">{plural(health.problems.length, 'problem')}</Link> : 'no problems'}
          </span>
        )}
      </div>
    </div>
  )
}
