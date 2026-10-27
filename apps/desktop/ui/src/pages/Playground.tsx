import { useEffect, useState, type CSSProperties } from 'react'
import { useSearchParams } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, type Face, type Family } from '../api'
import { familyStyle } from '../fonts'
import { FamilyPicker } from '../ui'
import { dur, easeOut } from '../motion'
import { useApp } from '../state'

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
    if (keys.length) api.listFamilies({ keys }).then(setFamilies, () => {})
  }, [a, b, libVersion])

  const fa = families.find((f) => f.key === a)
  const fb = families.find((f) => f.key === b)
  const px = (n: number) => Math.round(base * Math.pow(ratio, n))
  const sizes = { h1: px(4), h2: px(2), h3: px(1), lead: px(0.5), small: Math.round(base * 0.85) }
  const head: CSSProperties = familyStyle(fa, 600)
  const body: CSSProperties = familyStyle(fb)
  const elements = [
    ['H1', fa, sizes.h1], ['H2', fa, sizes.h2], ['Lead', fb, sizes.lead], ['Body', fb, base], ['Quote', fa, sizes.h3],
  ] as const

  async function copyCss() {
    if (!fa || !fb) return
    const faces = [...new Map([...fa.faces, ...fb.faces].map((f) => [f.hash, f])).values()]
    const A = `"${fa.family}", ${generic(fa.family)}`, B = `"${fb.family}", ${generic(fb.family)}`
    const css = [
      `/* ${fa.family} + ${fb.family} · base ${base}px · scale ${ratio.toFixed(2)} */`,
      ...faces.map(fontFace),
      `body {\n  font-family: ${B};\n  font-size: ${base}px;\n  line-height: 1.65;\n}`,
      `h1 {\n  font-family: ${A};\n  font-weight: 600;\n  font-size: ${sizes.h1}px;\n  line-height: 1.05;\n}`,
      `h2 {\n  font-family: ${A};\n  font-weight: 600;\n  font-size: ${sizes.h2}px;\n  line-height: 1.15;\n}`,
      `.lead {\n  font-family: ${B};\n  font-size: ${sizes.lead}px;\n  line-height: 1.5;\n}`,
      `blockquote {\n  font-family: ${A};\n  font-size: ${sizes.h3}px;\n  line-height: 1.3;\n}`,
      `small {\n  font-family: ${B};\n  font-size: ${sizes.small}px;\n}`,
    ].join('\n\n')
    try {
      await navigator.clipboard.writeText(css)
      toast('CSS copied to the clipboard')
    } catch {
      toast('Couldn’t copy to the clipboard.')
    }
  }

  const select = (id: string, label: string, value: string, set: (v: string) => void) => (
    <FamilyPicker id={id} label={label} value={families.find((f) => f.key === value)} onChange={set} />
  )

  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: '100%' }}>
      <div className="toolbar" style={{ alignItems: 'flex-end', gap: 16 }}>
        {select('font-a', 'Headings (font A)', a, setA)}
        <button type="button" className="btn" aria-label="Swap fonts A and B" onClick={() => (setA(b), setB(a))} style={{ height: 38 }}>⇄ Swap</button>
        {select('font-b', 'Body (font B)', b, setB)}
        <label className="field-label">
          Base size · {base}px
          <input type="range" min={14} max={22} value={base} onChange={(e) => setBase(Number(e.target.value))} style={{ width: 120 }} />
        </label>
        <label className="field-label">
          Scale · {ratio.toFixed(2)}
          <input type="range" min={1.1} max={1.6} step={0.05} value={ratio} onChange={(e) => setRatio(Number(e.target.value))} style={{ width: 120 }} />
        </label>
        <button type="button" className="btn primary" style={{ marginLeft: 'auto', height: 38 }} onClick={copyCss} disabled={!fa || !fb}>Copy CSS</button>
      </div>
      <div style={{ flex: 1, display: 'flex', flexWrap: 'wrap' }}>
        <article className="paper">
          {/* Switching fonts cross-fades the specimen; size changes ease via .spec transitions. */}
          <AnimatePresence mode="popLayout" initial={false}>
          <motion.div
            key={`${a}|${b}`} style={{ maxWidth: 680, display: 'flex', flexDirection: 'column', gap: 18 }}
            initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: dur.slow, ease: easeOut }}
          >
            <span className="faint spec" style={{ ...body, fontSize: base, letterSpacing: 1.5, textTransform: 'uppercase' }}>Field notes · Issue 12</span>
            <h1 className="spec" style={{ ...head, margin: 0, fontSize: sizes.h1, lineHeight: 1.05 }}>Quiet type makes room for loud ideas</h1>
            <p className="spec" style={{ ...body, margin: 0, fontSize: sizes.lead, lineHeight: 1.5, color: 'var(--ink-2)' }}>
              A good pairing lets the headline carry the voice while the text simply gets read.
            </p>
            <h2 className="spec" style={{ ...head, margin: '12px 0 0', fontSize: sizes.h2, lineHeight: 1.15 }}>Start with contrast, not novelty</h2>
            <p className="spec" style={{ ...body, margin: 0, fontSize: base, lineHeight: 1.65, color: 'var(--ink-2)' }}>
              Pick a display face with character, then a text face that stays out of its way. Check them side by side at the sizes you will actually use, in the colors you will actually print.
            </p>
            <blockquote className="spec" style={{ ...familyStyle(fa), margin: '8px 0', paddingLeft: 20, borderLeft: '3px solid var(--ink)', fontSize: sizes.h3, lineHeight: 1.3 }}>
              “Typography is what language looks like.”
            </blockquote>
            <p className="faint spec" style={{ ...body, margin: 0, fontSize: sizes.small }}>Ellen Lupton</p>
          </motion.div>
          </AnimatePresence>
        </article>
        <aside aria-label="Element settings" className="aside">
          <h2 className="h-small">Elements</h2>
          {elements.map(([tag, f, size]) => (
            <div key={tag} className="element-row">
              <span style={{ fontWeight: 500 }}>{tag}</span>
              <span className="muted">{f?.family ?? '—'} · {size}px</span>
            </div>
          ))}
          <p className="faint" style={{ margin: '4px 0 0', fontSize: 12, lineHeight: 1.5 }}>
            Sizes follow the base size and scale. Copy CSS gives you the @font-face rules and these element styles.
          </p>
        </aside>
      </div>
    </div>
  )
}
