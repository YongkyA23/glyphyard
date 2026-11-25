import { useEffect, useState, type CSSProperties } from 'react'
import { api, type Family } from '../api'

const TEXT = 'Hamburgefonstiv\nQuiet type, loud ideas'

/** Font B laid over font A on the same text; B is scaled so the x-heights match when both fonts give one. */
export function PlaygroundCompare({ fa, fb, styleA, styleB, size, color, bodyColor }: {
  fa?: Family; fb?: Family; styleA: CSSProperties; styleB: CSSProperties; size: number; color: string; bodyColor: string
}) {
  const pair = `${fa?.key ?? ''}|${fb?.key ?? ''}`
  const [xh, setXh] = useState<{ pair: string; a: number; b: number } | null>(null)
  useEffect(() => {
    if (!fa || !fb) return
    let live = true
    Promise.all([api.getFamily(fa.key), api.getFamily(fb.key)])
      .then(([da, db]) => live && setXh({ pair, a: da.xHeight, b: db.xHeight }), () => {})
    return () => void (live = false)
  }, [pair]) // eslint-disable-line react-hooks/exhaustive-deps
  const known = xh?.pair === pair ? xh : null
  const ratio = known && known.a > 0 && known.b > 0 ? known.a / known.b : null
  const sizeB = Math.round(size * (ratio ?? 1) * 10) / 10
  // One line height for both, so the lines of A and B sit on the same rows.
  const line: CSSProperties = { gridArea: '1 / 1', margin: 0, whiteSpace: 'pre', lineHeight: `${Math.round(size * 1.35)}px` }
  return (
    <div style={{ maxWidth: 680, display: 'flex', flexDirection: 'column', gap: 18 }}>
      <div style={{ display: 'grid', overflow: 'hidden' }}>
        <p style={{ ...styleA, ...line, fontSize: size, color }}>{TEXT}</p>
        <p aria-hidden="true" style={{ ...styleB, ...line, fontSize: sizeB, color: 'var(--accent)', opacity: 0.55 }}>{TEXT}</p>
      </div>
      <p style={{ margin: 0, fontSize: 13, lineHeight: 1.5, color: bodyColor, opacity: 0.7 }}>
        {fa?.family ?? 'Font A'} at {size}px, with {fb?.family ?? 'font B'} in blue on top at {sizeB}px
        {ratio ? ', sized so the x-heights match.' : known ? '. One of them has no x-height in its metrics, so they share the font size.' : '.'}
      </p>
    </div>
  )
}
