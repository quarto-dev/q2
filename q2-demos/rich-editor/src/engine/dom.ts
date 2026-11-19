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
 *
 * A field's text can be formatted (see `marks.ts`): its element then holds
 * the visible text in elements for its formats (`renderText`), and each
 * text node knows where it starts in the field's text, so a point in one
 * is a position, and the other way round. The marks have no text in the
 * page; where the caret is at a position no text node's edge stands for
 * (outside code at the end of a line, or in a format just turned on), the
 * element holds a zero-width space there, in the caret's formats, for the
 * caret to be drawn in (the holder): a point in it is the caret's position.
 */

import {
  edgeOf,
  hasMarks,
  PLAIN,
  sameStyle,
  scan,
  stopAt,
  tree,
  withoutMarks,
  type InlineNode,
  type Run,
  type Style,
} from './marks.ts'
import type { Position, Selection } from './model.ts'

/** How formatted text is shown. */
export interface MarkView {
  /** The element for a format (by its number), and its class. */
  element(format: number): { as: string; className?: string }
  /** Formats with two caret positions at each edge (a bit for each). */
  twoSided: number
  /** Formats that hold only text, innermost (a bit for each). */
  leaf: number
}

/** The zero-width space a holder holds. */
export const HOLDER = '\u200b'

/** Where each text node of a field's element starts in the element's text. */
const leafStarts = new WeakMap<Node, number>()
/** The position (in its element's text) of the caret a holder holds. */
const holders = new WeakMap<Node, number>()
/** The text each field's element shows, marks and all, and the key it was rendered for. */
const shown = new WeakMap<Element, { text: string; key: string; view: MarkView | undefined }>()
/** Whether `renderText` rebuilt an element the page's selection was in: it must be put back. */
let selectionLost = false

/** Whether the page's selection was lost to a rebuilt element since the last call. */
export function takeSelectionLost(): boolean {
  const lost = selectionLost
  selectionLost = false
  return lost
}

/**
 * Whether the caret at `caret` in `text` needs a holder to be drawn where
 * it is: when neither the text before it nor after it is in its style, or
 * it's at an edge of a format with two caret positions.
 */
export function needsHolder(text: string, caret: number, twoSided: number): boolean {
  if (!hasMarks(text)) return false
  const scanned = scan(text)
  const stop = stopAt(scanned, caret)
  const before = scanned.chars[stop.index - 1]?.style
  const after = scanned.chars[stop.index]?.style
  if (!before && !after) return !sameStyle(stop.style, PLAIN)
  if (edgeOf(scanned, stop.index, twoSided)) return true
  return !(before && sameStyle(before, stop.style)) && !(after && sameStyle(after, stop.style))
}

/**
 * Show `text` (marks and all) in `el`, a field's element, and a holder for
 * a collapsed caret at `caret` if it needs one (see `needsHolder`). Text
 * ending in a line break, and text with nothing visible, get a trailing
 * `<br>` (see `Field`). Nothing is done when `el` already shows it.
 */
export function renderText(el: HTMLElement, text: string, view: MarkView | undefined, caret: number | undefined): void {
  const holder = view && caret !== undefined && needsHolder(text, caret, view.twoSided) ? caret : undefined
  const key = `${text}\u0000${holder ?? ''}\u0000${el.dataset.start ?? ''}`
  const wantsBreak = text.endsWith('\n') || withoutMarks(text) === ''
  const hasBreak = el.lastChild?.nodeName === 'BR'
  if (shown.get(el)?.key === key && hasBreak === wantsBreak) return
  const selection = el.ownerDocument.getSelection()
  if (selection && [selection.anchorNode, selection.focusNode].some((n) => n && el.contains(n))) selectionLost = true
  shown.set(el, { text, key, view })
  const doc = el.ownerDocument
  el.replaceChildren()
  if (!view || (!hasMarks(text) && holder === undefined)) {
    if (text !== '') {
      const node = doc.createTextNode(text)
      leafStarts.set(node, 0)
      el.append(node)
    }
  } else {
    const nodes = tree(runsWithHolder(text, holder), view.leaf)
    build(doc, el, nodes, view, holder, Number(el.dataset.start ?? 0))
  }
  if (wantsBreak) el.append(doc.createElement('br'))
}

