import { memo, useCallback, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent, type PointerEvent as ReactPointerEvent } from 'react'
import { Link, useLocation, useNavigate, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import {
  api, type ActivationResult, type Family, type FamilyFilter, type FamilyPage, type FamilySort, type Folder, type FolderNode, type FontCategory, type Health,
  type LanguageCount, type LibraryCounts, type License, type ListFamiliesArgs,
} from '../api'
import { ErrorBoundary } from '../ErrorBoundary'
import { familyStyle } from '../fonts'
import { LibraryEggs } from '../extras/LibraryEggs'
import { NOT_ALL, useAddFonts, useFamilyDrag, useFontActions, type Targets } from '../fontActions'
import { clockMark, turnsOff } from '../temporary'
import { useContextMenu, type MenuItem } from '../menu'
import { PreselectBar } from '../PreselectBar'
import { collapse, dur, easeOut, rise, spring, STAGGER_CAP, staggerDelay } from '../motion'
import { useApp } from '../state'
import { RequestFontButton } from '../RequestFont'
import { t, tk } from '../i18n'
import { useBreathing } from '../useBreathing'
import { usePreviewWheel } from '../usePreviewWheel'
import { useSampleText } from '../useSampleText'
import {
  ago, andList, Badge, CATEGORY_LABEL, Chip, errMsg, fmtWhen, keysBusy, LICENSE_LABEL, LicenseBadge, num, Pager, plural, Skel, Switch, useSeen,
} from '../ui'

/** SuperSearch tokens (the backend parses them); clicking one in the "?" menu adds it to the search. */
const TOKENS: [string, string][] = [
  ['tag:headline', tk('Has this tag')], ['foundry:IBM', tk('Foundry contains')], ['foundry:"Monotype Imaging"', tk('Quotes for spaces')],
  ['designer:Carter', tk('Designer contains')], ['category:serif', tk('Or sans, display, mono…')],
  ['license:personal', tk('Or licensed, client…')], ['weight:700', tk('Has this weight')], ['weight:>600', tk('Heavier (or < lighter)')],
  ['weight:300-500', tk('Weight range')], ['italic', tk('Has italics')], ['variable', tk('Variable font')], ['active', tk('Turned on')], ['inactive', tk('Turned off')],
  ['pinned', tk('Pinned to the top')], ['system', tk('System font')], ['char:€ß', tk('Has these characters')], ['lang:vietnamese', tk('Covers a language')],
]
const PinMark = () => (
  <svg role="img" aria-label={t('Pinned')} className="pin-mark" width="13" height="13" viewBox="0 0 16 16" fill="currentColor">
    <title>{t('Pinned')}</title>
    <path d="M9.6 1.6 14.4 6.4l-1.9.6-2.7 2.7.3 2.9-1.3 1.3L2.1 7.2l1.3-1.3 2.9.3L9 3.5Z" /><path d="M5.6 10.4 1.8 14.2" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
  </svg>
)

const FILTERS: [FamilyFilter, string][] = [
  ['all', tk('All')], ['active', tk('Active')], ['inactive', tk('Inactive')], ['admin', tk('Set by admin')], ['unknown_license', tk('Unknown license')], ['duplicates', tk('Duplicates')],
]
const SORTS: [FamilySort, string][] = [['name', tk('Name A–Z')], ['name_desc', tk('Name Z–A')], ['date', tk('Recently added')], ['foundry', tk('Foundry')], ['random', tk('Random')]]
// Keys only: the labels are read at render time (CATEGORY_LABEL has language-aware getters).
const CATEGORIES = Object.keys(CATEGORY_LABEL).map((k) => [k, k] as [FontCategory, string])
const newSeed = () => Math.floor(Math.random() * 2 ** 31)
/** A URL value if it is one of `list`'s keys, else null. */
const known = <K extends string>(v: string | null, list: readonly (readonly [K, string])[]): K | null => list.find(([k]) => k === v)?.[0] ?? null
/** A URL value as a positive whole number, else null (so `1e400`, `Infinity`, `-2` and `abc` don't reach the engine). */
const posInt = (v: string | null) => {
  const n = Number(v)
  return Number.isSafeInteger(n) && n > 0 ? n : null
}
type View = 'list' | 'grid'
const VIEW_KEY = 'typeshelf-library-view'
function savedView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === 'grid' ? 'grid' : 'list'
  } catch {
    return 'list'
  }
}
const ADMIN_MSG = {
  activated: tk('Your admin turned on {fonts} for you'), deactivated: tk('Your admin turned off {fonts} for you'),
  blocked: tk('Your admin blocked {fonts} for you'), delivered: tk('Your admin added {fonts} to your library'),
}
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
type RowProps = { f: Family; i: number; first: boolean; sample: string; size: number; error?: string; selected: boolean; h: ItemHandlers; onSize: (delta: number) => void }

