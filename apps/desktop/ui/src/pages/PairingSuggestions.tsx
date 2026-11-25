import { useEffect, useState } from 'react'
import { api, type Family } from '../api'
import { familyStyle } from '../fonts'

/** One-click body fonts (B) that contrast with font A: another category, a close x-height. */
export function PairingSuggestions({ a, b, name, onPick }: { a: string; b: string; name?: string; onPick: (key: string) => void }) {
  const [list, setList] = useState<{ key: string; items: Family[] } | null>(null)
  useEffect(() => {
    if (!a) return
    let live = true
    api.pairingSuggestions(a).then((items) => live && setList({ key: a, items }), () => live && setList({ key: a, items: [] }))
    return () => void (live = false)
  }, [a])
  const items = list?.key === a ? list.items : []
  if (!items.length) return null
  return (
    <div role="group" aria-label={`Suggested body fonts for ${name ?? 'font A'}`} style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8, padding: '0 24px 12px' }}>
      <span className="faint" style={{ fontSize: 13 }}>Pairs with {name ?? 'font A'}:</span>
      {items.map((f) => (
        <button
          key={f.key} type="button" className="btn small" aria-pressed={f.key === b} aria-label={`Use ${f.family} as font B`}
          style={{ ...familyStyle(f), fontSize: 15 }} onClick={() => onPick(f.key)}
        >
          {f.family}
        </button>
      ))}
    </div>
  )
}
