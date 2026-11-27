// English-as-key translations. `t('English', { n })` returns the Indonesian entry when the language is Indonesian, else the English.
import { useSyncExternalStore } from 'react'

export type Lang = 'en' | 'id'
export type LangSetting = 'system' | 'en' | 'id'

const KEY = 'glyphyard.admin.lang'

const read = (): LangSetting => {
  try {
    const v = localStorage.getItem(KEY)
    if (v === 'en' || v === 'id' || v === 'system') return v
  } catch { /* storage blocked */ }
  return 'system'
}
const resolve = (s: LangSetting): Lang =>
  s === 'system' ? (navigator.languages?.some((l) => l.toLowerCase().startsWith('id')) ? 'id' : 'en') : s

let setting = read()
let lang = resolve(setting)
const listeners = new Set<() => void>()
const apply = () => { if (typeof document !== 'undefined') document.documentElement.lang = lang }
apply()

export const getLangSetting = () => setting
export function setLangSetting(s: LangSetting) {
  setting = s
  lang = resolve(s)
  try { localStorage.setItem(KEY, s) } catch { /* storage blocked */ }
  apply()
  listeners.forEach((l) => l())
}

export const useLang = (): Lang =>
  useSyncExternalStore((l) => (listeners.add(l), () => void listeners.delete(l)), () => lang)

let dict: Record<string, string> = {}
/** Called by i18n/id/index.ts, which holds the import.meta.glob (so bun scripts can import this file). */
export const setDict = (d: Record<string, string>) => { dict = d }
const warned = new Set<string>()

export const locale = () => (lang === 'id' ? 'id-ID' : 'en-GB')
export const numLocale = () => (lang === 'id' ? 'id-ID' : 'en-US')

export function t(en: string, vars?: Record<string, string | number>): string {
  let s = en
  if (lang === 'id') {
    const hit = dict[en]
    if (hit) s = hit
    else if (import.meta.env?.DEV && !warned.has(en)) { warned.add(en); console.debug('[i18n] missing id:', en) }
  }
  if (!vars) return s
  return s.replace(/\{(\w+)\}/g, (m, k: string) => {
    const v = vars[k]
    return v === undefined ? m : typeof v === 'number' ? v.toLocaleString(numLocale()) : v
  })
}

/** Marks a string for the checker without translating it; translate at render time with t(). */
export const tk = (en: string) => en
