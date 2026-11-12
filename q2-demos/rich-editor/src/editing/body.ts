/**
 * A document's body as the text of one multi-line field: a line per
 * block. A paragraph of plain text is its text. Every other block (a
 * heading, a paragraph with formatting, a list) is a line that can't be
 * edited (`ATOM`), and comes back from an edit as the block it was.
 */

import { ATOM } from '../engine/model.ts'
import { matchLines } from '../engine/rebase.ts'

export type Node = { t: string; c?: unknown }

/**
 * The text of a paragraph that can be edited as plain text, or undefined
 * for any other block.
 */
export function plainParagraph(block: Node): string | undefined {
  if (block.t !== 'Para' || !Array.isArray(block.c)) return undefined
  const text = plainText(block.c as Node[])
  return text === undefined || text === '' || text.includes(ATOM) ? undefined : text
}

/** The blocks that can't be edited, in order: what the body's atoms stand for. */
export function readOnlyBlocks(blocks: readonly Node[]): Node[] {
  return blocks.filter((block) => plainParagraph(block) === undefined)
}

/** The body's text: a line per block. */
export function bodyText(blocks: readonly Node[]): string {
  return blocks.map((block) => plainParagraph(block) ?? ATOM).join('\n')
}

/**
 * The blocks of a body edited to read `text`. A line that reads as its
 * block did is that block, untouched; any other line of text is a new
 * paragraph. Null when `text` doesn't have the body's read-only blocks,
 * which no edit should bring about.
 */
export function editBody(blocks: readonly Node[], text: string): Node[] | null {
  const lines = text.split('\n').map(kept).filter((line) => line !== '')
  const before = blocks.map((block) => plainParagraph(block) ?? ATOM)
  const readOnly = readOnlyBlocks(blocks)
  if (lines.filter((line) => line === ATOM).length !== readOnly.length) return null
  const match = matchLines(before, lines)
  let atoms = 0
  return lines.map((line, i) =>
    line === ATOM ? readOnly[atoms++] : match[i] >= 0 ? blocks[match[i]] : { t: 'Para', c: words(line) },
  )
}

/** The part of a line that qmd keeps: no spaces at its ends, and runs of spaces as one. */
export function kept(line: string): string {
  return line.trim().replace(/\s+/g, ' ')
}

/** A line of plain text as inlines: words and the spaces between them. */
export function words(line: string): Node[] {
  return line
    .split(' ')
    .flatMap((word, i): Node[] => (i === 0 ? [{ t: 'Str', c: word }] : [{ t: 'Space' }, { t: 'Str', c: word }]))
}

/** The text of inlines that are only text, or undefined when there's anything else. */
function plainText(inlines: Node[]): string | undefined {
  let text = ''
  for (const inline of inlines) {
    if (inline.t === 'Str' && typeof inline.c === 'string') text += inline.c
    else if (inline.t === 'Space' || inline.t === 'SoftBreak') text += ' '
    else if (inline.t === 'Quoted') {
      const [kind, inner] = inline.c as [{ t: string }, Node[]]
      const quoted = plainText(inner)
      if (quoted === undefined) return undefined
      text += kind.t === 'SingleQuote' ? `‘${quoted}’` : `“${quoted}”`
    } else return undefined
  }
  return text
}
