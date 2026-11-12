/**
 * Which edits share an undo step, following common text editors: a run of
 * typing in one field is one step, and so is a run of deleting. Switching
 * between typing and deleting, or pasting, starts a new step. (So do a
 * caret jump and a pause; see `editor.ts` and `COALESCE_MS` in
 * `history.ts`.)
 */

/**
 * The undo group for an edit, or undefined when the edit is a step of its
 * own.
 *
 * @param field the field the edit started in
 * @param inputType the edit's `InputEvent.inputType`
 */
export function undoGroup(field: string, inputType: string): string | undefined {
  const kind = editKind(inputType)
  return kind === undefined ? undefined : `${field}:${kind}`
}

function editKind(inputType: string): 'type' | 'delete' | undefined {
  if (inputType === 'insertText') return 'type'
  if (inputType.startsWith('deleteContent') || inputType.startsWith('deleteWord')) return 'delete'
  return undefined
}
