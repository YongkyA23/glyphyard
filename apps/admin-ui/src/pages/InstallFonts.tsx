import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, errorText, type FamilyRow, type FamilySort, type FontFilter, type FontSelection, type FontSource, type FontState, type InstallDetail } from '../api'
import { fadeRise, row } from '../motion'
import { toast } from '../toast'
import { ago, Chips, Empty, Loaded, Pager, plural, Skel, Toggle, useConfirm, useDebounced, useLoad, usePage } from '../ui'
import { PENDING_MS, useFontToggle } from './useFontToggle'
import './install-fonts.css'

const sourceText = (f: FontState, who: string) =>
  f.overridden ? `Turned on by admin · turned off by ${who}`
    : f.source === 'admin' ? 'Turned on by admin'
    : f.source === 'project_set' ? 'Project set'
    : f.source === 'library' ? 'Shared library'
    : f.active ? `Turned on by ${who}` : `Turned off by ${who}`

const num = (n: number) => n.toLocaleString('en-US')
const famText = (f: string) => ({ fontFamily: `'${f}', var(--font-ui)` })
type StateFilter = 'all' | 'on' | 'off'
type Action = 'on' | 'off' | 'block'

/** What is ticked. Tied to the filter `key`, so a new filter, search or sort starts empty. `families` remembers each one's style count, `hashes` the family each style belongs to. */
type Sel = { key: string; families: Map<string, number>; hashes: Map<string, string>; all: boolean }
const none = (key: string): Sel => ({ key, families: new Map(), hashes: new Map(), all: false })

const skeleton = (
  <div>
    {[55, 40, 65, 50].map((w) => (
      <div key={w} className="ifx-skel">
        <Skel w={16} h={16} r={4} />
        <span className="ifx-grow"><Skel w={`${w}%`} h={16} /><Skel w="35%" /></span>
        <Skel w={38} h={22} r={11} /><Skel w={28} h={28} r={6} />
      </div>
    ))}
  </div>
)

