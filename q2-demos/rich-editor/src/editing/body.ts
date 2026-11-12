/**
 * A document's body as the text of one multi-line field: a line per
 * block. A paragraph of plain text is its text. Every other block (a
 * heading, a paragraph with formatting, a list) is a line that can't be
 * edited (`ATOM`), and comes back from an edit as the block it was.
 *
 * A paragraph that the source wraps over several lines reads as one line,
 * and an edit keeps its line breaks wherever the text around them stays.
 */

import { ATOM } from '../engine/model.ts'
import { diff, matchLines } from '../engine/rebase.ts'

export type Node = { t: string; c?: unknown }

/**
 * The text of a paragraph that can be edited as plain text, or undefined
 * for any other block.
 */
export function plainParagraph(block: Node): string | undefined {
  if (block.t !== 'Para' || !Array.isArray(block.c)) return undefined
  const text = plainText(block.c as Node[], ' ')
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
 * paragraph, wrapped in the source where the text it keeps was. Null when
 * `text` doesn't have the body's read-only blocks, which no edit should
 * bring about.
 */
export function editBody(blocks: readonly Node[], text: string): Node[] | null {
  const lines = text.split('\n').map(kept).filter((line) => line !== '')
  const before = blocks.map((block) => plainParagraph(block) ?? ATOM)
  const readOnly = readOnlyBlocks(blocks)
  if (lines.filter((line) => line === ATOM).length !== readOnly.length) return null
  const match = matchLines(before, lines)
  const wrapped = keptWraps(blocks, before.join('\n'), lines.join('\n'))
  let atoms = 0
  let start = 0
  return lines.map((line, i) => {
    const lineStart = start
    start += line.length + 1
    if (line === ATOM) return readOnly[atoms++]
    if (match[i] >= 0) return blocks[match[i]]
    return { t: 'Para', c: words(line, (offset) => wrapped(lineStart + offset)) }
  })
}

/**
 * Whether the space at an offset of the body's edited text (`after`) is a
 * line break in the source: it is when it's in the text before or after
 * what the edit changed, and was one there.
 */
function keptWraps(blocks: readonly Node[], before: string, after: string): (offset: number) => boolean {
  const change = diff(before, after)
  if (!change) return () => false
  // The body's text with a line break where the source wraps a paragraph.
  const source = blocks
    .map((block) => (plainParagraph(block) === undefined ? ATOM : plainText(block.c as Node[], '\n')))
    .join('\n')
  const changeEnd = change.start + change.inserted.length
  const moved = before.length - after.length
  return (offset) => {
    const was = offset < change.start ? offset : offset >= changeEnd ? offset + moved : -1
    return before[was] === ' ' && source[was] === '\n'
  }
}

/** The part of a line that qmd keeps: no spaces at its ends, and runs of spaces as one. */
export function kept(line: string): string {
  return line.trim().replace(/\s+/g, ' ')
}

/**
 * A line of plain text as inlines: words and the spaces between them. A
 * space is a line break in the source when `wrapped` says so of its offset.
 */
export function words(line: string, wrapped: (offset: number) => boolean = () => false): Node[] {
  const inlines: Node[] = []
  let offset = 0
  for (const word of line.split(' ')) {
    if (offset > 0) inlines.push({ t: wrapped(offset - 1) ? 'SoftBreak' : 'Space' })
    inlines.push({ t: 'Str', c: word })
    offset += word.length + 1
  }
  return inlines
}

/**
 * The text of inlines that are only text, or undefined when there's
 * anything else. `softBreak` is what a line break in the source reads as.
 */
function plainText(inlines: Node[], softBreak: string): string | undefined {
  let text = ''
  for (const inline of inlines) {
    if (inline.t === 'Str' && typeof inline.c === 'string') text += inline.c
    else if (inline.t === 'Space') text += ' '
    else if (inline.t === 'SoftBreak') text += softBreak
    else if (inline.t === 'Quoted') {
      const [kind, inner] = inline.c as [{ t: string }, Node[]]
      const quoted = plainText(inner, softBreak)
      if (quoted === undefined) return undefined
      text += kind.t === 'SingleQuote' ? `‘${quoted}’` : `“${quoted}”`
    } else return undefined
  }
  return text
}
