import { useEffect, useState } from 'react'
import { applyTokens, hasTokens } from './sampleTokens'
import { useApp } from './state'

/** The sample text with its :tokens filled in. One timer per page, only while the text has a token, ticking on the minute. */
export function useSampleText(text: string): string {
  const { app } = useApp()
  const [now, setNow] = useState(() => new Date())
  const live = hasTokens(text)
  useEffect(() => {
    if (!live) return
    let t = 0
    const tick = () => {
      setNow(new Date())
      t = window.setTimeout(tick, 60_000 - (Date.now() % 60_000) + 50)
    }
    tick()
    return () => window.clearTimeout(t)
  }, [live])
  return live ? applyTokens(text, now, app.name) : text
}
