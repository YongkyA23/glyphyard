import { useEffect, useRef, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { animate, AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { api, type Axis, type FamilyDetail, type FileRow, type License, type ProjectSet } from '../api'
import { familyStyle } from '../fonts'
import { cssStagger, dur, easeOut, rise, spring } from '../motion'
import { useApp } from '../state'
import { Badge, basename, Chip, Dialog, dirname, errMsg, LicenseBadge, plural, Skel, type Tone } from '../ui'

const AXIS_NAME: Record<string, string> = {
  wght: 'Weight', wdth: 'Width', opsz: 'Optical size', ital: 'Italic', slnt: 'Slant', SOFT: 'Softness', WONK: 'Wonky', GRAD: 'Grade', CASL: 'Casual', MONO: 'Monospace',
}
const FEATURE_NAME: Record<string, string> = {
  liga: 'Ligatures', dlig: 'Discretionary ligatures', calt: 'Contextual alternates', kern: 'Kerning', onum: 'Old-style figures', lnum: 'Lining figures',
  tnum: 'Tabular figures', pnum: 'Proportional figures', frac: 'Fractions', smcp: 'Small caps', c2sc: 'Caps to small caps', zero: 'Slashed zero',
  case: 'Case-sensitive forms', salt: 'Stylistic alternates', swsh: 'Swashes', sups: 'Superscript', subs: 'Subscript', ordn: 'Ordinals',
}
const featureName = (t: string) => FEATURE_NAME[t] ?? (/^ss\d\d$/.test(t) ? `Stylistic set ${Number(t.slice(2))}` : /^cv\d\d$/.test(t) ? `Character variant ${Number(t.slice(2))}` : t)
const ON_BY_DEFAULT = new Set(['liga', 'calt', 'kern'])

const FILE_STATUS: Record<FileRow['status'], [string, Tone]> = {
  active: ['Active', 'ok'], older_version: ['Older version', 'warn'], alternate: ['Alternate', 'accent'], grouped: ['Grouped with variable', 'neutral'], inactive: ['Inactive', 'neutral'],
}
const LICENSE_OPTIONS: [License, string][] = [['licensed', 'Licensed'], ['personal', 'Personal use only'], ['client', 'Client-provided'], ['unknown', 'Unknown']]

const range = (a: number, b: number) => Array.from({ length: b - a + 1 }, (_, i) => String.fromCodePoint(a + i))
const BLOCKS: Record<string, string[]> = {
  'Basic Latin': range(0x21, 0x7e),
  'Latin-1 Supplement': range(0xa1, 0xff).filter((c) => c !== '­'),
}
const WATERFALL = [72, 48, 32, 24, 16]
const axisStep = (a: Axis) => (a.max - a.min > 2 ? 1 : a.max - a.min > 1 ? 0.1 : 1)
/** Seconds the Play showcase spends sweeping one axis (max → min → default). */
const SWEEP = 2.4
const TABS = [['preview', 'Preview'], ['glyphs', 'Glyphs'], ['files', 'Files and versions'], ['license', 'License']] as const
type Tab = (typeof TABS)[number][0]

export function FamilyPage() {
  const { key = '' } = useParams()
  const { libVersion, activate, toast } = useApp()
  const navigate = useNavigate()
  const [fam, setFam] = useState<FamilyDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [tab, setTab] = useState<Tab>('preview')
  const [axes, setAxes] = useState<Record<string, number>>({})
  const [feat, setFeat] = useState<Record<string, boolean>>({})
  const [block, setBlock] = useState('Basic Latin')
  const [note, setNote] = useState('')
  const [setsOpen, setSetsOpen] = useState(false)
  const [sets, setSets] = useState<ProjectSet[]>([])
  const [playing, setPlaying] = useState(false)
  const stopPlay = useRef<(() => void) | null>(null)
  const reduce = useReducedMotion()
  useEffect(() => () => stopPlay.current?.(), [key, tab])

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
    await activate((confirm) => api.setActive(fam.faces.map((x) => x.hash), !allOn, confirm))
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
            {fam.adminSet && <Badge tone="accent">Set by admin</Badge>}
          </div>
        </div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
          <button type="button" className={allOn ? 'btn primary' : 'btn'} aria-pressed={allOn} onClick={toggleFamily} style={{ fontWeight: 500 }}>
            {allOn ? 'Active · turn off' : 'Activate family'}
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

        {tab === 'glyphs' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div className="muted" style={{ display: 'flex', flexWrap: 'wrap', gap: 12, alignItems: 'center', fontSize: 13 }}>
              <label htmlFor="block">Unicode block</label>
              <select id="block" className="select" value={block} onChange={(e) => setBlock(e.target.value)}>
                {Object.keys(BLOCKS).map((b) => <option key={b}>{b}</option>)}
              </select>
              <span>{fam.glyphCount.toLocaleString('en-US')} glyphs in this font</span>
            </div>
            <div className="glyph-grid">
              {BLOCKS[block].map((g, i) => (
                <div key={g} className="glyph" style={{ ...previewStyle, ...cssStagger(i) }} title={`U+${g.codePointAt(0)!.toString(16).toUpperCase().padStart(4, '0')}`}>{g}</div>
              ))}
            </div>
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