const itemMotion = (i: number, first: boolean) => ({
  initial: first && i < STAGGER_CAP ? rise.initial : false, animate: rise.animate,
  transition: { duration: dur.slow, ease: easeOut, delay: first ? staggerDelay(i) : 0 },
})
const itemEvents = (f: Family, i: number, h: ItemHandlers) => ({
  onClick: (e: ReactMouseEvent) => h.pick(f, i, e),
  onContextMenu: (e: ReactMouseEvent) => h.menu(f, e),
  onPointerDown: (e: ReactPointerEvent) => h.drag(f, e),
  'data-drop-family': f.key, // drop another family here to pair the two in the Playground
})

const ItemSwitch = ({ f, h }: { f: Family; h: ItemHandlers }) => (
  // The wrapper carries the tour anchor (Switch takes no extra props); it must not change the row's layout.
  <span data-tour="font-switch" style={{ display: 'inline-flex', flexShrink: 0 }}>
    <Switch
      on={f.activeState !== 'none'} mixed={f.activeState === 'some'} disabled={f.system} onClick={() => h.toggle(f)} title={turnsOff(f) ?? undefined}
      label={f.system ? t('{family} is a system font, always on', { family: f.family })
        : `${f.activeState === 'all' ? t('Deactivate {family}', { family: f.family }) : t('Activate {family}', { family: f.family })}${turnsOff(f) ? `. ${turnsOff(f)}` : ''}`}
    />
  </span>
)
const ItemCheck = ({ f, i, selected, h }: { f: Family; i: number; selected: boolean; h: ItemHandlers }) => (
  <input type="checkbox" className="item-check" aria-label={t('Select {family}', { family: f.family })} checked={selected} onChange={() => {}} onClick={(e) => h.check(f, i, e)} />
)
const familyLink = (f: Family) => `/family/${encodeURIComponent(f.key)}`
/** Tells the family page it can go Back to this list (same filters and page). */
const FROM_LIBRARY = { fromLibrary: true }

