// Finds a "tofu": a character on a preview surface that the previewed font doesn't have. The app draws no
// tofu of its own; the webview shows a box (or a fallback font's glyph) in its place.

let ctx: CanvasRenderingContext2D | null = null

/** True when the font falls back for `ch`: the width then depends on the fallback, so two fallbacks disagree. */
function lacks(cs: CSSStyleDeclaration, ch: string) {
  ctx ??= document.createElement('canvas').getContext('2d')
  if (!ctx) return false
  const first = cs.fontFamily.split(',')[0]
  const width = (fallback: string) => {
    ctx!.font = `${cs.fontStyle} ${cs.fontWeight} 48px ${first}, ${fallback}`
    return ctx!.measureText(ch).width
  }
  // ponytail: a character both fallbacks also lack measures the same and isn't found; harmless for an egg.
  return width('monospace') !== width('serif')
}

/** The box of the missing character under (x, y) in a font preview (an element with its own font-family), or null. */
export function tofuAt(x: number, y: number): DOMRect | null {
  const caret = document.caretRangeFromPoint?.(x, y)
  const node = caret?.startContainer
  if (!caret || !(node instanceof Text)) return null
  const el = node.parentElement
  if (!el?.closest('[style*="font-family"]')) return null
  // The caret lands on whichever side of the clicked character is nearer.
  for (const i of [caret.startOffset, caret.startOffset - 1]) {
    const cp = i >= 0 && i < node.length ? node.data.codePointAt(i) : undefined
    if (cp === undefined || /\s/.test(String.fromCodePoint(cp))) continue
    const r = document.createRange()
    r.setStart(node, i)
    r.setEnd(node, i + (cp > 0xffff ? 2 : 1))
    const box = r.getBoundingClientRect()
    if (x < box.left || x > box.right || y < box.top || y > box.bottom) continue
    return lacks(getComputedStyle(el),String.fromCodePoint(cp)) ? box : null
  }
  return null
}
