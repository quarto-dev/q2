/**
 * Inline formatting, held in a line's text as characters the editor keeps
 * out of sight (from the Unicode private use area), so that formatting is
 * a text edit like any other: undo, collaborators' edits and the caret
 * handle it without knowing about bold.
 *
 * A format (up to 32, numbered; what each means is up to the schema) is
 * on from its open mark (`openMark`) to its close mark (`closeMark`), or
 * the end of the line. A link is on from `LINK_OPEN`, its address and
 * `LINK_TEXT` to `LINK_CLOSE`: the address is part of the opening mark,
 * and, like every mark, is never shown and never holds the caret. So each
 * of a line's visible characters has a style: the formats it's in, and
 * the address of the link it's in.
 *
 * A line's marks are kept canonical (`serialize`): only where the style
 * changes, the link's close first, then the formats' closes from the
 * highest-numbered down, then the formats' opens from the lowest up, then
 * the link's open. Where two characters' styles differ, the caret can be
 * at any of the offsets between the marks there, each with a style of its
 * own; typing there gives the typed text that style. So the schema can
 * give a format two caret positions at each of its edges, inside and
 * outside (`twoSided`, numbered highest so the outside is reachable), and
 * a line can hold a style no character has yet: an empty run, between
 * marks that open and close at once, which is what typing at the caret
 * would have after a format is turned on there. An empty run stays only
 * while the caret is in it.
 */

const OPEN = 0xe000
const CLOSE = 0xe020

/** Where a link's opening mark, and its address, start. */
export const LINK_OPEN = ''
/** Where a link's address ends, and its text starts. */
export const LINK_TEXT = ''
/** Where a link ends. */
export const LINK_CLOSE = ''

/** The character that starts `format`. */
export function openMark(format: number): string {
  return String.fromCharCode(OPEN + format)
}

/** The character that ends `format`. */
export function closeMark(format: number): string {
  return String.fromCharCode(CLOSE + format)
}

/** Whether `char` is one of the characters marks are made of. */
export function isMarkChar(char: string): boolean {
  return char.length === 1 && char >= '' && char <= LINK_CLOSE
}

const MARKS = /[^\n]*|[-]/g

/** Whether `text` has any marks. */
export function hasMarks(text: string): boolean {
  return /[-]/.test(text)
}

/** `text` without its marks (and links' addresses): what's shown of it. */
export function withoutMarks(text: string): string {
  return text.replace(MARKS, '')
}

/** The formats (a bit for each) and the link a character is in. */
export interface Style {
  readonly marks: number
  readonly link: string | null
}

export const PLAIN: Style = { marks: 0, link: null }

export function sameStyle(a: Style, b: Style): boolean {
  return a.marks === b.marks && a.link === b.link
}

/** Text in one style, and where it starts in its line. An empty run holds no text. */
export interface Run extends Style {
  readonly text: string
  readonly start: number
}

/** An offset the caret can be at: how many visible characters precede it, and the style there. */
export interface Stop {
  readonly offset: number
  readonly index: number
  readonly style: Style
}

/** A line of text (with no prefix and no line break) read for its marks. */
export interface Scanned {
  /** Each visible character, where it is, and its style. */
  readonly chars: readonly { char: string; offset: number; style: Style }[]
  /** Every offset the caret can be at, in order. */
  readonly stops: readonly Stop[]
}

/** Read the marks of `text`, one line without its prefix. */
export function scan(text: string): Scanned {
  const chars: { char: string; offset: number; style: Style }[] = []
  const stops: Stop[] = []
  let marks = 0
  let link: string | null = null
  let style: Style = PLAIN
  let index = 0
  let i = 0
  const stop = () => stops.push({ offset: i, index, style })
  stop()
  while (i < text.length) {
    const char = text[i]
    const code = char.charCodeAt(0)
    if (char === LINK_OPEN) {
      const end = text.indexOf(LINK_TEXT, i)
      if (end < 0) {
        // An address with no text after it: the rest of the line is hidden.
        i = text.length
        stop()
        break
      }
      link = text.slice(i + 1, end)
      i = end + 1
    } else if (char === LINK_CLOSE) {
      link = null
      i++
    } else if (char === LINK_TEXT) {
      i++
    } else if (code >= OPEN && code < OPEN + 32) {
      marks |= 1 << (code - OPEN)
      i++
    } else if (code >= CLOSE && code < CLOSE + 32) {
      marks &= ~(1 << (code - CLOSE))
      i++
    } else {
      chars.push({ char, offset: i, style })
      index++
      i++
      stop()
      continue
    }
    if (style.marks !== marks || style.link !== link) style = { marks, link }
    stop()
  }
  return { chars, stops }
}

