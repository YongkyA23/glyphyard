// "Name that font": a word in a random library font, four family names to pick from, ten rounds.
import { useEffect, useRef, useState } from 'react'
import type { Family } from '../api'
import { familyStyle } from '../fonts'
import { t } from '../i18n'
import { errMsg } from '../ui'
import { pick, randomFamilies } from './effects'

const ROUNDS = 10
const WORDS = ['Quiver', 'Jukebox', 'Fjord', 'Zephyr', 'Waltz', 'Glyph', 'Bramble', 'Sphinx', 'Vortex', 'Hazelnut']

export function NameThatFont() {
  const [game, setGame] = useState(0)
  const [round, setRound] = useState(1)
  const [score, setScore] = useState(0)
  const [q, setQ] = useState<{ choices: Family[]; answer: Family } | null>(null)
  const [picked, setPicked] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const next = useRef<HTMLButtonElement>(null)
  useEffect(() => void (picked && next.current?.focus()), [picked])

  useEffect(() => {
    if (round > ROUNDS) return
    let live = true
    randomFamilies(4).then(
      (fs) => live && (fs.length < 4 ? setError(t('You need at least four font families in your library to play.')) : setQ({ choices: fs, answer: pick(fs) })),
      (e) => live && setError(errMsg(e)),
    )
    return () => void (live = false)
  }, [round, game])

  if (error) return <p role="alert">{error}</p>
  if (round > ROUNDS)
    return (
      <>
        <p aria-live="polite">{score === ROUNDS ? t('You named {score} of {rounds} fonts. Perfect!', { score, rounds: ROUNDS }) : t('You named {score} of {rounds} fonts.', { score, rounds: ROUNDS })}</p>
        <div className="dialog-actions">
          <button type="button" className="btn primary" autoFocus onClick={() => (setQ(null), setPicked(null), setScore(0), setRound(1), setGame((g) => g + 1))}>{t('Play again')}</button>
        </div>
      </>
    )
  const nextRound = () => (setQ(null), setPicked(null), setRound((r) => r + 1))
  const choose = (f: Family) => {
    if (picked) return
    setPicked(f.key)
    if (f.key === q?.answer.key) setScore((s) => s + 1)
  }
  return (
    <>
      <span className="muted" style={{ fontSize: 13 }}>{t('Round {round} of {rounds} · {score} right so far', { round, rounds: ROUNDS, score })}</span>
      <p className="egg-word" style={q ? familyStyle(q.answer) : { visibility: 'hidden' }}>{WORDS[(round - 1) % WORDS.length]}</p>
      <div className="egg-choices" role="group" aria-label={t('Which font is this?')}>
        {q?.choices.map((f, i) => {
          const state = !picked ? '' : f.key === q.answer.key ? ' right' : f.key === picked ? ' wrong' : ''
          return (
            <button key={f.key} type="button" className={`btn${state}`} autoFocus={i === 0} aria-disabled={!!picked} onClick={() => choose(f)}>
              {f.family}
            </button>
          )
        })}
      </div>
      <p aria-live="polite" style={{ minHeight: '1.55em' }}>
        {picked && q && (picked === q.answer.key ? t('Right!') : t('Not quite: it’s {family}.', { family: q.answer.family }))}
      </p>
      <div className="dialog-actions">
        <button ref={next} type="button" className="btn primary" disabled={!picked} onClick={nextRound}>{round === ROUNDS ? t('See your score') : t('Next')}</button>
      </div>
    </>
  )
}
