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

type Node = { t: string; c?: unknown }

/**
 * A text field's plain text, as the title block shows it, or undefined
 * when the field is absent or isn't text.
 */
export function metaText(value: unknown): string | undefined {
  const m = value as Node | undefined
  if (m?.t === 'MetaString' && typeof m.c === 'string') return m.c
  if (m?.t === 'MetaInlines' && Array.isArray(m.c)) return inlinesText(m.c as Node[])
  return undefined
}

function inlinesText(inlines: Node[]): string {
  return inlines.map(inlineText).join('')
}

function inlineText(node: Node): string {
  const c = node.c as never
  switch (node.t) {
    case 'Str':
      return c
    case 'Space':
    case 'SoftBreak':
    case 'LineBreak':
      return ' '
    case 'Code':
    case 'Math':
      return (c as [unknown, string])[1]
    case 'Emph':
    case 'Strong':
    case 'Underline':
    case 'Strikethrough':
    case 'Superscript':
    case 'Subscript':
    case 'SmallCaps':
      return inlinesText(c)
    case 'Span':
    case 'Link':
    case 'Image':
    case 'Cite':
      return inlinesText((c as [unknown, Node[]])[1])
    case 'Quoted': {
      const [kind, inner] = c as [{ t: string }, Node[]]
      const q = kind.t === 'SingleQuote' ? ['‘', '’'] : ['“', '”']
      return q[0] + inlinesText(inner) + q[1]
    }
    default:
      return ''
  }
}
