/**
 * Pure edits of the editor's state. Each takes the state and returns the
 * next one (the same object when nothing changes).
 *
 * The rules, which hold however an edit crosses fields:
 * - Fields are never joined or split. Deleting or typing over a selection
 *   that spans fields trims each one; typed text goes into the first.
 * - Enter inserts a line break in a multi-line field. Elsewhere it moves to
 *   the start of the next field, adding it if it's absent and addable.
 * - Backspace at the start of a field moves to the end of the previous one
 *   without deleting (and Delete at the end moves to the next one).
 * - An empty field disappears once the selection leaves it.
 */

import {
  caret,
  isCollapsed,
  ordered,
  presentFields,
  sameSelection,
  type Fields,
  type Position,
  type Schema,
  type Selection,
  type State,
} from './model.ts'

/** Replace the selection with `text`. */
export function insertText(schema: Schema, state: State, text: string): State {
  if (!state.selection) return state
  const { from, to } = ordered(schema, state.selection)
  return replace(schema, state, from, to, text)
}

/**
 * Backspace. Deletes the selection, or else `target` (the range the
 * browser would delete, clipped to the caret's field), or else the
 * grapheme before the caret.
 */
export function deleteBackward(schema: Schema, state: State, target?: { from: Position; to: Position }): State {
  return deleteDirection(schema, state, 'backward', target)
}

/** Delete (forward). See `deleteBackward`. */
export function deleteForward(schema: Schema, state: State, target?: { from: Position; to: Position }): State {
  return deleteDirection(schema, state, 'forward', target)
}

/**
 * Paste: replace the selection with `text`, with its line breaks kept in a
 * multi-line field and flattened to single spaces elsewhere.
 */
export function paste(schema: Schema, state: State, text: string): State {
  if (!state.selection) return state
  const field = ordered(schema, state.selection).from.field
  const pasted = schema.multiline?.includes(field)
    ? text.replace(/\r\n?/g, '\n')
    : text.replace(/\s*[\r\n]+\s*/g, ' ')
  return insertText(schema, state, pasted)
}

/**
 * Enter: a line break in a multi-line field. Elsewhere, delete the
 * selection and move to the start of the next field.
 */
export function enter(schema: Schema, state: State): State {
  if (!state.selection) return state
  const field = ordered(schema, state.selection).from.field
  if (schema.multiline?.includes(field)) return insertText(schema, state, '\n')
  const next = schema.order
    .slice(schema.order.indexOf(field) + 1)
    .find((f) => state.fields[f] !== undefined || schema.addable?.includes(f))
  if (next === undefined) return state
  const cleared = insertText(schema, state, '')
  const fields = cleared.fields[next] === undefined ? { ...cleared.fields, [next]: '' } : cleared.fields
  return select(schema, { fields, selection: cleared.selection }, caret(next, 0))
}

/** Move the selection, removing any empty field it leaves. */
export function select(schema: Schema, state: State, selection: Selection | null): State {
  const fields = removeEmpty(schema, state.fields, selection)
  if (fields === state.fields && sameSelection(selection, state.selection)) return state
  return { fields, selection }
}

/**
 * Select all: the first press selects the field with the caret, the next
 * every field.
 */
export function selectAllStep(schema: Schema, state: State): State {
  const { selection, fields } = state
  if (!selection) return state
  const { from, to } = ordered(schema, selection)
  const length = (field: string) => (fields[field] ?? '').length
  const wholeField = from.field === to.field && from.offset === 0 && to.offset === length(to.field)
  if (!wholeField && from.field === to.field) {
    return select(schema, state, {
      anchor: { field: from.field, offset: 0 },
      focus: { field: from.field, offset: length(from.field) },
    })
  }
  const present = presentFields(schema, fields)
  const last = present[present.length - 1]
  return select(schema, state, {
    anchor: { field: present[0], offset: 0 },
    focus: { field: last, offset: length(last) },
  })
}

function deleteDirection(
  schema: Schema,
  state: State,
  direction: 'backward' | 'forward',
  target?: { from: Position; to: Position },
): State {
  const { selection, fields } = state
  if (!selection) return state
  if (!isCollapsed(selection)) return insertText(schema, state, '')

  const at = selection.focus
  const text = fields[at.field] ?? ''
  const backward = direction === 'backward'
  if (backward ? at.offset === 0 : at.offset === text.length) {
    // At the field's edge: move to the neighbouring field, never joining.
    const present = presentFields(schema, fields)
    const neighbour = present[present.indexOf(at.field) + (backward ? -1 : 1)]
    if (neighbour === undefined) return state
    return select(schema, state, caret(neighbour, backward ? (fields[neighbour] ?? '').length : 0))
  }

  let start: number
  let end: number
  if (target) {
    start = target.from.field === at.field ? target.from.offset : 0
    end = target.to.field === at.field ? target.to.offset : text.length
  } else if (backward) {
    start = at.offset - graphemeBefore(text, at.offset)
    end = at.offset
  } else {
    start = at.offset
    end = at.offset + graphemeAfter(text, at.offset)
  }
  if (start >= end) return state
  return replace(schema, state, { field: at.field, offset: start }, { field: at.field, offset: end }, '')
}

/**
 * Replace the text between `from` and `to` (in reading order), keeping
 * every field: the first keeps what precedes `from`, plus `text`; the last
 * keeps what follows `to`; fields in between are emptied.
 */
function replace(schema: Schema, state: State, from: Position, to: Position, text: string): State {
  const fields: Record<string, string | undefined> = { ...state.fields }
  const first = schema.order.indexOf(from.field)
  const last = schema.order.indexOf(to.field)
  for (let i = first; i <= last; i++) {
    const field = schema.order[i]
    const value = fields[field]
    if (value === undefined) continue
    const keepBefore = i === first ? value.slice(0, from.offset) + text : ''
    const keepAfter = i === last ? value.slice(to.offset) : ''
    fields[field] = i === first && i === last ? keepBefore + keepAfter : i === first ? keepBefore : keepAfter
  }
  return select(schema, { fields, selection: state.selection }, caret(from.field, from.offset + text.length))
}

/** Remove the empty fields that don't hold an end of `selection`. */
function removeEmpty(schema: Schema, fields: Fields, selection: Selection | null): Fields {
  const keep = new Set(selection ? [selection.anchor.field, selection.focus.field] : [])
  const empty = schema.order.filter((f) => fields[f] === '' && !keep.has(f))
  if (empty.length === 0) return fields
  const next = { ...fields }
  for (const f of empty) delete next[f]
  return next
}

const graphemes = new Intl.Segmenter(undefined, { granularity: 'grapheme' })

function graphemeBefore(text: string, offset: number): number {
  let length = 0
  for (const { index, segment } of graphemes.segment(text.slice(0, offset))) {
    if (index + segment.length === offset) length = segment.length
  }
  return length
}

function graphemeAfter(text: string, offset: number): number {
  const first = graphemes.segment(text.slice(offset))[Symbol.iterator]().next()
  return first.done ? 0 : first.value.segment.length
}