/** The runs of `text` (one line, without its prefix): a run per stretch of one style, none empty. */
export function runsOf(text: string): Run[] {
  const runs: Run[] = []
  for (const { char, offset, style } of scan(text).chars) {
    const last = runs[runs.length - 1]
    if (last && sameStyle(last, style) && last.start + last.text.length === offset) {
      runs[runs.length - 1] = { ...last, text: last.text + char }
    } else runs.push({ ...style, text: char, start: offset })
  }
  return runs
}

/** The marks that change the style from `from` to `to`, in canonical order. */
export function transition(from: Style, to: Style): string {
  let out = ''
  if (from.link !== null && from.link !== to.link) out += LINK_CLOSE
  for (let f = 31; f >= 0; f--) if (from.marks & (1 << f) && !(to.marks & (1 << f))) out += closeMark(f)
  for (let f = 0; f < 32; f++) if (!(from.marks & (1 << f)) && to.marks & (1 << f)) out += openMark(f)
  if (to.link !== null && to.link !== from.link) out += LINK_OPEN + to.link + LINK_TEXT
  return out
}

/** Where the caret goes in `serialize`'s text: before the `index`th visible character, in `style`. */
export interface CaretSpot {
  readonly index: number
  readonly style: Style
}

/**
 * The text of a line of `runs` (in order; their `start` is ignored), with
 * canonical marks. Runs of one style next to each other are one, and
 * empty runs are left out, except that the caret, when given, is put in
 * its style: inside a run of that style at its place, or else in an empty
 * run of its own. Returns where the caret is in the text, too.
 */
export function serialize(runs: readonly { text: string; marks: number; link: string | null }[], caret?: CaretSpot): { text: string; caret: number } {
  // The runs, merged, with the caret's run at its place.
  const pieces: { text: string; style: Style; caret?: number }[] = []
  let index = 0
  const push = (text: string, style: Style) => {
    const last = pieces[pieces.length - 1]
    if (last && sameStyle(last.style, style)) last.text += text
    else pieces.push({ text, style })
  }
  let placed = caret === undefined
  const place = () => {
    const last = pieces[pieces.length - 1]
    if (last && sameStyle(last.style, caret!.style)) last.caret = last.text.length
    else pieces.push({ text: '', style: caret!.style, caret: 0 })
    placed = true
  }
  for (const run of runs) {
    if (run.text === '') continue
    const style = { marks: run.marks, link: run.link }
    if (!placed && caret!.index < index + run.text.length) {
      const split = caret!.index - index
      if (split > 0) push(run.text.slice(0, split), style)
      // The caret at the start of a run of its style goes in that run.
      if (sameStyle(style, caret!.style)) {
        if (split === 0) pieces.push({ text: '', style, caret: 0 })
        else pieces[pieces.length - 1].caret = pieces[pieces.length - 1].text.length
        placed = true
        push(run.text.slice(split), style)
      } else {
        place()
        push(run.text.slice(split), style)
      }
    } else push(run.text, style)
    index += run.text.length
  }
  if (!placed) place()
  let text = ''
  let at = 0
  let style = PLAIN
  for (const piece of pieces) {
    // A piece emptied by merging (the caret's, merged into the next) is skipped.
    text += transition(style, piece.style)
    if (piece.caret !== undefined) at = text.length + piece.caret
    text += piece.text
    style = piece.style
  }
  text += transition(style, PLAIN)
  return { text, caret: at }
}

/** `text` (one line, without its prefix) with canonical marks and no empty runs. */
export function canonical(text: string): string {
  return hasMarks(text) ? serialize(runsOf(text)).text : text
}

/** The stop at `offset`, or the last one before it (an offset inside a link's address). */
export function stopAt(scanned: Scanned, offset: number): Stop {
  let found = scanned.stops[0]
  for (const s of scanned.stops) {
    if (s.offset > offset) break
    found = s
  }
  return found
}

/** The stops before the `index`th visible character (after the one before it). */
export function stopsAt(scanned: Scanned, index: number): Stop[] {
  return scanned.stops.filter((s) => s.index === index)
}

/**
 * The style typing at the `index`th visible character takes when nothing
 * says otherwise: a format of one caret position per edge carries on from
 * the character before (at the start of the line, the one after); one
 * with two (`twoSided`, a bit per format) is as `side` says, the
 * character before's or after's. A link carries on only inside it.
 */
