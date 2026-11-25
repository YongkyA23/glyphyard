// The easter eggs' window-wide listeners, mounted once in Shell: the Konami code, Escape, and frying a tofu.
import { useEffect, useState } from 'react'
import { useApp } from '../state'
import { clearFont, playful, reducedMotion, swapFont } from './effects'
import { tofuAt } from './tofu'
import './extras.css'

const KONAMI = ['arrowup', 'arrowup', 'arrowdown', 'arrowdown', 'arrowleft', 'arrowright', 'arrowleft', 'arrowright', 'b', 'a']
const typing = (t: EventTarget | null) => !!(t as Element | null)?.closest?.('input, textarea, select, [contenteditable]:not([contenteditable="false"])')

type Fry = { id: number; left: number; top: number; size: number }

export function Extras() {
  const { toast } = useApp()
  const [fries, setFries] = useState<Fry[]>([])

  useEffect(() => {
    let keys: string[] = []
    let id = 0
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        clearFont()
        return setFries([])
      }
      if (typing(e.target) || e.ctrlKey || e.metaKey || e.altKey) return
      keys = [...keys, e.key.toLowerCase()].slice(-KONAMI.length)
      if (keys.join() !== KONAMI.join()) return
      keys = []
      playful().then((on) => on && swapFont('wingdings'))
    }
    // Three clicks in a row (the browser counts them in `detail`) on a missing character in a preview.
    const onClick = (e: MouseEvent) => {
      if (e.detail !== 3) return
      const box = tofuAt(e.clientX, e.clientY)
      if (!box) return
      playful().then((on) => {
        if (!on) return
        if (reducedMotion()) return toast('Fried! That box stands for a character this font doesn’t have.')
        const size = Math.max(box.width, box.height)
        setFries((fs) => [...fs, { id: ++id, left: box.left + box.width / 2 - size / 2, top: box.top + box.height / 2 - size / 2, size }])
      })
    }
    addEventListener('keydown', onKey)
    addEventListener('click', onClick)
    return () => {
      removeEventListener('keydown', onKey)
      removeEventListener('click', onClick)
      clearFont()
    }
  }, [toast])

  return fries.map((f) => (
    <svg
      key={f.id} className="egg-fry" aria-hidden="true" viewBox="0 0 40 40" style={{ left: f.left, top: f.top, width: f.size, height: f.size }}
      onAnimationEnd={(e) => e.target === e.currentTarget && setFries((fs) => fs.filter((x) => x.id !== f.id))}
    >
      <rect className="egg-fry-pan" x="2" y="2" width="36" height="36" rx="6" />
      <path className="egg-fry-white" d="M20 5c7 0 9 5 13 7s3 10-1 14-6 9-13 8-12-3-13-9 1-9 3-13S13 5 20 5Z" />
      <circle className="egg-fry-yolk" cx="19" cy="20" r="6" />
    </svg>
  ))
}
