/**
 * The editor's state over time: the current state plus an undo history,
 * with one rule for which changes are undo steps.
 *
 * A change to any field's text is an edit, and becomes (or extends) an
 * undo step. Anything else — moving the selection, adding an empty field
 * or removing one — replaces the present state without a step of its own,
 * so undo goes straight back to the last edit, wherever the caret is now.
 */

import {
  createHistory,
  endGroup,
  record,
  replacePresent,
  type History,
} from './history.ts'
import {
  contentChanged,
  documentChanged,
  keepsEmpty,
  sameSelection,
  type Fields,
  type Position,
  type Schema,
  type Selection,
  type State,
} from './model.ts'
import { diffs } from './rebase.ts'
import type { FieldChanges } from './source.ts'
import { undoGroup } from './undoGroup.ts'

export { redo, undo } from './history.ts'

export type Editor = History<State>

export function createEditor(fields: Fields): Editor {
  return createHistory({ fields, selection: null })
}

export interface ApplyOptions {
  /** The `InputEvent.inputType` of the edit that produced `next`. */
  inputType?: string
  /**
   * The selection just before the edit, read from the page. Undo returns
   * to it. When it differs from the selection the editor last knew, the
   * caret jumped, which starts a new undo step.
   */
  before?: Selection | null
  /** The current time in ms (defaults to `Date.now()`). */
  now?: number
  /**
   * The document's schema, which says which fields stay when empty: adding
   * or removing one of those is an edit too.
   */
  schema?: Schema
}

export function apply(editor: Editor, next: State, options: ApplyOptions = {}): Editor {
  const { inputType = '', before = editor.present.selection, now, schema = NO_SCHEMA } = options
  const present = editor.present
  if (next === present) return editor

  if (!documentChanged(schema, present.fields, next.fields)) {
    // A selection change. Moving the caret ends the open undo step.
    const moved = !sameSelection(present.selection, next.selection)
    const h = replacePresent(editor, next)
    return moved ? endGroup(h) : h
  }

  // Adding or removing a field is a step of its own, never merged with
  // the typing or deleting around it.
  const addsOrRemoves = !contentChanged(present.fields, next.fields) || presenceChanged(schema, present.fields, next.fields)
  const h = sameSelection(before, present.selection) && !addsOrRemoves ? editor : endGroup(editor)
  const field = before?.focus.field ?? next.selection?.focus.field ?? ''
  return record(h, next, {
    group: addsOrRemoves ? undefined : undoGroup(field, inputType),
    now,
    before: { ...present, selection: before },
  })
}

/** Whether a field that stays when empty was added or removed. */
function presenceChanged(schema: Schema, a: Fields, b: Fields): boolean {
  return (schema.keepEmpty ?? []).some((k) => (a[k] === undefined) !== (b[k] === undefined))
}

const NO_SCHEMA: Schema = { order: [] }

/**
 * What an edit from `before` to `after` changed, to write to the document.
 * An emptied field is removed, unless it's one that stays when empty: that
 * one is written empty, and is removed only once it's absent.
 */
export function fieldChanges(schema: Schema, before: Fields, after: Fields): FieldChanges {
  const changes: Record<string, string | undefined> = {}
  for (const field of schema.order) {
    if (keepsEmpty(schema, field)) {
      if (before[field] !== after[field]) changes[field] = after[field]
    } else if ((before[field] ?? '') !== (after[field] ?? '')) changes[field] = after[field] || undefined
  }
  return changes
}

/**
 * Show the document's text (`base`) wherever the selection isn't: in every
 * field it isn't in, and in every line it isn't on of a multi-line field
 * it is in. While the caret is in a field (or on a line), the field shows
 * what was typed, even where the document reads it differently (a
 * trailing space it drops, an empty line); once the caret leaves, it
 * shows what the document says.
 */
export function settle(schema: Schema, editor: Editor, base: Fields): Editor {
  const { fields } = editor.present
  let selection = editor.present.selection
  let next: Record<string, string | undefined> | null = null
  for (const field of schema.order) {
    const kept = keepsEmpty(schema, field)
    const was = fields[field] ?? ''
    if (kept ? fields[field] === base[field] : was === (base[field] ?? '')) continue
    const ends = selection ? [selection.anchor, selection.focus].filter((p) => p.field === field) : []
    if (ends.length === 0) {
      next ??= { ...fields }
      // A field that stays when empty is there exactly when the document has it.
      if (kept ? base[field] !== undefined : base[field]) next[field] = base[field]
      else delete next[field]
    } else if (schema.multiline?.includes(field)) {
      const settled = settleLines(was, base[field] ?? '', ends.map((p) => p.offset))
      if (settled.text === was) continue
      next ??= { ...fields }
      next[field] = settled.text
      const moved = (p: Position): Position =>
        p.field === field ? { field, offset: settled.offsets[ends.indexOf(p)] } : p
      selection = selection && { anchor: moved(selection.anchor), focus: moved(selection.focus) }
    }
  }
  return next ? replacePresent(editor, { fields: next, selection }) : editor
}

/**
 * `variant` with the document's text (`base`) on every line that none of
 * `offsets` is on, and where each of the offsets is in it.
 */
function settleLines(variant: string, base: string, offsets: number[]): { text: string; offsets: number[] } {
  const lines = offsets.map((offset) => {
    const end = variant.indexOf('\n', offset)
    return { start: variant.lastIndexOf('\n', offset - 1) + 1, end: end < 0 ? variant.length : end }
  })
  // Each difference of `variant` from `base`, and where it starts in `variant`.
  let shift = 0
  const own = diffs(base, variant).map((d) => {
    const start = d.start + shift
    shift += d.inserted.length - d.removed
    return { ...d, at: start }
  })
  let text = base
  const moved = [...offsets]
  // Last first, so the offsets of the earlier differences still hold.
  for (const d of own.reverse()) {
    const end = d.at + d.inserted.length
    // A line break that ends a difference belongs to the line before it,
    // unless it's what makes the empty line the caret is on.
    const held = lines.some(
      (line) =>
        d.at <= line.end &&
        (end > line.start || (end === line.start && (!d.inserted.endsWith('\n') || line.start === line.end))),
    )
    if (held) text = text.slice(0, d.start) + d.inserted + text.slice(d.start + d.removed)
    else offsets.forEach((offset, i) => offset >= end && (moved[i] -= d.inserted.length - d.removed))
  }
  return { text, offsets: moved }
}
