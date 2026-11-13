/**
 * A document's body as the text of one multi-line field: a line per
 * block. A paragraph of plain text is its text, a heading of plain text
 * is its text after a prefix for its level (`headingPrefix`), and a block
 * quote of one paragraph of plain text is that paragraph's text after the
 * quote prefix (`QUOTE`). A bullet list whose items are each one line of
 * plain text (a tight list) is a line per item, after the bullet prefix
 * (`BULLET`), and the lines of items next to each other are one list. A
 * horizontal rule is a line that can be removed but not edited (`VOID`).
 * Every other block (a paragraph or heading with formatting, a quote or
 * list of anything else) is a line that can't be edited (`ATOM`), and
 * comes back from an edit as the block it was.
 *
 * An empty line is an empty paragraph, which qmd has no syntax for: it's
 * written as a paragraph of one non-breaking space, which takes up a line
 * when rendered, and a paragraph of only that reads as an empty line. An
 * empty quote is a quote of such a paragraph: qmd's `>` alone is a quote
 * with no paragraph in it, which the qmd writer can't write. An empty item
 * is an item with nothing in it, a bare `*` in qmd.
 *
 * A paragraph (or item) that the source wraps over several lines reads as
 * one line, and an edit keeps its line breaks wherever the text around
 * them stays.
 */

import { ATOM, isPrefix, VOID, withoutPrefixes } from '../engine/model.ts'
import { diff, matchLines } from '../engine/rebase.ts'

export type Node = { t: string; c?: unknown }

/** What an empty paragraph holds. */
const NBSP = '\u00a0'

type Attr = [string, string[], [string, string][]]

/** The line prefix of a heading of `level` (1 to 6). */
export function headingPrefix(level: number): string {
  return String.fromCharCode(0xfdd0 + level)
}

/** The line prefix of a block quote. */
export const QUOTE = '\ufdd7'

/** The line prefix of a bullet list's item. */
export const BULLET = '\ufdd8'

/** Whether a line of the body's text is a block quote. */
export function isQuote(line: string): boolean {
  return line.startsWith(QUOTE)
}

