/**
 * Carrying the editor's state over a change to the document that the
 * editor didn't make: a collaborator's edit.
 *
 * The editor's text for a field can differ from the document's: the
 * present state holds what was typed (a trailing space the document
 * doesn't keep), and the undo history holds older and newer texts. Each is
 * a variant of `base`, the document's text before the change. The change
 * is made to every variant, so the present keeps what was typed around it,
 * and undo and redo still change only what this editor changed.
 *
 * A change is one replaced range (`diff`), found by trimming the common
 * prefix and suffix. Positions inside text that a variant replaced move to
 * the end of the variant's replacement.
 */

import { ordered, type Fields, type Position, type Schema, type Selection, type State } from './model.ts'
import type { History } from './history.ts'

/** Replace `removed` characters at `start` with `inserted`. */
export interface Splice {
  readonly start: number
  readonly removed: number
  readonly inserted: string
}

/** The single replaced range that turns `before` into `after`, or null if equal. */
export function diff(before: string, after: string): Splice | null {
  if (before === after) return null
  const max = Math.min(before.length, after.length)
  let start = 0
  while (start < max && before[start] === after[start]) start++
  let end = 0
  while (end < max - start && before[before.length - 1 - end] === after[after.length - 1 - end]) end++
  return {
    start,
    removed: before.length - start - end,
    inserted: after.slice(start, after.length - end),
  }
}

function applySplice(text: string, splice: Splice): string {
  return text.slice(0, splice.start) + splice.inserted + text.slice(splice.start + splice.removed)
}

/**
 * Make `change`, a change to `base`, to `variant` instead. Returns the new
 * text and the change as made, in `variant`'s offsets.
 */
export function rebaseText(variant: string, base: string, change: Splice): { text: string; splice: Splice } {
  const own = diff(base, variant)
  let splice = change
  if (own) {
    const shift = own.inserted.length - own.removed
    const map = (offset: number) =>
      offset <= own.start
        ? offset
        : offset >= own.start + own.removed
          ? offset + shift
          : own.start + own.inserted.length
    const start = map(change.start)
    splice = { start, removed: map(change.start + change.removed) - start, inserted: change.inserted }
  }
  return { text: applySplice(variant, splice), splice }
}

/**
 * Where `offset` is after `splice`. Text inserted exactly at `offset` ends
 * up after it (`stick: 'before'`) or before it (`'after'`). An offset
 * inside replaced text moves to the end of the replacement.
 */
export function mapOffset(offset: number, splice: Splice, stick: 'before' | 'after' = 'before'): number {
  const end = splice.start + splice.removed
  if (offset < splice.start) return offset
  if (offset > end) return offset + splice.inserted.length - splice.removed
  if (splice.removed === 0) return stick === 'before' ? offset : offset + splice.inserted.length
  if (offset === splice.start) return offset
  return splice.start + splice.inserted.length
}

/**
 * `state` after the document's fields changed from `base` to `source`.
 * A field emptied by the change stays (empty) only while it holds an end
 * of the selection.
 */
export function rebaseState(schema: Schema, state: State, base: Fields, source: Fields): State {
  let fields: Record<string, string | undefined> | null = null
  let selection = state.selection
  for (const field of schema.order) {
    const change = diff(base[field] ?? '', source[field] ?? '')
    if (!change) continue
    const { text, splice } = rebaseText(state.fields[field] ?? '', base[field] ?? '', change)
    selection = selection && mapSelection(schema, selection, field, splice)
    const held = selection !== null && (selection.anchor.field === field || selection.focus.field === field)
    fields ??= { ...state.fields }
    if (text !== '' || held) fields[field] = text
    else delete fields[field]
  }
  return fields ? { fields, selection } : state
}

/** Every state of `history` after the document's fields changed from `base` to `source`. */
export function rebaseHistory(schema: Schema, history: History<State>, base: Fields, source: Fields): History<State> {
  const present = rebaseState(schema, history.present, base, source)
  const past = history.past.map((s) => rebaseState(schema, s, base, source))
  const future = history.future.map((s) => rebaseState(schema, s, base, source))
  const same =
    present === history.present &&
    past.every((s, i) => s === history.past[i]) &&
    future.every((s, i) => s === history.future[i])
  return same ? history : { ...history, past, present, future }
}

/**
 * A caret stays before text inserted at it. A selection doesn't grow to
 * take in text inserted at either of its ends.
 */
function mapSelection(schema: Schema, selection: Selection, field: string, splice: Splice): Selection {
  const { from, to } = ordered(schema, selection)
  const collapsed = from.field === to.field && from.offset === to.offset
  const map = (p: Position): Position => {
    if (p.field !== field) return p
    const stick = !collapsed && p === from ? 'after' : 'before'
    const offset = mapOffset(p.offset, splice, stick)
    return offset === p.offset ? p : { field, offset }
  }
  const anchor = map(selection.anchor)
  const focus = map(selection.focus)
  return anchor === selection.anchor && focus === selection.focus ? selection : { anchor, focus }
}
