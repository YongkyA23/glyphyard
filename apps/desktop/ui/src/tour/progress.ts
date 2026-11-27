// Which guides have been finished (Done on the last step). Kept in localStorage; a missing or broken value = none.
import { useSyncExternalStore } from 'react'
import type { GuideId } from './guides'

const KEY = 'typeshelf.guides.done'
const subs = new Set<() => void>()

function read(): Set<GuideId> {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(KEY) ?? '[]')
    return new Set(Array.isArray(v) ? v.filter((x): x is GuideId => typeof x === 'string') : [])
  } catch {
    return new Set()
  }
}

// One Set per change, so useSyncExternalStore sees a new snapshot only when something changed.
let done = read()

export const isDone = (id: GuideId) => done.has(id)

export function markDone(id: GuideId) {
  if (done.has(id)) return
  done = new Set(done).add(id)
  try {
    localStorage.setItem(KEY, JSON.stringify([...done]))
  } catch {
    // Blocked storage: remembered until reload.
  }
  subs.forEach((cb) => cb())
}

const subscribe = (cb: () => void) => (subs.add(cb), () => void subs.delete(cb))
const snapshot = () => done
export const useDone = (): Set<GuideId> => useSyncExternalStore(subscribe, snapshot, snapshot)
