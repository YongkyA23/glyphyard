import { useEffect, useState } from 'react'
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

export function GoogleFonts() {
  const { toast } = useApp()
  const [params] = useSearchParams()
  const [search, setSearch] = useState(params.get('q') ?? '')
  const [cat, setCat] = useState('All')
  const [rows, setRows] = useState<GoogleFont[] | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const category = CATS.find(([l]) => l === cat)?.[1]
    const t = setTimeout(() => api.googleCatalog(search.trim() || undefined, category).then((r) => (setRows(r), setError(null)), (e) => setError(errMsg(e))), search ? 200 : 0)
    return () => clearTimeout(t)
  }, [search, cat])

  async function toggle(f: GoogleFont) {
    setBusy(f.family)
    try {
      const res = await (f.installed ? api.googleRemove(f.family) : api.googleInstall(f.family))
      setRows((rs) => rs?.map((r) => (r.family === res.family ? { ...r, installed: res.installed } : r)) ?? null)
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
        <input id="gsearch" type="search" className="input" style={{ flex: '0 1 320px' }} placeholder="Search 1,700+ families" value={search} onChange={(e) => setSearch(e.target.value)} />
      </div>
      <div className="chips">
        {CATS.map(([label]) => <Chip key={label} pressed={cat === label} onClick={() => setCat(label)}>{label}</Chip>)}
      </div>
      {error && <p role="alert" style={{ margin: 0, color: 'var(--bad)' }}>{error}</p>}
      {rows?.length === 0 && <p className="faint">No families match.</p>}
      <div className="gf-grid">
        {rows === null && !error && [0, 1, 2, 3, 4, 5].map((i) => (
          <div key={i} aria-hidden="true" className="card" style={{ gap: 14, padding: 18 }}>
            <Skel w="75%" h={40} r={8} />
            <span className="row-between" style={{ flexWrap: 'nowrap' }}><Skel w={110} h={10} /><Skel w={72} h={34} r={8} /></span>
          </div>
        ))}
        {rows?.map((f, i) => {
          loadGoogle(f.family.replace(/ /g, '+'), f.family)
          const label = busy === f.family ? (f.installed ? 'Removing…' : 'Installing…') : f.installed ? 'Installed' : 'Install'
          return (
            <div key={f.family} className="card enter" style={{ gap: 14, padding: 18, ...cssStagger(i) }}>
              <span className="ellipsis" style={{ fontFamily: `'${f.family}', ${generic(f.category)}`, fontSize: 40, lineHeight: 1.1 }}>{f.family}</span>
              <span className="row-between" style={{ flexWrap: 'nowrap', gap: 10 }}>
                <span className="faint" style={{ fontSize: 12 }}>{shortCat(f.category)} · {f.variable ? 'Variable' : plural(f.styles, 'style')}</span>
                <button
                  type="button" className={f.installed ? 'btn small installed' : 'btn small primary'} aria-label={`${f.installed ? 'Remove' : 'Install'} ${f.family}`}
                  onClick={() => toggle(f)} disabled={busy === f.family}
                >
                  <AnimatePresence mode="popLayout" initial={false}>
                    <motion.span
                      key={label} style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
                      initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -8 }} transition={{ duration: dur.base, ease: easeOut }}
                    >
                      {busy === f.family && <Spinner />}
                      {label}
                      {label === 'Installed' && <Check />}
                    </motion.span>
                  </AnimatePresence>
                </button>
              </span>
            </div>
          )
        })}
      </div>
    </div>
  )
}
