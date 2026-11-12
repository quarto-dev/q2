/**
 * The editor's model: a fixed sequence of plain-text fields, some of which
 * may be absent, and a selection over them.
 *
 * Offsets count UTF-16 code units, like the DOM's.
 */

/**
 * A line of a multi-line field that holds only this character stands for
 * something the editor shows and can't edit (a heading or a table among
 * the paragraphs of a body). No edit removes it, joins it to another line
 * or puts text on its line, and it can't be typed.
 */
export const ATOM = '\uFFFC'

/** Each field's text, or undefined when the field is absent. */
export type Fields = Readonly<Record<string, string | undefined>>

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
   * Fields that stay when they're empty, rather than disappearing once the
   * selection leaves them: being there, empty, means something (a title
   * block's field the author added and hasn't filled in yet). Backspace in
   * one that's empty removes it. Adding or removing one is an edit.
   */
  readonly keepEmpty?: readonly string[]
}

/** Whether `field` stays when it's empty (see `Schema.keepEmpty`). */
export function keepsEmpty(schema: Schema, field: string): boolean {
  return schema.keepEmpty?.includes(field) ?? false
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

/** Whether any field's text differs, counting an absent field as empty. */
export function contentChanged(a: Fields, b: Fields): boolean {
  const keys = new Set([...Object.keys(a), ...Object.keys(b)])
  return [...keys].some((k) => (a[k] ?? '') !== (b[k] ?? ''))
}

/**
 * Whether the document changed: a field's text, or whether a field that
 * stays when empty is there at all.
 */
export function documentChanged(schema: Schema, a: Fields, b: Fields): boolean {
  return contentChanged(a, b) || (schema.keepEmpty ?? []).some((k) => (a[k] === undefined) !== (b[k] === undefined))
}

/** The selected text, one line per field. */
export function selectedText(schema: Schema, fields: Fields, selection: Selection): string {
  const { from, to } = ordered(schema, selection)
  const first = schema.order.indexOf(from.field)
  const last = schema.order.indexOf(to.field)
  const lines: string[] = []
  for (let i = first; i <= last; i++) {
    const value = fields[schema.order[i]]
    if (value === undefined) continue
    lines.push(value.slice(i === first ? from.offset : 0, i === last ? to.offset : value.length))
  }
  return lines.join('\n')
}
