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
 * A change is a list of replaced ranges (`diffs`): one for text on a
 * single line, and one per run of changed lines otherwise, so that a
 * change to one paragraph of a long text doesn't disturb what a variant
 * has in another. Positions inside text that a variant replaced move to
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

/**
 * The replaced ranges that turn `before` into `after`, in order, with
 * their offsets in `before`: one range per run of changed lines.
 */
export function diffs(before: string, after: string): Splice[] {
  if (before === after) return []
  if (!before.includes('\n') && !after.includes('\n')) return [diff(before, after)!]
  // Each line with its line break, so the pieces add up to the text.
  const a = before.split(/(?<=\n)/)
  const b = after.split(/(?<=\n)/)
  const match = matchLines(a, b)
  const splices: Splice[] = []
  let i = 0
  let offset = 0
  let removed = ''
  let inserted = ''
  const flush = () => {
    const start = offset - removed.length
    const inside = removed === '' ? -1 : inserted.indexOf(removed)
    const around = inserted === '' ? -1 : removed.indexOf(inserted)
    if (inside > 0 && inside + removed.length < inserted.length) {
      // Text added on both sides of what was there.
      splices.push({ start, removed: 0, inserted: inserted.slice(0, inside) })
      splices.push({ start: offset, removed: 0, inserted: inserted.slice(inside + removed.length) })
    } else if (around > 0 && around + inserted.length < removed.length) {
      // Text removed on both sides of what's left.
      splices.push({ start, removed: around, inserted: '' })
      splices.push({ start: start + around + inserted.length, removed: removed.length - around - inserted.length, inserted: '' })
    } else {
      const d = diff(removed, inserted)
      if (d) splices.push({ ...d, start: start + d.start })
    }
    removed = inserted = ''
  }
  for (let j = 0; j <= b.length; j++) {
    const upTo = j === b.length ? a.length : match[j]
    if (upTo < 0) {
      inserted += b[j]
      continue
    }
    for (; i < upTo; i++) {
      removed += a[i]
      offset += a[i].length
    }
    flush()
    if (j < b.length) offset += a[i++].length
  }
  return splices
}

/**
 * For each line of `b`, the index of the line of `a` it is, or -1 when
 * it's new: the longest run of lines the two share, in order.
 */
export function matchLines(a: readonly string[], b: readonly string[]): number[] {
  const match = new Array<number>(b.length).fill(-1)
  let start = 0
  while (start < a.length && start < b.length && a[start] === b[start]) {
    match[start] = start
    start++
  }
  let endA = a.length
  let endB = b.length
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) match[--endB] = --endA
  const n = endA - start
  const m = endB - start
  // Too much changed to compare line by line: treat it all as replaced.
  if (n === 0 || m === 0 || n * m > 4_000_000) return match
  // lengths[i * (m + 1) + j]: the longest shared run of a[start + i..] and b[start + j..].
  const lengths = new Uint32Array((n + 1) * (m + 1))
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lengths[i * (m + 1) + j] =
        a[start + i] === b[start + j]
          ? lengths[(i + 1) * (m + 1) + j + 1] + 1
          : Math.max(lengths[(i + 1) * (m + 1) + j], lengths[i * (m + 1) + j + 1])
    }
  }
  for (let i = 0, j = 0; i < n && j < m; ) {
    if (a[start + i] === b[start + j]) match[start + j++] = start + i++
    else if (lengths[(i + 1) * (m + 1) + j] >= lengths[i * (m + 1) + j + 1]) i++
    else j++
  }
  return match
}

function applySplice(text: string, splice: Splice): string {
  return text.slice(0, splice.start) + splice.inserted + text.slice(splice.start + splice.removed)
}

/**
 * Make `changes`, changes to `base`, to `variant` instead. Returns the new
 * text and the changes as made, in `variant`'s offsets.
 */
export function rebaseText(
  variant: string,
  base: string,
  changes: readonly Splice[],
): { text: string; splices: Splice[] } {
  const own = diffs(base, variant)
  /** Where an offset in `base` is in `variant`. */
  const map = (offset: number): number => {
    let shift = 0
    for (const o of own) {
      if (offset <= o.start) break
      if (offset < o.start + o.removed) return o.start + shift + o.inserted.length
      shift += o.inserted.length - o.removed
    }
    return offset + shift
  }
  const splices = changes.map((change) => {
    const start = map(change.start)
    return { start, removed: Math.max(0, map(change.start + change.removed) - start), inserted: change.inserted }
  })
  // Last first, so the offsets of the earlier ones still hold.
  const text = splices.reduceRight(applySplice, variant)
  return { text, splices }
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
    const changes = diffs(base[field] ?? '', source[field] ?? '')
    if (changes.length === 0) continue
    const { text, splices } = rebaseText(state.fields[field] ?? '', base[field] ?? '', changes)
    selection = selection && mapSelection(schema, selection, field, splices)
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
function mapSelection(schema: Schema, selection: Selection, field: string, splices: readonly Splice[]): Selection {
  const { from, to } = ordered(schema, selection)
  const collapsed = from.field === to.field && from.offset === to.offset
  const map = (p: Position): Position => {
    if (p.field !== field) return p
    const stick = !collapsed && p === from ? 'after' : 'before'
    // Last first, so each splice's offsets still hold when it's applied.
    const offset = splices.reduceRight((at, splice) => mapOffset(at, splice, stick), p.offset)
    return offset === p.offset ? p : { field, offset }
  }
  const anchor = map(selection.anchor)
  const focus = map(selection.focus)
  return anchor === selection.anchor && focus === selection.focus ? selection : { anchor, focus }
}
