/**
 * Selections inside an element that holds only text, as character offsets
 * into its `textContent`.
 */

export interface TextRange {
  start: number
  end: number
}

/** The selection inside `el`, or null when the selection is elsewhere. */
export function getTextRange(el: HTMLElement): TextRange | null {
  const selection = el.ownerDocument.getSelection()
  if (!selection || selection.rangeCount === 0) return null
  const range = selection.getRangeAt(0)
  if (!el.contains(range.startContainer) || !el.contains(range.endContainer)) return null
  const prefix = el.ownerDocument.createRange()
  prefix.selectNodeContents(el)
  prefix.setEnd(range.startContainer, range.startOffset)
  const start = prefix.toString().length
  return { start, end: start + range.toString().length }
}

/** Select `range` inside `el`, clamped to its text. */
export function setTextRange(el: HTMLElement, range: TextRange): void {
  const doc = el.ownerDocument
  const selection = doc.getSelection()
  if (!selection) return
  const dom = doc.createRange()
  const start = locate(el, range.start)
  const end = locate(el, range.end)
  dom.setStart(start.node, start.offset)
  dom.setEnd(end.node, end.offset)
  selection.removeAllRanges()
  selection.addRange(dom)
}

function locate(el: HTMLElement, offset: number): { node: Node; offset: number } {
  const walker = el.ownerDocument.createTreeWalker(el, NodeFilter.SHOW_TEXT)
  let remaining = Math.max(0, offset)
  let last: Text | null = null
  for (let node = walker.nextNode() as Text | null; node; node = walker.nextNode() as Text | null) {
    if (remaining <= node.length) return { node, offset: remaining }
    remaining -= node.length
    last = node
  }
  return last ? { node: last, offset: last.length } : { node: el, offset: 0 }
}
