/**
 * A document's body as the text of one multi-line field: a line per
 * block. A paragraph of plain text is its text, a heading of plain text
 * is its text after a prefix for its level (`headingPrefix`), and a block
 * quote of one paragraph of plain text is that paragraph's text after the
 * quote prefix (`QUOTE`). A bullet list whose items are each one line of
 * plain text (a tight list) is a line per item, after the bullet prefix
 * (`BULLET`), and the lines of items next to each other are one list. So
 * is a numbered list of such items, written `1.` or `1)` from any number,
 * after the numbered prefix (`NUMBERED`): an edit keeps its start number
 * and delimiter, and a new one is numbered `1.` from 1. A
 * horizontal rule is a line that can be removed but not edited (`VOID`).
 * "Plain text" here can be formatted: bold, italic, underline,
 * strikethrough, highlight, code and links are marks in the line's text
 * (see `inlines.ts`). Every other block (a paragraph or heading with math,
 * a note or an image in it, a quote or list of anything else) is a line
 * that can't be edited (`ATOM`), and comes back from an edit as the block
 * it was.
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

import { ATOM, isPrefix, VOID } from '../engine/model.ts'
import { diff, matchLines } from '../engine/rebase.ts'
import { inlinesOf, inlineText, keptText, type Node } from './inlines.ts'

export { words, type Node } from './inlines.ts'

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

/** The line prefix of a numbered list's item. */
export const NUMBERED = '\ufdd9'

/** Whether a line of the body's text is a block quote. */
export function isQuote(line: string): boolean {
  return line.startsWith(QUOTE)
}

/** Whether a line of the body's text is an item of a bullet list. */
export function isBullet(line: string): boolean {
  return line.startsWith(BULLET)
}

/** Whether a line of the body's text is an item of a numbered list. */
export function isNumbered(line: string): boolean {
  return line.startsWith(NUMBERED)
}

/** Whether a line of the body's text is a list's item, of either kind. */
export function isItem(line: string): boolean {
  return isBullet(line) || isNumbered(line)
}

/** The heading level of a line of the body's text, or 0 for a paragraph. */
export function headingLevel(line: string): number {
  const level = line.charCodeAt(0) - 0xfdd0
  return level >= 1 && level <= 6 ? level : 0
}

/** A line's prefix. */
function linePrefix(line: string): string {
  return line.slice(0, line.length - lineText(line).length)
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
  const text = inlineText(inlines)
  if (text === undefined || level < 1 || level > 6) return undefined
  return headingPrefix(level) + text
}

/**
 * The text of a paragraph that can be edited as plain text (none, for an
 * empty paragraph), or undefined for any other block.
 */
export function plainParagraph(block: Node): string | undefined {
  if (block.t !== 'Para' || !Array.isArray(block.c)) return undefined
  const text = inlineText(block.c as Node[])
  if (text === NBSP) return ''
  return text === undefined || text === '' ? undefined : text
}

/** A numbered list's start number, numbering style and delimiter. */
type ListAttrs = [number, { t: string }, { t: string }]

/** What a numbered list made in the editor is numbered with: `1.`, from 1. */
const NEW_LIST: ListAttrs = [1, { t: 'Decimal' }, { t: 'Period' }]

/** The number a numbered list starts at: 1 for a new list, or any other block. */
export function listStart(block: Node | undefined): number {
  return block?.t === 'OrderedList' ? (block.c as [ListAttrs])[0][0] : 1
}

/** A list's items. */
function listItems(list: Node): Node[][] {
  return (list.t === 'OrderedList' ? (list.c as [ListAttrs, Node[][]])[1] : list.c) as Node[][]
}

/**
 * The line prefix of a list's items, or undefined for a block that isn't
 * a list the editor writes back as it reads (a numbered list must be
 * numbered with digits, written `1.` or `1)`, which leaves out qmd's
 * example lists, `(@)`).
 */
function itemPrefix(list: Node): string | undefined {
  if (list.t === 'BulletList') return BULLET
  if (list.t !== 'OrderedList') return undefined
  const [, style, delim] = (list.c as [ListAttrs])[0]
  const decimal = style.t === 'Decimal' || style.t === 'DefaultStyle'
  const written = delim.t === 'Period' || delim.t === 'OneParen' || delim.t === 'DefaultDelim'
  return decimal && written ? NUMBERED : undefined
}

/**
 * The lines of a list's items, or undefined when it can't be edited: when
 * an item is anything but one line of plain text, or it's numbered in a
 * way the editor doesn't keep.
 */