export function naturalStyle(scanned: Scanned, index: number, twoSided: number, side: 'before' | 'after'): Style {
  const before = scanned.chars[index - 1]?.style
  const after = scanned.chars[index]?.style
  const base = before ?? after ?? PLAIN
  const sided = (side === 'before' ? before : after)?.marks ?? 0
  const marks = (base.marks & ~twoSided) | (sided & twoSided)
  const link = before && after && before.link === after.link ? before.link : null
  return { marks, link }
}

/**
 * Of the stops before the `index`th visible character, the one whose style
 * is `style`, or, if none is, the closest: the same in the formats with two
 * caret positions, then in the rest, then in the link.
 */
export function bestStop(scanned: Scanned, index: number, style: Style, twoSided: number): Stop {
  const stops = stopsAt(scanned, index)
  let best = stops[0]
  let bestScore = -1
  for (const s of stops) {
    const score =
      (sameStyle(s.style, style) ? 8 : 0) +
      ((s.style.marks & twoSided) === (style.marks & twoSided) ? 4 : 0) +
      ((s.style.marks & ~twoSided) === (style.marks & ~twoSided) ? 2 : 0) +
      (s.style.link === style.link ? 1 : 0)
    if (score > bestScore) {
      best = s
      bestScore = score
    }
  }
  return best
}

/**
 * Whether the caret at `offset` is in an empty run: a style that neither
 * the character before it nor the one after it has.
 */
export function inEmptyRun(scanned: Scanned, offset: number): boolean {
  const stop = stopAt(scanned, offset)
  const before = scanned.chars[stop.index - 1]?.style
  const after = scanned.chars[stop.index]?.style
  if (!before && !after) return stop.style.marks !== 0 || stop.style.link !== null
  return !(before && sameStyle(before, stop.style)) && !(after && sameStyle(after, stop.style))
}

/** Whether the styles on either side of the `index`th visible character differ in a format of `mask`. */
export function edgeOf(scanned: Scanned, index: number, mask: number): boolean {
  const before = scanned.chars[index - 1]?.style.marks ?? 0
  const after = scanned.chars[index]?.style.marks ?? 0
  return ((before ^ after) & mask) !== 0
}

/** The marks in `text` (one line), in order: what a deletion of `text` leaves. */
export function marksIn(text: string): string {
  return text.match(MARKS)?.join('') ?? ''
}

/** A node of a line's formatting as a tree (see `tree`). */
export type InlineNode =
  | { kind: 'text'; text: string; start: number }
  | { kind: 'mark'; format: number; children: InlineNode[] }
  | { kind: 'link'; url: string; children: InlineNode[] }

/**
 * A line's runs as a tree of formats, for writing it out as nested
 * elements: a link holds whatever is in it; of the formats that start at
 * a place, the one that runs furthest holds the others; and formats of
 * `leaf` (code) are innermost, holding text only.
 */
export function tree(runs: readonly Run[], leaf = 0): InlineNode[] {
  const out: InlineNode[] = []
  for (let i = 0; i < runs.length; ) {
    let j = i + 1
    while (j < runs.length && runs[j].link === runs[i].link) j++
    const children = nest(runs, i, j, 0, leaf)
    if (runs[i].link === null) out.push(...children)
    else out.push({ kind: 'link', url: runs[i].link!, children })
    i = j
  }
  return out
}

function nest(runs: readonly Run[], i: number, j: number, open: number, leaf: number): InlineNode[] {
  const out: InlineNode[] = []
  while (i < j) {
    const rest = runs[i].marks & ~open
    if (rest === 0) {
      out.push({ kind: 'text', text: runs[i].text, start: runs[i].start })
      i++
      continue
    }
    // The format that runs furthest from here, of those that aren't leaves.
    let format = -1
    let end = i
    for (let f = 0; f < 32; f++) {
      if (!(rest & (1 << f)) || leaf & (1 << f)) continue
      let e = i
      while (e < j && runs[e].marks & (1 << f)) e++
      if (e > end) {
        format = f
        end = e
      }
    }
    if (format < 0) {
      // Only leaves left: the lowest-numbered, over every run with the same ones.
      format = Math.log2(rest & -rest)
      end = i
      while (end < j && (runs[end].marks & ~open) === rest) end++
    }
    out.push({ kind: 'mark', format, children: nest(runs, i, end, open | (1 << format), leaf) })
    i = end
  }
  return out
}