/** One family. The font file is only requested once the row is near the viewport. */
const FamilyRow = memo(function FamilyRow({ f, i, first, sample, size, error, selected, h, onSize }: RowProps) {
  const [ref, seen] = useSeen<HTMLDivElement>()
  const [pvRef, pvStyle] = usePreviewWheel({ famKey: f.key, variable: f.isVariable, onSize })
  const on = f.activeState !== 'none'
  return (
    <motion.div ref={ref} className="font-row" data-selected={selected || undefined} {...itemMotion(i, first)} {...itemEvents(f, i, h)}>
      <ItemCheck f={f} i={i} selected={selected} h={h} />
      <ItemSwitch f={f} h={h} />
      <div style={{ width: 200, display: 'flex', flexDirection: 'column', gap: 4 }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
          <Link to={familyLink(f)} state={FROM_LIBRARY} className="plain-link ellipsis" style={{ fontWeight: 500 }} draggable={false}>{f.family}</Link>
          {f.pinned && <PinMark />}
          {clockMark(f)}
        </span>
        <span className="faint" style={{ fontSize: 12 }}>
          {plural(f.styles, 'style')} · {f.foundry}{f.isVariable && ` · ${t('variable')}`}{f.category && ` · ${t(CATEGORY_LABEL[f.category]) /* i18n-dynamic */}`}
        </span>
        <span style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
          <LicenseBadge license={f.license} />
          {f.system && <Badge tone="neutral">{t('System')}</Badge>}
          {f.adminSet && <Badge tone="accent">{t('Set by admin')}</Badge>}
          {f.hasDuplicates && <Link to="/organize" className="badge warn" style={{ textDecoration: 'none' }} draggable={false}>{t('Duplicates')}</Link>}
        </span>
      </div>
      <div ref={pvRef} className="sample" style={{ ...(seen ? familyStyle(f) : {}), ...pvStyle, fontSize: size, color: on ? 'var(--ink)' : 'var(--disabled)' }}>{sample}</div>
      {error && <p role="alert" className="row-error">{error}</p>}
    </motion.div>
  )
})

/** Grid view: a big preview on a card. */
const FamilyCard = memo(function FamilyCard({ f, i, first, sample, size, error, selected, h, onSize }: RowProps) {
  const [ref, seen] = useSeen<HTMLDivElement>()
  const [pvRef, pvStyle] = usePreviewWheel({ famKey: f.key, variable: f.isVariable, onSize })
  const on = f.activeState !== 'none'
  return (
    <motion.div ref={ref} className="font-card" data-selected={selected || undefined} {...itemMotion(i, first)} {...itemEvents(f, i, h)}>
      <div className="row-between" style={{ flexWrap: 'nowrap' }}>
        <ItemCheck f={f} i={i} selected={selected} h={h} />
        <ItemSwitch f={f} h={h} />
      </div>
      <div ref={pvRef} className="card-sample" style={{ ...(seen ? familyStyle(f) : {}), ...pvStyle, fontSize: size, color: on ? 'var(--ink)' : 'var(--disabled)' }}>{sample}</div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
          <Link to={familyLink(f)} state={FROM_LIBRARY} className="plain-link ellipsis" style={{ fontWeight: 500 }} draggable={false}>{f.family}</Link>
          {f.pinned && <PinMark />}
          {clockMark(f)}
        </span>
        <span className="faint ellipsis" style={{ fontSize: 12 }}>
          {plural(f.styles, 'style')}{f.category && ` · ${t(CATEGORY_LABEL[f.category]) /* i18n-dynamic */}`}{f.isVariable && ` · ${t('variable')}`}
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
  async function run(fn: () => Promise<ActivationResult[]>) {
    setBusy(true)
    await activate(fn)
    setBusy(false)
    onDone()
  }
  return (
    <div className="folder-bar">
      <span className="faint" style={{ fontSize: 13 }}>{t('{n} of {fonts} on', { n: num(node.activeCount), fonts: plural(node.fontCount, 'font') })}</span>
      <button type="button" className="btn small" disabled={busy || node.activeCount === node.fontCount} onClick={() => run(() => api.setFolderActive(node.path, true))}>{t('Turn all on')}</button>
      <button type="button" className="btn small" disabled={busy || node.activeCount === 0} onClick={() => run(() => api.setFolderActive(node.path, false))}>{t('Turn all off')}</button>
      <label className="check">
        <input type="checkbox" checked={node.autoActivate} disabled={busy} onChange={(e) => run(() => api.setFolderAuto(node.path, e.target.checked))} />
        {t('Auto-activate new fonts in this folder')}
      </label>
      <button type="button" className="btn small" onClick={() => api.showInFolder(node.path).catch((e) => toast(errMsg(e)))}>{t('Show in folder')}</button>
    </div>
  )
}

export function Library() {
  const { app, libVersion, activate, adminChange, dismissAdmin } = useApp()
  const [params, setParams] = useSearchParams()
  // Search, filters, sort, view and page start from the URL query and are written back to it (below), so Back from a
  // family page returns to the same list at the same page. They stay as they are when the sidebar changes the scope.
  // Values from the URL are checked: unknown ones fall back to the default.
  const [search, setSearch] = useState(() => params.get('q') ?? '')
  const [chars, setChars] = useState(() => params.get('chars') ?? '')
  const [lang, setLang] = useState(() => params.get('lang') ?? '')
  const [langs, setLangs] = useState<LanguageCount[]>([])
  // Counts are heavy and only change when the library does: load once, refresh when the list is opened.
  // A language the app doesn't know (an edited URL) is dropped once the list is in.
  const loadLangs = () => api.languageCounts().then((ls) => {
    setLangs(ls)
    setLang((l) => (l && !ls.some((x) => x.id === l) ? '' : l))
  }, () => {})
  const [presets, setPresets] = useState<string[]>([])
  const sampleBtn = useRef<HTMLButtonElement>(null)
  const navigate = useNavigate()
  const add = useAddFonts()
  const [filter, setFilter] = useState<FamilyFilter>(() => known(params.get('filter'), FILTERS) ?? 'all')
  const [sort, setSort] = useState<FamilySort>(() => known(params.get('sort'), SORTS) ?? 'name')
  const [seed, setSeed] = useState(() => posInt(params.get('seed')) ?? newSeed())
  const [category, setCategory] = useState<FontCategory | null>(() => known(params.get('category'), CATEGORIES))
  const [view, setView] = useState<View>(() => (params.get('view') === 'grid' ? 'grid' : params.has('view') ? 'list' : savedView()))
  const [sel, setSel] = useState<Map<string, Family>>(new Map())
  /**
   * "Select all N results": bulk actions apply to every match of the query, not just the selected rows. It holds only
   * for the results it was chosen on: a bulk action or a library change reloads them (`reload` / `libVersion`) and ends it.
   */
  const [allAt, setAllAt] = useState<string | null>(null)
  const anchor = useRef<number | null>(null)
  const searchRef = useRef<HTMLInputElement>(null)
  const [sample, setSample] = useState(DEFAULT_SAMPLE)
  const shown = useSampleText(sample)
  const [size, setSize] = useState(32)
  const nudgeSize = useCallback((d: number) => setSize((s) => Math.min(72, Math.max(16, s + d))), [])
  useBreathing()
  const [page, setPage] = useState(() => posInt(params.get('page')) ?? 1)
  const [data, setData] = useState<FamilyPage | null>(null)
  const [counts, setCounts] = useState<LibraryCounts | null>(null)
  const [folders, setFolders] = useState<Folder[]>([])
  const [node, setNode] = useState<FolderNode | null>(null)
  const [health, setHealth] = useState<Health | null>(null)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [reload, setReload] = useState(0)
  const resultsVersion = `${libVersion}|${reload}`
  const all = allAt === resultsVersion
  const top = useRef<HTMLDivElement>(null)
  // Rows stagger in only on the first load; later pages and filters just appear.
  const firstLoad = useRef(true)
  useEffect(() => void (data && (firstLoad.current = false)), [data])

  const path = params.get('path'), collectionId = params.get('collection'), license = params.get('license') as License | null
  const system = params.get('system') === '1'
  const tag = params.get('tag')
  // navigate('/', { state: { select: keys } }) shows just those families, all selected (Stats › Never used).
  // The hand-off rides in location state, not the URL; its keys tell one hand-off from the next (the URL updates
  // below replace the history entry, so `location.key` changes while the hand-off stays the same).
  const location = useLocation()
  const preselect = (location.state as { select?: string[] } | null)?.select ?? null
  const handoff = preselect?.join('\n') ?? ''
  const scopeKey = [search.trim(), chars.trim(), lang, filter, sort, sort === 'random' ? seed : '', category, path, collectionId, license, tag, system, handoff].join('|')

  /** The list's query, for a page of it and for "Select all N results". */
  function query(): ListFamiliesArgs {
    const args: ListFamiliesArgs = { search: search.trim() || undefined, filter: system ? 'system' : filter, sort }
    if (chars.trim()) args.chars = chars.trim()
    if (lang) args.lang = lang
    if (path) args.path = path
    if (collectionId) args.collectionId = Number(collectionId)
    if (license) args.license = license
    if (tag) args.tag = tag
    if (preselect) args.keys = preselect
    if (category) args.category = category
    if (sort === 'random') args.seed = seed
    return args
  }
  // Write the list state into the URL query (replacing the history entry, keeping the Stats hand-off state).
  const urlState = new URLSearchParams(params)
  const put = (k: string, v: string | number | null | false) => (v ? urlState.set(k, String(v)) : urlState.delete(k))
  put('q', search)
  put('chars', chars)
  put('lang', lang)
  put('filter', filter !== 'all' && filter)
  put('category', category)
  put('sort', sort !== 'name' && sort)
  put('seed', sort === 'random' && seed)
  put('view', view === 'grid' && view)
  put('page', page > 1 && page)
  const urlWanted = urlState.toString()
  // Only while the Library is the current page: on the way out it still renders (exit animation) with the next page's URL.
  const here = location.pathname === '/'
  const urlNow = params.toString()
  useEffect(() => {
    if (here && urlWanted !== urlNow) setParams(new URLSearchParams(urlWanted), { replace: true, state: location.state })
  }, [urlWanted, urlNow, here]) // eslint-disable-line react-hooks/exhaustive-deps

  const actions = useFontActions((clear) => {
    if (clear) {
      setSel(new Map())
      setAllAt(null)
    }
    setReload((n) => n + 1)
  })
  const targets: Targets = all && data ? { query: query(), count: data.total } : [...sel.values()]
  const { show, menu } = useContextMenu(t('Font actions'))
  const tokensMenu = useContextMenu(t('Search tokens'))
  const samplesMenu = useContextMenu(t('Sample texts'))
  const addMenu = useContextMenu(t('Add fonts'))
  const drag = useFamilyDrag((c, fams) => actions.addTo(c, fams), (target, dragged) =>
    navigate(`/playground?a=${encodeURIComponent(target)}&b=${encodeURIComponent(dragged.key)}`))

  useEffect(() => {
    api.getSettings().then((s) => {
      if (s.sampleText) setSample(s.sampleText)
      setPresets(s.sampleTexts ?? [])
    }, () => {})
    api.health().then(setHealth, () => {})
    loadLangs()
  }, [])
  useEffect(() => {
    api.libraryCounts().then(setCounts, () => {})
    api.listFolders().then(setFolders, () => {})
  }, [libVersion, reload])
  useEffect(() => {
    if (!path) return setNode(null)
    api.folderTree().then((t) => setNode(findNode(t, path)), () => setNode(null))
  }, [path, libVersion, reload])
  // A new search or scope starts at page 1 with nothing selected (not on mount: the page came from the URL).
  const lastScope = useRef(scopeKey)
  useEffect(() => {
    if (lastScope.current === scopeKey) return
    lastScope.current = scopeKey
    setPage(1)
    setSel(new Map())
    setAllAt(null)
    anchor.current = null
  }, [scopeKey])
  useEffect(() => {
    if (preselect) api.listFamilies({ keys: preselect }).then((fs) => setSel(new Map(fs.map((f) => [f.key, f]))), () => {})
  }, [handoff]) // eslint-disable-line react-hooks/exhaustive-deps
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
    const args = query()
    const timer = setTimeout(() => api.familyPage(args, (page - 1) * PAGE, PAGE).then((d) => {
      setData(d)
      // A page past the end (an old or edited URL, or a shrunk list) moves to the last page; the URL follows.
      if (page > 1 && (page - 1) * PAGE >= d.total) setPage(Math.max(1, Math.ceil(d.total / PAGE)))
    }, () => setData({ items: [], total: 0 })), search || chars ? 150 : 0)
    return () => clearTimeout(timer)
  }, [scopeKey, page, libVersion, reload]) // eslint-disable-line react-hooks/exhaustive-deps

  async function toggle(f: Family) {
    const active = f.activeState !== 'all'
    const res = await activate(() => api.setActive(f.faces.map((x) => x.hash), active), { quiet: true })
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
    setAllAt(null)
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
  const live = useRef({ toggle, select, show, actions, sel, all, targets, rows: data?.items ?? [] })
  live.current = { toggle, select, show, actions, sel, all, targets, rows: data?.items ?? [] }
  const dragStart = drag.start
  const h = useMemo<ItemHandlers>(() => ({
    toggle: (f) => live.current.toggle(f),
    pick: (f, i, e) => {
      const el = e.target as Element
      const ctrl = e.ctrlKey || e.metaKey
      if (el.closest('button, input, label, select, textarea')) return
      if (el.closest('a')) {
        if (!ctrl && !e.shiftKey) return // a plain click on the name opens the family
        e.preventDefault()
      }
      live.current.select(f, i, ctrl, e.shiftKey)
    },
    check: (f, i, e) => live.current.select(f, i, true, e.shiftKey),
    menu: (f, e) => {
      const { sel, all, targets, show, actions } = live.current
      if (all || sel.has(f.key)) return show(e, actions.menuItems(targets)) // every row counts as selected in "all results"
      setSel(new Map([[f.key, f]]))
      show(e, actions.menuItems([f]))
    },
    // Dragging carries the rows on this page; in "all results" it falls back to the page selection, as the bar then says.
    drag: (f, e) => dragStart(e, () => (setAllAt(null), live.current.sel.has(f.key) ? [...live.current.sel.values()] : [f])),
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
      const { sel, rows, actions, targets } = live.current
      if (k === 'enter' && !mod && !e.altKey && !e.shiftKey && !(e.target as Element).closest?.('a, button, input, summary, [tabindex]')) {
        e.preventDefault()
        sampleBtn.current?.click() // like FontBase: Enter opens the sample text picker
      } else if (mod && !e.altKey && k === 'a') {
        e.preventDefault()
        if (e.shiftKey) return void actions.setActive(targets, true)
        setSel(new Map(rows.map((f) => [f.key, f])))
        setAllAt(null)
      } else if (mod && e.shiftKey && !e.altKey && k === 'd') {
        e.preventDefault()
        actions.setActive(targets, false)
      } else if (k === 'escape' && sel.size) {
        setSel(new Map())
        setAllAt(null)
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
  const scope = system ? t('System fonts · always on') : path ? t('Folder: {path}', { path })
    : collectionId ? (collName ? t('Collection: {name}', { name: collName }) : t('Collection'))
      : license ? t('License: {label}', { label: t(LICENSE_LABEL[license]) /* i18n-dynamic */ }) : tag ? t('Tag: {tag}', { tag }) : null
  const selList = [...sel.values()]
  const mod = app.platform === 'macos' ? '⌘' : 'Ctrl+'
  const offline = folders.filter((f) => f.status === 'offline').length
  const adminFamilies = adminChange ? [...new Set(adminChange.fonts.map((f) => f.family))] : []
  const rows = data?.items ?? null
  const first = firstLoad.current
  const total = data?.total ?? 0
  /** Back from "all results" to the rows on screen, which is what stays highlighted. */
  const justThisPage = () => {
    setSel(new Map((rows ?? []).map((f) => [f.key, f])))
    setAllAt(null)
  }
  const offerAll = !all && !!rows?.length && total > rows.length && rows.every((f) => sel.has(f.key))

  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: '100%' }}>
      <div className="toolbar" ref={top}>
        <div className="input-group" style={{ flex: '1 1 220px' }} data-tour="library-search">
          <label htmlFor="search" className="sr-only">{t('Search fonts')}</label>
          <input
            ref={searchRef} id="search" type="search" className="input" value={search} onChange={(e) => setSearch(e.target.value)}
            placeholder={counts ? t('Search {n} fonts', { n: num(counts.library) }) : t('Search fonts')}
          />
          <button
            type="button" className="input-addon" aria-label={t('Search tokens')} title={t('Search tokens')} aria-haspopup="menu"
            onClick={(e) => tokensMenu.show(e, TOKENS.map(([token, hint]): MenuItem => ({
              label: token, hint: t(hint), // i18n-dynamic
              onSelect: () => {
                setSearch((s) => `${s.trim() ? `${s.trim()} ` : ''}${token} `)
                searchRef.current?.focus()
              },
            })), true)}
          >?</button>
        </div>
        <label htmlFor="chars" className="sr-only">{t('Only fonts that have these characters')}</label>
        <input
          id="chars" type="search" className="input" style={{ flex: '0 1 130px' }} value={chars} onChange={(e) => setChars(e.target.value)}
          placeholder={t('Characters')} title={t('Only fonts that have every one of these characters')}
        />
        <label htmlFor="lang" className="sr-only">{t('Only fonts that cover this language')}</label>
        <select
          id="lang" className="select" value={lang} style={{ width: 'auto' }} onChange={(e) => setLang(e.target.value)} onFocus={loadLangs}
          title={t('Only fonts that have every letter of a language')}
        >
          <option value="">{t('Any language')}</option>
          {langs.map((l) => <option key={l.id} value={l.id}>{l.name} ({num(l.families)})</option>)}
        </select>
        <div className="input-group" style={{ flex: '2 1 260px' }}>
          <label htmlFor="sample" className="sr-only">{t('Sample text')}</label>
          <input
            id="sample" type="text" className="input" value={sample} onChange={(e) => setSample(e.target.value)}
            onBlur={() => api.updateSettings({ sampleText: sample }).catch(() => {})}
          />
          <button
            ref={sampleBtn} type="button" className="input-addon" aria-label={t('Sample text presets')} title={t('Sample text presets (Enter)')} aria-haspopup="menu"
            onClick={(e) => samplesMenu.show(e, [
              ...presets.map((txt): MenuItem => ({
                label: txt, onSelect: () => (setSample(txt), api.updateSettings({ sampleText: txt }).catch(() => {})),
              })),
              ...(presets.length ? ['sep' as const] : []),
              { label: t('Edit presets…'), onSelect: () => navigate('/settings') },
            ], true)}
          >
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d="M3 4.5 6 7.5 9 4.5" /></svg>
          </button>
        </div>
        <label className="muted" style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13 }}>
          {t('Size')}
          <input type="range" min={16} max={72} value={size} onChange={(e) => setSize(Number(e.target.value))} style={{ width: 120 }} />
          <span style={{ width: 36 }}>{size}px</span>
        </label>
        <label className="sr-only" htmlFor="sort">{t('Sort')}</label>
        <select
          id="sort" className="select" value={sort} style={{ width: 'auto' }}
          onChange={(e) => {
            const v = e.target.value as FamilySort
            if (v === 'random') setSeed(newSeed())
            setSort(v)
          }}
        >
          {SORTS.map(([k, label]) => <option key={k} value={k}>{t(label) /* i18n-dynamic */}</option>)}
        </select>
        <AnimatePresence initial={false}>
          {sort === 'random' && (
            <motion.button
              key="shuffle" type="button" className="btn small" onClick={() => setSeed(newSeed())} title={t('Shuffle again')}
              initial={{ opacity: 0, scale: 0.9 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.9 }} transition={spring.pop}
            >
              <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d="M2 4h2.5c3 0 4 8 7 8H14M2 12h2.5c1.2 0 2-1.3 2.7-3M9 6.5C9.8 5 10.6 4 11.5 4H14M12 2l2 2-2 2M12 10l2 2-2 2" /></svg>
              {t('Shuffle')}
            </motion.button>
          )}
        </AnimatePresence>
        <div className="seg" role="group" aria-label={t('View')}>
          {([['list', t('List view'), ListIcon], ['grid', t('Grid view'), GridIcon]] as const).map(([v, label, Icon]) => (
            <button key={v} type="button" aria-pressed={view === v} aria-label={label} title={label} onClick={() => setView(v)}>
              {view === v && <motion.span layoutId="seg-hi" className="seg-hi" transition={spring.glide} />}
              <Icon />
            </button>
          ))}
        </div>
        {app.serverConfigured && <RequestFontButton />}
        <button
          type="button" className="btn small" aria-haspopup="menu" data-tour="library-add"
          onClick={(e) => addMenu.show(e, [
            { label: t('Add fonts…'), hint: 'F', onSelect: add.addFonts },
            { label: t('Add folder (copy)…'), hint: 'D', onSelect: add.addFolder },
            { label: t('Watch a folder…'), hint: 'W', onSelect: add.watchFolder },
          ], true)}
        >
          {t('+ Add ▾')}
        </button>
      </div>

      <div className="chips" style={{ padding: '12px 24px' }}>
        {!system && FILTERS.filter(([k]) => k !== 'admin' || app.serverConfigured).map(([k, label]) => (
          <Chip key={k} pressed={filter === k} onClick={() => setFilter(k)}>{t(label) /* i18n-dynamic */}</Chip>
        ))}
        <span className="chip-divider" aria-hidden="true" />
        <span role="group" aria-label={t('Category')} className="chips">
          <Chip pressed={category === null} onClick={() => setCategory(null)}>{t('Any type')}</Chip>
          {CATEGORIES.map(([k]) => (
            <Chip key={k} pressed={category === k} onClick={() => setCategory(category === k ? null : k)}>{t(CATEGORY_LABEL[k]) /* i18n-dynamic */}</Chip>
          ))}
        </span>
        {scope && (
          <Chip pressed onClick={() => setParams({})}>
            <span className="ellipsis" style={{ maxWidth: 260 }}>{scope}</span> <span aria-hidden="true">✕</span><span className="sr-only">{t('(clear)')}</span>
          </Chip>
        )}
      </div>
      {node && <FolderBar node={node} onDone={() => setReload((n) => n + 1)} />}
      {preselect && <PreselectBar count={preselect.length} />}

      <AnimatePresence initial={false}>
      {adminChange && (
        <motion.div key="admin" {...collapse} style={{ overflow: 'hidden' }}>
        <div role="status" className="banner" style={{ margin: '4px 24px 8px' }}>
          <span style={{ fontWeight: 500, color: 'var(--accent-ink)' }}>
            {t(ADMIN_MSG[adminChange.action], { fonts: plural(adminChange.fonts.length, 'font') }) /* i18n-dynamic */}
          </span>
          <span style={{ color: 'var(--banner-ink)', flex: '1 1 200px' }}>
            {andList(adminFamilies)} · {ago(adminChange.at)}.{adminChange.action === 'activated' && ` ${t('You can turn any of them off.')}`}
          </span>
          <button type="button" className="btn small" style={{ borderColor: 'var(--accent-line)' }} onClick={dismissAdmin}>{t('Got it')}</button>
        </div>
        </motion.div>
      )}
      </AnimatePresence>

      <AnimatePresence initial={false}>
        {sel.size > 0 && (
          <motion.div key="selbar" className="select-bar" role="toolbar" aria-label={t('Selected fonts')} {...collapse}>
            <div className="select-bar-inner">
              <span style={{ fontWeight: 500, marginRight: 4 }} role="status">{all ? t('All {n} results selected', { n: num(total) }) : t('{n} selected', { n: num(sel.size) })}</span>
              {offerAll && <button type="button" className="btn small link" onClick={() => setAllAt(resultsVersion)}>{t('Select all {n} results', { n: num(total) })}</button>}
              {all && <button type="button" className="btn small link" onClick={justThisPage}>{t('Select just this page')}</button>}
              <button type="button" className="btn small" onClick={() => actions.setActive(targets, true)} title={`${mod}Shift+A`}>{t('Activate')}</button>
              <button type="button" className="btn small" onClick={() => actions.setActive(targets, false)} title={`${mod}Shift+D`}>{t('Deactivate')}</button>
              <button type="button" className="btn small" aria-haspopup="menu" onClick={(e) => show(e, actions.collectionItems(targets), true)}>{t('Add to collection ▾')}</button>
              <button type="button" className="btn small" onClick={() => actions.openTags(targets)}>{t('Tags…')}</button>
              {all
                ? <button type="button" className="btn small" disabled title={t('{reason}: select just this page to pin', { reason: t(NOT_ALL) /* i18n-dynamic */ })} aria-label={t('Pin to top. {reason}', { reason: t(NOT_ALL) /* i18n-dynamic */ })}>{t('Pin to top')}</button>
                : selList.every((f) => f.pinned)
                  ? <button type="button" className="btn small" onClick={() => actions.setPinned(selList, false)}>{t('Unpin')}</button>
                  : <button type="button" className="btn small" onClick={() => actions.setPinned(selList, true)}>{t('Pin to top')}</button>}
              <button type="button" className="btn small" aria-haspopup="menu" onClick={(e) => show(e, actions.menuItems(targets), true)}>{t('More ▾')}</button>
              <button type="button" className="btn small link" style={{ marginLeft: 'auto' }} onClick={() => (setSel(new Map()), setAllAt(null))} title="Esc">{t('Clear')}</button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <div className={sel.size ? 'results has-selection' : 'results'} data-tour="font-list">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={view} className={view === 'grid' ? 'font-grid' : 'font-list'}
            initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: dur.fast }}
          >
            {rows === null && [0, 1, 2, 3, 4, 5].map((i) => (view === 'grid' ? <CardSkeleton key={i} size={size} /> : <RowSkeleton key={i} size={size} />))}
            {rows?.map((f, i) => {
              const Item = view === 'grid' ? FamilyCard : FamilyRow
              return <Item key={f.key} f={f} i={i} first={first} sample={shown} size={size} error={errors[f.key]} selected={all || sel.has(f.key)} h={h} onSize={nudgeSize} />
            })}
          </motion.div>
        </AnimatePresence>
        {rows?.length === 0 && <motion.p className="faint" style={{ padding: '24px 0' }} initial={{ opacity: 0 }} animate={{ opacity: 1 }}>{t('No fonts match.')}</motion.p>}
        {data && <div style={{ paddingTop: 16 }}><Pager page={page} size={PAGE} total={data.total} onPage={go} /></div>}
      </div>
      {menu}
      {tokensMenu.menu}
      {samplesMenu.menu}
      {addMenu.menu}
      {drag.ghost}
      {actions.dialogs}
      <ErrorBoundary quiet><LibraryEggs search={search} sample={sample} /></ErrorBoundary>

      <div className="statusbar">
        {counts && <span>{t('{n} active', { n: num(counts.active) })}</span>}
        {counts && <span>{plural(counts.library, 'font')}</span>}
        <span>
          {folders.length - offline === 1 ? t('{n} folder watching', { n: 1 }) : t('{n} folders watching', { n: folders.length - offline })}{offline ? ` · ${t('{n} offline', { n: offline })}` : ''}
        </span>
        {health && (
          <span>
            {t('Last activation check: {when},', { when: fmtWhen(health.lastCheck) })}{' '}
            {health.problems.length ? <Link to="/settings">{plural(health.problems.length, 'problem')}</Link> : t('no problems')}
          </span>
        )}
      </div>
    </div>
  )
}
