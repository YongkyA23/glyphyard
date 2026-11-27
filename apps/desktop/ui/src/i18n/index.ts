// UI language: English source strings are the keys; Indonesian comes from src/i18n/id/*.ts and falls back to English.
// A module store (not context): `useLang()` re-renders, and main.tsx remounts the routed content on a switch.
// The resolved language is mirrored to localStorage so the first paint and the Quick window already use it.
import { useSyncExternalStore } from 'react'
import { api } from '../api'
import ID from './id'

export type Lang = 'en' | 'id'
export type LangSetting = 'system' | 'en' | 'id'

const KEY = 'typeshelf.lang'
const subs = new Set<() => void>()

export function resolveLang(s: LangSetting): Lang {
  if (s !== 'system') return s
  const langs = globalThis.navigator?.languages ?? []
  return langs.some((l) => l.toLowerCase().startsWith('id')) ? 'id' : 'en'
}

function stored(): Lang | null {
  try {
    const l = localStorage.getItem(KEY)
    return l === 'en' || l === 'id' ? l : null
  } catch {
    return null
  }
}

let lang: Lang = stored() ?? resolveLang('system')

function apply(l: Lang) {
  lang = l
  if (typeof document !== 'undefined') document.documentElement.lang = l
  subs.forEach((cb) => cb())
}

export const getLang = (): Lang => lang

export function setLang(l: Lang) {
  try {
    localStorage.setItem(KEY, l)
  } catch {
    // Blocked storage: the language still applies, it just isn't remembered before settings load.
  }
  apply(l) // subscribers re-render only when the value changed (useSyncExternalStore compares snapshots)
}

const subscribe = (cb: () => void) => (subs.add(cb), () => void subs.delete(cb))
export const useLang = (): Lang => useSyncExternalStore(subscribe, getLang, getLang)

/** Resolves the saved setting, applies it and relabels the tray. */
export function applyLanguage(s: LangSetting) {
  const l = resolveLang(s)
  setLang(l)
  api.setTrayLanguage(l).catch(() => {})
}

if (typeof window !== 'undefined') {
  document.documentElement.lang = lang
  // The other window (main or Quick) changed the language.
  window.addEventListener('storage', (e) => {
    if (e.key === KEY && (e.newValue === 'en' || e.newValue === 'id') && e.newValue !== lang) apply(e.newValue)
  })
}

export const locale = () => (lang === 'id' ? 'id-ID' : 'en-GB')
export const numLocale = () => (lang === 'id' ? 'id-ID' : 'en-US')

const reported = new Set<string>()

/** The current language's text for the English `en`, with `{name}` placeholders filled from `vars` (numbers localised).
 *  Always pass a string literal so scripts/i18n-check.ts can see it. */
export function t(en: string, vars?: Record<string, string | number>): string {
  let s = en
  if (lang === 'id') {
    const hit = ID.get(en)
    if (hit !== undefined) s = hit
    else if (import.meta.env?.DEV && !reported.has(en)) {
      reported.add(en)
      console.debug('[i18n] missing id:', en)
    }
  }
  if (!vars) return s
  return s.replace(/\{(\w+)\}/g, (m, k: string) => {
    const v = vars[k]
    return v === undefined ? m : typeof v === 'number' ? v.toLocaleString(numLocale()) : v
  })
}

/** Marks an English string for translation where it's rendered later through `t()` (e.g. guide content); returns it unchanged. */
export const tk = (en: string): string => en
