/**
 * The document the editor edits, seen as fields. The editor never touches
 * the document any other way, so this is everything an editing surface
 * needs from a document to stay in sync with it:
 *
 * - read what the document says now (`get`),
 * - write an edit and read back what the document made of it (`set`),
 * - hear when the document changes under it (`subscribe`).
 *
 * The document is the truth. It may read an edit back differently from
 * what was written (a qmd title drops a trailing space, turns `'` into
 * `’`), and it changes without the editor's doing (a collaborator's edit).
 * The editor handles both the same way: see `rebase.ts`.
 */

import type { Fields } from './model.ts'

/** New text for some fields; `undefined` removes a field. */
export type FieldChanges = Readonly<Record<string, string | undefined>>

export interface FieldSource {
  /** The document's fields now. Empty fields are absent. */
  get(): Fields
  /** Whether `set` works now (the document can be read). */
  writable(): boolean
  /**
   * Write `changes` to the document, leaving every other field alone. On
   * return, `get` gives the document's fields as it reads them back.
   */
  set(changes: FieldChanges): void
  /**
   * Call `listener` after every change to the fields or to `writable`,
   * the editor's own writes included.
   */
  subscribe(listener: () => void): () => void
}
