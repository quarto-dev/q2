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
 * A figure of one image, with a caption of plain text (or an image alone
 * in a paragraph, a figure with no caption yet), is a line of its caption,
 * after the figure prefix (`FIGURE`) and its image, as more prefix
 * characters (`figurePrefix`): the image goes wherever the line does, so
 * undo and collaborators' edits bring it back with its caption, and it's
 * gone when the line is joined to the one before.
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

/** The line prefix of a figure's caption, followed by its image (see `figurePrefix`). */
export const FIGURE = '\ufdda'

/** A figure's image: its address, title, and attributes (the figure's id among them). */
export interface FigureImage {
  src: string
  title: string
  attr: Attr
}

/**
 * The line prefix of a caption of a figure of `image`: `FIGURE`, then the
 * image as JSON, its UTF-8 bytes as hex digits written U+FDE0 to U+FDEF.
 */
export function figurePrefix(image: FigureImage): string {
  const bytes = new TextEncoder().encode(JSON.stringify([image.src, image.title, image.attr]))
  let out = FIGURE
  for (const byte of bytes) out += String.fromCharCode(0xfde0 + (byte >> 4), 0xfde0 + (byte & 15))
  return out
}

/** The image a figure's caption line (or its prefix) holds, or undefined for any other line. */
export function figureImage(line: string): FigureImage | undefined {
  if (!isFigure(line)) return undefined
  const prefix = linePrefix(line)
  const bytes = new Uint8Array((prefix.length - 1) >> 1)
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = ((prefix.charCodeAt(1 + 2 * i) - 0xfde0) << 4) | (prefix.charCodeAt(2 + 2 * i) - 0xfde0)
  }
  const [src, title, attr] = JSON.parse(new TextDecoder().decode(bytes)) as [string, string, Attr]
  return { src, title, attr }
}

/** Whether a line of the body's text is a figure's caption. */
export function isFigure(line: string): boolean {
  return line.startsWith(FIGURE)
}

/**
 * After an item's prefix, one of these for each level it's nested at: an
 * item of a list in the item at the first level has one, and so on.
 */
export const INDENT = '\ufddb'

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

/** How deep a list's item is nested (0: at the first level), or -1 for a line that isn't an item. */
export function itemDepth(line: string): number {
  if (!isItem(line)) return -1
  let depth = 0
  while (line[1 + depth] === INDENT) depth++
  return depth
}

/** The prefix of an item of `kind` (`BULLET` or `NUMBERED`) at `depth`. */
function itemPrefixAt(kind: string, depth: number): string {
  return kind + INDENT.repeat(depth)
}

/**
 * What's left of a line's prefix when it's taken off by Backspace at the
 * start of the line's text or by Enter on an empty item: a nested item
 * moves out a level, and anything else becomes a paragraph.
 */
export function outdentPrefix(prefix: string): string {
  return isItem(prefix) && itemDepth(prefix) > 0 ? prefix.slice(0, -1) : ''
}

/**
 * The prefixes of the body's lines (`prefixes`), with the items among
 * lines `first` to `last` moved in a level (`by` 1, under the item before
 * them) or out a level (-1), each with the items nested in it; or null
 * when none can be. An item can move in only under an item before it at
 * its level or deeper, and out only from a nested level; moving in, it's
 * all the items or none.
 */
