/**
 * The editor's model: a fixed sequence of text fields, some of which may
 * be absent, and a selection over them. A field's text can hold
 * formatting, as marks the editor keeps out of sight (see `marks.ts`).
 *
 * Offsets count UTF-16 code units, like the DOM's.
 */

import { canonical, hasMarks, LINK_OPEN, LINK_TEXT, withoutMarks } from './marks.ts'

/**
 * A line of a multi-line field that holds only this character stands for
 * something the editor shows and can't edit (a heading or a table among
 * the paragraphs of a body). No edit removes it, joins it to another line
 * or puts text on its line, and it can't be typed.
 */
export const ATOM = '\uFFFC'

/**
 * A line of a multi-line field that holds only this character stands for
 * something the editor shows and can't edit, but can remove (a horizontal
 * rule). Like an atom, it's on a line of its own and can't be typed; but a
 * deletion that covers it removes it, and so do Backspace at the start of
 * the line after it and Delete at the end of the line before it.
 */
export const VOID = '\uFFFB'

/** Whether `char` stands for a line that can't be edited (`ATOM` or `VOID`). */
export function isAtom(char: string): boolean {
  return char === ATOM || char === VOID
}

/**
 * A line of a multi-line field can start with a run of prefix characters,
 * the Unicode noncharacters U+FDD0 to U+FDEF, which give it a kind (what
 * each means is up to the schema; in the qmd editor, a heading's level).
 * A prefix can't be typed or pasted, the caret never goes before it, and
 * it's dropped anywhere but at the start of a line, so a line joined to
 * the one before takes that line's kind.
 */
export function isPrefix(char: string): boolean {
  return char.length === 1 && char >= '\uFDD0' && char <= '\uFDEF'
}

const PREFIXES = /[\uFDD0-\uFDEF]/g

/** `text` without prefix characters. */
export function withoutPrefixes(text: string): string {
  return text.replace(PREFIXES, '')
}

/** Where the line that `offset` is on starts in `text`. */
export function lineStartOf(text: string, offset: number): number {
  // Not `lastIndexOf` from `offset - 1`: from -1, it would look at index 0.
  return offset === 0 ? 0 : text.lastIndexOf('\n', offset - 1) + 1
}

/** The prefix of the line that `offset` is on. */
export function linePrefix(text: string, offset: number): string {
  const start = lineStartOf(text, offset)
  let end = start
  while (end < text.length && isPrefix(text[end])) end++
  return text.slice(start, end)
}

/** Each field's text, or undefined when the field is absent. */
export type Fields = Readonly<Record<string, string | undefined>>

/** Inline formatting (see `marks.ts`). */
export interface MarksSchema {
  /** Fields whose lines can be formatted. */
  readonly fields: readonly string[]
  /**
   * Formats with two caret positions at each edge, inside and outside (a
   * bit for each): an arrow key at the edge moves from one to the other
   * before it moves on. They don't carry on to a line Enter starts.
   */
  readonly twoSided?: number
  /** Formats that hold only text, innermost (code): a bit for each. */
  readonly leaf?: number
}

export interface Position {
  readonly field: string
  readonly offset: number
}

/** Where the selection started (`anchor`) and where it ends (`focus`). */
export interface Selection {
  readonly anchor: Position
  readonly focus: Position
}

export interface State {
  readonly fields: Fields
  /** Null when the editor doesn't have focus. */
  readonly selection: Selection | null
}

/** What the editor knows about the document it edits. */
export interface Schema {
  /** Every field the editor can show, in reading order. */
  readonly order: readonly string[]
  /** Absent fields that Enter in the field before adds (empty). */
  readonly addable?: readonly string[]
  /**
   * Fields whose text can hold line breaks (`\n`): Enter inserts one, and
   * pasted line breaks are kept. In other fields Enter moves to the next
   * field, and pasted line breaks become spaces.
   */
  readonly multiline?: readonly string[]
  /**
   * What typing turns into: given a field and the text of the caret's line
   * up to the caret (its prefix included) just after something was typed,
   * the text to replace it with, or null to leave it. Applied as an undo
   * step of its own (see `applyInputRule`).
   */
  readonly inputRule?: (field: string, line: string) => string | null
  /**
   * Fields that Backspace at the start of stays in, rather than moving to
   * the end of the field before (the body, after the title block). Empty,
   * they don't hold it: Backspace moves on, and the field goes away.
   */
  readonly backspaceStops?: readonly string[]
  /**
   * Whether Enter carries a line prefix on to the line it starts (a list
   * item's, so Enter in an item starts another): Enter at the end of a
   * line with one starts a line with it too, and at the start of its text
   * puts one with it above; on an empty line with one, Enter takes it off
   * instead. (None is, unless given.)
   */
  readonly continued?: (prefix: string) => boolean
  /** A line prefix as text, for the clipboard (none unless given). */
  readonly prefixText?: (prefix: string) => string
  /** A `VOID` line as text, for the clipboard (none unless given). */
  readonly voidText?: string
  /**
   * Fields that stay when they're empty, rather than disappearing once the
   * selection leaves them: being there, empty, means something (a title
   * block's field the author added and hasn't filled in yet). Backspace in
   * one that's empty removes it. Adding or removing one is an edit.
   */
  readonly keepEmpty?: readonly string[]
  /**
   * Fields that are always there, empty or not, never removed (a
   * document's body: an empty one is one empty line to type in). Empty,
   * one is written as nothing.
   */
  readonly always?: readonly string[]
  /** Inline formatting, in the fields it's given for (none unless given). */
  readonly marks?: MarksSchema
  /**
   * What typing turns into inside a line that can be formatted (markdown
   * that becomes formatting, say): given a field, the caret's line (its
   * prefix included) and where the caret is in it, just after something
   * was typed, the line to replace it with and where the caret goes in
   * it, or null to leave it. Applied as an undo step of its own, after
   * `inputRule` (when that gives nothing).
   */
  readonly inlineRule?: (field: string, line: string, caret: number) => { line: string; caret: number } | null
}

