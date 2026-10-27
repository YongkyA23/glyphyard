import { memo, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent, type PointerEvent as ReactPointerEvent } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import {
  api, type ActivationResult, type Family, type FamilyFilter, type FamilyPage, type FamilySort, type Folder, type FolderNode, type FontCategory, type Health,
  type LibraryCounts, type License, type ListFamiliesArgs,
} from '../api'
import { familyStyle } from '../fonts'
import { useAddFonts, useFamilyDrag, useFontActions } from '../fontActions'
import { useContextMenu, type MenuItem } from '../menu'
import { collapse, dur, easeOut, rise, spring, STAGGER_CAP, staggerDelay } from '../motion'
import { useApp } from '../state'
import {
  ago, andList, Badge, CATEGORY_LABEL, Chip, errMsg, fmtWhen, keysBusy, LICENSE_LABEL, LicenseBadge, num, Pager, plural, Skel, Switch, useSeen,
} from '../ui'

/** SuperSearch tokens (the backend parses them); clicking one in the "?" menu adds it to the search. */
const TOKENS: [string, string][] = [
  ['tag:headline', 'Has this tag'], ['foundry:IBM', 'Foundry contains'], ['foundry:"Monotype Imaging"', 'Quotes for spaces'],
  ['designer:Carter', 'Designer contains'], ['category:serif', 'Or sans, display, mono…'],
  ['license:personal', 'Or licensed, client…'], ['weight:700', 'Has this weight'], ['weight:>600', 'Heavier (or < lighter)'],
  ['weight:300-500', 'Weight range'], ['italic', 'Has italics'], ['variable', 'Variable font'], ['active', 'Turned on'], ['inactive', 'Turned off'],
  ['pinned', 'Pinned to the top'], ['system', 'System font'], ['char:€ß', 'Has these characters'],
]
const PinMark = () => (
  <svg role="img" aria-label="Pinned" className="pin-mark" width="13" height="13" viewBox="0 0 16 16" fill="currentColor">
    <title>Pinned</title>
    <path d="M9.6 1.6 14.4 6.4l-1.9.6-2.7 2.7.3 2.9-1.3 1.3L2.1 7.2l1.3-1.3 2.9.3L9 3.5Z" /><path d="M5.6 10.4 1.8 14.2" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
  </svg>
)

const FILTERS: [FamilyFilter, string][] = [
  ['all', 'All'], ['active', 'Active'], ['inactive', 'Inactive'], ['admin', 'Set by admin'], ['unknown_license', 'Unknown license'], ['duplicates', 'Duplicates'],
]
const SORTS: [FamilySort, string][] = [['name', 'Name A–Z'], ['name_desc', 'Name Z–A'], ['date', 'Recently added'], ['foundry', 'Foundry'], ['random', 'Random']]
const CATEGORIES = Object.entries(CATEGORY_LABEL) as [FontCategory, string][]
const newSeed = () => Math.floor(Math.random() * 2 ** 31)
type View = 'list' | 'grid'
const VIEW_KEY = 'typeshelf-library-view'
function savedView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === 'grid' ? 'grid' : 'list'
  } catch {
    return 'list'
  }
}
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

/** Pointer, click and menu handlers shared by every row/card; one stable object so memoized items don't re-render. */
type ItemHandlers = {
  toggle: (f: Family) => void
  pick: (f: Family, i: number, e: ReactMouseEvent) => void
  check: (f: Family, i: number, e: ReactMouseEvent) => void
  menu: (f: Family, e: ReactMouseEvent) => void
  drag: (f: Family, e: ReactPointerEvent) => void
}
type RowProps = { f: Family; i: number; first: boolean; sample: string; size: number; error?: string; selected: boolean; h: ItemHandlers }

