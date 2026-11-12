/**
 * A document's body as the text of one multi-line field: a line per
 * block. A paragraph of plain text is its text, and a heading of plain
 * text is its text after a prefix for its level (`headingPrefix`). Every
 * other block (a paragraph or heading with formatting, a list) is a line
 * that can't be edited (`ATOM`), and comes back from an edit as the block
 * it was.
 *
 * An empty line is an empty paragraph, which qmd has no syntax for: it's
 * written as a paragraph of one non-breaking space, which takes up a line
 * when rendered, and a paragraph of only that reads as an empty line.
 *
 * A paragraph that the source wraps over several lines reads as one line,
 * and an edit keeps its line breaks wherever the text around them stays.
 */

import { ATOM, isPrefix, withoutPrefixes } from '../engine/model.ts'
import { diff, matchLines } from '../engine/rebase.ts'

export type Node = { t: string; c?: unknown }

/** What an empty paragraph holds. */
const NBSP = '\u00a0'

type Attr = [string, string[], [string, string][]]

/** The line prefix of a heading of `level` (1 to 6). */
export function headingPrefix(level: number): string {
  return String.fromCharCode(0xfdd0 + level)
}

/** The heading level of a line of the body's text, or 0 for a paragraph. */
export function headingLevel(line: string): number {
  const level = line.charCodeAt(0) - 0xfdd0
  return level >= 1 && level <= 6 ? level : 0
}

/** A line's text, without its prefix. */
function lineText(line: string): string {
  let start = 0
  while (start < line.length && isPrefix(line[start])) start++
  return line.slice(start)
}

/**
 * A block's line of the body's text, or undefined for a block that can't
 * be edited.
 */
export function lineOf(block: Node): string | undefined {
  if (block.t === 'Para') return plainParagraph(block)
  if (block.t !== 'Header' || !Array.isArray(block.c)) return undefined
  const [level, , inlines] = block.c as [number, Attr, Node[]]
  const text = plainText(inlines, ' ')
  if (text === undefined || !editable(text) || level < 1 || level > 6) return undefined
  return headingPrefix(level) + text
}

/** Whether text can be a line's: it holds no character the editor gives a meaning. */
function editable(text: string): boolean {
  return !text.includes(ATOM) && withoutPrefixes(text) === text
}

/**
 * The text of a paragraph that can be edited as plain text (none, for an
 * empty paragraph), or undefined for any other block.
 */
export function plainParagraph(block: Node): string | undefined {
  if (block.t !== 'Para' || !Array.isArray(block.c)) return undefined
  const text = plainText(block.c as Node[], ' ')
  if (text === NBSP) return ''
  return text === undefined || text === '' || !editable(text) ? undefined : text
}

/** The blocks that can't be edited, in order: what the body's atoms stand for. */
export function readOnlyBlocks(blocks: readonly Node[]): Node[] {
  return blocks.filter((block) => lineOf(block) === undefined)
}

/**
 * What typing turns into in the body: one to six `#` and a space at the
 * start of a paragraph make it a heading of that level, as in qmd.
 */
