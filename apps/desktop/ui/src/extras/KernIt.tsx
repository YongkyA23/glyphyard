// "Kern it": five letters in a library font start badly spaced; drag them (or use the arrow keys) and compare
// your gaps with the ones the browser lays out for the same word in that font.
import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react'
import type { Family } from '../api'
import { familyStyle } from '../fonts'
import { t } from '../i18n'
import { errMsg } from '../ui'
import { pick, randomFamilies } from './effects'

const WORDS = ['WAVES', 'TOAST', 'VOTED', 'YAWNS', 'LATTE', 'AVOWS', 'TAWNY']
const SIZE = 72
/** Room to the left of the word to drag into. */
const PAD = SIZE / 2

/** 100 when every gap matches the font's own; 0 once the gaps are half an em off in total. */
const scoreOf = (xs: number[], natural: number[]) => {
  const off = xs.slice(1).reduce((sum, x, i) => sum + Math.abs(x - xs[i] - (natural[i + 1] - natural[i])), 0) / SIZE
  return Math.round(100 * Math.max(0, 1 - off / 0.5))
}

export function KernIt() {
  const [game, setGame] = useState(0)
  const [fam, setFam] = useState<Family | null>(null)
  const [word, setWord] = useState('')
  const [natural, setNatural] = useState<number[] | null>(null)
  const [xs, setXs] = useState<number[]>([])
  const [score, setScore] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const ref = useRef<HTMLSpanElement>(null)
  const drag = useRef<{ i: number; x0: number; from: number } | null>(null)
  const style = fam ? familyStyle(fam) : {}

  useEffect(() => {
    let live = true
    randomFamilies(1).then(
      ([f]) => live && (f ? (setFam(f), setWord(pick(WORDS))) : setError(t('Add some fonts to your library to play.'))),
      (e) => live && setError(errMsg(e)),
    )
    return () => void (live = false)
  }, [game])

  // Where each letter starts when the browser sets the word normally in this font (kerning included).
  useEffect(() => {
    const el = ref.current
    if (!fam || !el) return
    let live = true
    document.fonts.load(`${SIZE}px ${getComputedStyle(el).fontFamily}`, word).catch(() => {}).then(() => {
      const text = el.firstChild
      if (!live || !(text instanceof Text)) return
      const r = document.createRange()
      const lefts = [...word].map((_, i) => (r.setStart(text, i), r.setEnd(text, i + 1), r.getBoundingClientRect().left))
      // Rects include the dialog's opening scale; offsetWidth doesn't, so divide it back out.
      const scale = el.getBoundingClientRect().width / (el.offsetWidth || 1) || 1
      const nat = lefts.map((l) => (l - lefts[0]) / scale)
      setNatural(nat)
      // Badly spaced to start: every letter after the first pushed 0.12–0.3 em one way or the other.
      setXs(nat.map((x, i) => (i ? x + (i % 2 ? 1 : -1) * (0.12 + Math.random() * 0.18) * SIZE : x)))
    })
    return () => void (live = false)
  }, [fam, word])

  const move = (i: number, x: number) => (setXs((all) => all.map((v, j) => (j === i ? x : v))), setScore(null))
  const down = (i: number) => (e: PointerEvent) => {
    e.currentTarget.setPointerCapture(e.pointerId)
    drag.current = { i, x0: e.clientX, from: xs[i] }
  }
  const moveTo = (e: PointerEvent) => {
    const d = drag.current
    if (d) move(d.i, d.from + e.clientX - d.x0)
  }
  const nudge = (i: number) => (e: KeyboardEvent) => {
    const step = { ArrowLeft: -1, ArrowRight: 1 }[e.key]
    if (!step) return
    e.preventDefault()
    move(i, xs[i] + step * (e.shiftKey ? 10 : 1))
  }

  if (error) return <p role="alert">{error}</p>
  const width = natural ? natural[natural.length - 1] + SIZE * 1.5 : 0
  return (
    <>
      <p>{t('Drag the letters (or focus one and use the arrow keys, Shift for bigger steps) until the spacing looks even, then check it against the font’s own.')}</p>
      <div className="egg-kern" style={{ height: SIZE * 1.3, width: width + PAD }} aria-busy={!natural}>
        {natural && [...word].map((ch, i) => (
          <span
            key={i} role="slider" tabIndex={0} className="egg-kern-letter" style={{ ...style, fontSize: SIZE, left: PAD + xs[i] }}
            aria-label={t('Letter {ch}, {i} of {n}', { ch, i: i + 1, n: word.length })} aria-valuemin={-PAD} aria-valuemax={width} aria-valuenow={Math.round(xs[i])}
            onPointerDown={down(i)} onPointerMove={moveTo} onPointerUp={() => (drag.current = null)} onKeyDown={nudge(i)}
          >{ch}</span>
        ))}
      </div>
      <div className={score === null ? 'egg-kern-ref' : 'egg-kern-ref shown'} aria-hidden={score === null}>
        <span className="faint" style={{ fontSize: 12 }}>{t('{family}’s own spacing', { family: fam?.family ?? '' })}</span>
        <span ref={ref} style={{ ...style, fontSize: SIZE, lineHeight: 1 }}>{word}</span>
      </div>
      <p aria-live="polite" style={{ minHeight: '1.55em' }}>{score !== null && t('Your spacing scores {score} out of 100.', { score })}</p>
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={() => (setFam(null), setNatural(null), setScore(null), setGame((g) => g + 1))}>{t('New word')}</button>
        <button type="button" className="btn primary" disabled={!natural} onClick={() => natural && setScore(scoreOf(xs, natural))}>{t('Check my spacing')}</button>
      </div>
    </>
  )
}
