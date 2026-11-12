/**
 * Which edits share an undo step, following common text editors: a run of
 * typing is one step, and so is a run of deleting, as long as each edit
 * continues where the last one left the caret. Switching between typing
 * and deleting, moving the caret, or pasting starts a new step. (So does a
 * pause; see `COALESCE_MS` in `history.ts`.)
 */

import type { TextRange } from './textRange.ts'

/**
 * The undo group for an edit, or undefined when the edit is a step of its
 * own.
 *
 * @param inputType the edit's `InputEvent.inputType`
 * @param before the selection just before the edit
 * @param lastAfter the selection the previous edit left, if any
 */
export function undoGroup(
  field: string,
  inputType: string,
  before: TextRange | null,
  lastAfter: TextRange | null,
): string | undefined {
  const kind = editKind(inputType)
  if (kind === undefined) return undefined
  if (lastAfter && (!before || before.start !== lastAfter.start || before.end !== lastAfter.end)) {
    return undefined
  }
  return `${field}:${kind}`
}

function editKind(inputType: string): 'type' | 'delete' | undefined {
  if (inputType === 'insertText') return 'type'
  if (inputType.startsWith('deleteContent') || inputType.startsWith('deleteWord')) return 'delete'
  return undefined
}