/** Show again what `el` was last given, throwing away whatever the browser wrote into it (a composition not taken). */
export function restoreText(el: HTMLElement): void {
  const was = shown.get(el)
  if (!was) return
  shown.delete(el)
  renderText(el, was.text, was.view, undefined)
}

/** The runs of `text`, with an empty one for the holder at `holder`, in the caret's style. */
function runsWithHolder(text: string, holder: number | undefined): Run[] {
  const scanned = scan(text)
  const runs: Run[] = []
  let style: Style | null = null
  const stop = holder === undefined ? undefined : stopAt(scanned, holder)
  const pushHolder = () => runs.push({ text: '', start: holder!, ...stop!.style })
  scanned.chars.forEach((c, i) => {
    if (stop && stop.index === i) {
      pushHolder()
      style = null
    }
    const last = runs[runs.length - 1]
    if (last && style && sameStyle(style, c.style) && last.start + last.text.length === c.offset) {
      runs[runs.length - 1] = { ...last, text: last.text + c.char }
    } else runs.push({ text: c.char, start: c.offset, ...c.style })
    style = c.style
  })
  if (stop && stop.index === scanned.chars.length) pushHolder()
  return runs
}

function build(doc: Document, parent: Node, nodes: readonly InlineNode[], view: MarkView, holder: number | undefined, start: number): void {
  for (const node of nodes) {
    if (node.kind === 'text') {
      const text = doc.createTextNode(node.text === '' ? HOLDER : node.text)
      if (node.text === '') holders.set(text, holder!)
      else leafStarts.set(text, node.start)
      parent.appendChild(text)
    } else if (node.kind === 'mark') {
      const { as, className } = view.element(node.format)
      const el = doc.createElement(as)
      if (className) el.className = className
      el.dataset.qeMark = String(node.format)
      build(doc, el, node.children, view, holder, start)
      parent.appendChild(el)
    } else {
      const a = doc.createElement('a')
      a.setAttribute('href', node.url)
      const first = firstStart(node.children)
      if (first !== undefined) a.dataset.qeAt = String(start + first)
      build(doc, a, node.children, view, holder, start)
      parent.appendChild(a)
    }
  }
}

function firstStart(nodes: readonly InlineNode[]): number | undefined {
  for (const node of nodes) {
    if (node.kind === 'text') {
      if (node.text !== '') return node.start
    } else {
      const found = firstStart(node.children)
      if (found !== undefined) return found
    }
  }
  return undefined
}

/** The text nodes of a field's element that stand for its text (and its holder), in order. */
function leaves(el: HTMLElement): Text[] {
  const walker = el.ownerDocument.createTreeWalker(el, NodeFilter.SHOW_TEXT)
  const out: Text[] = []
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (leafStarts.has(node) || holders.has(node)) out.push(node as Text)
  }
  return out
}

/** The span of the element's text a leaf stands for. */
function leafSpan(leaf: Text): { start: number; end: number } {
  const holder = holders.get(leaf)
  if (holder !== undefined) return { start: holder, end: holder }
  const start = leafStarts.get(leaf)!
  return { start, end: start + leaf.length }
}

/** The text a field's element stands for (marks and all). */
function textOf(el: HTMLElement): string {
  return shown.get(el)?.text ?? el.textContent ?? ''
}

/** Where a DOM point inside a field's element is in the element's text. */
function offsetIn(el: HTMLElement, node: Node, offset: number): number {
  const holder = holders.get(node)
  if (holder !== undefined) return holder
  const start = leafStarts.get(node)
  if (start !== undefined) return start + Math.min(offset, (node as Text).length)
  const own = leaves(el)
  if (own.length === 0) {
    // Text the element shows that it wasn't given (during a composition).
    const prefix = el.ownerDocument.createRange()
    prefix.selectNodeContents(el)
    prefix.setEnd(node, offset)
    return prefix.toString().length
  }
  // A point between nodes: the end of the text before it, or the start of the text after.
  const point = el.ownerDocument.createRange()
  point.setStart(node, offset)
  let found: number | undefined
  for (const leaf of own) {
    if (point.comparePoint(leaf, 0) >= 0) return found ?? leafSpan(leaf).start
    found = leafSpan(leaf).end
  }
  return found ?? 0
}

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
    return { field: field.dataset.field!, offset: startOf(field) + offsetIn(field, node, offset) }
  }
  const fields = fieldsIn(root)
  if (fields.length === 0) return null
  const point = doc.createRange()
  point.setStart(node, offset)
  const after = fields.find((f) => point.comparePoint(f, 0) >= 0)
  const before = fields.filter((f) => f !== after && point.comparePoint(f, f.childNodes.length) <= 0).pop()
  const start = after && { field: after.dataset.field!, offset: startOf(after) }
  const end = before && { field: before.dataset.field!, offset: startOf(before) + textOf(before).length }
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
  return locateIn(field, which === 'start' ? 0 : textOf(field).length)
}