/** Whether a line of the body's text is an item of a bullet list. */
export function isBullet(line: string): boolean {
  return line.startsWith(BULLET)
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
 * be edited as one line (a list is a line per item: see `bodyLines`).
 */
export function lineOf(block: Node): string | undefined {
  if (block.t === 'Para') return plainParagraph(block)
  if (block.t === 'HorizontalRule') return VOID
  if (block.t === 'BlockQuote') {
    const blocks = block.c as Node[]
    const text = blocks.length === 1 ? plainParagraph(blocks[0]) : undefined
    return text === undefined ? undefined : QUOTE + text
  }
  if (block.t !== 'Header' || !Array.isArray(block.c)) return undefined
  const [level, , inlines] = block.c as [number, Attr, Node[]]
  const text = plainText(inlines, ' ')
  if (text === undefined || !editable(text) || level < 1 || level > 6) return undefined
  return headingPrefix(level) + text
}

/** Whether text can be a line's: it holds no character the editor gives a meaning. */
function editable(text: string): boolean {
  return !text.includes(ATOM) && !text.includes(VOID) && withoutPrefixes(text) === text
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

/**
 * The lines of a bullet list's items, or undefined when it can't be
 * edited: when an item is anything but one line of plain text.
 */
function itemLines(list: Node): string[] | undefined {
  const lines: string[] = []
  for (const item of list.c as Node[][]) {
    if (item.length === 0) {
      lines.push(BULLET)
      continue
    }
    if (item.length !== 1 || item[0].t !== 'Plain') return undefined
    const text = plainText(item[0].c as Node[], ' ')
    if (text === undefined || !editable(text)) return undefined
    lines.push(BULLET + (text === NBSP ? '' : text))
  }
  return lines
}

/** A line of the body's text, and the block it's from (and the item, for a list's). */
interface BodyLine {
  line: string
  block: number
  /** The index of the list item the line is, or -1. */
  item: number
}

/**
 * The body's lines: one per block, or per item of a list that can be
 * edited, and an atom for each block that can't be. A list right after
 * one that can be edited can't be, since the lines of items next to each
 * other are one list.
 */
function bodyLines(blocks: readonly Node[]): BodyLine[] {
  const lines: BodyLine[] = []
  blocks.forEach((block, i) => {
    if (block.t === 'BulletList') {
      const prev = lines[lines.length - 1]
      const items = prev?.item >= 0 ? undefined : itemLines(block)
      if (items) items.forEach((line, item) => lines.push({ line, block: i, item }))
      else lines.push({ line: ATOM, block: i, item: -1 })
      return
    }
    lines.push({ line: lineOf(block) ?? ATOM, block: i, item: -1 })
  })
  return lines
}

/** The item a line of a list is. */
function itemOf(blocks: readonly Node[], line: BodyLine): Node[] {
  return (blocks[line.block].c as Node[][])[line.item]
}

/** The blocks that can't be edited, in order: what the body's atoms stand for. */
export function readOnlyBlocks(blocks: readonly Node[]): Node[] {
  return bodyLines(blocks)
    .filter((line) => line.line === ATOM)
    .map((line) => blocks[line.block])
}

/**
 * What typing turns into in the body: one to six `#` and a space at the
 * start of a paragraph make it a heading of that level, `>` and a space a
 * block quote, and `-`, `*` or `+` and a space an item of a bullet list,
 * as in qmd. `---`, `***` or `___` there make a horizontal rule, and the
 * rest of the paragraph goes on the line after it.
 */
export function bodyInputRule(line: string): string | null {
  if (line === '> ') return QUOTE
  if (line === '- ' || line === '* ' || line === '+ ') return BULLET
  if (line === '---' || line === '***' || line === '___') return `${VOID}\n`
  const match = /^(#{1,6}) $/.exec(line)
  return match ? headingPrefix(match[1].length) : null
}

/** A line's prefix as qmd, for the clipboard. */
export function prefixMarkdown(prefix: string): string {
  if (isQuote(prefix)) return '> '
  if (isBullet(prefix)) return '- '
  const level = headingLevel(prefix)
  return level > 0 ? `${'#'.repeat(level)} ` : ''
}

/**
 * For each line of `text`, a version of the body's text (`blocks`), the
 * block it shows, when it reads as one of them.
 */
export function blocksOfLines(blocks: readonly Node[], text: string): (Node | undefined)[] {
  const before = bodyLines(blocks)
  const match = matchLines(
    before.map((line) => line.line),
    text.split('\n'),
  )
  return match.map((i) => (i >= 0 ? blocks[before[i].block] : undefined))
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

/** The body's text: a line per block, or per item of a list. */
export function bodyText(blocks: readonly Node[]): string {
  return bodyLines(blocks)
    .map((line) => line.line)
    .join('\n')
}

/**
 * The blocks of a body edited to read `text`. A line that reads as its
 * block did is that block, untouched; any other line is a new paragraph
 * (an empty one, for a line without text), heading, quote or rule. Lines
 * of items next to each other are one list: the list that was, untouched,
 * when they're its items as they were, or else a new list, of the items
 * that read the same and new ones. A paragraph or item, in a quote or
 * not, is wrapped in the source where the text it keeps was. A heading
 * made from one that was there (the one in the same place among the
 * changed lines) keeps its attributes and source, so only its text is
 * rewritten. Null when `text` doesn't have the body's read-only blocks,
 * which no edit should bring about.
 */
export function editBody(blocks: readonly Node[], text: string): Node[] | null {
  const lines = text === '' ? [] : text.split('\n').map(kept)
  const units = bodyLines(blocks)
  const before = units.map((unit) => unit.line)
  const readOnly = readOnlyBlocks(blocks)
  if (lines.filter((line) => line === ATOM).length !== readOnly.length) return null
  const match = matchLines(before, lines)
  const replaced = replacedBlocks(match, before.length)
  const wrapped = keptWraps(blocks, units, before.join('\n'), lines.join('\n'))
  const edited: Node[] = []
  // The items of the list the lines so far end in, and the lines they were.
  let items: { item: Node[]; was: number }[] = []
  const endList = () => {
    if (items.length > 0) edited.push(listOf(blocks, units, items))
    items = []
  }
  let atoms = 0
  let start = 0
  lines.forEach((line, i) => {
    const lineStart = start
    start += line.length + 1
    const text = lineText(line)
    const textStart = lineStart + line.length - text.length
    const wraps = (offset: number) => wrapped(textStart + offset)
    if (isBullet(line)) {
      const was = match[i]
      items.push({ item: was >= 0 ? itemOf(blocks, units[was]) : item(text, wraps), was })
      return
    }
    endList()
    if (line === ATOM) edited.push(readOnly[atoms++])
    else if (match[i] >= 0) edited.push(blocks[units[match[i]].block])
    else if (line === VOID) edited.push({ t: 'HorizontalRule' })
    else if (headingLevel(line) > 0) {
      const was = replaced[i] >= 0 ? blocks[units[replaced[i]].block] : undefined
      edited.push(heading(headingLevel(line), text, was))
    } else {
      const para = paragraph(text, wraps)
      edited.push(isQuote(line) ? { t: 'BlockQuote', c: [para] } : para)
    }
  })
  endList()
  return edited
}

/**
 * The list of `items`: the list they were all the items of, in order, or
 * else a new one.
 */
function listOf(blocks: readonly Node[], units: readonly BodyLine[], items: { item: Node[]; was: number }[]): Node {
  const first = items[0].was >= 0 ? units[items[0].was] : undefined
  const whole =
    first !== undefined &&
    first.item === 0 &&
    (blocks[first.block].c as Node[][]).length === items.length &&
    items.every(({ was }, i) => was === items[0].was + i)
  return whole ? blocks[first.block] : { t: 'BulletList', c: items.map(({ item }) => item) }
}

/** An item reading `text` (nothing in it, for no text), wrapped in the source where `wrapped` says. */
function item(text: string, wrapped: (offset: number) => boolean): Node[] {
  return text === '' ? [] : [{ t: 'Plain', c: words(text, wrapped) }]
}

/**
 * A paragraph reading `text` (an empty one, for no text), wrapped in the
 * source where `wrapped` says.
 */
function paragraph(text: string, wrapped: (offset: number) => boolean): Node {
  if (text === '') return { t: 'Para', c: [{ t: 'Str', c: NBSP }] }
  return { t: 'Para', c: words(text, wrapped) }
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
 * For each new line, the index of the old line it replaces, or -1: among
 * the lines that changed between two that didn't, the first new one
 * replaces the first old one, and so on.
 */
function replacedBlocks(match: readonly number[], lineCount: number): number[] {
  const replaced = new Array<number>(match.length).fill(-1)
  let nextOld = 0
  let i = 0
  while (i <= match.length) {
    // A run of new lines, up to the next line that's an old one.
    let end = i
    while (end < match.length && match[end] < 0) end++
    const oldEnd = end < match.length ? match[end] : lineCount
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
function keptWraps(
  blocks: readonly Node[],
  units: readonly BodyLine[],
  before: string,
  after: string,
): (offset: number) => boolean {
  const change = diff(before, after)
  if (!change) return () => false
  // The body's text with a line break where the source wraps a paragraph.
  const source = units
    .map(({ line, block: index, item }) => {
      const block = blocks[index]
      if (line === ATOM) return ATOM
      if (item >= 0) {
        const [plain] = itemOf(blocks, { line, block: index, item })
        return line === BULLET ? line : BULLET + plainText(plain.c as Node[], '\n')
      }
      if (block.t === 'Para' && line) return plainText(block.c as Node[], '\n')
      if (block.t === 'BlockQuote' && line !== QUOTE) {
        return QUOTE + plainText((block.c as Node[])[0].c as Node[], '\n')
      }
      return line
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
