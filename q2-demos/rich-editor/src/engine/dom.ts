/**
 * Between the page and the model: selections as `{ field, offset }`
 * positions, and the editing root's markup.
 *
 * The root is one `contenteditable` element. Each field is an element
 * inside it with a `data-field` attribute holding only its text; everything
 * else inside the root is made non-editable, so the browser's caret moves
 * only through the fields.
 *
 * A multi-line field can be several elements instead, one per line, each
 * with the offset of its line in the field's text as `data-start`. A line
 * with no element of its own (one that can't be edited) has no caret
 * positions: a position on it is shown at the end of the line before.
 */

import type { Position, Selection } from './model.ts'

const FIELD = '[data-field]'

function fieldsIn(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FIELD)]
}

/** Where the text of a field's element starts in the field's text. */
function startOf(el: HTMLElement): number {
  return Number(el.dataset.start ?? 0)
}

/**
 * The part of a field's text that the element `el` shows: all of it, or
 * one line of it.
 */
export function shownRange(el: HTMLElement, text: string): { start: number; end: number } {
  if (el.dataset.start === undefined) return { start: 0, end: text.length }
  const start = startOf(el)
  const lineEnd = text.indexOf('\n', start)
  return { start, end: lineEnd < 0 ? text.length : lineEnd }
}

/**
 * The model position of a DOM position inside `root`. A position outside
 * the fields (in markup around them) snaps to the nearest field edge in
 * the direction the caret was moving: `backward` to the end of the field
 * before it, otherwise to the start of the field after it (or else the
 * nearest field edge in the other direction).
 */
export function positionAt(
  root: HTMLElement,
  node: Node,
  offset: number,
  bias: 'forward' | 'backward' = 'forward',
): Position | null {
  const doc = root.ownerDocument
  const element = node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement
  const field = element?.closest<HTMLElement>(FIELD)
  if (field && root.contains(field)) {
    const prefix = doc.createRange()
    prefix.selectNodeContents(field)
    prefix.setEnd(node, offset)
    return { field: field.dataset.field!, offset: startOf(field) + prefix.toString().length }
  }
  const fields = fieldsIn(root)
  if (fields.length === 0) return null
  const point = doc.createRange()
  point.setStart(node, offset)
  const after = fields.find((f) => point.comparePoint(f, 0) >= 0)
  const before = fields.filter((f) => f !== after && point.comparePoint(f, f.childNodes.length) <= 0).pop()
  const start = after && { field: after.dataset.field!, offset: startOf(after) }
  const end = before && { field: before.dataset.field!, offset: startOf(before) + (before.textContent ?? '').length }
  return (bias === 'backward' ? (end ?? start) : (start ?? end)) ?? null
}

/**
 * The page's selection in model terms, or null when `root` isn't focused.
 * `previous`, the selection before it moved, says which way an end outside
 * the fields snaps: an end that moved backward snaps backward, so the caret
 * keeps going the way it was going. (A mouse selection snaps differently;
 * see `readMouseSelection`.)
 */
export function readSelection(root: HTMLElement, previous?: Selection | null): Selection | null {
  const doc = root.ownerDocument
  const selection = doc.getSelection()
  if (doc.activeElement !== root || !selection || !selection.anchorNode || !selection.focusNode) return null
  const bias = (node: Node, offset: number, was: Position | undefined): 'forward' | 'backward' => {
    const at = was && locate(root, was)
    if (!at) return 'forward'
    const point = doc.createRange()
    point.setStart(node, offset)
    return point.comparePoint(at.node, at.offset) > 0 ? 'backward' : 'forward'
  }
  const { anchorNode, anchorOffset, focusNode, focusOffset } = selection
  const anchor = positionAt(root, anchorNode, anchorOffset, bias(anchorNode, anchorOffset, previous?.anchor))
  const focus = positionAt(root, focusNode, focusOffset, bias(focusNode, focusOffset, previous?.focus))
  return anchor && focus ? { anchor, focus } : null
}

/** The field element `node` is in, if any. */
export function fieldOf(root: HTMLElement, node: Node | null): HTMLElement | null {
  const el = node?.nodeType === Node.ELEMENT_NODE ? (node as Element) : node?.parentElement
  const field = el?.closest<HTMLElement>(FIELD) ?? null
  return field && root.contains(field) ? field : null
}

/** The fields in `root`, in document order. */
export function fieldElements(root: HTMLElement): HTMLElement[] {
  return fieldsIn(root)
}

/** How many ends of the page's selection lie outside the fields. */
export function selectionOutside(root: HTMLElement): 'none' | 'some' | 'all' {
  const selection = root.ownerDocument.getSelection()
  if (!selection || !selection.anchorNode) return 'all'
  const outside = [selection.anchorNode, selection.focusNode].filter((n) => !fieldOf(root, n)).length
  return outside === 0 ? 'none' : outside === 2 ? 'all' : 'some'
}