function itemLines(list: Node): string[] | undefined {
  const prefix = itemPrefix(list)
  if (prefix === undefined) return undefined
  const lines: string[] = []
  for (const item of listItems(list)) {
    if (item.length === 0) {
      lines.push(prefix)
      continue
    }
    if (item.length !== 1 || item[0].t !== 'Plain') return undefined
    const text = inlineText(item[0].c as Node[])
    if (text === undefined) return undefined
    lines.push(prefix + (text === NBSP ? '' : text))
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
 * one of its kind that can be edited can't be, since the lines of items
 * of a kind next to each other are one list.
 */
function bodyLines(blocks: readonly Node[]): BodyLine[] {
  const lines: BodyLine[] = []
  blocks.forEach((block, i) => {
    if (block.t === 'BulletList' || block.t === 'OrderedList') {
      const prev = lines[lines.length - 1]
      const follows = prev?.item >= 0 && linePrefix(prev.line) === itemPrefix(block)
      const items = follows ? undefined : itemLines(block)
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
  return listItems(blocks[line.block])[line.item]
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
 * block quote, `-`, `*` or `+` and a space an item of a bullet list, and
 * `1.` and a space an item of a numbered list, as in qmd. `---`, `***` or `___` there make a horizontal rule, and the
 * rest of the paragraph goes on the line after it.
 */
export function bodyInputRule(line: string): string | null {
  if (line === '> ') return QUOTE
  if (line === '- ' || line === '* ' || line === '+ ') return BULLET
  if (line === '1. ') return NUMBERED
  if (line === '---' || line === '***' || line === '___') return `${VOID}\n`
  const match = /^(#{1,6}) $/.exec(line)
  return match ? headingPrefix(match[1].length) : null
}

/** A line's prefix as qmd, for the clipboard. */
export function prefixMarkdown(prefix: string): string {
  if (isQuote(prefix)) return '> '
  if (isBullet(prefix)) return '- '
  if (isNumbered(prefix)) return '1. '
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
 * of items of a kind next to each other are one list: the list that was,
 * untouched, when they're its items as they were, or else a new list, of
 * the items that read the same and new ones (numbered as the list of the
 * first that read the same was). A paragraph or item, in a quote or
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
  // The items of the list the lines so far end in, the lines they were,
  // and their prefix.
  let items: { item: Node[]; was: number }[] = []
  let kind = ''
  const endList = () => {
    if (items.length > 0) edited.push(listOf(blocks, units, kind, items))
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
    if (isItem(line)) {
      const prefix = linePrefix(line)
      if (prefix !== kind) endList()
      kind = prefix
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
 * The list of `items`, whose lines have the prefix `kind`: the list they
 * were all the items of, in order, or else a new one, numbered (if it's
 * numbered) as the list of the first item that was in one was.
 */
function listOf(
  blocks: readonly Node[],
  units: readonly BodyLine[],
  kind: string,
  items: { item: Node[]; was: number }[],
): Node {
  const first = items[0].was >= 0 ? units[items[0].was] : undefined
  const whole =
    first !== undefined &&
    first.item === 0 &&
    listItems(blocks[first.block]).length === items.length &&
    items.every(({ was }, i) => was === items[0].was + i)
  if (whole) return blocks[first.block]
  const content = items.map(({ item }) => item)
  if (kind !== NUMBERED) return { t: 'BulletList', c: content }
  const kept = items.find(({ was }) => was >= 0)
  const attrs = kept ? (blocks[units[kept.was].block].c as [ListAttrs])[0] : NEW_LIST
  return { t: 'OrderedList', c: [attrs, content] }
}

/** An item reading `text` (nothing in it, for no text), wrapped in the source where `wrapped` says. */
function item(text: string, wrapped: (offset: number) => boolean): Node[] {
  return text === '' ? [] : [{ t: 'Plain', c: inlinesOf(text, wrapped) }]
}

/**
 * A paragraph reading `text` (an empty one, for no text), wrapped in the
 * source where `wrapped` says.
 */
function paragraph(text: string, wrapped: (offset: number) => boolean): Node {
  if (text === '') return { t: 'Para', c: [{ t: 'Str', c: NBSP }] }
  return { t: 'Para', c: inlinesOf(text, wrapped) }
}

/**
 * A heading of `level` reading `text`, made from the heading `was` when
 * there is one: it keeps its attributes and their source, except an id
 * the reader made from the old text, which isn't written in the source.
 */
function heading(level: number, text: string, was: Node | undefined): Node {
  const inlines = text === '' ? [] : inlinesOf(text)
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
        const prefix = linePrefix(line)
        return line === prefix ? line : prefix + inlineText(plain.c as Node[], '\n')
      }
      if (block.t === 'Para' && line) return inlineText(block.c as Node[], '\n')
      if (block.t === 'BlockQuote' && line !== QUOTE) {
        return QUOTE + inlineText((block.c as Node[])[0].c as Node[], '\n')
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
 * spaces at its ends and runs of spaces as one (but in code), and spaces
 * at the ends of formatting outside it (see `keptText`).
 */
export function kept(line: string): string {
  const text = lineText(line)
  return line.slice(0, line.length - text.length) + keptText(text)
}
