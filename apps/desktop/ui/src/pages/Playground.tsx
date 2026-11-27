import { useEffect, useState, type CSSProperties } from 'react'
import { useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, type Axis, type Face, type Family } from '../api'
import { AXIS_NAME, axisStep, familyStyle } from '../fonts'
import { FamilyPicker } from '../ui'
import { Hint } from '../Hint'
import { t, tk } from '../i18n'
import { dur, easeOut } from '../motion'
import { useApp } from '../state'
import { PlaygroundCompare } from './PlaygroundCompare'
import { PairingSuggestions } from './PairingSuggestions'
import { AxisPresets } from './AxisPresets'

/** A family's variable axes and the values the sliders set. */
export type AxisState = { key: string; axes: Axis[]; v: Record<string, number> }
const fvs = (s: AxisState | null) => (s?.axes.length ? s.axes.map((a) => `'${a.tag}' ${s.v[a.tag] ?? a.default}`).join(', ') : null)
/** Light-theme ink, ink-2 and paper from tokens.css. */
const DEFAULT_COLORS = { heading: '#1c1917', body: '#44403c', background: '#fffdf9' }
const COLOR_LABEL = { heading: tk('Headings'), body: tk('Body text'), background: tk('Background') }

/** Loads `key`'s axes; `weight` starts the wght axis there (headings use 600). */
function useAxes(key: string, weight: number) {
  const [s, setS] = useState<AxisState | null>(null)
  useEffect(() => {
    if (!key) return // the key check below already hides a stale family
    let live = true
    api.getFamily(key).then((f) => live && setS({
      key, axes: f.axes,
      v: Object.fromEntries(f.axes.map((a) => [a.tag, a.tag === 'wght' ? Math.min(a.max, Math.max(a.min, weight)) : a.default])),
    }), () => {})
    return () => void (live = false)
  }, [key, weight])
  return [s?.key === key ? s : null, setS] as const
}

function AxisSliders({ label, s, set }: { label: string; s: AxisState | null; set: (s: AxisState) => void }) {
  if (!s?.axes.length) return null
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
      <h3 className="h-small" style={{ margin: '8px 0 0' }}>{label}</h3>
      {s.axes.map((a) => (
        <label key={a.tag} style={{ display: 'flex', flexDirection: 'column', gap: 4, fontSize: 13 }}>
          <span className="row-between">
            <span>{AXIS_NAME[a.tag] ? t(AXIS_NAME[a.tag] /* i18n-dynamic */) : a.tag} <span className="faint">({a.tag})</span></span>
            <span className="muted">{s.v[a.tag]}</span>
          </span>
          <input
            type="range" min={a.min} max={a.max} step={axisStep(a)} value={s.v[a.tag] ?? a.default}
            onChange={(e) => set({ ...s, v: { ...s.v, [a.tag]: Number(e.target.value) } })}
          />
        </label>
      ))}
    </div>
  )
}

const fileUrl = (p: string) => encodeURI('file:///' + p.replace(/\\/g, '/').replace(/^\/+/, ''))
const generic = (name: string) => (/mono/i.test(name) ? 'monospace' : /serif/i.test(name) && !/sans/i.test(name) ? 'serif' : 'sans-serif')

function fontFace(face: Face) {
  const weight = face.isVariable ? '100 900' : String(face.weight)
  return `@font-face {\n  font-family: "${face.family}";\n  src: local("${face.fullName}"), local("${face.psName}"), url("${fileUrl(face.path)}");\n  font-weight: ${weight};\n  font-style: ${face.italic ? 'italic' : 'normal'};\n  font-display: swap;\n}`
}

