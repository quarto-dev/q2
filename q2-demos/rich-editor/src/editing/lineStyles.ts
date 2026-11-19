/**
 * What the toolbar's style menu, list buttons and quote button do, and when
 * they can, for the toolbar and for their keyboard shortcuts alike. Each
 * change is a function from a line's prefix to its new one, for the
 * editor's `setPrefixes`, which applies it to every line the selection is
 * on; whether it's a toggle on or off goes by the caret's line.
 *
 * A heading can't be in a quote or a list item yet, so a quote or an item
 * can't be made a heading, nor a heading quoted or made an item; and a
 * quote can't be an item. A figure's caption takes no style: it holds its
 * image, which a paragraph, a heading, a quote or an item can't.
 */

import { BULLET, headingLevel, headingPrefix, indentLines, isBullet, isFigure, isItem, isNumbered, isQuote, NUMBERED, QUOTE } from './body.ts'
import { linePrefix, type State } from '../engine/model.ts'

/** The style of the caret's line in the body. */
export interface CaretLine {
  /** The heading's level, or 0. */
  level: number
  quoted: boolean
  bulleted: boolean
  numbered: boolean
  /** A figure's caption. */
  caption: boolean
}

/** The caret's line in the body (null: the caret isn't in the body, or the document can't be edited). */
export function caretLine(state: State, writable: boolean): CaretLine | null {
  const focus = state.selection?.focus
  if (!writable || focus?.field !== 'body') return null
  const prefix = linePrefix(state.fields.body ?? '', focus.offset)
  return {
    level: headingLevel(prefix),
    quoted: isQuote(prefix),
    bulleted: isBullet(prefix),
    numbered: isNumbered(prefix),
    caption: isFigure(prefix),
  }
}

/** Whether the line can take the style `level` (0: Paragraph). */
export function canChoose(line: CaretLine, level: number): boolean {
  return !line.caption && (level === 0 || !(line.quoted || line.bulleted || line.numbered))
}

export function canList(line: CaretLine): boolean {
  return line.level === 0 && !line.quoted && !line.caption
}

export function canQuote(line: CaretLine): boolean {
  return line.level === 0 && !line.bulleted && !line.numbered && !line.caption
}

/** Give the lines the style `level` (0: Paragraph); a quote's, an item's or a caption's line stays as it is. */
export function chooseStyle(level: number): (prefix: string) => string {
  return (p) => (isQuote(p) || isItem(p) || isFigure(p) ? p : level > 0 ? headingPrefix(level) : '')
}

/**
 * The bulleted (`kind` BULLET) or numbered (NUMBERED) list toggle: with the
 * caret on an item of its kind, it makes the items of that kind
 * paragraphs; else it makes paragraphs, and the other kind's items, items
 * of its kind (at the level they're nested at).
 */
export function toggleList(line: CaretLine, kind: typeof BULLET | typeof NUMBERED): (prefix: string) => string {
  const on = kind === BULLET ? line.bulleted : line.numbered
  return (p) => (on ? (p.startsWith(kind) ? '' : p) : p === '' ? kind : isItem(p) ? kind + p.slice(1) : p)
}

/**
 * Indent (`by` 1) or outdent (-1) the items the selection is on, each
 * with the items nested in it (see `indentLines`), for the editor's
 * `setFieldPrefixes`.
 */
export function indent(by: 1 | -1): (prefixes: string[], first: number, last: number) => string[] | null {
  return (prefixes, first, last) => indentLines(prefixes, first, last, by)
}

/** Whether the items the selection is on can be indented (`by` 1) or outdented (-1). */
export function canIndent(state: State, by: 1 | -1): boolean {
  const { selection } = state
  const body = state.fields.body
  if (!selection || body === undefined) return false
  // The body is the last field: a selection's end in a field before it is before its first line.
  const lineOf = (p: { field: string; offset: number }) =>
    p.field === 'body' ? body.slice(0, p.offset).split('\n').length - 1 : 0
  const [a, b] = [lineOf(selection.anchor), lineOf(selection.focus)]
  const prefixes = body.split('\n').map((line) => linePrefix(line, 0))
  return indentLines(prefixes, Math.min(a, b), Math.max(a, b), by) !== null
}

/** The quote toggle: in a quote, it unquotes the lines; else it quotes the paragraphs. */
export function toggleQuote(line: CaretLine): (prefix: string) => string {
  return (p) => (line.quoted ? (isQuote(p) ? '' : p) : p === '' ? QUOTE : p)
}
