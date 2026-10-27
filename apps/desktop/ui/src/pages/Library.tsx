import { useEffect, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, type Family, type FamilyFilter, type Folder, type Health, type License, type ListFamiliesArgs } from '../api'
import { familyStyle } from '../fonts'
import { collapse, dur, easeOut, rise, spring, STAGGER_CAP, staggerDelay } from '../motion'
import { useApp } from '../state'
import { ago, andList, Badge, Chip, fmtWhen, LICENSE_LABEL, LicenseBadge, num, plural, Skel, Switch } from '../ui'

const FILTERS: [FamilyFilter, string][] = [
  ['all', 'All'], ['active', 'Active'], ['inactive', 'Inactive'], ['admin', 'Set by admin'], ['unknown_license', 'Unknown license'], ['duplicates', 'Duplicates'],
]
const ADMIN_VERB = { activated: 'turned on', deactivated: 'turned off', blocked: 'blocked', delivered: 'added' }
const DEFAULT_SAMPLE = 'Sphinx of black quartz, judge my vow'
/** Above this many rows, skip enter/exit/layout animation so filtering stays fluid. */
const ANIMATE_MAX = 150

const RowSkeleton = ({ size }: { size: number }) => (
  <div className="font-row" aria-hidden="true">
    <Skel w={40} h={22} r={11} />
    <div style={{ width: 200, display: 'flex', flexDirection: 'column', gap: 6 }}>
      <Skel w="70%" h={14} /><Skel w="90%" h={10} /><Skel w={70} h={16} r={10} />
    </div>
    <div style={{ flex: '1 1 300px' }}><Skel w="80%" h={Math.round(size * 0.8)} /></div>
  </div>
)

export function Library() {
  const { libVersion, activate, adminChange, dismissAdmin } = useApp()
  const [params, setParams] = useSearchParams()
  const [search, setSearch] = useState('')
  const [filter, setFilter] = useState<FamilyFilter>('all')
  const [sample, setSample] = useState(DEFAULT_SAMPLE)
  const [size, setSize] = useState(32)
  const [rows, setRows] = useState<Family[] | null>(null)
  const [all, setAll] = useState<Family[]>([])
  const [folders, setFolders] = useState<Folder[]>([])
  const [health, setHealth] = useState<Health | null>(null)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [reload, setReload] = useState(0)
  // Rows stagger in only on the first load; later filtering just fades rows in/out.
  const firstLoad = useRef(true)
  useEffect(() => void (rows && (firstLoad.current = false)), [rows])

  const folderId = params.get('folder'), collectionId = params.get('collection'), license = params.get('license') as License | null
  const system = params.get('system') === '1'

  useEffect(() => {
    api.getSettings().then((s) => s.sampleText && setSample(s.sampleText), () => {})
    api.health().then(setHealth, () => {})
  }, [])
  useEffect(() => {
    api.listFamilies({}).then(setAll, () => {})
    api.listFolders().then(setFolders, () => {})
  }, [libVersion, reload])
  useEffect(() => {
    const args: ListFamiliesArgs = { search: search.trim() || undefined, filter: system ? 'system' : filter }
    if (folderId) args.folderId = Number(folderId)
    if (collectionId) args.collectionId = Number(collectionId)
    if (license) args.license = license
    const t = setTimeout(() => api.listFamilies(args).then(setRows, () => setRows([])), search ? 150 : 0)
    return () => clearTimeout(t)
  }, [search, filter, system, folderId, collectionId, license, libVersion, reload])

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

  const scope = system ? 'System fonts · always on' : folderId
    ? `Folder: ${folders.find((d) => String(d.id) === folderId)?.path ?? ''}`
    : collectionId ? 'Collection' : license ? `License: ${LICENSE_LABEL[license]}` : null
  const activeCount = all.filter((f) => f.activeState !== 'none').length
  const offline = folders.filter((f) => f.status === 'offline').length
  const adminFamilies = adminChange ? [...new Set(adminChange.fonts.map((f) => f.family))] : []
  const big = (rows?.length ?? 0) > ANIMATE_MAX

  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: '100%' }}>
      <div className="toolbar">
        <label htmlFor="search" className="sr-only">Search fonts</label>
        <input id="search" type="search" className="input" style={{ flex: '1 1 220px' }} placeholder={`Search ${num(all.length)} fonts`} value={search} onChange={(e) => setSearch(e.target.value)} />
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

      <div className={big ? 'rows-big' : undefined} style={{ display: 'flex', flexDirection: 'column', padding: '0 24px 24px', position: 'relative' }}>
        {rows === null && [0, 1, 2, 3, 4, 5].map((i) => <RowSkeleton key={i} size={size} />)}
        {rows?.length === 0 && <motion.p className="faint" style={{ padding: '24px 0' }} initial={{ opacity: 0 }} animate={{ opacity: 1 }}>No fonts match.</motion.p>}
        <AnimatePresence mode="popLayout" initial={false}>
        {rows?.map((f, i) => {
          const on = f.activeState !== 'none'
          // ponytail: read during render on purpose; it only picks each row's mount delay and must not trigger re-renders.
          const first = firstLoad.current
          return (
            <motion.div
              key={f.key} className="font-row" layout={big ? false : 'position'}
              initial={big || (first && i >= STAGGER_CAP) ? false : rise.initial} animate={rise.animate} exit={big ? undefined : rise.exit}
              transition={{ duration: dur.slow, ease: easeOut, delay: first ? staggerDelay(i) : 0, layout: spring.glide }}
            >
              <Switch
                on={on} mixed={f.activeState === 'some'} disabled={f.system} onClick={() => toggle(f)}
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
              <div className="sample" style={{ ...familyStyle(f), fontSize: size, color: on ? 'var(--ink)' : 'var(--disabled)' }}>{sample}</div>
              {errors[f.key] && <p role="alert" className="row-error">{errors[f.key]}</p>}
            </motion.div>
          )
        })}
        </AnimatePresence>
      </div>

      <div className="statusbar">
        <span>{num(activeCount)} active</span>
        <span>{plural(all.length, 'font')}</span>
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
