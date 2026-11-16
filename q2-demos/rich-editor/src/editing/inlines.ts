/**
 * A line of the body's text with inline formatting (see `engine/marks.ts`)
 * and the Pandoc inlines it stands for: bold (`Strong`), italic (`Emph`),
 * underline (`Underline`, `[text]{.underline}` in qmd), strikethrough
 * (`Strikeout`), highlight (a `Span` with the class `mark`,
 * `[text]{.mark}`), code (`Code`, with no attributes) and links (`Link`,
 * with no attributes or title), in any combination. Inlines with anything
 * else in them (math, a citation, a note, an image, a span of another
 * kind) can't be edited yet.
 *
 * Writing a line: markdown can't format the spaces at the ends of a run of
 * bold (or italic, underline, strikethrough or highlight), so they go
 * outside it; spaces at the ends of the line, and runs of spaces, go as
 * they do in plain text (qmd doesn't keep them), except in code.
 */

import {
  closeMark,
  hasMarks,
  isMarkChar,
  openMark,
  scan,
  serialize,
  tree,
  type InlineNode,
  type Run,
  type Style,
} from '../engine/marks.ts'

export type Node = { t: string; c?: unknown }
type Attr = [string, string[], [string, string][]]

export const STRONG = 0
export const EMPH = 1
export const UNDERLINE = 2
export const STRIKEOUT = 3
export const HIGHLIGHT = 4
/** Code is numbered highest: it has two caret positions at each edge, and its marks are outermost at a place. */
export const CODE = 5

/** The formats with two caret positions at each edge, inside and outside. */
export const TWO_SIDED = 1 << CODE
/** The formats that hold only text. */
export const LEAF = 1 << CODE

const NO_ATTR: Attr = ['', [], []]

/** The element each format is shown as, as `q2 render` writes it. */
export function formatElement(format: number): { as: string; className?: string } {
  switch (format) {
    case STRONG:
      return { as: 'strong' }
    case EMPH:
      return { as: 'em' }
    case UNDERLINE:
      return { as: 'u' }
    case STRIKEOUT:
      return { as: 'del' }
    case HIGHLIGHT:
      return { as: 'span', className: 'mark' }
    default:
      return { as: 'code' }
  }
}

const WRAPPERS: Record<string, number> = { Strong: STRONG, Emph: EMPH, Underline: UNDERLINE, Strikeout: STRIKEOUT }

function noAttr(attr: Attr): boolean {
  return attr[0] === '' && attr[1].length === 0 && attr[2].length === 0
}

/** Whether `text` holds a character the editor gives a meaning (and so can't be a line's). */
function special(text: string): boolean {
  return [...text].some((c) => isMarkChar(c) || c === '\n' || c === '￼' || c === '￻' || (c >= '﷐' && c <= '﷯'))
}

/**
 * The line of text `inlines` stand for, with marks for their formatting,
 * or undefined when they hold anything the editor can't edit.
 * `softBreak` is what a line break in the source reads as.
 */
export function inlineText(inlines: readonly Node[], softBreak = ' '): string | undefined {
  const runs: { text: string; marks: number; link: string | null }[] = []
  const push = (text: string, style: Style) => runs.push({ text, ...style })
  const walk = (nodes: readonly Node[], style: Style): boolean => {
    for (const node of nodes) {
      const c = node.c as never
      if (node.t === 'Str') {
        if (typeof node.c !== 'string' || special(node.c)) return false
        push(node.c, style)
      } else if (node.t === 'Space') push(' ', style)
      else if (node.t === 'SoftBreak') push(softBreak, style)
      else if (node.t === 'Quoted') {
        const [kind, inner] = c as [{ t: string }, Node[]]
        const [open, close] = kind.t === 'SingleQuote' ? ['‘', '’'] : ['“', '”']
        push(open, style)
        if (!walk(inner, style)) return false
        push(close, style)
      } else if (node.t in WRAPPERS) {
        if (!walk(c as Node[], { ...style, marks: style.marks | (1 << WRAPPERS[node.t]) })) return false
      } else if (node.t === 'Span') {
        const [attr, inner] = c as [Attr, Node[]]
        const format = attr[0] === '' && attr[2].length === 0 && attr[1].length === 1 ? { mark: HIGHLIGHT, underline: UNDERLINE }[attr[1][0]] : undefined
        if (format === undefined || !walk(inner, { ...style, marks: style.marks | (1 << format) })) return false
      } else if (node.t === 'Code') {
        const [attr, text] = c as [Attr, string]
        if (!noAttr(attr) || special(text) || text === '') return false
        push(text, { ...style, marks: style.marks | (1 << CODE) })
      } else if (node.t === 'Link') {
        const [attr, inner, [url, title]] = c as [Attr, Node[], [string, string]]
        if (style.link !== null || !noAttr(attr) || title !== '' || special(url)) return false
        if (!walk(inner, { ...style, link: url })) return false
      } else return false
    }
    return true
  }
  if (!walk(inlines, { marks: 0, link: null })) return undefined
  return serialize(runs).text
}

