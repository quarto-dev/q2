/**
 * Visual lines inside an editable element, for moving the caret between
 * fields with the arrow keys the way a word processor does: Down on a
 * field's last line goes to the next field's first line, at the same
 * horizontal position (and Up the reverse).
 */

export interface CaretLine {
  /** The caret's horizontal position, in client coordinates. */
  x: number
  onFirstLine: boolean
  onLastLine: boolean
}

/** Where the collapsed caret inside `el` sits, or null if it isn't there. */
export function caretLine(el: HTMLElement): CaretLine | null {
  const selection = el.ownerDocument.getSelection()
  if (!selection || selection.rangeCount === 0 || !selection.isCollapsed) return null
  const range = selection.getRangeAt(0)
  if (!el.contains(range.startContainer)) return null

  const box = el.getBoundingClientRect()
  // A collapsed range in an empty element has no rects; use the element.
  const rects = range.getClientRects()
  const caret = rects.length > 0 ? rects[rects.length - 1] : box
  const half = lineHeight(el) / 2
  return {
    x: rects.length > 0 ? caret.left : box.left,
    onFirstLine: caret.top < box.top + half,
    onLastLine: caret.bottom > box.bottom - half,
  }
}

/** Focus `el` with the caret on its first or last line, as close to `x` as possible. */
export function placeCaretOnLine(el: HTMLElement, x: number, line: 'first' | 'last'): void {
  el.focus()
  const doc = el.ownerDocument
  const box = el.getBoundingClientRect()
  const half = lineHeight(el) / 2
  const y = line === 'first' ? box.top + half : box.bottom - half
  const clampedX = Math.min(Math.max(x, box.left), box.right - 1)

  const position = doc.caretPositionFromPoint?.(clampedX, y)
  const node = position?.offsetNode
  const offset = position?.offset
  const selection = doc.getSelection()
  if (!selection) return
  const range = doc.createRange()
  if (node && offset !== undefined && el.contains(node)) {
    range.setStart(node, offset)
  } else {
    // Empty field, or the point missed the text: an edge of the field.
    range.selectNodeContents(el)
    range.collapse(line === 'first')
  }
  range.collapse(true)
  selection.removeAllRanges()
  selection.addRange(range)
}

function lineHeight(el: HTMLElement): number {
  const style = getComputedStyle(el)
  const lh = parseFloat(style.lineHeight)
  return Number.isNaN(lh) ? parseFloat(style.fontSize) * 1.2 : lh
}
