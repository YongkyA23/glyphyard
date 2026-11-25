// Glyphs tab: a character's anatomy lines while Alt is held over it.
import { useLayoutEffect, useRef, useState, type CSSProperties } from 'react'
import { createPortal } from 'react-dom'
import { uPlus } from './hooks'
import './glyph-tools.css'

const GAP = 8

/**
 * A card next to the glyph cell `[data-cp]` showing the character large with labelled baseline, x-height and cap height
 * from the face's stored metrics. Lines the metrics leave at 0 are not drawn.
 */
export function GlyphAnatomy({ cp, style, metrics }: { cp: number; style: CSSProperties; metrics?: { x_height: number; cap_height: number } }) {
  const card = useRef<HTMLDivElement>(null)
  const glyph = useRef<HTMLDivElement>(null)
  const probe = useRef<HTMLSpanElement>(null)
  const [layout, setLayout] = useState<{ left: number; top: number; base: number; size: number } | null>(null)

  useLayoutEffect(() => {
    const cell = document.querySelector(`.glyph[data-cp="${cp}"]`)
    if (!cell || !card.current || !glyph.current || !probe.current) return setLayout(null)
    const r = cell.getBoundingClientRect()
    const c = card.current.getBoundingClientRect()
    // A zero-height inline-block sits on the baseline, so its offset is the baseline's y in the stage.
    const base = probe.current.offsetTop
    const size = parseFloat(getComputedStyle(glyph.current).fontSize)
    let top = r.bottom + GAP
    if (top + c.height > innerHeight - GAP) top = Math.max(GAP, r.top - GAP - c.height)
    const left = Math.max(GAP, Math.min(r.left + r.width / 2 - c.width / 2, innerWidth - GAP - c.width))
    setLayout({ left, top, base, size })
  }, [cp])

  const lines: [string, number][] = [['Baseline', 0]]
  if (metrics?.x_height) lines.push(['x-height', metrics.x_height])
  if (metrics?.cap_height) lines.push(['Cap height', metrics.cap_height])
  // Lines at the same height share one label.
  const merged = new Map<number, string[]>()
  for (const [label, em] of lines) merged.set(em, [...(merged.get(em) ?? []), label])

  return createPortal(
    <div ref={card} className="anatomy" aria-hidden="true" style={layout ? { left: layout.left, top: layout.top } : { left: 0, top: 0, visibility: 'hidden' }}>
      <div className="anatomy-stage">
        <div ref={glyph} className="anatomy-glyph" style={style}>
          {String.fromCodePoint(cp)}<span ref={probe} style={{ display: 'inline-block', width: 0, height: 0, verticalAlign: 'baseline' }} />
        </div>
        {layout && [...merged].map(([em, labels]) => (
          <div key={em} className={em ? 'anatomy-line' : 'anatomy-line base'} style={{ top: layout.base - em * layout.size }}>
            <span>{labels.join(' · ')}</span>
          </div>
        ))}
      </div>
      <span className="anatomy-foot">{uPlus(cp)}{!metrics?.x_height && !metrics?.cap_height && ' · this font doesn’t store its x-height or cap height'}</span>
    </div>,
    document.body,
  )
}
