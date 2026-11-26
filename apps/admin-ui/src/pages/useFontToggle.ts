import { useCallback, useEffect, useRef, useState } from 'react'
import { api, errorText } from '../api'
import { toast } from '../toast'

/** A font toggle is only a request: the desktop applies it later. `pending` holds the requested state per font until a poll confirms it or PENDING_MS passes. */
export const PENDING_MS = 120_000

export function useFontToggle(installId: string) {
  const [error, setError] = useState('')
  const [sent, setSent] = useState<Record<string, boolean>>({})
  const [shakes, setShakes] = useState<Record<string, number>>({})
  const [pending, setPending] = useState<Record<string, boolean>>({})
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>())
  const drop = useCallback((hash: string) => {
    clearTimeout(timers.current.get(hash))
    timers.current.delete(hash)
    setPending((p) => { if (!(hash in p)) return p; const { [hash]: _, ...rest } = p; return rest })
  }, [])
  useEffect(() => { const t = timers.current; return () => { t.forEach(clearTimeout); t.clear() } }, [])
  /** Marks fonts as waiting for the desktop, e.g. after a batch. */
  const mark = useCallback((hashes: string[], active: boolean) => {
    setPending((p) => ({ ...p, ...Object.fromEntries(hashes.map((h) => [h, active])) }))
    for (const h of hashes) {
      clearTimeout(timers.current.get(h))
      timers.current.set(h, setTimeout(() => drop(h), PENDING_MS))
    }
  }, [drop])
  /** Throws on failure (after clearing the pending state); the toast Undo uses it so a failure shows an error toast. */
  const send = async (hash: string, active: boolean) => {
    mark([hash], active)
    try {
      await api.setFont(installId, hash, active)
      setSent((s) => ({ ...s, [hash]: true }))
    } catch (e) {
      drop(hash)
      throw e
    }
  }
  /** `announce`: toast text for the request, with an Undo that sends the opposite request. */
  const set = async (hash: string, active: boolean, announce?: string) => {
    if (timers.current.has(hash)) return
    setError('')
    try {
      await send(hash, active)
      if (announce) toast(announce, { undo: () => { drop(hash); return send(hash, !active) } })
    } catch (e) {
      setShakes((s) => ({ ...s, [hash]: (s[hash] ?? 0) + 1 }))
      setError(errorText(e))
    }
  }
  return { set, error, sent, shakes, pending, drop, mark }
}