/** A character of a line, with its style and where it is in the line. */
interface Char extends Style {
  char: string
  offset: number
}

/**
 * The characters of a line as qmd keeps them when it's written: no spaces
 * at its ends, runs of spaces as one, outside code; spaces at the ends of
 * a run of a format other than code (and a link) outside it.
 */
function keptChars(text: string): Char[] {
  const isCode = (c: Char) => (c.marks & (1 << CODE)) !== 0
  const space = (c: Char | undefined) => c !== undefined && !isCode(c) && /\s/.test(c.char)
  const all: Char[] = scan(text).chars.map((c) => ({ char: c.char, offset: c.offset, ...c.style }))
  const chars: Char[] = []
  for (const c of all) {
    if (space(c) && (chars.length === 0 || space(chars[chars.length - 1]))) continue
    chars.push(space(c) ? { ...c, char: ' ' } : c)
  }
  while (space(chars[chars.length - 1])) chars.pop()
  for (const format of [STRONG, EMPH, UNDERLINE, STRIKEOUT, HIGHLIGHT]) {
    const bit = 1 << format
    for (let i = 0; i < chars.length; ) {
      if (!(chars[i].marks & bit)) {
        i++
        continue
      }
      let j = i
      while (j < chars.length && chars[j].marks & bit) j++
      for (let s = i; s < j && space(chars[s]); s++) chars[s] = { ...chars[s], marks: chars[s].marks & ~bit }
      for (let e = j - 1; e >= i && space(chars[e]); e--) chars[e] = { ...chars[e], marks: chars[e].marks & ~bit }
      i = j
    }
  }
  return chars
}

/** `chars` as runs, a run per stretch of one style that's next to each other in the line. */
function charRuns(chars: readonly Char[]): Run[] {
  const runs: Run[] = []
  for (const c of chars) {
    const last = runs[runs.length - 1]
    if (last && last.marks === c.marks && last.link === c.link && last.start + last.text.length === c.offset) {
      runs[runs.length - 1] = { ...last, text: last.text + c.char }
    } else runs.push({ text: c.char, marks: c.marks, link: c.link, start: c.offset })
  }
  return runs
}

/**
 * The part of a line (without its prefix) that qmd keeps (see
 * `keptChars`), with canonical marks: what the document reads back once
 * the line is written.
 */
export function keptText(text: string): string {
  if (!hasMarks(text)) return text.trim().replace(/\s+/g, ' ')
  return serialize(keptChars(text).map((c) => ({ text: c.char, marks: c.marks, link: c.link }))).text
}

/**
 * A line of text (without its prefix, its formatting in marks) as
 * inlines. A space is a line break in the source when `wrapped` says so
 * of its offset in `text`.
 */
export function inlinesOf(text: string, wrapped: (offset: number) => boolean = () => false): Node[] {
  if (!hasMarks(text)) return words(text, wrapped)
  return nodes(tree(charRuns(keptChars(text)), LEAF), wrapped)
}

function nodes(tree: readonly InlineNode[], wrapped: (offset: number) => boolean): Node[] {
  const out: Node[] = []
  for (const node of tree) {
    if (node.kind === 'text') {
      out.push(...textNodes(node.text, node.start, wrapped))
    } else if (node.kind === 'link') {
      out.push({ t: 'Link', c: [NO_ATTR, nodes(node.children, wrapped), [node.url, '']] })
    } else if (node.format === CODE) {
      out.push({ t: 'Code', c: [NO_ATTR, codeText(node.children)] })
    } else {
      const children = nodes(node.children, wrapped)
      if (node.format === HIGHLIGHT) out.push({ t: 'Span', c: [['', ['mark'], []], children] })
      else out.push({ t: ['Strong', 'Emph', 'Underline', 'Strikeout'][node.format], c: children })
    }
  }
  return merged(out)
}

function codeText(children: readonly InlineNode[]): string {
  return children.map((n) => (n.kind === 'text' ? n.text : codeText(n.children))).join('')
}

