/**
 * Between the page and the model: selections as `{ field, offset }`
 * positions, and the editing root's markup.
 *
 * The root is one `contenteditable` element. Each field is an element
 * inside it with a `data-field` attribute holding only its text; everything
 * else inside the root is made non-editable, so the browser's caret moves
 * only through the fields.
 */

import type { Position, Selection } from './model.ts'

const FIELD = '[data-field]'

function fieldsIn(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FIELD)]
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
    return { field: field.dataset.field!, offset: prefix.toString().length }
  }
  const fields = fieldsIn(root)
  if (fields.length === 0) return null
  const point = doc.createRange()
  point.setStart(node, offset)
  const after = fields.find((f) => point.comparePoint(f, 0) >= 0)
  const before = fields.filter((f) => f !== after && point.comparePoint(f, f.childNodes.length) <= 0).pop()
  const start = after && { field: after.dataset.field!, offset: 0 }
  const end = before && { field: before.dataset.field!, offset: (before.textContent ?? '').length }
  return (bias === 'backward' ? (end ?? start) : (start ?? end)) ?? null
}

/**
 * The page's selection in model terms, or null when `root` isn't focused.
 * `previous`, the selection before it moved, says which way an end outside
 * the fields snaps: an end that moved backward snaps backward, so the caret
 * keeps going the way it was going.
 */
export function readSelection(root: HTMLElement, previous?: Selection | null): Selection | null {
  const doc = root.ownerDocument
  const selection = doc.getSelection()
  if (doc.activeElement !== root || !selection || !selection.anchorNode || !selection.focusNode) return null
  const bias = (node: Node, offset: number, was: Position | undefined) => {
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

/** Whether both ends of the page's selection are inside fields in `root`. */
export function selectionInFields(root: HTMLElement): boolean {
  const selection = root.ownerDocument.getSelection()
  const inField = (node: Node | null) => {
    const el = node?.nodeType === Node.ELEMENT_NODE ? (node as Element) : node?.parentElement
    const field = el?.closest(FIELD)
    return !!field && root.contains(field)
  }
  return !!selection && inField(selection.anchorNode) && inField(selection.focusNode)
}

/** Focus `root` and select `selection` in it. */
export function writeSelection(root: HTMLElement, selection: Selection): void {
  const anchor = locate(root, selection.anchor)
  const focus = locate(root, selection.focus)
  if (!anchor || !focus) return
  if (root.ownerDocument.activeElement !== root) root.focus({ preventScroll: true })
  root.ownerDocument.getSelection()?.setBaseAndExtent(anchor.node, anchor.offset, focus.node, focus.offset)
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
 * non-editable (labels, the author and date grid).
 */
export function markIslands(root: HTMLElement): void {
  for (const child of root.children) {
    if (!(child instanceof HTMLElement) || child.matches(FIELD)) continue
    if (child.querySelector(FIELD)) markIslands(child)
    else if (child.contentEditable !== 'false') child.contentEditable = 'false'
  }
}

function locate(root: HTMLElement, position: Position): { node: Node; offset: number } | null {
  const field = fieldsIn(root).find((f) => f.dataset.field === position.field)
  if (!field) return null
  const walker = root.ownerDocument.createTreeWalker(field, NodeFilter.SHOW_TEXT)
  let remaining = Math.max(0, position.offset)
  let last: Text | null = null
  for (let node = walker.nextNode() as Text | null; node; node = walker.nextNode() as Text | null) {
    if (remaining <= node.length) return { node, offset: remaining }
    remaining -= node.length
    last = node
  }
  return last ? { node: last, offset: last.length } : { node: field, offset: 0 }
}
