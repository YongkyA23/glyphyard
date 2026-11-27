// The sidebar mark; five quick clicks open the font roulette: three reels spin and stop on one random family.
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { useReducedMotion } from 'motion/react'
import type { Family } from '../api'
import { familyStyle } from '../fonts'
import { t } from '../i18n'
import { Dialog, errMsg } from '../ui'
import { playful, randomFamilies, useTaps } from './effects'

const REELS = 3
/** Each reel stops this long after the previous one (ms). */
const STOP = [900, 1300, 1700]

function Roulette({ onClose }: { onClose: () => void }) {
  const navigate = useNavigate()
  const [pool, setPool] = useState<Family[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [round, setRound] = useState(0)
  const [tick, setTick] = useState(0)
  const [spun, setSpun] = useState(0)
  const reduce = useReducedMotion()

  useEffect(() => {
    let live = true
    randomFamilies(12).then((p) => live && setPool(p), (e) => live && setError(errMsg(e)))
    return () => void (live = false)
  }, [round])
  useEffect(() => {
    if (!pool?.length || reduce) return
    const spin = setInterval(() => setTick((t) => t + 1), 70)
    const stops = STOP.map((ms, i) => setTimeout(() => setSpun(i + 1), ms))
    const end = setTimeout(() => clearInterval(spin), STOP[REELS - 1])
    return () => (clearInterval(spin), clearTimeout(end), stops.forEach(clearTimeout))
  }, [pool, reduce])

  const win = pool?.[0]
  // Reduced motion: the reels show the result without spinning.
  const stopped = reduce ? REELS : spun
  const done = !!win && stopped >= REELS
  return (
    <>
      {error ? <p role="alert">{error}</p> : pool && !win ? <p>{t('Your library is empty. Add some fonts and spin again.')}</p> : (
        <div className="egg-reels" aria-hidden="true">
          {Array.from({ length: REELS }, (_, i) => {
            const f = !pool ? null : i < stopped ? win : pool[(tick + i * 4) % pool.length]
            return <span key={i} className={i < stopped ? 'egg-reel stopped' : 'egg-reel'} style={f ? familyStyle(f) : {}}>Ag</span>
          })}
        </div>
      )}
      <p className="egg-result" aria-live="polite" style={done ? familyStyle(win) : { visibility: 'hidden' }}>{done ? win.family : t('Spinning…')}</p>
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={() => (setPool(null), setError(null), setSpun(0), setRound((r) => r + 1))}>{t('Spin again')}</button>
        <button type="button" className="btn primary" disabled={!done} onClick={() => (onClose(), navigate(`/family/${encodeURIComponent(win!.key)}`))}>{t('Open')}</button>
      </div>
    </>
  )
}

/** `brand` and `logo` (an image data URL) are set by the admin on the server; the logo replaces the whole mark. */
export function Logo({ brand, logo }: { brand?: string; logo?: string }) {
  const [open, setOpen] = useState(false)
  const tap = useTaps(5, () => void playful().then((on) => on && setOpen(true)))
  const custom = /^data:image\//.test(logo ?? '')
  return (
    <>
      <button type="button" className="egg-logo" aria-label={brand ? `${brand} · Glyphyard` : 'Glyphyard'} onClick={tap}>
        {custom ? <img className="brand-logo" src={logo} alt="" /> : brand ? <>
          <span aria-hidden="true" className="gy-mark" />
          <span aria-hidden="true" className="brand-name">{brand}</span>
        </> : <span aria-hidden="true" className="gy-lockup" />}
      </button>
      <Dialog open={open} onClose={() => setOpen(false)} title={t('Font roulette')}>
        <Roulette onClose={() => setOpen(false)} />
      </Dialog>
    </>
  )
}
