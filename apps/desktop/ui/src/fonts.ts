// Preview fonts. In Tauri every Face gets one @font-face ("f-<hash>") pointing at its file via the asset protocol.
// In the browser mock, sample families load from Google Fonts instead.
import type { CSSProperties } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import { inTauri, type Axis, type Face, type Family } from './api'
import { MOCK_GF } from './mock-fonts'

const done = new Set<string>()
let sheet: HTMLStyleElement | null = null

function addRule(id: string, css: string) {
  if (done.has(id)) return
  done.add(id)
  sheet ??= document.head.appendChild(document.createElement('style'))
  sheet.appendChild(document.createTextNode(css + '\n'))
}

/** Loads a Google Fonts CSS2 family spec (e.g. "Lora:wght@500"); `text` limits the download to those characters. */
export function loadGoogle(spec: string, text?: string, display: 'block' | 'swap' = 'block') {
  const href = `https://fonts.googleapis.com/css2?family=${spec}&display=${display}${text ? `&text=${encodeURIComponent(text)}` : ''}`
  if (done.has(href)) return
  done.add(href)
  const link = document.createElement('link')
  link.rel = 'stylesheet'
  link.href = href
  document.head.appendChild(link)
}

const fallback = (name: string) => (/mono/i.test(name) ? 'monospace' : /serif|fraunces/i.test(name) && !/sans/i.test(name) ? 'serif' : 'sans-serif')

/** Style that renders text in this face. Safe to call during render (injection is idempotent). */
export function faceStyle(face: Face | null | undefined): CSSProperties {
  if (!face) return {}
  if (!inTauri) {
    loadGoogle(MOCK_GF[face.family] ?? face.family.replace(/ /g, '+'))
    return { fontFamily: `'${face.family}', ${fallback(face.family)}`, fontWeight: face.weight, fontStyle: face.italic ? 'italic' : 'normal' }
  }
  const name = `f-${face.hash}`
  addRule(name, `@font-face { font-family: "${name}"; src: url("${convertFileSrc(face.path)}"); font-display: block; }`)
  return { fontFamily: `"${name}", ${fallback(face.family)}` }
}

/** The face that best represents a family: upright, closest to `weight` (Regular), variable preferred. */
export function pickFace(fam: Pick<Family, 'faces'> | null | undefined, weight = 400): Face | null {
  if (!fam?.faces.length) return null
  return [...fam.faces].sort(
    (a, b) => Number(a.italic) - Number(b.italic) || Number(b.isVariable) - Number(a.isVariable) || Math.abs(a.weight - weight) - Math.abs(b.weight - weight),
  )[0]
}

export const familyStyle = (fam: Pick<Family, 'faces'> | null | undefined, weight = 400): CSSProperties => {
  const face = pickFace(fam, weight)
  // A variable face can hit the requested weight itself.
  return { ...faceStyle(face), ...(face?.isVariable && weight !== 400 ? { fontVariationSettings: `'wght' ${weight}` } : {}) }
}

export const AXIS_NAME: Record<string, string> = {
  wght: 'Weight', wdth: 'Width', opsz: 'Optical size', ital: 'Italic', slnt: 'Slant', SOFT: 'Softness', WONK: 'Wonky', GRAD: 'Grade', CASL: 'Casual', MONO: 'Monospace',
}
/** Slider step: whole numbers, tenths for short ranges. */
export const axisStep = (a: Axis) => (a.max - a.min > 2 ? 1 : a.max - a.min > 1 ? 0.1 : 1)