export function InstallFonts({ u, who }: { u: InstallDetail; who: string }) {
  const [q, setQ] = useState('')
  const query = useDebounced(q.trim(), 150)
  const [stateF, setStateF] = useState<StateFilter>('all')
  const [sort, setSort] = useState<FamilySort>('name')
  const [source, setSource] = useState<'' | FontSource>('')
  const [overridden, setOverridden] = useState(false)
  const filter = useMemo<FontFilter>(() => ({
    q: query || undefined, active: stateF === 'all' ? undefined : stateF === 'on', source: source || undefined, overridden: overridden || undefined,
  }), [query, stateF, source, overridden])
  const key = `${query}|${stateF}|${source}|${overridden}|${sort}`
  const [page, setPage] = usePage(key)
  const state = useLoad(() => api.families(u.id, filter, sort, page), [u.id, key, page], 10_000)
  const { set, error, shakes, pending, drop, mark } = useFontToggle(u.id)
  const [confirmDialog, confirm] = useConfirm()

  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [rev, setRev] = useState(0)
  const flip = (f: string) => setExpanded((s) => { const n = new Set(s); if (n.has(f)) n.delete(f); else n.add(f); return n })

  /* Selection: cleared whenever the key changes, so "All N matching" can never act on another filter. */
  const [selState, setSelState] = useState(() => none(key))
  const sel = selState.key === key ? selState : none(key)
  const edit = (f: (s: Sel) => Sel) => setSelState((prev) => f(prev.key === key ? prev : none(key)))
  const lastPicked = useRef('')
  const headingRef = useRef<HTMLHeadingElement>(null)
  useEffect(() => { lastPicked.current = '' }, [key])

  /* Families waiting for the desktop after a batch (hash-level waiting lives in `pending`). */
  const [famWait, setFamWait] = useState<Record<string, { want: boolean; n?: number }>>({})
  const famTimers = useRef(new Map<string, ReturnType<typeof setTimeout>>())
  const dropFam = useCallback((f: string) => {
    clearTimeout(famTimers.current.get(f))
    famTimers.current.delete(f)
    setFamWait((w) => { if (!(f in w)) return w; const { [f]: _, ...rest } = w; return rest })
  }, [])
  useEffect(() => { const t = famTimers.current; return () => { t.forEach(clearTimeout); t.clear() } }, [])
  useEffect(() => {
    state.data?.items.forEach((f) => { const w = famWait[f.family]; if (w && (w.want ? f.on === f.styles : f.on === 0)) dropFam(f.family) })
  }, [state.data, famWait, dropFam])

  const refresh = () => { void state.reload(true); setRev((r) => r + 1) }

  /** One batch: `sels` is one request per selection kind; results are merged. */
  const batch = async (action: Action, sels: FontSelection[], fams: string[]) => {
    const rs = await Promise.allSettled(sels.map((s) => api.bulkFonts(u.id, action, s, 'hashes' in s ? {} : filter)))
    const done = rs.flatMap((r) => (r.status === 'fulfilled' ? [r.value] : []))
    const failed = rs.flatMap((r) => (r.status === 'rejected' ? [r.reason] : []))
    if (!done.length) {
      toast(errorText(failed[0]), { tone: 'bad' })
      return
    }
    const changed = done.flatMap((r) => r.changed)
    const skipped = done.reduce((n, r) => n + r.skipped, 0)
    const want = action === 'on'
    mark(changed, want)
    // A family toggle only waits as a whole when every style was sent; block never changes on/off here.
    if (changed.length && action !== 'block' && !skipped && !failed.length) {
      for (const f of fams) {
        clearTimeout(famTimers.current.get(f))
        famTimers.current.set(f, setTimeout(() => dropFam(f), PENDING_MS))
        setFamWait((w) => ({ ...w, [f]: { want, n: fams.length === 1 ? changed.length : undefined } }))
      }
    }
    const why = action === 'block' ? 'not in Shared library' : 'blocked'
    const left = (skipped ? ` · ${skipped} skipped (${why})` : '') + (failed.length ? ' · part of the selection failed' : '')
    const n = plural(changed.length, 'font')
    const opposite = action === 'on' ? 'off' : 'on'
    const undo = async () => {
      fams.forEach(dropFam)
      changed.forEach(drop)
      let r: { changed: string[]; skipped: number } | undefined
      if (action === 'block') await api.bulk('unblock', { hashes: changed })
      else {
        r = await api.bulkFonts(u.id, opposite, { hashes: changed }, {})
        mark(r.changed, opposite === 'on')
      }
      refresh()
      return r?.skipped ? `Undone · ${r.skipped} skipped (blocked)` : undefined
    }
    if (failed.length) toast(`${errorText(failed[0])} · the rest was applied`, { tone: 'bad' })
    if (!changed.length) toast(`No fonts changed${left}`)
    else toast((action === 'block' ? `Blocked ${n}` : `Turning ${action} ${n} on ${who}’s computer`) + left, { undo })
    setSelState(none(key))
    headingRef.current?.focus()
    refresh()
  }

  const fromBar = async (action: Action) => {
    if (action === 'block' && !(await confirm('Block these fonts on every computer? Only fonts in the Shared library can be blocked; others are skipped.', 'Block', true))) return
    const sels: FontSelection[] = sel.all ? [{ all: true }] : [
      ...(sel.families.size ? [{ families: [...sel.families.keys()] }] : []),
      ...(sel.hashes.size ? [{ hashes: [...sel.hashes.keys()] }] : []),
    ]
    await batch(action, sels, sel.all ? [] : [...sel.families.keys()])
  }

  const pickFamily = (items: FamilyRow[], i: number, checked: boolean, shift: boolean) => {
    const from = shift ? items.findIndex((x) => x.family === lastPicked.current) : -1
    const range = from < 0 ? [items[i]] : items.slice(Math.min(from, i), Math.max(from, i) + 1)
    lastPicked.current = items[i].family
    edit((s) => {
      const families = new Map(s.families)
      const hashes = new Map(s.hashes)
      for (const r of range) {
        if (checked) {
          families.set(r.family, r.styles)
          for (const [h, f] of hashes) if (f === r.family) hashes.delete(h)
        } else families.delete(r.family)
      }
      return { ...s, families, hashes }
    })
  }
  const pickHash = (hash: string, family: string, checked: boolean) =>
    edit((s) => { const hashes = new Map(s.hashes); if (checked) hashes.set(hash, family); else hashes.delete(hash); return { ...s, hashes } })

  const filtered = !!(query || stateF !== 'all' || source || overridden)
  const clearFilters = () => { setQ(''); setStateF('all'); setSource(''); setOverridden(false) }

  return (
    <motion.section aria-labelledby="af" className="card pad-18 stack-10" variants={fadeRise}>
      <div className="row-between">
        <h2 id="af" ref={headingRef} tabIndex={-1} className="h2 h2-sm">Fonts · {u.active_fonts} on</h2>
        <Link to="/library" className="small">Turn on fonts…</Link>
      </div>
      <div className="ifx-tools">
        <label htmlFor="fsearch" className="sr-only">Search this user's fonts</label>
        <input id="fsearch" type="search" className="input ifx-search" placeholder={`Search ${who}’s fonts`} value={q} onChange={(e) => setQ(e.target.value)} />
        <Chips small label="Font state" value={stateF} onChange={setStateF} options={[['all', 'All'], ['on', 'On'], ['off', 'Off']]} />
        <Chips small label="Sort families" value={sort} onChange={setSort} options={[['name', 'A–Z'], ['recent', 'Recently changed']]} />
        <select className="input" aria-label="Source" value={source} onChange={(e) => setSource(e.target.value as '' | FontSource)}>
          <option value="">Any source</option>
          <option value="user">Turned on by {who}</option>
          <option value="admin">Turned on by admin</option>
          <option value="library">Shared library</option>
          <option value="project_set">Project set</option>
        </select>
        <label className="ifx-check"><input type="checkbox" className="check" checked={overridden} onChange={(e) => setOverridden(e.target.checked)} />User overrides only</label>
      </div>
      {error && <span role="alert" className="form-error">{error}</span>}
      <Loaded state={state} skeleton={skeleton}>{({ items, total, fonts }) => <>
        <span className="meta" aria-live="polite">{plural(total, 'family', 'families')} · {plural(fonts, 'font')}</span>
        {total === 0 && (filtered
          ? <Empty title="No fonts match these filters" action={<button type="button" className="btn btn-sm" onClick={clearFilters}>Clear filters</button>} />
          : <Empty title="No fonts reported by this computer yet." />)}
        <div>
          <AnimatePresence initial={false}>
            {items.map((f, i) => {
              const open = expanded.has(f.family)
              const wait = famWait[f.family]
              const on = wait ? wait.want : f.on === f.styles
              const mixed = !wait && f.on > 0 && f.on < f.styles
              const checked = sel.all || sel.families.has(f.family)
              const sources = f.sources.map((s) => s === 'user' ? `by ${who}` : s === 'admin' ? 'by admin' : s === 'library' ? 'Shared library' : 'Project set').join(', ')
              return (
                <motion.div key={f.family} className="ifx-fam" {...row(i, items.length)} layout={false}>
                  <div className="font-row">
                    <input type="checkbox" className="check" aria-label={`Select ${f.family}`} checked={checked} disabled={sel.all}
                      onChange={(e) => pickFamily(items, i, e.target.checked, (e.nativeEvent as MouseEvent).shiftKey)} />
                    <span className="font-info">
                      <span className="font-name ellipsis" style={famText(f.family)}>{f.family}</span>
                      <span className="meta">
                        {plural(f.styles, 'style')} · {f.on} on · {sources}
                        {f.overridden > 0 && ` · ${f.overridden} overridden`}
                        {wait && ` · ${wait.n ? `${wait.n} waiting` : 'waiting'}`}
                      </span>
                    </span>
                    <Toggle on={on} mixed={mixed} disabled={!!wait} label={`${on ? 'Turn off' : 'Turn on'} all ${f.family} styles for ${who}`}
                      onChange={() => void batch(on ? 'off' : 'on', [{ families: [f.family] }], [f.family])} />
                    <button type="button" className="ifx-chev" aria-expanded={open} aria-controls={open ? `ifx-${i}` : undefined} aria-label={`${open ? 'Hide' : 'Show'} ${f.family} styles`} onClick={() => flip(f.family)}>›</button>
                  </div>
                  <AnimatePresence initial={false}>
                    {open && (
                      <motion.div id={`ifx-${i}`} className="ifx-styles" initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }}>
                        <Styles u={u} who={who} family={f.family} filter={filter} fkey={key} rev={rev} sel={sel} famSelected={checked}
                          onPick={pickHash} pending={pending} drop={drop} set={set} shakes={shakes} />
                      </motion.div>
                    )}
                  </AnimatePresence>
                </motion.div>
              )
            })}
          </AnimatePresence>
        </div>
        <Pager label="Font pages" page={page} total={total} onPage={setPage} />
        {(sel.all || sel.families.size + sel.hashes.size > 0) && (
          <div className="ifx-bar" role="region" aria-label="Selected fonts">
            <span className="ifx-bar-text">
              {sel.all ? `All ${num(fonts)} matching fonts` : [
                sel.families.size ? plural(sel.families.size, 'family', 'families') : '',
                plural([...sel.families.values()].reduce((a, b) => a + b, 0) + sel.hashes.size, 'font') + ' selected',
              ].filter(Boolean).join(', ')}
              {!sel.all && <button type="button" className="link-btn" onClick={() => edit((s) => ({ ...none(s.key), all: true }))}>Select all {num(fonts)} matching</button>}
            </span>
            <button type="button" className="btn btn-sm btn-primary" onClick={() => void fromBar('on')}>Turn on</button>
            <button type="button" className="btn btn-sm" onClick={() => void fromBar('off')}>Turn off</button>
            <button type="button" className="btn btn-sm btn-danger" onClick={() => void fromBar('block')}>Block everywhere</button>
            <button type="button" className="btn btn-sm" onClick={() => setSelState(none(key))}>Clear</button>
          </div>
        )}
      </>}</Loaded>
      <span className="meta">Changes reach {who}’s computer within seconds while online.</span>
      {confirmDialog}
    </motion.section>
  )
}

