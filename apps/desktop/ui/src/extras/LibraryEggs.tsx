// Library eggs: "comic sans" / "papyrus" typed in the search, and the fox that runs past for the pangrams.
import { useEffect, useState } from 'react'
import { t } from '../i18n'
import { useApp } from '../state'
import { playful, reducedMotion, swapFont, type EggFont } from './effects'

const SEARCH_FONT: Record<string, EggFont> = { 'comic sans': 'comic', papyrus: 'papyrus' }
const pangram = (s: string) => /^the quick brown fox\b/i.test(s.trim()) || /^muharjo seorang xenofobia\b/i.test(s.trim())

export function LibraryEggs({ search, sample }: { search: string; sample: string }) {
  const { toast } = useApp()
  const [fox, setFox] = useState(false)
  const font = SEARCH_FONT[search.trim().toLowerCase()]
  useEffect(() => void (font && playful().then((on) => on && swapFont(font))), [font])

  // Only while typing in the sample field, so the saved sample (often the English pangram) doesn't set it off on every visit.
  const match = pangram(sample)
  useEffect(() => {
    if (!match || document.activeElement?.id !== 'sample') return
    playful().then((on) => on && (reducedMotion() ? toast(t('A quick brown fox just ran past.')) : setFox(true)))
  }, [match, toast])

  useEffect(() => {
    if (!fox) return
    const onKey = (e: KeyboardEvent) => void (e.key === 'Escape' && setFox(false))
    addEventListener('keydown', onKey)
    return () => removeEventListener('keydown', onKey)
  }, [fox])

  if (!fox) return null
  return (
    <svg className="egg-fox" aria-hidden="true" viewBox="0 0 64 40" width="64" height="40" onAnimationEnd={(e) => e.target === e.currentTarget && setFox(false)}>
      <path className="egg-fox-tail" d="M14 22C6 20 2 14 3 8c4 6 9 8 15 9Z" />
      <path className="egg-fox-tip" d="M3 8c0 3 1 5 3 7l-3-1Z" />
      <path className="egg-fox-body" d="M14 17c6-3 18-4 26-2l6-6 3 2 2-5 3 7c3 1 6 3 6 5l-6 2c-2 3-6 4-10 4-8 1-18 2-24 0Z" />
      <circle className="egg-fox-eye" cx="53" cy="15" r="1.2" />
      <g className="egg-fox-legs">
        <path d="M20 25l-3 9M26 25l3 9" />
        <path d="M38 25l-3 9M44 24l3 9" />
      </g>
    </svg>
  )
}