export function indentLines(prefixes: readonly string[], first: number, last: number, by: 1 | -1): string[] | null {
  const depths = prefixes.map(itemDepth)
  const moved = new Array<boolean>(prefixes.length).fill(false)
  for (let i = first; i <= last; i++) {
    if (depths[i] < 0 || moved[i] || (by < 0 && depths[i] === 0)) continue
    moved[i] = true
    for (let j = i + 1; j < prefixes.length && depths[j] > depths[i]; j++) moved[j] = true
  }
  if (!moved.includes(true)) return null
  const next = depths.map((d, i) => (moved[i] ? d + by : d))
  // Every item is at most a level deeper than the item before it, and an
  // item after a line that isn't one is at the first level.
  const fits = next.every((d, i) => d < 0 || d <= (i > 0 && next[i - 1] >= 0 ? next[i - 1] + 1 : 0))
  if (!fits) return null
  return prefixes.map((p, i) => (moved[i] ? itemPrefixAt(p[0], next[i]) : p))
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
  const figure = figureLine(block)
  if (figure !== undefined) return figure
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
 * The line of a figure of one image whose caption is plain text, or of an
 * image alone in a paragraph with no alt text (a figure with no caption
 * yet); undefined for any other block. The image's attributes take in the
 * figure's id, which qmd writes on the image.
 */
function figureLine(block: Node): string | undefined {
  let image: Node
  let caption: Node[]
  let id = ''
  if (block.t === 'Para') {
    const inlines = block.c as Node[]
    if (inlines.length !== 1 || inlines[0].t !== 'Image' || (inlines[0].c as [Attr, Node[]])[1].length > 0) return undefined
    image = inlines[0]
    caption = []
  } else if (block.t === 'Figure') {
    const [[figureId], [, captionBlocks], content] = block.c as [Attr, [unknown, Node[]], Node[]]
    if (content.length !== 1 || content[0].t !== 'Plain') return undefined
    const inlines = content[0].c as Node[]
    if (inlines.length !== 1 || inlines[0].t !== 'Image') return undefined
    if (captionBlocks.length > 1 || (captionBlocks.length === 1 && captionBlocks[0].t !== 'Plain')) return undefined
    image = inlines[0]
    caption = captionBlocks.length === 1 ? (captionBlocks[0].c as Node[]) : []
    id = figureId
  } else return undefined
  const [[imageId, classes, kvs], , [src, title]] = image.c as [Attr, Node[], [string, string]]
  const text = caption.length === 0 ? '' : inlineText(caption)
  if (text === undefined) return undefined
  return figurePrefix({ src, title, attr: [id || imageId, classes, kvs] }) + text
}

/**
 * A figure of `image` with the caption `text` (formatted, as a line's
 * text is), or, with no caption, the image alone in a paragraph, as qmd
 * has a figure without one. The caption is the image's alt text too.
 */
function figure(image: FigureImage, text: string, wrapped: (offset: number) => boolean): Node {
  const [id, classes, kvs] = image.attr
  const target = [image.src, image.title]
  if (text === '') return { t: 'Para', c: [{ t: 'Image', c: [[id, classes, kvs], [], target] }] }
  const caption = inlinesOf(text, wrapped)
  return {
    t: 'Figure',
    c: [[id, [], []], [null, [{ t: 'Plain', c: caption }]], [{ t: 'Plain', c: [{ t: 'Image', c: [['', classes, kvs], caption, target] }] }]],
  }
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
 * The lines of a list's items, nested `depth` levels deep, or undefined
 * when it can't be edited: when an item is anything but one line of plain
 * text followed by lists that can be edited (no two of a kind in a row,
 * whose items would read as one list's), or it's numbered in a way the
 * editor doesn't keep.
 */
function itemLines(list: Node, block: number, depth = 0): BodyLine[] | undefined {
  const kind = itemPrefix(list)
  if (kind === undefined) return undefined
  const prefix = itemPrefixAt(kind, depth)
  const lines: BodyLine[] = []
  for (const item of listItems(list)) {
    const plain = item[0]?.t === 'Plain' ? item[0] : undefined
    const text = plain ? inlineText(plain.c as Node[]) : ''
    if (text === undefined) return undefined
    lines.push({ line: prefix + (text === NBSP ? '' : text), block, list, plain, item })
    let last: string | undefined
    for (const sub of item.slice(plain ? 1 : 0)) {
      const subKind = itemPrefix(sub)
      if (subKind === undefined || subKind === last) return undefined
      last = subKind
      const subLines = itemLines(sub, block, depth + 1)
      if (subLines === undefined) return undefined
      lines.push(...subLines)
    }
  }
  return lines
}

/** A line of the body's text, and the block it's from (and the item, for a list's). */
interface BodyLine {
  line: string
  block: number
  /** For an item's line: the list it's an item of (nested in the block, or the block). */
  list?: Node
  /** For an item's line: the item's text, if it has any. */
  plain?: Node
  /** For an item's line: the item. */
  item?: Node[]
}

/**
 * The body's lines: one per block, or per item of a list that can be
 * edited (an item in an item after it), and an atom for each block that
 * can't be. A list right after one of its kind that can be edited can't
 * be, since the lines of items of a kind next to each other are one list.
 */
function bodyLines(blocks: readonly Node[]): BodyLine[] {
  const lines: BodyLine[] = []
  blocks.forEach((block, i) => {
    if (block.t === 'BulletList' || block.t === 'OrderedList') {
      const prev = lines[lines.length - 1]
      const follows = prev?.list !== undefined && prev.line[0] === itemPrefix(block)
      const items = follows ? undefined : itemLines(block, i)
      if (items) lines.push(...items)
      else lines.push({ line: ATOM, block: i })
      return
    }
    lines.push({ line: lineOf(block) ?? ATOM, block: i })
  })
  return lines
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
  const indent = '    '.repeat(Math.max(0, itemDepth(prefix)))
  if (isBullet(prefix)) return `${indent}- `
  if (isNumbered(prefix)) return `${indent}1. `
  if (isFigure(prefix)) return ''
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
  return match.map((i) => (i >= 0 ? (before[i].list ?? blocks[before[i].block]) : undefined))
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
  // The items of the lists the lines so far end in.
  let items: Entry[] = []
  const endList = () => {
    if (items.length > 0) edited.push(...listsOf(blocks, units, items))
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
      const was = match[i]
      const plain = was >= 0 ? units[was].plain : undefined
      const content = was >= 0 ? (plain ? [plain] : []) : item(text, wraps)
      // At most a level deeper than the item before it.
      const depth = Math.min(itemDepth(line), items.length > 0 ? items[items.length - 1].depth + 1 : 0)
      items.push({ kind: line[0], depth, content, was, item: was >= 0 ? units[was].item : undefined })
      return
    }
    endList()
    if (line === ATOM) edited.push(readOnly[atoms++])
    else if (match[i] >= 0) edited.push(blocks[units[match[i]].block])
    else if (line === VOID) edited.push({ t: 'HorizontalRule' })
    else if (isFigure(line)) edited.push(figure(figureImage(line)!, text, wraps))
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

/** An item's line, for `editBody`: its kind, depth and text, and the line it was (or -1). */
interface Entry {
  kind: string
  depth: number
  content: Node[]
  was: number
  /** The item the line was. */
  item?: Node[]
}

/**
 * The lists of a run of items' lines next to each other, each item with
 * the lists of the deeper items after it in it. A list whose lines are a
 * list that was, all of them, as they were, is that list.
 */
function listsOf(blocks: readonly Node[], units: readonly BodyLine[], entries: readonly Entry[]): Node[] {
  const lists: Node[] = []
  let i = 0
  while (i < entries.length) {
    const from = i
    const { list, next } = buildList(units, entries, i)
    i = next
    const first = entries[from].was >= 0 ? units[entries[from].was] : undefined
    const block = first?.block
    const whole =
      first !== undefined &&
      first.list === blocks[block!] &&
      (block === 0 || units[entries[from].was - 1]?.block !== block) &&
      units.filter((u) => u.block === block).length === next - from &&
      entries.slice(from, next).every(({ was }, k) => was === entries[from].was + k)
    lists.push(whole ? blocks[block!] : list)
  }
  return lists
}

/**
 * The list of the items from `entries[from]` at its depth and of its kind,
 * each with the deeper items after it as lists in it, and where the
 * entries after it start. Numbered (if it's numbered) as the list of the
 * first item that was in one was.
 */
function buildList(units: readonly BodyLine[], entries: readonly Entry[], from: number): { list: Node; next: number } {
  const { kind, depth } = entries[from]
  const content: Node[][] = []
  const kept: Entry[] = []
  let i = from
  while (i < entries.length && entries[i].depth === depth && entries[i].kind === kind) {
    const entry = entries[i++]
    const item = [...entry.content]
    while (i < entries.length && entries[i].depth > depth) {
      const sub = buildList(units, entries, i)
      item.push(sub.list)
      i = sub.next
    }
    // An item with no items in it, before or now, is the item it was.
    const same = entry.item !== undefined && item.length === entry.content.length && entry.item.length === item.length
    content.push(same ? entry.item! : item)
    kept.push(entry)
  }
  if (kind !== NUMBERED) return { list: { t: 'BulletList', c: content }, next: i }
  const was = kept.find((e) => e.was >= 0 && units[e.was].list?.t === 'OrderedList')
  const attrs = was ? (units[was.was].list!.c as [ListAttrs])[0] : NEW_LIST
  return { list: { t: 'OrderedList', c: [attrs, content] }, next: i }
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
    .map(({ line, block: index, list, plain }) => {
      const block = blocks[index]
      if (line === ATOM) return ATOM
      if (list !== undefined) {
        const prefix = linePrefix(line)
        return line === prefix || !plain ? line : prefix + inlineText(plain.c as Node[], '\n')
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
