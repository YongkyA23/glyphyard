import { useCallback, useEffect, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, type GoogleFont } from '../api'
import { loadGoogle } from '../fonts'
import { cssStagger, dur, easeOut } from '../motion'
import { useApp } from '../state'
import { Check, Chip, errMsg, plural, Skel, Spinner } from '../ui'

// Chip label → Google Fonts metadata category (the contract doesn't fix these values).
const CATS: [string, string | undefined][] = [
  ['All', undefined], ['Serif', 'Serif'], ['Sans', 'Sans Serif'], ['Display', 'Display'], ['Handwriting', 'Handwriting'], ['Mono', 'Monospace'],
]
const shortCat = (c: string) => CATS.find(([, v]) => v === c)?.[0] ?? c
const generic = (c: string) => ({ Serif: 'serif', Monospace: 'monospace', Handwriting: 'cursive' })[c] ?? 'sans-serif'

const PAGE = 30
const CACHE_MS = 5 * 60 * 1000
type Loaded = { items: GoogleFont[]; total: number; at: number }
// Pages already fetched per search + category, kept across visits so going back is instant.
// ponytail: unbounded map of small objects; a handful of queries per session is fine.
const cache = new Map<string, Loaded>()

/** True once the element has come near the viewport (it never goes back to false). */
function useSeen<T extends Element>(margin = '200px') {
  const ref = useRef<T>(null)
  const [seen, setSeen] = useState(false)
  useEffect(() => {
    const el = ref.current
    if (!el || seen) return
    const io = new IntersectionObserver((es) => es.some((e) => e.isIntersecting) && setSeen(true), { rootMargin: margin })
    io.observe(el)
    return () => io.disconnect()
  }, [seen, margin])
  return [ref, seen] as const
}

export function GoogleFonts() {
  const { toast } = useApp()
  const [params] = useSearchParams()
  const [search, setSearch] = useState(params.get('q') ?? '')
  const [cat, setCat] = useState('All')
  const category = CATS.find(([l]) => l === cat)?.[1]
  const key = `${search.trim().toLowerCase()}|${category ?? ''}`
  const [data, setData] = useState<Loaded | null>(() => cache.get(key) ?? null)
  const [loadingMore, setLoadingMore] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const req = useRef(0)

  const store = useCallback((k: string, d: Loaded) => (cache.set(k, d), setData(d)), [])

  // First page for this search + category (debounced while typing); cached results show at once.
  useEffect(() => {
    const hit = cache.get(key)
    if (hit && Date.now() - hit.at < CACHE_MS) {
      setData(hit)
      return
    }
    setData(null)
    const id = ++req.current
    const t = setTimeout(() => {
      api.googleCatalog(search.trim() || undefined, category, 0, PAGE).then(
        (p) => id === req.current && (store(key, { ...p, at: Date.now() }), setError(null)),
        (e) => id === req.current && setError(errMsg(e)),
      )
    }, search ? 250 : 0)
    return () => clearTimeout(t)
  }, [key, search, category, store])

  const hasMore = !!data && data.items.length < data.total
  const loadMore = useCallback(() => {
    if (!data || !hasMore || loadingMore) return
    const id = req.current
    setLoadingMore(true)
    api.googleCatalog(search.trim() || undefined, category, data.items.length, PAGE)
      .then((p) => id === req.current && store(key, { items: [...data.items, ...p.items], total: p.total, at: data.at }), (e) => toast(errMsg(e)))
      .finally(() => setLoadingMore(false))
  }, [data, hasMore, loadingMore, search, category, key, store, toast])

  // Infinite scroll: load the next page when the sentinel below the grid gets close.
  const sentinel = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = sentinel.current
    if (!el || !hasMore) return
    const io = new IntersectionObserver((es) => es.some((e) => e.isIntersecting) && loadMore(), { rootMargin: '600px' })
    io.observe(el)
    return () => io.disconnect()
  }, [hasMore, loadMore])

  async function toggle(f: GoogleFont) {
    setBusy(f.family)
    try {
      const res = await (f.installed ? api.googleRemove(f.family) : api.googleInstall(f.family))
      // Keep every cached query in step so going back doesn't show a stale button.
      for (const [k, d] of cache) cache.set(k, { ...d, items: d.items.map((r) => (r.family === res.family ? { ...r, installed: res.installed } : r)) })
      setData(cache.get(key) ?? null)
      toast(res.installed ? `${f.family} installed and labeled Licensed (OFL)` : `${f.family} removed`)
    } catch (e) {
      toast(errMsg(e))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="page" style={{ gap: 16 }}>
      <div className="row-between">
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <h1 className="h-page">Google Fonts</h1>
          <span className="muted">Free fonts. Installed fonts are labeled Licensed (SIL Open Font License) automatically.</span>
        </div>
        <label htmlFor="gsearch" className="sr-only">Search Google Fonts</label>
        <input
          id="gsearch" type="search" className="input" style={{ flex: '0 1 320px' }} value={search} onChange={(e) => setSearch(e.target.value)}
          placeholder={data && !search && cat === 'All' ? `Search ${data.total.toLocaleString()} families` : 'Search families'}
        />
      </div>
      <div className="chips">
        {CATS.map(([label]) => <Chip key={label} pressed={cat === label} onClick={() => setCat(label)}>{label}</Chip>)}
      </div>
      {error && <p role="alert" style={{ margin: 0, color: 'var(--bad)' }}>{error}</p>}
      {data?.total === 0 && <p className="faint">No families match.</p>}
      <div className="gf-grid">
        {data === null && !error && Array.from({ length: 9 }, (_, i) => <CardSkeleton key={i} />)}
        {data?.items.map((f, i) => (
          <GoogleCard key={f.family} f={f} stagger={i % PAGE} busy={busy === f.family} onToggle={() => toggle(f)} />
        ))}
        {loadingMore && Array.from({ length: 3 }, (_, i) => <CardSkeleton key={`more-${i}`} />)}
      </div>
      <div ref={sentinel} aria-hidden="true" style={{ height: 1 }} />
      {data && data.total > 0 && (
        <p className="faint" role="status" style={{ margin: 0, fontSize: 12, textAlign: 'center' }}>
          {hasMore ? `Showing ${data.items.length} of ${data.total.toLocaleString()}` : `All ${data.total.toLocaleString()} shown`}
        </p>
      )}
    </div>
  )
}