/** Whether `field`'s lines can be formatted. */
export function isMarked(schema: Schema, field: string): boolean {
  return schema.marks?.fields.includes(field) ?? false
}

/** Whether `field` stays when it's empty (see `Schema.keepEmpty`). */
export function keepsEmpty(schema: Schema, field: string): boolean {
  return schema.keepEmpty?.includes(field) ?? false
}

/** Whether `field` is always there (see `Schema.always`). */
export function isAlways(schema: Schema, field: string): boolean {
  return schema.always?.includes(field) ?? false
}

export function caret(field: string, offset: number): Selection {
  const at = { field, offset }
  return { anchor: at, focus: at }
}

export function range(anchorField: string, anchorOffset: number, focusField: string, focusOffset: number): Selection {
  return {
    anchor: { field: anchorField, offset: anchorOffset },
    focus: { field: focusField, offset: focusOffset },
  }
}

/**
 * `position`, moved past the prefix of its line if it's before the end of
 * it, and out of a link's address (to the start of its opening mark).
 */
export function pastPrefix(fields: Fields, position: Position): Position {
  const text = fields[position.field]
  if (text === undefined) return position
  const start = lineStartOf(text, position.offset)
  const end = start + linePrefix(text, position.offset).length
  if (position.offset < end) return { field: position.field, offset: end }
  const open = text.lastIndexOf(LINK_OPEN, position.offset - 1)
  if (open >= start && position.offset > open && text.indexOf(LINK_TEXT, open) >= position.offset) {
    return { field: position.field, offset: open }
  }
  return position
}

/** `selection` with neither end before a line's prefix. */
export function pastPrefixes(fields: Fields, selection: Selection | null): Selection | null {
  if (!selection) return selection
  const anchor = pastPrefix(fields, selection.anchor)
  const focus = pastPrefix(fields, selection.focus)
  return anchor === selection.anchor && focus === selection.focus ? selection : { anchor, focus }
}

/** Negative when `a` comes before `b`, zero when they're the same. */
export function compare(schema: Schema, a: Position, b: Position): number {
  return schema.order.indexOf(a.field) - schema.order.indexOf(b.field) || a.offset - b.offset
}

export function samePosition(a: Position, b: Position): boolean {
  return a.field === b.field && a.offset === b.offset
}

export function sameSelection(a: Selection | null, b: Selection | null): boolean {
  if (a === null || b === null) return a === b
  return samePosition(a.anchor, b.anchor) && samePosition(a.focus, b.focus)
}

export function isCollapsed(selection: Selection): boolean {
  return samePosition(selection.anchor, selection.focus)
}

/** The selection's start and end, in reading order. */
export function ordered(schema: Schema, selection: Selection): { from: Position; to: Position } {
  const { anchor, focus } = selection
  return compare(schema, anchor, focus) <= 0 ? { from: anchor, to: focus } : { from: focus, to: anchor }
}

/** The fields present in the document, in reading order. */
export function presentFields(schema: Schema, fields: Fields): string[] {
  return schema.order.filter((f) => fields[f] !== undefined)
}

/**
 * Whether any field's text differs, counting an absent field as empty.
 * Marks that format nothing (an empty run, where a format was turned on
 * at the caret) aren't content.
 */
export function contentChanged(a: Fields, b: Fields): boolean {
  const keys = new Set([...Object.keys(a), ...Object.keys(b)])
  return [...keys].some((k) => {
    const x = a[k] ?? ''
    const y = b[k] ?? ''
    return x !== y && (!hasMarks(x + y) || canonicalText(x) !== canonicalText(y))
  })
}

/** `text` with each line's marks canonical (see `canonical`), and no empty runs. */
export function canonicalText(text: string): string {
  if (!hasMarks(text)) return text
  return text
    .split('\n')
    .map((line) => {
      if (!hasMarks(line)) return line
      const prefix = linePrefix(line, 0)
      return prefix + canonical(line.slice(prefix.length))
    })
    .join('\n')
}

/**
 * Whether the document changed: a field's text, or whether a field that
 * stays when empty is there at all.
 */
export function documentChanged(schema: Schema, a: Fields, b: Fields): boolean {
  return contentChanged(a, b) || (schema.keepEmpty ?? []).some((k) => (a[k] === undefined) !== (b[k] === undefined))
}

/**
 * The selected text, one line per field. A line's prefix, when the
 * selection takes it in, is written as the schema's `prefixText` says,
 * and a `VOID` line as its `voidText` says.
 */
export function selectedText(schema: Schema, fields: Fields, selection: Selection): string {
  const { from, to } = ordered(schema, selection)
  const first = schema.order.indexOf(from.field)
  const last = schema.order.indexOf(to.field)
  const lines: string[] = []
  for (let i = first; i <= last; i++) {
    const value = fields[schema.order[i]]
    if (value === undefined) continue
    let start = i === first ? from.offset : 0
    // A selection from the start of a line's text takes in its prefix.
    if (i === first && linePrefix(value, start) !== '' && pastPrefix(fields, { field: from.field, offset: lineStartOf(value, start) }).offset === start) {
      start = lineStartOf(value, start)
    }
    const text = value.slice(start, i === last ? to.offset : value.length)
    lines.push(text.replace(/(^|\n)([\uFDD0-\uFDEF]+)/g, (_, start: string, prefix: string) => start + (schema.prefixText?.(prefix) ?? '')))
  }
  return withoutMarks(withoutPrefixes(lines.join('\n'))).replaceAll(VOID, schema.voidText ?? '')
}
