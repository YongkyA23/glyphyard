import { useEffect, useState } from 'react'
import { api, type Family } from '../api'
import { familyStyle } from '../fonts'
import { t } from '../i18n'

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
    <div role="group" aria-label={t('Suggested body fonts for {name}', { name: name ?? t('font A') })} style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8, padding: '12px 24px', borderBottom: '1px solid var(--line)' }}>
      <span className="faint" style={{ fontSize: 13 }}>{t('Pairs with {name}:', { name: name ?? t('font A') })}</span>
      {items.map((f) => (
        <button
          key={f.key} type="button" className="btn small" aria-pressed={f.key === b} aria-label={t('Use {family} as font B', { family: f.family })}
          style={{ ...familyStyle(f), fontSize: 15 }} onClick={() => onPick(f.key)}
        >
          {f.family}
        </button>
      ))}
    </div>
  )
}
