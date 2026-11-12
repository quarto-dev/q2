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
import { contentChanged, sameSelection, type Fields, type Schema, type Selection, type State } from './model.ts'
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
}

export function apply(editor: Editor, next: State, options: ApplyOptions = {}): Editor {
  const { inputType = '', before = editor.present.selection, now } = options
  const present = editor.present
  if (next === present) return editor

  if (!contentChanged(present.fields, next.fields)) {
    // A selection change. Moving the caret ends the open undo step.
    const moved = !sameSelection(present.selection, next.selection)
    const h = replacePresent(editor, next)
    return moved ? endGroup(h) : h
  }

  const h = sameSelection(before, present.selection) ? editor : endGroup(editor)
  const field = before?.focus.field ?? next.selection?.focus.field ?? ''
  return record(h, next, {
    group: undoGroup(field, inputType),
    now,
    before: { ...present, selection: before },
  })
}

/** What an edit from `before` to `after` changed, to write to the document. */
export function fieldChanges(schema: Schema, before: Fields, after: Fields): FieldChanges {
  const changes: Record<string, string | undefined> = {}
  for (const field of schema.order) {
    if ((before[field] ?? '') !== (after[field] ?? '')) changes[field] = after[field] || undefined
  }
  return changes
}

/**
 * Show the document's text (`base`) in every field the selection isn't
 * in. While the caret is in a field, the field shows what was typed, even
 * where the document reads it differently (a trailing space it drops);
 * once the caret leaves, the field shows what the document says.
 */
export function settle(schema: Schema, editor: Editor, base: Fields): Editor {
  const { fields, selection } = editor.present
  let next: Record<string, string | undefined> | null = null
  for (const field of schema.order) {
    if (selection && (selection.anchor.field === field || selection.focus.field === field)) continue
    if ((fields[field] ?? '') === (base[field] ?? '')) continue
    next ??= { ...fields }
    if (base[field]) next[field] = base[field]
    else delete next[field]
  }
  return next ? replacePresent(editor, { fields: next, selection }) : editor
}