function CardSkeleton() {
  return (
    <div aria-hidden="true" className="card" style={{ gap: 14, padding: 18 }}>
      <Skel w="75%" h={40} r={8} />
      <span className="row-between" style={{ flexWrap: 'nowrap' }}><Skel w={110} h={10} /><Skel w={72} h={34} r={8} /></span>
    </div>
  )
}

function GoogleCard({ f, stagger, busy, onToggle }: { f: GoogleFont; stagger: number; busy: boolean; onToggle: () => void }) {
  const [ref, seen] = useSeen<HTMLDivElement>()
  // Lazy preview: fetch the typeface only once the card is near the screen, and only the glyphs of its name.
  useEffect(() => {
    if (seen) loadGoogle(f.family.replace(/ /g, '+'), f.family, 'swap')
  }, [seen, f.family])
  const label = busy ? (f.installed ? 'Removing…' : 'Installing…') : f.installed ? 'Installed' : 'Install'
  return (
    <div ref={ref} className="card enter" style={{ gap: 14, padding: 18, ...cssStagger(stagger) }}>
      <span className="ellipsis" style={{ fontFamily: `'${f.family}', ${generic(f.category)}`, fontSize: 40, lineHeight: 1.1 }}>{f.family}</span>
      <span className="row-between" style={{ flexWrap: 'nowrap', gap: 10 }}>
        <span className="faint" style={{ fontSize: 12 }}>{shortCat(f.category)} · {f.variable ? 'Variable' : plural(f.styles, 'style')}</span>
        <button
          type="button" className={f.installed ? 'btn small installed' : 'btn small primary'} aria-label={`${f.installed ? 'Remove' : 'Install'} ${f.family}`}
          onClick={onToggle} disabled={busy}
        >
          <AnimatePresence mode="popLayout" initial={false}>
            <motion.span
              key={label} style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
              initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -8 }} transition={{ duration: dur.base, ease: easeOut }}
            >
              {busy && <Spinner />}
              {label}
              {label === 'Installed' && <Check />}
            </motion.span>
          </AnimatePresence>
        </button>
      </span>
    </div>
  )
}