export function bodyInputRule(line: string): string | null {
  const match = /^(#{1,6}) $/.exec(line)
  return match ? headingPrefix(match[1].length) : null
}

/** A line's prefix as qmd, for the clipboard. */
export function prefixMarkdown(prefix: string): string {
  const level = headingLevel(prefix)
  return level > 0 ? `${'#'.repeat(level)} ` : ''
}

/**
 * For each line of `text`, a version of the body's text (`blocks`), the
 * block it shows, when it reads as one of them.
 */
export function blocksOfLines(blocks: readonly Node[], text: string): (Node | undefined)[] {
  const before = blocks.map((block) => lineOf(block) ?? ATOM)
  return matchLines(before, text.split('\n')).map((i) => (i >= 0 ? blocks[i] : undefined))
}

/**
 * A heading's id (written, or made by the reader from its text) and the
 * classes written on it.
 */
export function headingAttr(block: Node | undefined): { id?: string; classes: string[] } {
  if (block?.t !== 'Header') return { classes: [] }
  const [id, classes] = (block.c as [number, Attr])[1]
  return { id: id || undefined, classes }
}

/** The body's text: a line per block. */
export function bodyText(blocks: readonly Node[]): string {
  return blocks.map((block) => lineOf(block) ?? ATOM).join('\n')
}

/**
 * The blocks of a body edited to read `text`. A line that reads as its
 * block did is that block, untouched; any other line is a new paragraph
 * (an empty one, for a line without text) or heading. A paragraph is
 * wrapped in the source where the text it keeps was. A heading made from
 * one that was there (the one in the same place among the changed lines)
 * keeps its attributes and source, so only its text is rewritten. Null when
 * `text` doesn't have the body's read-only blocks, which no edit should
 * bring about.
 */
export function editBody(blocks: readonly Node[], text: string): Node[] | null {
  const lines = text === '' ? [] : text.split('\n').map(kept)
  const before = blocks.map((block) => lineOf(block) ?? ATOM)
  const readOnly = readOnlyBlocks(blocks)
  if (lines.filter((line) => line === ATOM).length !== readOnly.length) return null
  const match = matchLines(before, lines)
  const replaced = replacedBlocks(match, blocks.length)
  const wrapped = keptWraps(blocks, before.join('\n'), lines.join('\n'))
  let atoms = 0
  let start = 0
  return lines.map((line, i) => {
    const lineStart = start
    start += line.length + 1
    if (line === ATOM) return readOnly[atoms++]
    if (match[i] >= 0) return blocks[match[i]]
    const level = headingLevel(line)
    if (level > 0) return heading(level, lineText(line), blocks[replaced[i]])
    if (line === '') return { t: 'Para', c: [{ t: 'Str', c: NBSP }] }
    return { t: 'Para', c: words(line, (offset) => wrapped(lineStart + offset)) }
  })
}

/**
 * A heading of `level` reading `text`, made from the heading `was` when
 * there is one: it keeps its attributes and their source, except an id
 * the reader made from the old text, which isn't written in the source.
 */
function heading(level: number, text: string, was: Node | undefined): Node {
  const inlines = text === '' ? [] : words(text)
  if (was?.t !== 'Header' || lineOf(was) === undefined) return { t: 'Header', c: [level, ['', [], []], inlines] }
  const [, [id, classes, kvs]] = was.c as [number, Attr, Node[]]
  const written = (was as { a?: { id?: unknown } }).a?.id != null
  return { ...was, c: [level, [written ? id : '', classes, kvs], inlines] }
}

/**
 * For each new line, the index of the old block it replaces, or -1: among
 * the lines that changed between two that didn't, the first new one
 * replaces the first old one, and so on.
 */
function replacedBlocks(match: readonly number[], blockCount: number): number[] {
  const replaced = new Array<number>(match.length).fill(-1)
  let nextOld = 0
  let i = 0
  while (i <= match.length) {
    // A run of new lines, up to the next line that's an old one.
    let end = i
    while (end < match.length && match[end] < 0) end++
    const oldEnd = end < match.length ? match[end] : blockCount
    for (let j = i, k = nextOld; j < end && k < oldEnd; j++, k++) replaced[j] = k
    nextOld = oldEnd + 1
    i = end + 1
  }
  return replaced
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
    .map((block) => {
      const line = lineOf(block)
      return line === undefined ? ATOM : block.t === 'Para' && line ? plainText(block.c as Node[], '\n') : line
    })
    .join('\n')
  const changeEnd = change.start + change.inserted.length
  const moved = before.length - after.length
  return (offset) => {
    const was = offset < change.start ? offset : offset >= changeEnd ? offset + moved : -1
    return before[was] === ' ' && source[was] === '\n'
  }
}

/**
 * The part of a line that qmd keeps: its prefix, and its text with no
 * spaces at its ends and runs of spaces as one.
 */
export function kept(line: string): string {
  const text = lineText(line)
  return line.slice(0, line.length - text.length) + text.trim().replace(/\s+/g, ' ')
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