/** A DOM point in `field` on its first or last line, as close to `x` as possible. */
export function pointOnLine(field: HTMLElement, x: number, line: 'first' | 'last'): { node: Node; offset: number } {
  const doc = field.ownerDocument
  const box = field.getBoundingClientRect()
  const style = getComputedStyle(field)
  const lineHeight = parseFloat(style.lineHeight) || parseFloat(style.fontSize) * 1.2
  const y = line === 'first' ? box.top + lineHeight / 2 : box.bottom - lineHeight / 2
  const clampedX = Math.min(Math.max(x, box.left), box.right - 1)
  const position = doc.caretPositionFromPoint?.(clampedX, y)
  if (position && field.contains(position.offsetNode)) {
    return { node: position.offsetNode, offset: position.offset }
  }
  // Empty field, or the point missed the text: an edge of the field.
  return edge(field, line === 'first' ? 'start' : 'end')
}

/** The DOM point at the start or end of `field`'s text. */
export function edge(field: HTMLElement, which: 'start' | 'end'): { node: Node; offset: number } {
  const text = field.textContent ?? ''
  return locateIn(field, which === 'start' ? 0 : text.length)
}

/** Focus `root` and select `selection` in it. */
export function writeSelection(root: HTMLElement, selection: Selection): void {
  const anchor = locate(root, selection.anchor)
  const focus = locate(root, selection.focus)
  if (!anchor || !focus) return
  if (root.ownerDocument.activeElement !== root) root.focus({ preventScroll: true })
  root.ownerDocument.getSelection()?.setBaseAndExtent(anchor.node, anchor.offset, focus.node, focus.offset)
}

/**
 * The page's selection after a mouse gesture, in model terms: an end
 * outside the fields snaps toward the other end, so a drag from the title
 * into the author grid selects to the end of the last field it crossed.
 * Null when `root` isn't focused or both ends are outside the fields.
 */
export function readMouseSelection(root: HTMLElement): Selection | null {
  const doc = root.ownerDocument
  const selection = doc.getSelection()
  if (doc.activeElement !== root || !selection || !selection.anchorNode || !selection.focusNode) return null
  if (selectionOutside(root) === 'all') return null
  const { anchorNode, anchorOffset, focusNode, focusOffset } = selection
  const point = doc.createRange()
  point.setStart(anchorNode, anchorOffset)
  const focusAfter = point.comparePoint(focusNode, focusOffset) > 0
  const anchor = positionAt(root, anchorNode, anchorOffset, focusAfter ? 'forward' : 'backward')
  const focus = positionAt(root, focusNode, focusOffset, focusAfter ? 'backward' : 'forward')
  return anchor && focus ? { anchor, focus } : null
}

/** The range an `InputEvent` targets, in model terms. */
export function targetRange(root: HTMLElement, e: InputEvent): { from: Position; to: Position } | undefined {
  const [range] = e.getTargetRanges()
  if (!range) return undefined
  const from = positionAt(root, range.startContainer, range.startOffset)
  const to = positionAt(root, range.endContainer, range.endOffset)
  return from && to ? { from, to } : undefined
}

/**
 * Make everything in `root` that is neither a field nor around one
 * non-editable (labels, the author and date grid), with no visible caret:
 * a click there selects nothing to type into, so it shouldn't look as if
 * it does.
 */
export function markIslands(root: HTMLElement): void {
  // Not `instanceof HTMLElement`: the root may be in another frame, whose
  // elements are instances of that frame's own classes.
  for (const child of root.children as HTMLCollectionOf<HTMLElement>) {
    if (child.matches(FIELD)) continue
    if (child.querySelector(FIELD)) {
      // A field came back into what was an island (a removed title added
      // again to the title block): editable again, with a caret.
      if (child.hasAttribute(ISLAND)) {
        child.removeAttribute(ISLAND)
        child.removeAttribute('contenteditable')
        child.style.caretColor = ''
      }
      markIslands(child)
    } else if (child.contentEditable !== 'false') {
      child.setAttribute(ISLAND, '')
      child.contentEditable = 'false'
      child.style.caretColor = 'transparent'
    }
  }
}

/** Marks what `markIslands` made non-editable, so it can be made editable again. */
const ISLAND = 'data-qe-island'

function locate(root: HTMLElement, position: Position): { node: Node; offset: number } | null {
  const elements = fieldsIn(root).filter((f) => f.dataset.field === position.field)
  if (elements.length === 0) return null
  // The element whose text holds the offset, or else the one before it.
  const field = elements.filter((f) => startOf(f) <= position.offset).pop() ?? elements[0]
  return locateIn(field, position.offset - startOf(field))
}

function locateIn(field: HTMLElement, offset: number): { node: Node; offset: number } {
  // At the very end of text ending in a line break, the end of the text
  // node draws at the end of the line above; the caret belongs just before
  // the trailing <br> (see `Field`).
  const text = field.textContent ?? ''
  if (offset >= text.length && text.endsWith('\n') && field.lastChild?.nodeName === 'BR') {
    return { node: field, offset: field.childNodes.length - 1 }
  }
  const walker = field.ownerDocument.createTreeWalker(field, NodeFilter.SHOW_TEXT)
  let remaining = Math.max(0, offset)
  let last: Text | null = null
  for (let node = walker.nextNode() as Text | null; node; node = walker.nextNode() as Text | null) {
    if (remaining <= node.length) return { node, offset: remaining }
    remaining -= node.length
    last = node
  }
  return last ? { node: last, offset: last.length } : { node: field, offset: 0 }
}
