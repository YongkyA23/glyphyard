import { Fragment, useCallback, useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { animate, AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { api, tagsOf, type Axis, type Face, type Family, type FamilyDetail, type FileRow, type FontDetails, type GlyphBlock, type License, type ProjectSet } from '../api'
import { AXIS_NAME, axisStep, faceStyle, familyStyle, pickFace } from '../fonts'
import { cssStagger, dur, easeOut, pop, rise, spring } from '../motion'
import { useApp } from '../state'
import { useBreathing } from '../useBreathing'
import { usePreviewWheel } from '../usePreviewWheel'
import { useSampleText } from '../useSampleText'
import { Badge, basename, CATEGORY_LABEL, Chip, Dialog, dirname, errMsg, LicenseBadge, num, plural, Skel, Switch, useSeen, type Tone } from '../ui'

const FEATURE_NAME: Record<string, string> = {
  liga: 'Ligatures', dlig: 'Discretionary ligatures', calt: 'Contextual alternates', kern: 'Kerning', onum: 'Old-style figures', lnum: 'Lining figures',
  tnum: 'Tabular figures', pnum: 'Proportional figures', frac: 'Fractions', smcp: 'Small caps', c2sc: 'Caps to small caps', zero: 'Slashed zero',
  case: 'Case-sensitive forms', salt: 'Stylistic alternates', swsh: 'Swashes', sups: 'Superscript', subs: 'Subscript', ordn: 'Ordinals',
}
const featureName = (t: string) => FEATURE_NAME[t] ?? (/^ss\d\d$/.test(t) ? `Stylistic set ${Number(t.slice(2))}` : /^cv\d\d$/.test(t) ? `Character variant ${Number(t.slice(2))}` : t)
const ON_BY_DEFAULT = new Set(['liga', 'calt', 'kern'])

const FILE_STATUS: Record<FileRow['status'], [string, Tone]> = {
  active: ['Active', 'ok'], older_version: ['Older version', 'warn'], alternate: ['Alternate', 'accent'], grouped: ['Grouped with variable', 'neutral'], inactive: ['Inactive', 'neutral'],
  system: ['System font', 'neutral'],
}
const LICENSE_OPTIONS: [License, string][] = [['licensed', 'Licensed'], ['personal', 'Personal use only'], ['client', 'Client-provided'], ['unknown', 'Unknown']]

const WATERFALL = [72, 48, 32, 24, 16]
/** Seconds the Play showcase spends sweeping one axis (max → min → default). */
const SWEEP = 2.4
const TABS = [
  ['styles', 'Styles'], ['preview', 'Preview'], ['glyphs', 'Glyphs'], ['similar', 'Similar'], ['details', 'Details'], ['files', 'Files and versions'], ['license', 'License'],
] as const
type Tab = (typeof TABS)[number][0]
/** Weight order, each italic right after its upright. */
const byWeight = (faces: Face[]) => [...faces].sort((a, b) => a.weight - b.weight || Number(a.italic) - Number(b.italic) || a.style.localeCompare(b.style))

/** One face with its own preview line and on/off switch. The font file loads once the row is near the viewport. */
function FaceRow({ face, i, sample, size, axes, onSize, onToggle }: { face: Face; i: number; sample: string; size: number; axes: Axis[]; onSize: (delta: number) => void; onToggle: () => void }) {
  const [ref, seen] = useSeen<HTMLDivElement>()
  const [pvRef, pvStyle] = usePreviewWheel({ axes, variable: face.isVariable, onSize })
  const on = face.active || face.system
  return (
    <div ref={ref} className="face-row enter" style={cssStagger(i)}>
      <Switch on={on} disabled={face.system} onClick={onToggle} label={face.system ? `${face.fullName} is a system font, always on` : `${on ? 'Deactivate' : 'Activate'} ${face.fullName}`} />
      <div className="face-meta">
        <span className="ellipsis" style={{ fontWeight: 500 }}>{face.style}</span>
        <span className="faint ellipsis" style={{ fontSize: 12 }}>
          {face.weight}{face.italic && ' · italic'}{face.isVariable && ' · variable'} · v{face.version}
        </span>
      </div>
      <div ref={pvRef} className="sample" style={{ ...(seen ? faceStyle(face) : {}), ...pvStyle, fontSize: size, color: on ? 'var(--ink)' : 'var(--ink-2)' }}>{sample}</div>
    </div>
  )
}

/** Blocks show at most this many cells until "Show all". */
const GLYPH_CAP = 512
const uPlus = (cp: number) => `U+${cp.toString(16).toUpperCase().padStart(4, '0')}`

/** One Unicode block; its cells render only once it comes near the viewport. */
function GlyphSection({ b, style, onPeek, onCopy }: { b: GlyphBlock; style: CSSProperties; onPeek: (cp: number) => void; onCopy: (cp: number) => void }) {
  const [ref, seen] = useSeen<HTMLElement>('400px')
  const [all, setAll] = useState(false)
  const cps = all ? b.chars : b.chars.slice(0, GLYPH_CAP)
  return (
    <section ref={ref} className="glyph-block" aria-label={b.name}>
      <h3 className="h-small" style={{ margin: 0 }}>
        {b.name} <span className="faint" style={{ fontWeight: 400, fontSize: 12 }}>{num(b.chars.length)} · {uPlus(b.start)}–{uPlus(b.end)}</span>
      </h3>
      {seen ? (
        <div className="glyph-grid">
          {cps.map((cp, i) => {
            const ch = String.fromCodePoint(cp)
            return (
              <button
                key={cp} type="button" className="glyph" style={{ ...style, ...cssStagger(i) }} aria-label={`${ch} ${uPlus(cp)}, copy`}
                onClick={() => onCopy(cp)} onMouseEnter={() => onPeek(cp)} onFocus={() => onPeek(cp)}
              >{ch}</button>
            )
          })}
        </div>
      ) : (
        <div className="glyph-placeholder" style={{ height: Math.min(Math.ceil(b.chars.length / 10), 6) * 70 }} />
      )}
      {seen && b.chars.length > GLYPH_CAP && (
        <button type="button" className="btn small" style={{ alignSelf: 'flex-start' }} onClick={() => setAll(!all)}>
          {all ? `Show the first ${num(GLYPH_CAP)}` : `Show all ${num(b.chars.length)}`}
        </button>
      )}
    </section>
  )
}

/** Every character the chosen face maps, grouped by Unicode block. Click a cell to copy it. */
function GlyphsTab({ fam, extra }: { fam: FamilyDetail; extra: CSSProperties }) {
  const { toast } = useApp()
  const faces = byWeight(fam.faces)
  const [hash, setHash] = useState(() => pickFace(fam)?.hash ?? '')
  const face = faces.find((x) => x.hash === hash) ?? faces[0]
  const [data, setData] = useState<{ hash: string; total: number; blocks: GlyphBlock[] } | null>(null)
  const [block, setBlock] = useState('')
  const [peek, setPeek] = useState<number | null>(null)
  useEffect(() => {
    if (!face) return
    let live = true
    api.faceGlyphs(face.hash).then(
      (g) => live && setData({ hash: face.hash, ...g }),
      (e) => live && (toast(errMsg(e)), setData({ hash: face.hash, total: 0, blocks: [] })),
    )
    return () => void (live = false)
  }, [face?.hash, toast]) // eslint-disable-line react-hooks/exhaustive-deps
  if (!face) return <p className="faint" style={{ margin: 0 }}>This family has no fonts left.</p>

  const ready = data?.hash === face.hash ? data : null
  const cur = ready?.blocks.some((b) => b.name === block) ? block : ''
  const style = { ...faceStyle(face), ...extra }
  const copy = (cp: number) => {
    const ch = String.fromCodePoint(cp)
    navigator.clipboard.writeText(ch).then(() => toast(`Copied ${ch} (${uPlus(cp)})`), () => toast('Couldn’t copy to the clipboard.'))
  }
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div className="glyph-tools muted">
        {faces.length > 1 && (
          <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            Style
            <select className="select" value={face.hash} onChange={(e) => (setHash(e.target.value), setPeek(null))}>
              {faces.map((x) => <option key={x.hash} value={x.hash}>{x.style}</option>)}
            </select>
          </label>
        )}
        <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          Unicode block
          <select className="select" value={cur} onChange={(e) => setBlock(e.target.value)} disabled={!ready}>
            <option value="">All blocks</option>
            {ready?.blocks.map((b) => <option key={b.name} value={b.name}>{b.name} ({num(b.chars.length)})</option>)}
          </select>
        </label>
        {ready && <span>{plural(ready.total, 'character')} mapped · {plural(fam.glyphCount, 'glyph')} in the font</span>}
        <span className="glyph-info" aria-hidden="true">
          <b style={style}>{peek !== null ? String.fromCodePoint(peek) : ''}</b>
          <span className="mono" style={{ fontSize: 12 }}>{peek !== null ? uPlus(peek) : 'Hover a character'}</span>
        </span>
      </div>
      {!ready ? (
        <div className="glyph-grid" aria-busy="true">{Array.from({ length: 24 }, (_, i) => <Skel key={i} h={64} r={8} />)}</div>
      ) : ready.blocks.length === 0 ? (
        <p className="faint" style={{ margin: 0 }}>No characters found in this font’s character map.</p>
      ) : (
        <div onMouseLeave={() => setPeek(null)}>
          {ready.blocks.filter((b) => !cur || b.name === cur).map((b) => (
            <GlyphSection key={`${face.hash}-${b.name}`} b={b} style={style} onPeek={setPeek} onCopy={copy} />
          ))}
        </div>
      )}
    </div>
  )
}

/** Only http(s) links leave the app; anything else from the font file shows as text. */
function ExtLink({ url }: { url: string }) {
  const href = /^https?:\/\//i.test(url) ? url : /^www\./i.test(url) ? `https://${url}` : null
  if (!href) return <>{url}</>
  // The webview can't open new windows; the browser is opened from Rust (open_url).
  const open = (e: React.MouseEvent) => (e.preventDefault(), void api.openUrl(href).catch(() => {}))
  return <a href={href} onClick={open} rel="noreferrer noopener">{url.replace(/^https?:\/\//i, '').replace(/\/$/, '')} <span aria-hidden="true">↗</span><span className="sr-only">(opens in your browser)</span></a>
}
const withLink = (text: string | null, url: string | null): ReactNode =>
  text && url ? <>{text} · <ExtLink url={url} /></> : url ? <ExtLink url={url} /> : text

function DetailsTab({ d }: { d: FontDetails }) {
  const rows: [string, ReactNode][] = [
    ['Copyright', d.copyright], ['Trademark', d.trademark], ['Manufacturer', withLink(d.manufacturer, d.vendorUrl)],
    ['Designer', withLink(d.designer, d.designerUrl)], ['Description', d.description], ['License', withLink(d.license, d.licenseUrl)],
    ['Version', d.version], ['Unique ID', d.uniqueId], ['Format', d.format], ['Units per em', d.upm != null ? num(d.upm) : null],
    ['Glyphs', num(d.glyphCount)],
  ]
  return (
    <dl className="details">
      {rows.filter(([, v]) => v).map(([k, v], i) => (
        <Fragment key={k}>
          <dt className="enter" style={cssStagger(i)}>{k}</dt>
          <dd className="enter" style={cssStagger(i)}>{v}</dd>
        </Fragment>
      ))}
    </dl>
  )
}

/** Tag chips with remove buttons, plus an input that suggests existing tags. */
function TagEditor({ familyKey }: { familyKey: string }) {
  const { libVersion, toast, bump } = useApp()
  const [tags, setTags] = useState<string[] | null>(null)
  const [known, setKnown] = useState<string[]>([])
  const [draft, setDraft] = useState('')
  useEffect(() => {
    tagsOf([familyKey]).then((m) => setTags(m.get(familyKey) ?? []), () => setTags([]))
    api.listTags().then((t) => setKnown(t.map((x) => x.name)), () => {})
  }, [familyKey, libVersion])

  async function save(next: string[]) {
    const prev = tags
    setTags(next)
    try {
      setTags(await api.setTags(familyKey, next))
      bump()
    } catch (e) {
      setTags(prev)
      toast(errMsg(e))
    }
  }
  if (!tags) return <Skel w={180} h={26} r={13} />
  return (
    <div className="tag-editor" role="group" aria-label="Tags">
      <AnimatePresence initial={false} mode="popLayout">
        {tags.map((t) => (
          <motion.span key={t} layout className="tag-chip" {...pop} transition={spring.pop}>
            <Link to={`/?tag=${encodeURIComponent(t)}`} className="plain-link">#{t}</Link>
            <button type="button" aria-label={`Remove tag ${t}`} onClick={() => save(tags.filter((x) => x !== t))}>✕</button>
          </motion.span>
        ))}
      </AnimatePresence>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          const t = draft.trim()
          if (t && !tags.includes(t)) save([...tags, t])
          setDraft('')
        }}
      >
        <input
          className="tag-input" list="tag-suggestions" aria-label="Add a tag" placeholder={tags.length ? '+ Tag' : '+ Add a tag'}
          value={draft} onChange={(e) => setDraft(e.target.value)}
        />
        <datalist id="tag-suggestions">{known.filter((t) => !tags.includes(t)).map((t) => <option key={t} value={t} />)}</datalist>
      </form>
    </div>
  )
}

function SimilarCard({ f, i }: { f: Family; i: number }) {
  const [ref, seen] = useSeen<HTMLAnchorElement>()
  const style = seen ? familyStyle(f) : {}
  return (
    <Link ref={ref} to={`/family/${encodeURIComponent(f.key)}`} className="select-card similar-card enter" style={cssStagger(i)}>
      <span aria-hidden="true" style={{ ...style, fontSize: 44, lineHeight: 1.1 }}>Ag</span>
      <span className="ellipsis" style={{ ...style, fontSize: 18, color: 'var(--ink-2)' }}>{f.family}</span>
      <span className="faint" style={{ fontSize: 12 }}>
        {plural(f.styles, 'style')}{f.category && ` · ${CATEGORY_LABEL[f.category]}`} · {f.foundry}
      </span>
    </Link>
  )
}

export function FamilyPage() {
  const { key = '' } = useParams()
  const { libVersion, activate, toast } = useApp()
  const navigate = useNavigate()
  const [fam, setFam] = useState<FamilyDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [tab, setTab] = useState<Tab>('styles')
  const [axes, setAxes] = useState<Record<string, number>>({})
  const [feat, setFeat] = useState<Record<string, boolean>>({})
  const [sample, setSample] = useState('The quick brown fox jumps over the lazy dog')
  const [note, setNote] = useState('')
  const [setsOpen, setSetsOpen] = useState(false)
  const [sets, setSets] = useState<ProjectSet[]>([])
  const [playing, setPlaying] = useState(false)
  const [similar, setSimilar] = useState<Family[] | null>(null)
  const stopPlay = useRef<(() => void) | null>(null)
  const [previewSize, setPreviewSize] = useState(30)
  const nudgeSize = useCallback((d: number) => setPreviewSize((s) => Math.min(72, Math.max(16, s + d))), [])
  const shownSample = useSampleText(sample)
  useBreathing()
  const reduce = useReducedMotion()
  useEffect(() => () => stopPlay.current?.(), [key, tab])
  useEffect(() => void api.getSettings().then((s) => s.sampleText && setSample(s.sampleText), () => {}), [])
  useEffect(() => {
    if (tab !== 'similar' || similar) return
    api.similarFamilies(key, 12).then(setSimilar, (e) => (toast(errMsg(e)), setSimilar([])))
  }, [tab, key, similar, toast])

  useEffect(() => {
    api.getFamily(key).then((f) => {
      setFam(f)
      setNote(f.licenseInfo.note)
      setAxes((cur) => Object.fromEntries(f.axes.map((a) => [a.tag, cur[a.tag] ?? a.default])))
      setFeat((cur) => Object.fromEntries(f.features.map((t) => [t, cur[t] ?? ON_BY_DEFAULT.has(t)])))
    }, (e) => setError(errMsg(e)))
  }, [key, libVersion])

  if (error) return <div className="page"><Link to="/" className="back">← Library</Link><p role="alert">{error}</p></div>
  if (!fam)
    return (
      <div className="page" aria-busy="true" style={{ gap: 20, padding: '20px 32px 32px' }}>
        <Link to="/" className="back">← Library</Link>
        <Skel w="45%" h={56} r={8} />
        <Skel w="30%" h={14} />
        <Skel h={40} r={0} />
        {WATERFALL.map((s) => <Skel key={s} w={`${40 + s}%`} h={Math.round(s * 0.8)} />)}
      </div>
    )

  const fvs = fam.axes.map((a) => `'${a.tag}' ${axes[a.tag] ?? a.default}`).join(', ') || 'normal'
  const ffs = Object.entries(feat).map(([t, v]) => `'${t}' ${v ? 1 : 0}`).join(', ') || 'normal'
  const previewStyle = { ...familyStyle(fam), fontVariationSettings: fvs, fontFeatureSettings: ffs }
  const allOn = fam.activeState === 'all'

  /** Showcase: sweep each variable axis through its range, one after another. */
  const play = async () => {
    if (stopPlay.current) return stopPlay.current()
    let stopped = false
    let ctl: ReturnType<typeof animate> | undefined
    stopPlay.current = () => ((stopped = true), ctl?.stop())
    setPlaying(true)
    for (const a of fam.axes) {
      if (stopped) break
      const step = axisStep(a)
      await new Promise<void>((done) => {
        ctl = animate(axes[a.tag] ?? a.default, [a.max, a.min, a.default], {
          duration: SWEEP, ease: 'easeInOut', onComplete: done, onStop: done,
          onUpdate: (v) => setAxes((cur) => ({ ...cur, [a.tag]: Number((Math.round(v / step) * step).toFixed(step < 1 ? 1 : 0)) })),
        })
      })
    }
    stopPlay.current = null
    setPlaying(false)
  }

  const toggleFamily = async () => {
    await activate(() => api.setActive(fam.faces.filter((x) => !x.system).map((x) => x.hash), !allOn))
    api.getFamily(key).then(setFam, () => {})
  }
  const toggleFace = async (face: Face) => {
    await activate(() => api.setActive([face.hash], !face.active))
    api.getFamily(key).then(setFam, () => {})
  }
  const setLicense = async (label: License, file?: string | null, nextNote?: string) => {
    try {
      const licenseInfo = await api.setLicense(fam.key, label, file, nextNote)
      setFam({ ...fam, license: licenseInfo.label, licenseInfo })
    } catch (e) {
      toast(errMsg(e))
    }
  }
  const attach = async () => {
    const path = await api.pickFile([{ name: 'License', extensions: ['txt', 'pdf', 'md', 'rtf', 'html'] }]).catch(() => null)
    if (path) setLicense(fam.licenseInfo.label, path)
  }
  const openSets = () => {
    setSetsOpen(true)
    api.listProjectSets().then(setSets, (e) => toast(errMsg(e)))
  }
  const addToSet = async (s: ProjectSet) => {
    try {
      setSets(await api.saveProjectSet({ id: s.id, name: s.name, note: s.note, familyKeys: [...s.familyKeys, fam.key] }))
      toast(`Added ${fam.family} to ${s.name}`)
    } catch (e) {
      toast(errMsg(e))
    }
  }

  return (
    <div className="page" style={{ gap: 20, padding: '20px 32px 32px' }}>
      <Link to="/" className="back">← Library</Link>
      <div className="row-between" style={{ alignItems: 'flex-end', gap: 20 }}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8, minWidth: 0 }}>
          <h1 className="family-title" style={familyStyle(fam)}>{fam.family}</h1>
          <div className="muted" style={{ display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'center', fontSize: 13 }}>
            <span>{fam.foundry}</span><span aria-hidden="true">·</span>
            <span>{plural(fam.styles, 'style')}{fam.isVariable && ' + variable'}</span>
            {fam.faces[0] && <><span aria-hidden="true">·</span><span>v{fam.faces[0].version}</span></>}
            <LicenseBadge license={fam.license} />
            {fam.system && <Badge tone="neutral">System font · always on</Badge>}
            {fam.adminSet && <Badge tone="accent">Set by admin</Badge>}
            {fam.category && <Badge tone="neutral">{CATEGORY_LABEL[fam.category]}</Badge>}
            {fam.pinned && <Badge tone="accent">Pinned</Badge>}
          </div>
          <TagEditor familyKey={fam.key} />
        </div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
          <button type="button" className={allOn ? 'btn primary' : 'btn'} aria-pressed={allOn} onClick={toggleFamily} disabled={fam.system} style={{ fontWeight: 500 }}>
            {allOn ? 'All on · turn off' : fam.activeState === 'some' ? 'Turn all on' : 'Activate family'}
          </button>
          <button type="button" className="btn" onClick={openSets}>Add to project set</button>
          <button type="button" className="btn" onClick={() => fam.faces[0] && api.showInFolder(fam.faces[0].path).catch((e) => toast(errMsg(e)))}>Show in folder</button>
        </div>
      </div>

      <div role="tablist" aria-label="Font views" className="tabs">
        {TABS.map(([k, label]) => (
          <button key={k} type="button" role="tab" id={`tab-${k}`} aria-controls={`panel-${k}`} aria-selected={tab === k} onClick={() => setTab(k)}>
            {label}
            {tab === k && <motion.span layoutId="tab-ink" className="tab-ink" transition={spring.glide} />}
          </button>
        ))}
      </div>

      <AnimatePresence mode="wait" initial={false}>
      <motion.div
        key={tab} role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`}
        initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: dur.fast, ease: easeOut }}
      >
        {tab === 'styles' && (
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            {byWeight(fam.faces).map((x, i) => <FaceRow key={x.hash} face={x} i={i} sample={shownSample} size={previewSize} axes={fam.axes} onSize={nudgeSize} onToggle={() => toggleFace(x)} />)}
            {!fam.faces.length && <p className="faint" style={{ margin: 0 }}>This family has no fonts left.</p>}
          </div>
        )}

        {tab === 'preview' && (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 24 }}>
            <div style={{ flex: '2 1 440px', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 14 }}>
              {WATERFALL.map((s, i) => (
                <motion.div key={s} className="waterfall-row" {...rise} transition={{ duration: dur.slow, ease: easeOut, delay: 0.05 + i * 0.05 }}>
                  <span className="faint" style={{ width: 40, flexShrink: 0, fontSize: 12 }}>{s}</span>
                  <span className="ellipsis" style={{ ...previewStyle, fontSize: s, lineHeight: 1.15 }}>Office hours ½ off · Affinity 2026</span>
                </motion.div>
              ))}
            </div>
            <div className="panel" style={{ flex: '1 1 280px' }}>
              {fam.axes.length > 0 && (
                <>
                  <div className="row-between">
                    <h2 className="h-small">Variable axes</h2>
                    {!reduce && (
                      <button type="button" className="btn small" aria-pressed={playing} onClick={play}>{playing ? '■ Stop' : '▶ Play'}</button>
                    )}
                  </div>
                  {fam.axes.map((a) => (
                    <label key={a.tag} style={{ display: 'flex', flexDirection: 'column', gap: 6, fontSize: 13 }}>
                      <span className="row-between">
                        <span>{AXIS_NAME[a.tag] ?? a.tag} <span className="faint">({a.tag})</span></span>
                        <span className="muted">{axes[a.tag]}</span>
                      </span>
                      <input
                        type="range" min={a.min} max={a.max} step={axisStep(a)} value={axes[a.tag] ?? a.default}
                        onChange={(e) => (stopPlay.current?.(), setAxes({ ...axes, [a.tag]: Number(e.target.value) }))}
                      />
                    </label>
                  ))}
                </>
              )}
              {fam.features.length > 0 && (
                <>
                  <h2 className="h-small" style={{ marginTop: fam.axes.length ? 4 : 0 }}>OpenType features</h2>
                  <div className="chips">
                    {fam.features.map((t) => (
                      <Chip key={t} className="chip square" pressed={!!feat[t]} onClick={() => setFeat({ ...feat, [t]: !feat[t] })}>
                        {featureName(t)} ({t})
                      </Chip>
                    ))}
                  </div>
                </>
              )}
              {!fam.axes.length && !fam.features.length && <p className="muted" style={{ margin: 0 }}>This font has no variable axes or optional OpenType features.</p>}
              <button type="button" className="btn" onClick={() => navigate(`/playground?a=${encodeURIComponent(fam.key)}`)}>Open in playground</button>
            </div>
          </div>
        )}

        {tab === 'glyphs' && <GlyphsTab fam={fam} extra={{ fontVariationSettings: fvs, fontFeatureSettings: ffs }} />}

        {tab === 'details' && (fam.details ? <DetailsTab d={fam.details} /> : <p className="faint" style={{ margin: 0 }}>No details available for this font.</p>)}

        {tab === 'similar' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <p className="muted" style={{ margin: 0, fontSize: 13 }}>Fonts in your library that look close to {fam.family}, most similar first.</p>
            <div className="similar-grid">
              {similar === null && [0, 1, 2, 3, 4, 5].map((i) => <div key={i} className="select-card similar-card"><Skel w={60} h={40} /><Skel w="70%" h={14} /><Skel w="50%" h={10} /></div>)}
              {similar?.map((f, i) => <SimilarCard key={f.key} f={f} i={i} />)}
            </div>
            {similar?.length === 0 && <p className="faint" style={{ margin: 0 }}>No similar fonts found.</p>}
          </div>
        )}

        {tab === 'files' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div className="table-wrap">
              <table style={{ minWidth: 720 }}>
                <thead><tr><th>File</th><th>Version</th><th>Location</th><th>Status</th></tr></thead>
                <tbody>
                  {fam.files.map((r) => (
                    <tr key={r.path}>
                      <td>{basename(r.path)}</td><td>{r.version}</td><td className="muted">{dirname(r.path)}</td>
                      <td><Badge tone={FILE_STATUS[r.status][1]}>{FILE_STATUS[r.status][0]}</Badge></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p className="muted" style={{ margin: 0, fontSize: 13, lineHeight: 1.5 }}>
              Only one version of a font can be active at a time. Older versions were renamed by auto-organize, so they keep their internal font name but are easy to tell apart here.
            </p>
          </div>
        )}

        {tab === 'license' && (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 24 }}>
            <fieldset className="fieldset">
              <legend>License label</legend>
              {LICENSE_OPTIONS.map(([v, label]) => (
                <label key={v} style={{ display: 'flex', alignItems: 'center', gap: 10, minHeight: 36 }}>
                  <input type="radio" name="lic" checked={fam.licenseInfo.label === v} onChange={() => setLicense(v)} style={{ width: 16, height: 16 }} />
                  <span>{label}</span>
                </label>
              ))}
            </fieldset>
            <div style={{ flex: '2 1 360px', display: 'flex', flexDirection: 'column', gap: 14 }}>
              <div className="card row-between" style={{ padding: 14 }}>
                {fam.licenseInfo.file ? (
                  <>
                    <span style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
                      <span className="ellipsis" style={{ fontWeight: 500 }}>{basename(fam.licenseInfo.file)}</span>
                      <span className="faint ellipsis" style={{ fontSize: 12 }}>{dirname(fam.licenseInfo.file)}</span>
                    </span>
                    <span style={{ display: 'flex', gap: 8 }}>
                      <button type="button" className="btn small" onClick={() => api.showInFolder(fam.licenseInfo.file!).catch((e) => toast(errMsg(e)))}>Show in folder</button>
                      <button type="button" className="btn small" onClick={attach}>Replace</button>
                    </span>
                  </>
                ) : (
                  <>
                    <span className="muted">No license file attached</span>
                    <button type="button" className="btn small" onClick={attach}>Attach file…</button>
                  </>
                )}
              </div>
              <label htmlFor="licnote" style={{ fontWeight: 500 }}>Notes</label>
              <textarea
                id="licnote" rows={4} className="textarea" value={note} onChange={(e) => setNote(e.target.value)}
                onBlur={() => note !== fam.licenseInfo.note && setLicense(fam.licenseInfo.label, undefined, note)}
              />
              <p className="muted" style={{ margin: 0, fontSize: 13 }}>The label and file move with the font when auto-organize tidies folders.</p>
            </div>
          </div>
        )}
      </motion.div>
      </AnimatePresence>

      <Dialog open={setsOpen} onClose={() => setSetsOpen(false)} title={`Add ${fam.family} to a project set`}>
        {sets.length === 0 && <p className="muted">No project sets yet. <Link to="/sets">Create one</Link>.</p>}
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          {sets.map((s) => {
            const has = s.familyKeys.includes(fam.key)
            return (
              <div key={s.id} className="row-between list-row">
                <span>{s.name} <span className="faint" style={{ fontSize: 12 }}>· {plural(s.familyKeys.length, 'font')}</span></span>
                <button type="button" className="btn small" disabled={has} onClick={() => addToSet(s)}>{has ? 'Added' : 'Add'}</button>
              </div>
            )
          })}
        </div>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => setSetsOpen(false)}>Done</button>
        </div>
      </Dialog>
    </div>
  )
}