/** Where the text from `from` to `to` is drawn: the box of its last line, or null when it isn't shown. */
export function rectOf(root: HTMLElement, from: Position, to: Position): DOMRect | null {
  const start = locate(root, from)
  const end = locate(root, to)
  if (!start || !end) return null
  const range = root.ownerDocument.createRange()
  range.setStart(start.node, start.offset)
  range.setEnd(end.node, end.offset)
  const rects = range.getClientRects()
  return rects.length > 0 ? rects[rects.length - 1] : range.getBoundingClientRect()
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
 * Marks what a line's `wrap` adds around its element (a figure's image
 * around its caption) as the line's (see `LineElement.wrap`).
 */
export const WRAP_ATTR = 'data-qe-wrap'

/** Whether a DOM point is in what a line's `wrap` adds, outside the line's own element. */
function inWrap(root: HTMLElement, node: Node): boolean {
  const element = node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement
  const wrap = element?.closest(`[${WRAP_ATTR}]`)
  return Boolean(wrap && root.contains(wrap) && !element?.closest(FIELD))
}

/**
 * The page's selection after a mouse gesture, in model terms: an end
 * outside the fields snaps toward the other end, so a drag from the title
 * into the author grid selects to the end of the last field it crossed.
 * An end in what a line's `wrap` adds (a figure's image) snaps away from
 * the other end instead, so a drag onto an image takes it in: down onto
 * it, to the start of its caption's text; up onto it, to the end of the
 * line before. Null when `root` isn't focused or both ends are outside
 * the fields.
 */
export function readMouseSelection(root: HTMLElement): Selection | null {
  const doc = root.ownerDocument
  const selection = doc.getSelection()
  if (doc.activeElement !== root || !selection || !selection.anchorNode || !selection.focusNode) return null
  const { anchorNode, anchorOffset, focusNode, focusOffset } = selection
  const wrapped = !selection.isCollapsed && (inWrap(root, anchorNode) || inWrap(root, focusNode))
  if (selectionOutside(root) === 'all' && !wrapped) return null
  const point = doc.createRange()
  point.setStart(anchorNode, anchorOffset)
  const focusAfter = point.comparePoint(focusNode, focusOffset) > 0
  const away = (node: Node, toward: 'forward' | 'backward') =>
    !selection.isCollapsed && inWrap(root, node) ? (toward === 'forward' ? 'backward' : 'forward') : toward
  const anchor = positionAt(root, anchorNode, anchorOffset, away(anchorNode, focusAfter ? 'forward' : 'backward'))
  const focus = positionAt(root, focusNode, focusOffset, away(focusNode, focusAfter ? 'backward' : 'forward'))
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
  const text = textOf(field)
  if (offset >= text.length && text.endsWith('\n') && field.lastChild?.nodeName === 'BR') {
    return { node: field, offset: field.childNodes.length - 1 }
  }
  const own = leaves(field)
  const holder = own.find((leaf) => holders.get(leaf) === offset)
  if (holder) return { node: holder, offset: 1 }
  const texts = own.filter((leaf) => leafStarts.has(leaf))
  const inside = texts.find((leaf) => leafSpan(leaf).start <= offset && offset <= leafSpan(leaf).end)
  if (inside) return { node: inside, offset: offset - leafSpan(inside).start }
  // Between text nodes (among marks): the end of the one before, or the start of the first.
  const before = texts.filter((leaf) => leafSpan(leaf).end <= offset).pop()
  if (before) return { node: before, offset: before.length }
  if (texts[0]) return { node: texts[0], offset: 0 }
  if (own[0]) return { node: own[0], offset: 1 }
  return { node: field, offset: 0 }
}