const itemMotion = (i: number, first: boolean) => ({
  initial: first && i < STAGGER_CAP ? rise.initial : false, animate: rise.animate,
  transition: { duration: dur.slow, ease: easeOut, delay: first ? staggerDelay(i) : 0 },
})
const itemEvents = (f: Family, i: number, h: ItemHandlers) => ({
  onClick: (e: ReactMouseEvent) => h.pick(f, i, e),
  onContextMenu: (e: ReactMouseEvent) => h.menu(f, e),
  onPointerDown: (e: ReactPointerEvent) => h.drag(f, e),
})

const ItemSwitch = ({ f, h }: { f: Family; h: ItemHandlers }) => (
  <Switch
    on={f.activeState !== 'none'} mixed={f.activeState === 'some'} disabled={f.system} onClick={() => h.toggle(f)}
    label={f.system ? `${f.family} is a system font, always on` : `${f.activeState === 'all' ? 'Deactivate' : 'Activate'} ${f.family}`}
  />
)
const ItemCheck = ({ f, i, selected, h }: { f: Family; i: number; selected: boolean; h: ItemHandlers }) => (
  <input type="checkbox" className="item-check" aria-label={`Select ${f.family}`} checked={selected} onChange={() => {}} onClick={(e) => h.check(f, i, e)} />
)
const familyLink = (f: Family) => `/family/${encodeURIComponent(f.key)}`

