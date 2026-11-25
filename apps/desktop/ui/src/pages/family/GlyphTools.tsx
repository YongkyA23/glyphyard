// Glyphs tab: a character's anatomy lines, while Alt is held over it or with the "Show anatomy" toggle.
import { useLayoutEffect, useRef, useState, type CSSProperties } from 'react'
import { createPortal } from 'react-dom'
import type { FaceMetrics } from '../../api'
import { uPlus } from './hooks'
import './glyph-tools.css'

const GAP = 8
const em = (v: number) => `${v.toFixed(2)} em`

/**
 * A card next to the glyph cell `[data-cp]` (bottom-right of the window when that cell isn't rendered) showing the
 * character large with labelled baseline, x-height, cap height, ascender and descender from the face's stored
 * metrics. Lines the metrics leave at 0 (unknown) are not drawn. `announce`: shown on request, so assistive tech
 * reads it; the Alt peek is a transient visual and stays hidden from it.
 */
export function GlyphAnatomy({ cp, style, metrics, announce }: { cp: number; style: CSSProperties; metrics?: FaceMetrics; announce?: boolean }) {
  const card = useRef<HTMLDivElement>(null)
  const glyph = useRef<HTMLDivElement>(null)
  const probe = useRef<HTMLSpanElement>(null)
  const [layout, setLayout] = useState<{ left: number; top: number; base: number; size: number } | null>(null)

  useLayoutEffect(() => {
    if (!card.current || !glyph.current || !probe.current) return
    const c = card.current.getBoundingClientRect()
    // A zero-height inline-block sits on the baseline, so its offset is the baseline's y in the stage.
    const base = probe.current.offsetTop
    const size = parseFloat(getComputedStyle(glyph.current).fontSize)
    const cell = document.querySelector(`.glyph[data-cp="${cp}"]`)
    if (!cell) return setLayout({ left: innerWidth - GAP * 3 - c.width, top: innerHeight - GAP * 3 - c.height, base, size })
    const r = cell.getBoundingClientRect()
    let top = r.bottom + GAP
    if (top + c.height > innerHeight - GAP) top = Math.max(GAP, r.top - GAP - c.height)
    const left = Math.max(GAP, Math.min(r.left + r.width / 2 - c.width / 2, innerWidth - GAP - c.width))
    setLayout({ left, top, base, size })
  }, [cp])

  const m = metrics
  const lines: [string, number][] = [['Baseline', 0]]
  for (const [label, v] of [['x-height', m?.x_height], ['Cap height', m?.cap_height], ['Ascender', m?.ascender], ['Descender', m?.descender]] as const) {
    if (v) lines.push([`${label} ${em(v)}`, v])
  }
  // Lines at the same height share one label.
  const merged = new Map<number, string[]>()
  for (const [label, v] of lines) merged.set(v, [...(merged.get(v) ?? []), label])
  const ch = String.fromCodePoint(cp)

  return createPortal(
    <div
      ref={card} className="anatomy" {...(announce ? { role: 'group', 'aria-label': `Anatomy of ${ch} (${uPlus(cp)})` } : { 'aria-hidden': true })}
      style={layout ? { left: layout.left, top: layout.top } : { left: 0, top: 0, visibility: 'hidden' }}
    >
      <div className="anatomy-stage">
        <div ref={glyph} className="anatomy-glyph" style={style} aria-hidden="true">
          {ch}<span ref={probe} style={{ display: 'inline-block', width: 0, height: 0, verticalAlign: 'baseline' }} />
        </div>
        {layout && [...merged].sort((a, b) => b[0] - a[0]).map(([v, labels]) => (
          <div key={v} className={v ? 'anatomy-line' : 'anatomy-line base'} style={{ top: layout.base - v * layout.size }}>
            <span>{labels.join(' · ')}</span>
          </div>
        ))}
      </div>
      <span className="anatomy-foot">{uPlus(cp)}{lines.length === 1 && ' · this font doesn’t store its vertical metrics'}</span>
    </div>,
    document.body,
  )
}
