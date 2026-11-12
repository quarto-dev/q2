/**
 * Edits to a document's raw (untransformed) metadata, as plain values.
 */

import type { RustQmdJson } from '@quarto/pandoc-types'

export type Meta = RustQmdJson['meta']

/**
 * Set a top-level text field, or remove it when `text` is undefined.
 *
 * Writes a `MetaString`, so inline markup in the old value (emphasis in a
 * title) is dropped. The title block renders fields as plain text anyway.
 */
export function setMetaText(meta: Meta, key: string, text: string | undefined): Meta {
  const next = { ...meta }
  if (text === undefined) delete next[key]
  else next[key] = { t: 'MetaString', c: text } as Meta[string]
  return next
}