export function Playground() {
  const { libVersion, toast } = useApp()
  const [params] = useSearchParams()
  const [families, setFamilies] = useState<Family[]>([])
  const [a, setA] = useState(params.get('a') ?? '')
  const [b, setB] = useState(params.get('b') ?? '')
  const [base, setBase] = useState(17)
  const [ratio, setRatio] = useState(1.3)
  const [colors, setColors] = useState(DEFAULT_COLORS)
  const [compare, setCompare] = useState(false)
  const [axA, setAxA] = useAxes(a, 600)
  const [axB, setAxB] = useAxes(b, 400)

  // Start with two fonts that are already on (or any two).
  useEffect(() => {
    if (a && b) return
    ;(async () => {
      let pool = (await api.familyPage({ filter: 'active', sort: 'name' }, 0, 2)).items
      if (pool.length < 2) pool = (await api.familyPage({ sort: 'name' }, 0, 2)).items
      const first = a || pool[0]?.key || ''
      setA(first)
      setB((cur) => cur || pool.find((f) => f.key !== first)?.key || '')
    })().catch(() => {})
  }, []) // eslint-disable-line react-hooks/exhaustive-deps
  // Only the two chosen families are loaded.
  useEffect(() => {
    const keys = [a, b].filter(Boolean)
    if (keys.length) api.listFamilies({ keys, includeHidden: true }).then(setFamilies, () => {})
  }, [a, b, libVersion])

  const fa = families.find((f) => f.key === a)
  const fb = families.find((f) => f.key === b)
  const px = (n: number) => Math.round(base * Math.pow(ratio, n))
  const sizes = { h1: px(4), h2: px(2), h3: px(1), lead: px(0.5), small: Math.round(base * 0.85) }
  const fvsA = fvs(axA), fvsB = fvs(axB)
  const quote: CSSProperties = { ...familyStyle(fa), ...(fvsA && { fontVariationSettings: fvsA }), color: colors.heading }
  const head: CSSProperties = { ...familyStyle(fa, 600), ...(fvsA && { fontVariationSettings: fvsA }), color: colors.heading }
  const body: CSSProperties = { ...familyStyle(fb), ...(fvsB && { fontVariationSettings: fvsB }), color: colors.body }
  const elements = [
    ['H1', fa, sizes.h1], ['H2', fa, sizes.h2], [t('Lead'), fb, sizes.lead], [t('Body'), fb, base], [t('Quote'), fa, sizes.h3],
  ] as const

  async function copyCss() {
    if (!fa || !fb) return
    const faces = [...new Map([...fa.faces, ...fb.faces].map((f) => [f.hash, f])).values()]
    const A = `"${fa.family}", ${generic(fa.family)}`, B = `"${fb.family}", ${generic(fb.family)}`
    const varA = fvsA ? `\n  font-variation-settings: ${fvsA};` : '', varB = fvsB ? `\n  font-variation-settings: ${fvsB};` : ''
    const css = [
      `/* ${fa.family} + ${fb.family} · base ${base}px · scale ${ratio.toFixed(2)} */`,
      ...faces.map(fontFace),
      `body {\n  font-family: ${B};${varB}\n  font-size: ${base}px;\n  line-height: 1.65;\n  color: ${colors.body};\n  background: ${colors.background};\n}`,
      `h1 {\n  font-family: ${A};${varA}\n  font-weight: 600;\n  font-size: ${sizes.h1}px;\n  line-height: 1.05;\n  color: ${colors.heading};\n}`,
      `h2 {\n  font-family: ${A};${varA}\n  font-weight: 600;\n  font-size: ${sizes.h2}px;\n  line-height: 1.15;\n  color: ${colors.heading};\n}`,
      `.lead {\n  font-family: ${B};${varB}\n  font-size: ${sizes.lead}px;\n  line-height: 1.5;\n}`,
      `blockquote {\n  font-family: ${A};${varA}\n  font-size: ${sizes.h3}px;\n  line-height: 1.3;\n  color: ${colors.heading};\n  border-left: 3px solid ${colors.heading};\n}`,
      `small {\n  font-family: ${B};${varB}\n  font-size: ${sizes.small}px;\n}`,
    ].join('\n\n')
    try {
      await navigator.clipboard.writeText(css)
      toast(t('CSS copied to the clipboard'))
    } catch {
      toast(t('Couldn’t copy to the clipboard.'))
    }
  }

  const select = (id: string, label: string, value: string, set: (v: string) => void) => (
    <FamilyPicker id={id} label={label} value={families.find((f) => f.key === value)} onChange={set} />
  )

  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: '100%' }}>
      <div className="toolbar" style={{ alignItems: 'flex-end', gap: 16 }}>
        {select('font-a', t('Headings (font A)'), a, setA)}
        <button type="button" className="btn" aria-label={t('Swap fonts A and B')} onClick={() => (setA(b), setB(a))} style={{ height: 38 }}>⇄ {t('Swap')}</button>
        {select('font-b', t('Body (font B)'), b, setB)}
        <label className="field-label">
          {t('Base size · {n}px', { n: base })}
          <input type="range" min={14} max={22} value={base} onChange={(e) => setBase(Number(e.target.value))} style={{ width: 120 }} />
        </label>
        <label className="field-label">
          {t('Scale · {n}', { n: ratio.toFixed(2) })}
          <input type="range" min={1.1} max={1.6} step={0.05} value={ratio} onChange={(e) => setRatio(Number(e.target.value))} style={{ width: 120 }} />
        </label>
        <button type="button" className="btn" aria-pressed={compare} onClick={() => setCompare(!compare)} style={{ marginLeft: 'auto', height: 38 }}>{t('Compare')}</button>
        <button type="button" className="btn primary" style={{ height: 38 }} onClick={copyCss} disabled={!fa || !fb}>{t('Copy CSS')}</button>
      </div>
      <PairingSuggestions a={a} b={b} name={fa?.family} onPick={setB} />
      <div style={{ flex: 1, display: 'flex', flexWrap: 'wrap' }}>
        <article className="paper" style={{ background: colors.background, transition: `background-color ${dur.base}s` }}>
          {/* Switching fonts cross-fades the specimen; size changes ease via .spec transitions. */}
          {compare ? <PlaygroundCompare fa={fa} fb={fb} styleA={quote} styleB={body} size={sizes.h1} color={colors.heading} bodyColor={colors.body} /> : <AnimatePresence mode="popLayout" initial={false}>
          <motion.div
            key={`${a}|${b}`} style={{ maxWidth: 680, display: 'flex', flexDirection: 'column', gap: 18 }}
            initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: dur.slow, ease: easeOut }}
          >
            <span className="spec" style={{ ...body, opacity: 0.7, fontSize: base, letterSpacing: 1.5, textTransform: 'uppercase' }}>Field notes · Issue 12</span>
            <h1 className="spec" style={{ ...head, margin: 0, fontSize: sizes.h1, lineHeight: 1.05 }}>Quiet type makes room for loud ideas</h1>
            <p className="spec" style={{ ...body, margin: 0, fontSize: sizes.lead, lineHeight: 1.5 }}>
              A good pairing lets the headline carry the voice while the text simply gets read.
            </p>
            <h2 className="spec" style={{ ...head, margin: '12px 0 0', fontSize: sizes.h2, lineHeight: 1.15 }}>Start with contrast, not novelty</h2>
            <p className="spec" style={{ ...body, margin: 0, fontSize: base, lineHeight: 1.65 }}>
              Pick a display face with character, then a text face that stays out of its way. Check them side by side at the sizes you will actually use, in the colors you will actually print.
            </p>
            <blockquote className="spec" style={{ ...quote, margin: '8px 0', paddingLeft: 20, borderLeft: `3px solid ${colors.heading}`, fontSize: sizes.h3, lineHeight: 1.3 }}>
              “Typography is what language looks like.”
            </blockquote>
            <p className="spec" style={{ ...body, opacity: 0.7, margin: 0, fontSize: sizes.small }}>Ellen Lupton</p>
          </motion.div>
          </AnimatePresence>}
        </article>
        <aside aria-label={t('Element settings')} className="aside">
          <h2 className="h-small">{t('Elements')}</h2>
          {elements.map(([tag, f, size]) => (
            <div key={tag} className="element-row">
              <span style={{ fontWeight: 500 }}>{tag}</span>
              <span className="muted">{f?.family ?? '—'} · {size}px</span>
            </div>
          ))}
          <h2 className="h-small" style={{ marginTop: 8 }}>{t('Colors')}</h2>
          {(Object.keys(COLOR_LABEL) as (keyof typeof COLOR_LABEL)[]).map((k) => (
            <label key={k} className="color-field">
              {t(COLOR_LABEL[k] /* i18n-dynamic */)}
              <input type="color" value={colors[k]} onChange={(e) => setColors({ ...colors, [k]: e.target.value })} />
            </label>
          ))}
          {colors !== DEFAULT_COLORS && (
            <button type="button" className="btn small link" style={{ alignSelf: 'flex-start', paddingLeft: 0 }} onClick={() => setColors(DEFAULT_COLORS)}>{t('Reset colors')}</button>
          )}
          <AxisSliders label={t('{name} axes', { name: fa?.family ?? t('Font A') })} s={axA} set={setAxA} />
          <AxisPresets s={axA} set={setAxA} />
          <AxisSliders label={t('{name} axes', { name: fb?.family ?? t('Font B') })} s={axB} set={setAxB} />
          <AxisPresets s={axB} set={setAxB} />
          <span className="faint" style={{ marginTop: 4, fontSize: 12 }}>
            {t('About sizes and CSS')} <Hint text={t('Sizes follow the base size and scale. Copy CSS gives you the @font-face rules and these element styles, with colors and variable axis settings.')} />
          </span>
        </aside>
      </div>
    </div>
  )
}