const STYLE_LIMIT = 200

function Styles({ u, who, family, filter, fkey, rev, sel, famSelected, onPick, pending, drop, set, shakes }: {
  u: InstallDetail; who: string; family: string; filter: FontFilter; fkey: string; rev: number; sel: Sel; famSelected: boolean
  onPick: (hash: string, family: string, checked: boolean) => void
  pending: Record<string, boolean>; drop: (hash: string) => void
  set: (hash: string, active: boolean, announce?: string) => Promise<void>; shakes: Record<string, number>
}) {
  const state = useLoad(
    () => api.fonts(u.id, filter.q ?? '', 1, filter.active, { family, source: filter.source, overridden: filter.overridden, size: STYLE_LIMIT }),
    [u.id, family, fkey, rev], 10_000)
  // A poll that reports the requested state ends the wait.
  useEffect(() => {
    state.data?.items.forEach((f) => { if (pending[f.hash] === f.active) drop(f.hash) })
  }, [state.data, pending, drop])
  return (
    <Loaded state={state} skeleton={<span className="stack-10"><Skel w="50%" h={16} /><Skel w="40%" h={16} /></span>}>{({ items, total }) => {
      // Active first, like the server; toggling re-sorts so rows glide to their new place.
      const shown = items.map((f) => (f.hash in pending ? { ...f, active: pending[f.hash] } : f)).sort((a, b) => Number(b.active) - Number(a.active))
      return <>
        <AnimatePresence initial={false}>
          {shown.map((f, i) => {
            const name = `${f.family} ${f.style}`
            const lock = sel.all || famSelected
            return (
              <motion.div key={f.hash} className="font-row" {...row(i, shown.length)}>
                <input type="checkbox" className="check" aria-label={`Select ${name}`} checked={lock || sel.hashes.has(f.hash)} disabled={lock}
                  onChange={(e) => onPick(f.hash, family, e.target.checked)} />
                <span className="font-info">
                  <span className="font-name ellipsis" style={famText(f.family)}>{f.style}</span>
                  <span className="meta">
                    {f.hash in pending ? 'Waiting for this computer' : sourceText(f, who)}{f.blocked && ' · Blocked'} · {ago(f.updated_at)}
                  </span>
                </span>
                <Toggle on={f.active} label={`${f.active ? 'Turn off' : 'Turn on'} ${name} for ${who}`} disabled={f.hash in pending} shakeKey={shakes[f.hash]}
                  onChange={() => void set(f.hash, !f.active, `Turning ${name} ${f.active ? 'off' : 'on'} on ${who}’s computer.`)} />
              </motion.div>
            )
          })}
        </AnimatePresence>
        {total > STYLE_LIMIT && <span className="meta ifx-more">Showing {STYLE_LIMIT} of {num(total)}</span>}
      </>
    }}</Loaded>
  )
}
