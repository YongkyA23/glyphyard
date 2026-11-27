// Fonts turned on with "Turn on for…": the words for when they turn off, and the small clock mark on their row.
import type { Family } from './api'
import { t } from './i18n'
import { fmtWhen } from './ui'

/** When the family's temporary fonts turn off; null when none of its fonts is on temporarily. */
// ponytail: the first temporary face speaks for the family; styles turned on at different times show one of them.
export function turnsOff(f: Pick<Family, 'faces'>): string | null {
  const until = f.faces.find((x) => x.active && x.temporaryUntil)?.temporaryUntil
  if (!until) return null
  return until === 'restart' ? t('Turns off when Glyphyard restarts') : t('Turns off {when}', { when: fmtWhen(until) })
}

/** The clock mark for a row (a plain function, so this file exports no component). */
export function clockMark(f: Family) {
  const text = turnsOff(f)
  if (!text) return null
  return (
    <svg role="img" aria-label={text} className="pin-mark" width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
      <title>{text}</title>
      <circle cx="8" cy="8" r="6.2" /><path d="M8 4.6V8l2.4 1.6" />
    </svg>
  )
}
