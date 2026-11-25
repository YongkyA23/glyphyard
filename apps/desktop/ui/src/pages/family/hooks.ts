// Glyphs tab extras: copy a character as SVG, and whether Alt is held (for the anatomy card).
import { useCallback, useEffect, useState } from 'react'
import { api } from '../../api'
import { useApp } from '../../state'
import { errMsg } from '../../ui'

export const uPlus = (cp: number) => `U+${cp.toString(16).toUpperCase().padStart(4, '0')}`

/** Copies codepoint `cp` of face `hash` to the clipboard as SVG text and says so in a toast. */
export function useCopySvg(hash: string) {
  const { toast } = useApp()
  return useCallback(async (cp: number) => {
    const label = `${String.fromCodePoint(cp)} (${uPlus(cp)})`
    let svg: string
    try {
      svg = await api.glyphSvg(hash, cp)
    } catch (e) {
      return toast(errMsg(e))
    }
    navigator.clipboard.writeText(svg).then(() => toast(`Copied ${label} as SVG`), () => toast('Couldn’t copy to the clipboard.'))
  }, [hash, toast])
}

/** True while Alt is held down in this window. */
export function useAltHeld() {
  const [held, setHeld] = useState(false)
  useEffect(() => {
    const down = (e: KeyboardEvent) => e.key === 'Alt' && (e.preventDefault(), setHeld(true))
    const up = (e: KeyboardEvent) => e.key === 'Alt' && (e.preventDefault(), setHeld(false))
    const off = () => setHeld(false)
    addEventListener('keydown', down)
    addEventListener('keyup', up)
    addEventListener('blur', off)
    return () => (removeEventListener('keydown', down), removeEventListener('keyup', up), removeEventListener('blur', off))
  }, [])
  return held
}