/** One family. The font file is only requested once the row is near the viewport. */
const FamilyRow = memo(function FamilyRow({ f, i, first, sample, size, error, selected, h }: RowProps) {
  const [ref, seen] = useSeen<HTMLDivElement>()
  const on = f.activeState !== 'none'
  return (
    <motion.div ref={ref} className="font-row" data-selected={selected || undefined} {...itemMotion(i, first)} {...itemEvents(f, i, h)}>
      <ItemCheck f={f} i={i} selected={selected} h={h} />
      <ItemSwitch f={f} h={h} />
      <div style={{ width: 200, display: 'flex', flexDirection: 'column', gap: 4 }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
          <Link to={familyLink(f)} className="plain-link ellipsis" style={{ fontWeight: 500 }} draggable={false}>{f.family}</Link>
          {f.pinned && <PinMark />}
        </span>
        <span className="faint" style={{ fontSize: 12 }}>
          {plural(f.styles, 'style')} · {f.foundry}{f.isVariable && ' · variable'}{f.category && ` · ${CATEGORY_LABEL[f.category]}`}
        </span>
        <span style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
          <LicenseBadge license={f.license} />
          {f.system && <Badge tone="neutral">System</Badge>}
          {f.adminSet && <Badge tone="accent">Set by admin</Badge>}
          {f.hasDuplicates && <Link to="/organize" className="badge warn" style={{ textDecoration: 'none' }} draggable={false}>Duplicates</Link>}
        </span>
      </div>
      <div className="sample" style={{ ...(seen ? familyStyle(f) : {}), fontSize: size, color: on ? 'var(--ink)' : 'var(--disabled)' }}>{sample}</div>
      {error && <p role="alert" className="row-error">{error}</p>}
    </motion.div>
  )
})

/** Grid view: a big preview on a card. */
const FamilyCard = memo(function FamilyCard({ f, i, first, sample, size, error, selected, h }: RowProps) {
  const [ref, seen] = useSeen<HTMLDivElement>()
  const on = f.activeState !== 'none'
  return (
    <motion.div ref={ref} className="font-card" data-selected={selected || undefined} {...itemMotion(i, first)} {...itemEvents(f, i, h)}>
      <div className="row-between" style={{ flexWrap: 'nowrap' }}>
        <ItemCheck f={f} i={i} selected={selected} h={h} />
        <ItemSwitch f={f} h={h} />
      </div>
      <div className="card-sample" style={{ ...(seen ? familyStyle(f) : {}), fontSize: size, color: on ? 'var(--ink)' : 'var(--disabled)' }}>{sample}</div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
          <Link to={familyLink(f)} className="plain-link ellipsis" style={{ fontWeight: 500 }} draggable={false}>{f.family}</Link>
          {f.pinned && <PinMark />}
        </span>
        <span className="faint ellipsis" style={{ fontSize: 12 }}>
          {plural(f.styles, 'style')}{f.category && ` · ${CATEGORY_LABEL[f.category]}`}{f.isVariable && ' · variable'}
        </span>
      </div>
      {error && <p role="alert" className="row-error" style={{ padding: 0 }}>{error}</p>}
    </motion.div>
  )
})

const CardSkeleton = ({ size }: { size: number }) => (
  <div className="font-card" aria-hidden="true">
    <div className="row-between"><Skel w={16} h={16} r={4} /><Skel w={40} h={22} r={11} /></div>
    <Skel w="85%" h={Math.round(size * 0.8)} />
    <Skel w="60%" h={14} /><Skel w="40%" h={10} />
  </div>
)

const ListIcon = () => (
  <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round"><path d="M2.5 4h11M2.5 8h11M2.5 12h11" /></svg>
)
const GridIcon = () => (
  <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6"><rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1" /><rect x="9" y="2.5" width="4.5" height="4.5" rx="1" /><rect x="2.5" y="9" width="4.5" height="4.5" rx="1" /><rect x="9" y="9" width="4.5" height="4.5" rx="1" /></svg>
)

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
  const { app, libVersion, activate, adminChange, dismissAdmin } = useApp()
  const [params, setParams] = useSearchParams()
  const [search, setSearch] = useState('')
  const [chars, setChars] = useState('')
  const [presets, setPresets] = useState<string[]>([])
  const sampleBtn = useRef<HTMLButtonElement>(null)
  const navigate = useNavigate()
  const add = useAddFonts()
  const [filter, setFilter] = useState<FamilyFilter>('all')
  const [sort, setSort] = useState<FamilySort>('name')
  const [seed, setSeed] = useState(newSeed)
  const [category, setCategory] = useState<FontCategory | null>(null)
  const [view, setView] = useState<View>(savedView)
  const [sel, setSel] = useState<Map<string, Family>>(new Map())
  const anchor = useRef<number | null>(null)
  const searchRef = useRef<HTMLInputElement>(null)
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
  const tag = params.get('tag')
  const scopeKey = [search.trim(), chars.trim(), filter, sort, sort === 'random' ? seed : '', category, path, collectionId, license, tag, system].join('|')

  const actions = useFontActions((clear) => {
    if (clear) setSel(new Map())
    setReload((n) => n + 1)
  })
  const { show, menu } = useContextMenu('Font actions')
  const tokensMenu = useContextMenu('Search tokens')
  const samplesMenu = useContextMenu('Sample texts')
  const addMenu = useContextMenu('Add fonts')
  const drag = useFamilyDrag((c, fams) => actions.addTo(c, fams))

  useEffect(() => {
    api.getSettings().then((s) => {
      if (s.sampleText) setSample(s.sampleText)
      setPresets(s.sampleTexts ?? [])
    }, () => {})
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
  // A new search or scope starts at page 1 with nothing selected.
  useEffect(() => {
    setPage(1)
    setSel(new Map())
    anchor.current = null
  }, [scopeKey])
  useEffect(() => void (anchor.current = null), [page])
  useEffect(() => {
    try {
      localStorage.setItem(VIEW_KEY, view)
    } catch { /* storage blocked: the view just isn't remembered */ }
  }, [view])
  // Keep selected families' data (on/off state) current after a reload.
  useEffect(() => {
    if (data?.items.some((f) => sel.has(f.key)))
      setSel((cur) => new Map([...cur].map(([k, f]) => [k, data.items.find((x) => x.key === k) ?? f])))
  }, [data]) // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    const args: ListFamiliesArgs = { search: search.trim() || undefined, filter: system ? 'system' : filter, sort }
    if (chars.trim()) args.chars = chars.trim()
    if (path) args.path = path
    if (collectionId) args.collectionId = Number(collectionId)
    if (license) args.license = license
    if (tag) args.tag = tag
    if (category) args.category = category
    if (sort === 'random') args.seed = seed
    const t = setTimeout(() => api.familyPage(args, (page - 1) * PAGE, PAGE).then(setData, () => setData({ items: [], total: 0 })), search || chars ? 150 : 0)
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
  /** Click = select just this one, Ctrl/Cmd+click = add/remove, Shift+click = range from the last clicked. */
  function select(f: Family, i: number, ctrl: boolean, shift: boolean) {
    const rows = data?.items ?? []
    if (shift && anchor.current !== null) {
      const [a, b] = [anchor.current, i].sort((x, y) => x - y)
      setSel((cur) => {
        const next = ctrl ? new Map(cur) : new Map<string, Family>()
        rows.slice(a, b + 1).forEach((r) => next.set(r.key, r))
        return next
      })
      return
    }
    anchor.current = i
    setSel((cur) => {
      if (!ctrl) return new Map([[f.key, f]])
      const next = new Map(cur)
      if (!next.delete(f.key)) next.set(f.key, f)
      return next
    })
  }
  // Latest state for the stable handler object and the window key listener.
  const live = useRef({ toggle, select, show, actions, sel, rows: data?.items ?? [] })
  live.current = { toggle, select, show, actions, sel, rows: data?.items ?? [] }
  const dragStart = drag.start
  const h = useMemo<ItemHandlers>(() => ({
    toggle: (f) => live.current.toggle(f),
    pick: (f, i, e) => {
      const t = e.target as Element
      const ctrl = e.ctrlKey || e.metaKey
      if (t.closest('button, input, label, select, textarea')) return
      if (t.closest('a')) {
        if (!ctrl && !e.shiftKey) return // a plain click on the name opens the family
        e.preventDefault()
      }
      live.current.select(f, i, ctrl, e.shiftKey)
    },
    check: (f, i, e) => live.current.select(f, i, true, e.shiftKey),
    menu: (f, e) => {
      const { sel, show, actions } = live.current
      let targets = [f]
      if (sel.has(f.key)) targets = [...sel.values()]
      else setSel(new Map([[f.key, f]]))
      show(e, actions.menuItems(targets))
    },
    drag: (f, e) => dragStart(e, () => (live.current.sel.has(f.key) ? [...live.current.sel.values()] : [f])),
  }), [dragStart])

  // FontBase shortcuts. Ctrl+F works everywhere; the rest stay out of the way while typing or in a dialog/menu.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey
      const k = e.key.toLowerCase()
      if (mod && !e.shiftKey && !e.altKey && k === 'f') {
        e.preventDefault()
        searchRef.current?.focus()
        searchRef.current?.select()
        return
      }
      if (keysBusy(e)) return
      const { sel, rows, actions } = live.current
      if (k === 'enter' && !mod && !e.altKey && !e.shiftKey && !(e.target as Element).closest?.('a, button, input, summary, [tabindex]')) {
        e.preventDefault()
        sampleBtn.current?.click() // like FontBase: Enter opens the sample text picker
      } else if (mod && !e.altKey && k === 'a') {
        e.preventDefault()
        if (e.shiftKey) actions.setActive([...sel.values()], true)
        else setSel(new Map(rows.map((f) => [f.key, f])))
      } else if (mod && e.shiftKey && !e.altKey && k === 'd') {
        e.preventDefault()
        actions.setActive([...sel.values()], false)
      } else if (k === 'escape' && sel.size) {
        setSel(new Map())
      }
    }
    addEventListener('keydown', onKey)
    return () => removeEventListener('keydown', onKey)
  }, [])

  function go(p: number) {
    setPage(p)
    top.current?.scrollIntoView({ block: 'start' })
  }

  const collName = actions.collections.find((c) => String(c.id) === collectionId)?.name
  const scope = system ? 'System fonts · always on' : path ? `Folder: ${path}`
    : collectionId ? `Collection${collName ? `: ${collName}` : ''}` : license ? `License: ${LICENSE_LABEL[license]}` : tag ? `Tag: ${tag}` : null
  const selList = [...sel.values()]
  const mod = app.platform === 'macos' ? '⌘' : 'Ctrl+'
  const offline = folders.filter((f) => f.status === 'offline').length
  const adminFamilies = adminChange ? [...new Set(adminChange.fonts.map((f) => f.family))] : []
  const rows = data?.items ?? null
  const first = firstLoad.current

  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: '100%' }}>
      <div className="toolbar" ref={top}>
        <div className="input-group" style={{ flex: '1 1 220px' }}>
          <label htmlFor="search" className="sr-only">Search fonts</label>
          <input
            ref={searchRef} id="search" type="search" className="input" value={search} onChange={(e) => setSearch(e.target.value)}
            placeholder={counts ? `Search ${num(counts.library)} fonts` : 'Search fonts'}
          />
          <button
            type="button" className="input-addon" aria-label="Search tokens" title="Search tokens" aria-haspopup="menu"
            onClick={(e) => tokensMenu.show(e, TOKENS.map(([token, hint]): MenuItem => ({
              label: token, hint,
              onSelect: () => {
                setSearch((s) => `${s.trim() ? `${s.trim()} ` : ''}${token} `)
                searchRef.current?.focus()
              },
            })), true)}
          >?</button>
        </div>
        <label htmlFor="chars" className="sr-only">Only fonts that have these characters</label>
        <input
          id="chars" type="search" className="input" style={{ flex: '0 1 130px' }} value={chars} onChange={(e) => setChars(e.target.value)}
          placeholder="Characters" title="Only fonts that have every one of these characters"
        />
        <div className="input-group" style={{ flex: '2 1 260px' }}>
          <label htmlFor="sample" className="sr-only">Sample text</label>
          <input
            id="sample" type="text" className="input" value={sample} onChange={(e) => setSample(e.target.value)}
            onBlur={() => api.updateSettings({ sampleText: sample }).catch(() => {})}
          />
          <button
            ref={sampleBtn} type="button" className="input-addon" aria-label="Sample text presets" title="Sample text presets (Enter)" aria-haspopup="menu"
            onClick={(e) => samplesMenu.show(e, [
              ...presets.map((t): MenuItem => ({
                label: t, onSelect: () => (setSample(t), api.updateSettings({ sampleText: t }).catch(() => {})),
              })),
              ...(presets.length ? ['sep' as const] : []),
              { label: 'Edit presets…', onSelect: () => navigate('/settings') },
            ], true)}
          >
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d="M3 4.5 6 7.5 9 4.5" /></svg>
          </button>
        </div>
        <label className="muted" style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13 }}>
          Size
          <input type="range" min={16} max={72} value={size} onChange={(e) => setSize(Number(e.target.value))} style={{ width: 120 }} />
          <span style={{ width: 36 }}>{size}px</span>
        </label>
        <label className="sr-only" htmlFor="sort">Sort</label>
        <select
          id="sort" className="select" value={sort} style={{ width: 'auto' }}
          onChange={(e) => {
            const v = e.target.value as FamilySort
            if (v === 'random') setSeed(newSeed())
            setSort(v)
          }}
        >
          {SORTS.map(([k, label]) => <option key={k} value={k}>{label}</option>)}
        </select>
        <AnimatePresence initial={false}>
          {sort === 'random' && (
            <motion.button
              key="shuffle" type="button" className="btn small" onClick={() => setSeed(newSeed())} title="Shuffle again"
              initial={{ opacity: 0, scale: 0.9 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.9 }} transition={spring.pop}
            >
              <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d="M2 4h2.5c3 0 4 8 7 8H14M2 12h2.5c1.2 0 2-1.3 2.7-3M9 6.5C9.8 5 10.6 4 11.5 4H14M12 2l2 2-2 2M12 10l2 2-2 2" /></svg>
              Shuffle
            </motion.button>
          )}
        </AnimatePresence>
        <div className="seg" role="group" aria-label="View">
          {([['list', 'List view', ListIcon], ['grid', 'Grid view', GridIcon]] as const).map(([v, label, Icon]) => (
            <button key={v} type="button" aria-pressed={view === v} aria-label={label} title={label} onClick={() => setView(v)}>
              {view === v && <motion.span layoutId="seg-hi" className="seg-hi" transition={spring.glide} />}
              <Icon />
            </button>
          ))}
        </div>
        <button
          type="button" className="btn small" aria-haspopup="menu"
          onClick={(e) => addMenu.show(e, [
            { label: 'Add fonts…', hint: 'F', onSelect: add.addFonts },
            { label: 'Add folder (copy)…', hint: 'D', onSelect: add.addFolder },
            { label: 'Watch a folder…', hint: 'W', onSelect: add.watchFolder },
          ], true)}
        >
          + Add ▾
        </button>
      </div>

      <div className="chips" style={{ padding: '12px 24px' }}>
        {!system && FILTERS.map(([k, label]) => (
          <Chip key={k} pressed={filter === k} onClick={() => setFilter(k)}>{label}</Chip>
        ))}
        <span className="chip-divider" aria-hidden="true" />
        <span role="group" aria-label="Category" className="chips">
          <Chip pressed={category === null} onClick={() => setCategory(null)}>Any type</Chip>
          {CATEGORIES.map(([k, label]) => (
            <Chip key={k} pressed={category === k} onClick={() => setCategory(category === k ? null : k)}>{label}</Chip>
          ))}
        </span>
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

      <AnimatePresence initial={false}>
        {sel.size > 0 && (
          <motion.div key="selbar" className="select-bar" role="toolbar" aria-label="Selected fonts" {...collapse}>
            <div className="select-bar-inner">
              <span style={{ fontWeight: 500, marginRight: 4 }} role="status">{num(sel.size)} selected</span>
              <button type="button" className="btn small" onClick={() => actions.setActive(selList, true)} title={`${mod}Shift+A`}>Activate</button>
              <button type="button" className="btn small" onClick={() => actions.setActive(selList, false)} title={`${mod}Shift+D`}>Deactivate</button>
              <button type="button" className="btn small" aria-haspopup="menu" onClick={(e) => show(e, actions.collectionItems(selList), true)}>Add to collection ▾</button>
              <button type="button" className="btn small" onClick={() => actions.openTags(selList)}>Tags…</button>
              {selList.every((f) => f.pinned)
                ? <button type="button" className="btn small" onClick={() => actions.setPinned(selList, false)}>Unpin</button>
                : <button type="button" className="btn small" onClick={() => actions.setPinned(selList, true)}>Pin to top</button>}
              <button type="button" className="btn small" aria-haspopup="menu" onClick={(e) => show(e, actions.menuItems(selList), true)}>More ▾</button>
              <button type="button" className="btn small link" style={{ marginLeft: 'auto' }} onClick={() => setSel(new Map())} title="Esc">Clear</button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <div className={sel.size ? 'results has-selection' : 'results'}>
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={view} className={view === 'grid' ? 'font-grid' : 'font-list'}
            initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: dur.fast }}
          >
            {rows === null && [0, 1, 2, 3, 4, 5].map((i) => (view === 'grid' ? <CardSkeleton key={i} size={size} /> : <RowSkeleton key={i} size={size} />))}
            {rows?.map((f, i) => {
              const Item = view === 'grid' ? FamilyCard : FamilyRow
              return <Item key={f.key} f={f} i={i} first={first} sample={sample} size={size} error={errors[f.key]} selected={sel.has(f.key)} h={h} />
            })}
          </motion.div>
        </AnimatePresence>
        {rows?.length === 0 && <motion.p className="faint" style={{ padding: '24px 0' }} initial={{ opacity: 0 }} animate={{ opacity: 1 }}>No fonts match.</motion.p>}
        {data && <div style={{ paddingTop: 16 }}><Pager page={page} size={PAGE} total={data.total} onPage={go} /></div>}
      </div>
      {menu}
      {tokensMenu.menu}
      {samplesMenu.menu}
      {addMenu.menu}
      {drag.ghost}
      {actions.dialogs}

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