/** Words and the spaces between them, from text that starts at `start` in its line. */
function textNodes(text: string, start: number, wrapped: (offset: number) => boolean): Node[] {
  const out: Node[] = []
  let offset = 0
  for (const word of text.split(' ')) {
    if (offset > 0) out.push({ t: wrapped(start + offset - 1) ? 'SoftBreak' : 'Space' })
    if (word !== '') out.push({ t: 'Str', c: word })
    offset += word.length + 1
  }
  return out
}

/** Inlines with neighbouring `Str`s joined, as the reader gives them. */
function merged(inlines: Node[]): Node[] {
  const out: Node[] = []
  for (const inline of inlines) {
    const last = out[out.length - 1]
    if (inline.t === 'Str' && last?.t === 'Str') out[out.length - 1] = { t: 'Str', c: (last.c as string) + (inline.c as string) }
    else out.push(inline)
  }
  return out
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
 * Markdown typed out and just closed, which becomes formatting:
 * `**bold**`, `*italic*`, `__bold__`, `_italic_`, `~~strikethrough~~`,
 * `` `code` `` and `[text](url)` (not an image's `![text](url)`). An
 * underscore inside a word (snake_case) opens nothing. The marks' characters go; what's typed next is in the
 * style the text before the opening mark had.
 */
const RULES: [RegExp, number, number][] = [
  [/(?<![*\\])\*\*([^*\s](?:[^*]*[^*\s])?)\*\*$/, STRONG, 2],
  [/(?<![*\\])\*([^*\s](?:[^*]*[^*\s])?)\*$/, EMPH, 1],
  [/(?<![\w\\])__([^_\s](?:[^_]*[^_\s])?)__$/, STRONG, 2],
  [/(?<![\w\\])_([^_\s](?:[^_]*[^_\s])?)_$/, EMPH, 1],
  [/(?<!\\)~~([^~\s](?:[^~]*[^~\s])?)~~$/, STRIKEOUT, 2],
  [/(?<!\\)`([^`\s](?:[^`]*[^`\s])?)`$/, CODE, 1],
]
const LINK = /(?<![\\!])\[([^\]]+)\]\(([^)\s]*)\)$/

/**
 * The inline rule for the body (see `Schema.inlineRule`): `line` (with its
 * prefix) after a character was typed at `caret`, with the markdown that
 * typing closed made formatting, or null.
 */
export function inlineRule(line: string, caret: number): { line: string; caret: number } | null {
  let p = 0
  while (p < line.length && line[p] >= '﷐' && line[p] <= '﷯') p++
  const prefix = line.slice(0, p)
  const scanned = scan(line.slice(p))
  const chars = scanned.chars.map((c) => ({ text: c.char, ...c.style }))
  const caretIndex = scanned.stops.filter((s) => s.offset <= caret - p).pop()?.index ?? 0
  const typed = chars[caretIndex - 1]
  // Not in code: there, markdown is what's typed.
  if (!typed || typed.marks & (1 << CODE)) return null
  // The text before the caret, back to the last code (whose characters are its own).
  let from = caretIndex
  while (from > 0 && !(chars[from - 1].marks & (1 << CODE))) from--
  const before = chars.slice(from, caretIndex).map((c) => c.text).join('')

  const link = LINK.exec(before)
  if (link) {
    const at = from + link.index
    const outer = { marks: chars[at].marks, link: chars[at].link }
    if (outer.link !== null) return null
    const inner = chars.slice(at + 1, at + 1 + link[1].length).map((c) => ({ ...c, link: link[2] }))
    const runs = [...chars.slice(0, at), ...inner, ...chars.slice(caretIndex)]
    const out = serialize(runs, { index: at + inner.length, style: outer })
    return { line: prefix + out.text, caret: p + out.caret }
  }
  for (const [re, format, length] of RULES) {
    const match = re.exec(before)
    if (!match) continue
    const at = from + match.index
    const outer = { marks: chars[at].marks, link: chars[at].link }
    const inner = chars
      .slice(at + length, at + length + match[1].length)
      .map((c) => ({ ...c, marks: c.marks | (1 << format) }))
    const runs = [...chars.slice(0, at), ...inner, ...chars.slice(caretIndex)]
    const out = serialize(runs, { index: at + inner.length, style: outer })
    return { line: prefix + out.text, caret: p + out.caret }
  }
  return null
}

/** The marks that start and end `format` around `text`: for tests and fixtures. */
export function formatted(format: number, text: string): string {
  return openMark(format) + text + closeMark(format)
}
