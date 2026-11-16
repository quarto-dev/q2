/**
 * Pure edits of the editor's state. Each takes the state and returns the
 * next one (the same object when nothing changes).
 *
 * The rules, which hold however an edit crosses fields:
 * - Fields are never joined or split. Deleting or typing over a selection
 *   that spans fields trims each one; typed text goes into the first.
 * - Enter inserts a line break in a multi-line field. Elsewhere it moves to
 *   the start of the next field, adding it if it's absent and addable.
 * - Backspace at the start of a field moves to the end of the previous one
 *   without deleting (and Delete at the end moves to the next one). Held
 *   down (repeating), it stops there instead. In a field Backspace stops
 *   at (`Schema.backspaceStops`), it always stops there, unless the field
 *   is empty.
 * - An empty field disappears once the selection leaves it, unless it's
 *   one that stays when empty (`Schema.keepEmpty`). Backspace in an empty
 *   field removes it.
 * - A line that can't be edited (`ATOM`) stays as it is, on a line of its
 *   own: a deletion that covers it keeps it, and one that would join it
 *   to another line does nothing.
 * - A line that can't be edited but can be removed (`VOID`) is on a line of
 *   its own too, but a deletion that covers it removes it, and so does
 *   Backspace at the start of the line after it, or Delete at the end of
 *   the line before it.
 * - A line's prefix (see `isPrefix`) is never typed, and the caret never
 *   goes before it. A line joined to the one before takes that line's
 *   prefix. Enter in the middle of a line keeps the prefix on both halves;
 *   at the end of the line the new line has none; and at the start of the
 *   line's text, the new line goes above, so the prefix stays with the
 *   text. A prefix the schema carries on (`Schema.continued`) is on the
 *   new line at the end or above too, and Enter on an empty line with
 *   one takes it off.
 * - In a field that can be formatted (`Schema.marks`, see `marks.ts`),
 *   marks are never typed, pasted or deleted: a deletion keeps the marks
 *   of what it deletes, so what follows keeps its formats, and deletes
 *   visible characters only. Typed text takes the formats of the caret
 *   position it's typed at; over a selection, those of its first
 *   character. Enter carries the caret's formats on to the line it
 *   starts, except those with two caret positions at their edges (code).
 *   Every line's marks are kept canonical, with an empty run only where a
 *   collapsed caret is.
 */

import {
  bestStop,
  hasMarks,
  marksIn,
  naturalStyle,
  runsOf,
  canonical,
  PLAIN,
  sameStyle,
  scan,
  serialize,
  stopAt,
  transition,
  withoutMarks,
  type Scanned,
  type Style,
} from './marks.ts'
import {
  ATOM,
  caret,
  compare,
  isAtom,
  isCollapsed,
  isMarked,
  isPrefix,
  keepsEmpty,
  lineStartOf,
  linePrefix,
  pastPrefixes,
  VOID,
  withoutPrefixes,
  ordered,
  presentFields,
  sameSelection,
  type Fields,
  type Position,
  type Schema,
  type Selection,
  type State,
} from './model.ts'

/**
 * Replace the selection with `text`. Typed over a selection, text takes
 * the style of the selection's first character.
 */
export function insertText(schema: Schema, state: State, text: string): State {
  if (!state.selection) return state
  const { from, to } = ordered(schema, state.selection)
  if (isCollapsed(state.selection) || text === '') return replace(schema, state, from, to, text)
  return replace(schema, state, styledLikeNext(schema, state.fields, from), to, text)
}

/** `position` moved, among the caret positions at its place, to the one in the style of the character after it. */
function styledLikeNext(schema: Schema, fields: Fields, position: Position): Position {
  const line = lineAround(schema, fields, position)
  const next = line?.scanned.chars[line.stop.index]
  if (!line || !next || sameStyle(line.stop.style, next.style)) return position
  return line.at(bestStop(line.scanned, line.stop.index, next.style, 0).offset)
}

export interface DeleteOptions {
  /**
   * The key is held down, and this is one of its repeats: it deletes, but
   * stops at the field's edge rather than moving on or removing the field.
   */
  repeat?: boolean
}

/**
 * Backspace. Deletes the selection, or else `target` (the range the
 * browser would delete, clipped to the caret's field), or else the
 * grapheme before the caret. In an empty field, removes the field: the
 * caret goes to the end of the field before, or, when there's none (a
 * first field that stays when empty), nowhere.
 */
export function deleteBackward(
  schema: Schema,
  state: State,
  target?: { from: Position; to: Position },
  options: DeleteOptions = {},
): State {
  return deleteDirection(schema, state, 'backward', target, options)
}

/**
 * Add `field`, empty, if it's absent, and put the caret at its start.
 */
export function addField(schema: Schema, state: State, field: string): State {
  const fields = state.fields[field] === undefined ? { ...state.fields, [field]: '' } : state.fields
  return select(schema, { fields, selection: state.selection }, caret(field, 0))
}

/** Delete (forward). See `deleteBackward`. */
export function deleteForward(schema: Schema, state: State, target?: { from: Position; to: Position }): State {
  return deleteDirection(schema, state, 'forward', target)
}

/**
 * Paste: replace the selection with `text`, with its line breaks kept in a
 * multi-line field and flattened to single spaces elsewhere.
 */
export function paste(schema: Schema, state: State, text: string): State {
  if (!state.selection) return state
  const field = ordered(schema, state.selection).from.field
  const pasted = schema.multiline?.includes(field)
    ? text.replace(/\r\n?/g, '\n')
    : text.replace(/\s*[\r\n]+\s*/g, ' ')
  return insertText(schema, state, pasted)
}

/**
 * Enter: a line break in a multi-line field. Elsewhere, delete the
 * selection and move to the start of the next field.
 */
export function enter(schema: Schema, state: State): State {
  if (!state.selection) return state
  const field = ordered(schema, state.selection).from.field
  if (schema.multiline?.includes(field)) return lineBreak(schema, state)
  const next = schema.order
    .slice(schema.order.indexOf(field) + 1)
    .find((f) => state.fields[f] !== undefined || schema.addable?.includes(f))
  if (next === undefined) return state
  const cleared = insertText(schema, state, '')
  const fields = cleared.fields[next] === undefined ? { ...cleared.fields, [next]: '' } : cleared.fields
  return select(schema, { fields, selection: cleared.selection }, caret(next, 0))
}

/**
 * Enter in a multi-line field: a line break, which keeps a line's prefix
 * on both halves of a line split in the middle, gives none to the line
 * started at its end, and goes before the prefix at the start of its text.
 * A prefix the schema carries on (`Schema.continued`) is on the line
 * started at the end, or put above at the start, too; and Enter on an
 * empty line with one takes it off.
 */
function lineBreak(schema: Schema, state: State): State {
  const cleared = insertText(schema, state, '')
  const marked = lineAround(schema, cleared.fields, cleared.selection!.focus, false)
  if (marked) return formattedLineBreak(schema, cleared, marked)
  const at = cleared.selection!.focus
  const text = cleared.fields[at.field] ?? ''
  const start = lineStartOf(text, at.offset)
  const prefix = linePrefix(text, at.offset)
  const lineEnd = text.indexOf('\n', at.offset) < 0 ? text.length : text.indexOf('\n', at.offset)
  const textStart = start + prefix.length
  const continued = prefix !== '' && (schema.continued?.(prefix) ?? false)
  const fields = { ...cleared.fields }
  if (continued && textStart === lineEnd) {
    fields[at.field] = text.slice(0, start) + text.slice(textStart)
    return select(schema, { fields, selection: cleared.selection }, caret(at.field, start))
  }
  if (prefix === '' || (!continued && at.offset === lineEnd && at.offset > textStart)) {
    return insertText(schema, cleared, '\n')
  }
  if (at.offset === textStart) {
    const above = continued ? prefix : ''
    fields[at.field] = text.slice(0, start) + above + '\n' + text.slice(start)
    return select(schema, { fields, selection: cleared.selection }, caret(at.field, at.offset + above.length + 1))
  }
  fields[at.field] = text.slice(0, at.offset) + '\n' + prefix + text.slice(at.offset)
  return select(schema, { fields, selection: cleared.selection }, caret(at.field, at.offset + 1 + prefix.length))
}

/**
 * Enter in a line that can be formatted: as `lineBreak`, and the line
 * started takes the caret's formats (except those with two caret positions
 * at their edges, code) for what's typed there, and the link if the caret
 * is inside one. Text that moves to the new line keeps its style.
 */
function formattedLineBreak(schema: Schema, state: State, line: LineAround): State {
  const at = state.selection!.focus
  const text = state.fields[at.field] ?? ''
  const { prefix, textStart, scanned, stop } = line
  const lineStart = textStart - prefix.length
  const lineText = text.slice(textStart, textStart + line.length)
  const after = text.slice(textStart + line.length)
  const count = scanned.chars.length
  const continued = prefix !== '' && (schema.continued?.(prefix) ?? false)
  const fields = { ...state.fields }
  if (continued && count === 0) {
    fields[at.field] = text.slice(0, lineStart) + text.slice(textStart)
    return select(schema, { fields, selection: state.selection }, caret(at.field, at.offset - prefix.length))
  }
  if (prefix !== '' && stop.index === 0) {
    const above = continued ? prefix : ''
    fields[at.field] = text.slice(0, lineStart) + above + '\n' + text.slice(lineStart)
    return select(schema, { fields, selection: state.selection }, caret(at.field, at.offset + above.length + 1))
  }
  const ownPrefix = !continued && stop.index === count && stop.index > 0 ? '' : prefix
  const head = canonical(lineText.slice(0, stop.offset))
  const carried: Style = {
    marks: stop.style.marks & ~(schema.marks?.twoSided ?? 0),
    link: stop.index < count ? stop.style.link : null,
  }
  const rest = serialize(runsOf(transition(PLAIN, stop.style) + lineText.slice(stop.offset)), { index: 0, style: carried })
  fields[at.field] = text.slice(0, textStart) + head + '\n' + ownPrefix + rest.text + after
  const offset = textStart + head.length + 1 + ownPrefix.length + rest.caret
  return select(schema, { fields, selection: state.selection }, caret(at.field, offset))
}

/**
 * Apply the schema's input rule to the text before a collapsed caret:
 * replace it with what the rule gives; or else its inline rule to the
 * caret's line (the same state when neither gives anything).
 */
export function applyInputRule(schema: Schema, state: State): State {
  const { selection, fields } = state
  if (!selection || !isCollapsed(selection)) return state
  const { field, offset } = selection.focus
  const text = fields[field] ?? ''
  const start = lineStartOf(text, offset)
  const replacement = schema.inputRule?.(field, text.slice(start, offset)) ?? null
  if (replacement !== null) {
    const next = { ...fields, [field]: text.slice(0, start) + replacement + text.slice(offset) }
    return select(schema, { fields: next, selection }, caret(field, start + replacement.length))
  }
  if (!schema.inlineRule || !isMarked(schema, field)) return state
  const end = text.indexOf('\n', offset) < 0 ? text.length : text.indexOf('\n', offset)
  const ruled = schema.inlineRule(field, text.slice(start, end), offset - start)
  if (ruled === null) return state
  const next = { ...fields, [field]: text.slice(0, start) + ruled.line + text.slice(end) }
  return select(schema, { fields: next, selection }, caret(field, start + ruled.caret))
}

/**
 * Give each line the selection is on, in a multi-line field, the prefix
 * `prefixFor` returns for the prefix it has (a line's kind, changed from a
 * toolbar, say). Lines that can't be edited (`ATOM`, `VOID`) keep theirs.
 * The selection stays on the same text.
 */
export function setLinePrefixes(schema: Schema, state: State, prefixFor: (prefix: string) => string): State {
  const { selection } = state
  if (!selection) return state
  const { from, to } = ordered(schema, selection)
  const first = schema.order.indexOf(from.field)
  const last = schema.order.indexOf(to.field)
  const fields: Record<string, string | undefined> = { ...state.fields }
  // For each changed field, where its lines started and how far each moved.
  const shifts = new Map<string, { start: number; shift: number }[]>()
  for (let i = first; i <= last; i++) {
    const field = schema.order[i]
    const value = fields[field]
    if (value === undefined || !schema.multiline?.includes(field)) continue
    const start = i === first ? from.offset : 0
    const end = i === last ? to.offset : value.length
    let out = ''
    let at = 0
    let shift = 0
    const lines: { start: number; shift: number }[] = []
    for (const line of value.split('\n')) {
      const prefix = linePrefix(line, 0)
      const covered = at <= end && at + line.length >= start && !isAtom(line)
      const next = covered ? prefixFor(prefix) : prefix
      shift += next.length - prefix.length
      lines.push({ start: at, shift })
      out += (at > 0 ? '\n' : '') + next + line.slice(prefix.length)
      at += line.length + 1
    }
    if (out !== value) {
      fields[field] = out
      shifts.set(field, lines)
    }
  }
  if (shifts.size === 0) return state
  const moved = (p: Position): Position => {
    const lines = shifts.get(p.field)
    if (!lines) return p
    let line = 0
    while (line + 1 < lines.length && lines[line + 1].start <= p.offset) line++
    return { field: p.field, offset: p.offset + lines[line].shift }
  }
  return select(schema, { fields, selection }, { anchor: moved(selection.anchor), focus: moved(selection.focus) })
}

/**
 * Move the selection, removing any empty field it leaves. Neither end of
 * it stays before a line's prefix.
 */
export function select(schema: Schema, state: State, selection: Selection | null): State {
  const canonical = canonicalMarks(schema, removeEmpty(schema, state.fields, selection), pastPrefixes(state.fields, selection))
  const fields = canonical.fields
  selection = pastPrefixes(fields, canonical.selection)
  if (fields === state.fields && sameSelection(selection, state.selection)) return state
  return { fields, selection }
}

/**
 * Select all: the first press selects the field with the caret, the next
 * every field.
 */
export function selectAllStep(schema: Schema, state: State): State {
  const { selection, fields } = state
  if (!selection) return state
  const { from, to } = ordered(schema, selection)
  const length = (field: string) => (fields[field] ?? '').length
  const wholeField = from.field === to.field && from.offset === 0 && to.offset === length(to.field)
  if (!wholeField && from.field === to.field) {
    return select(schema, state, {
      anchor: { field: from.field, offset: 0 },
      focus: { field: from.field, offset: length(from.field) },
    })
  }
  const present = presentFields(schema, fields)
  const last = present[present.length - 1]
  return select(schema, state, {
    anchor: { field: present[0], offset: 0 },
    focus: { field: last, offset: length(last) },
  })
}

function deleteDirection(
  schema: Schema,
  state: State,
  direction: 'backward' | 'forward',
  target?: { from: Position; to: Position },
  { repeat = false }: DeleteOptions = {},
): State {
  const { selection, fields } = state
  if (!selection) return state
  if (!isCollapsed(selection)) return insertText(schema, state, '')

  // In a line with marks, the caret before its first visible character
  // (or after its last) is at the edge of the line, whatever marks are
  // around it; and what's deleted is visible text, never a mark.
  const line = lineAround(schema, fields, selection.focus)
  const backward = direction === 'backward'
  const visibleEdge = line && (backward ? line.stop.index === 0 : line.stop.index === line.scanned.chars.length)
  const at = visibleEdge ? line.at(backward ? 0 : line.length) : selection.focus
  const text = fields[at.field] ?? ''
  if (backward ? at.offset === 0 : at.offset === text.length) {
    if (repeat) return state
    // At the field's edge: move to the neighbouring field, never joining.
    const present = presentFields(schema, fields)
    const neighbour = present[present.indexOf(at.field) + (backward ? -1 : 1)]
    const neighbourEnd = (f: string) => caret(f, backward ? (fields[f] ?? '').length : 0)
    if (backward && text === '' && keepsEmpty(schema, at.field)) {
      // Backspace in an empty field that stays when empty: remove it.
      const rest = { ...fields }
      delete rest[at.field]
      return select(schema, { fields: rest, selection }, neighbour === undefined ? null : neighbourEnd(neighbour))
    }
    if (neighbour === undefined) return state
    if (backward && text !== '' && schema.backspaceStops?.includes(at.field)) return state
    return select(schema, state, neighbourEnd(neighbour))
  }

  // What to delete: the range the browser would, within the caret's line,
  // or else the grapheme (or line break) next to the caret.
  const lineStart = lineStartOf(text, at.offset)
  const lineEnd = text.indexOf('\n', at.offset) < 0 ? text.length : text.indexOf('\n', at.offset)
  let start = 0
  let end = 0
  if (target) {
    start = Math.max(lineStart, target.from.field === at.field ? target.from.offset : 0)
    end = Math.min(lineEnd, target.to.field === at.field ? target.to.offset : text.length)
  }
  if (start < end && withoutMarks(text.slice(start, end)) !== '') {
    // The browser's range.
  } else if (backward && at.offset === lineStart && lineStart > 0 && lineAt(text, lineStart - 1) === VOID) {
    // The line before can be removed: remove it, and its line break.
    start = lineStart - 1 - VOID.length
    end = lineStart
  } else if (!backward && at.offset === lineEnd && lineEnd < text.length && lineAt(text, lineEnd + 1) === VOID) {
    start = lineEnd
    end = lineEnd + 1 + VOID.length
  } else if (line && !visibleEdge) {
    // A grapheme of the line's visible text.
    const visible = line.scanned.chars.map((c) => c.char).join('')
    const index = line.stop.index
    const length = backward ? graphemeBefore(visible, index) : graphemeAfter(visible, index)
    const first = line.scanned.chars[backward ? index - length : index]
    const last = line.scanned.chars[backward ? index - 1 : index + length - 1]
    start = line.at(first.offset).offset
    end = line.at(last.offset + 1).offset
  } else if (backward) {
    start = at.offset - graphemeBefore(text, at.offset)
    end = at.offset
  } else {
    start = at.offset
    end = at.offset + graphemeAfter(text, at.offset)
  }
  if (start >= end) return state
  const next = replace(schema, state, { field: at.field, offset: start }, { field: at.field, offset: end }, '')
  if (!line) return next
  // Deleting backward the last character of a format leaves the format on
  // for what's typed next; otherwise the caret takes the formats of the
  // character it now follows, so Backspace into bold types bold.
  const gone = line.scanned.chars[line.stop.index - 1]?.style
  const before = start >= line.textStart ? line.scanned.chars[stopAt(line.scanned, start - line.textStart).index - 1]?.style : undefined
  const keep = backward && !visibleEdge && gone !== undefined && (!before || (gone.marks & ~before.marks) !== 0)
  return keep ? next : naturalCaret(schema, next)
}

/**
 * `state` with a collapsed caret in a line that can be formatted moved,
 * among the caret positions at its place, to the one typing there would
 * naturally take (see `naturalStyle`: the formats of the character before
 * it).
 */
function naturalCaret(schema: Schema, state: State): State {
  const { selection } = state
  if (!selection || !isCollapsed(selection)) return state
  const line = lineAround(schema, state.fields, selection.focus)
  if (!line) return state
  const style = naturalStyle(line.scanned, line.stop.index, schema.marks?.twoSided ?? 0, 'before')
  const stop = bestStop(line.scanned, line.stop.index, style, schema.marks?.twoSided ?? 0)
  return select(schema, state, caret(selection.focus.field, line.at(stop.offset).offset))
}

/** The line of `text` that `offset` is on. */
function lineAt(text: string, offset: number): string {
  const end = text.indexOf('\n', offset)
  return text.slice(lineStartOf(text, offset), end < 0 ? text.length : end)
}

/**
 * Replace the text between `from` and `to` (in reading order), keeping
 * every field: the first keeps what precedes `from`, plus `text`; the last
 * keeps what follows `to`; fields in between are emptied.
 */
function replace(schema: Schema, state: State, from: Position, to: Position, typed: string): State {
  const text = withoutPrefixes(typed.replaceAll(ATOM, '').replaceAll(VOID, '')).replace(MARK_CHARS, '')
  const fields: Record<string, string | undefined> = { ...state.fields }
  const first = schema.order.indexOf(from.field)
  const last = schema.order.indexOf(to.field)
  let offset = from.offset + text.length
  for (let i = first; i <= last; i++) {
    const field = schema.order[i]
    const value = fields[field]
    if (value === undefined) continue
    const start = i === first ? from.offset : 0
    const end = i === last ? to.offset : value.length
    const atoms = ATOM.repeat(value.slice(start, end).split(ATOM).length - 1)
    // The deleted text's marks stay, so what follows keeps its style.
    const head = value.slice(0, start) + (i === first ? text : '')
    const before = head + atoms + marksIn(value.slice(start, end))
    // A line joined to the one before takes that line's prefix, unless
    // that line is left empty.
    let after = value.slice(end)
    if (value.slice(start, end).includes('\n') && !(head === '' || head.endsWith('\n'))) {
      let prefixEnd = 0
      while (prefixEnd < after.length && isPrefix(after[prefixEnd])) prefixEnd++
      after = after.slice(prefixEnd)
    }
    const next = before + after
    if (![...next].some(isAtom)) fields[field] = next
    else {
      const apart = atomsApart(next, i === first ? offset : 0)
      fields[field] = apart.text
      if (i === first) offset = apart.offset
    }
  }
  return select(schema, { fields, selection: state.selection }, caret(from.field, offset))
}

/**
 * `text` with every atom on a line of its own, and where `offset` is in
 * it. Text before an atom stays before the line break added there, and
 * text after one goes after it.
 */
function atomsApart(text: string, offset: number): { text: string; offset: number } {
  let out = ''
  let at = offset
  for (let i = 0; i < text.length; i++) {
    const atom = isAtom(text[i])
    const needsBreak = out !== '' && !out.endsWith('\n') && (atom || isAtom(out[out.length - 1])) && text[i] !== '\n'
    if (needsBreak) {
      // A caret between text and the atom after it stays with the text.
      if (atom ? i < offset : i <= offset) at++
      out += '\n'
    }
    out += text[i]
  }
  return { text: out, offset: at }
}

/** Remove the empty fields that don't hold an end of `selection` (and don't stay when empty). */
function removeEmpty(schema: Schema, fields: Fields, selection: Selection | null): Fields {
  const keep = new Set(selection ? [selection.anchor.field, selection.focus.field] : [])
  const empty = schema.order.filter((f) => fields[f] === '' && !keep.has(f) && !keepsEmpty(schema, f))
  if (empty.length === 0) return fields
  const next = { ...fields }
  for (const f of empty) delete next[f]
  return next
}

const MARK_CHARS = /[\ue000-\ue042]/g

const graphemes = new Intl.Segmenter(undefined, { granularity: 'grapheme' })

function graphemeBefore(text: string, offset: number): number {
  let length = 0
  for (const { index, segment } of graphemes.segment(text.slice(0, offset))) {
    if (index + segment.length === offset) length = segment.length
  }
  return length
}

function graphemeAfter(text: string, offset: number): number {
  const first = graphemes.segment(text.slice(offset))[Symbol.iterator]().next()
  return first.done ? 0 : first.value.segment.length
}

// ---------------------------------------------------------------------------
// Inline formatting (see `marks.ts`)

/** The line a position is on, in a field that can be formatted, read for its marks. */
interface LineAround {
  /** The line's prefix, and where its text (after the prefix) starts in the field. */
  prefix: string
  textStart: number
  /** The length of the line's text, after its prefix. */
  length: number
  scanned: Scanned
  /** The caret position the position is at (offsets in the line's text). */
  stop: ReturnType<typeof stopAt>
  /** The position `offset` characters into the line's text. */
  at(offset: number): Position
}

/**
 * The line `position` is on, if it's in a field that can be formatted and
 * is a line of text (not one that can't be edited); unless `anyLine` is
 * false, only if the line has marks.
 */
function lineAround(schema: Schema, fields: Fields, position: Position, anyLine = false): LineAround | null {
  if (!isMarked(schema, position.field)) return null
  const text = fields[position.field]
  if (text === undefined) return null
  const lineStart = lineStartOf(text, position.offset)
  const lineEnd = text.indexOf('\n', position.offset) < 0 ? text.length : text.indexOf('\n', position.offset)
  const prefix = linePrefix(text, position.offset)
  const textStart = lineStart + prefix.length
  const lineText = text.slice(textStart, lineEnd)
  if ([...lineText].some(isAtom) || (!anyLine && !hasMarks(lineText))) return null
  const scanned = scan(lineText)
  return {
    prefix,
    textStart,
    length: lineText.length,
    scanned,
    stop: stopAt(scanned, Math.max(0, position.offset - textStart)),
    at: (offset) => ({ field: position.field, offset: textStart + offset }),
  }
}

/**
 * The fields with each formatted line's marks canonical, and `selection`
 * moved with them. A collapsed caret keeps its style, in an empty run of
 * its own if it must be; other empty runs go.
 */
function canonicalMarks(schema: Schema, fields: Fields, selection: Selection | null): { fields: Fields; selection: Selection | null } {
  let next: Record<string, string | undefined> | null = null
  let moved = selection
  for (const field of schema.marks?.fields ?? []) {
    const text = fields[field]
    if (text === undefined || !hasMarks(text)) continue
    const collapsed = selection !== null && isCollapsed(selection)
    const ends = selection ? [selection.anchor, selection.focus].filter((p) => p.field === field) : []
    let out = ''
    let start = 0
    const offsets = new Map<Position, number>()
    for (const line of text.split('\n')) {
      const lineEnd = start + line.length
      const prefix = linePrefix(line, 0)
      // Where the line starts in the text written so far.
      const base = out.length + (start > 0 ? 1 : 0)
      let written = line
      if (hasMarks(line) && ![...line].some(isAtom)) {
        const body = line.slice(prefix.length)
        const scanned = scan(body)
        const here = ends.filter((p) => p.offset >= start && p.offset <= lineEnd)
        const caretSpot =
          collapsed && here.length > 0 ? stopAt(scanned, Math.max(0, here[0].offset - start - prefix.length)) : undefined
        const result = serialize(runsOf(body), caretSpot && { index: caretSpot.index, style: caretSpot.style })
        written = prefix + result.text
        for (const p of here) {
          if (caretSpot) offsets.set(p, base + prefix.length + result.caret)
          else {
            const stop = stopAt(scanned, Math.max(0, p.offset - start - prefix.length))
            const after = bestStop(scan(result.text), stop.index, stop.style, schema.marks?.twoSided ?? 0)
            offsets.set(p, base + prefix.length + after.offset)
          }
        }
      } else {
        for (const p of ends) if (p.offset >= start && p.offset <= lineEnd) offsets.set(p, base + (p.offset - start))
      }
      out += (start > 0 ? '\n' : '') + written
      start = lineEnd + 1
    }
    if (out === text) continue
    next ??= { ...fields }
    next[field] = out
    if (moved) {
      const map = (p: Position) => (offsets.has(p) ? { field, offset: offsets.get(p)! } : p)
      moved = { anchor: map(selection!.anchor), focus: map(selection!.focus) }
    }
  }
  return next ? { fields: next, selection: moved } : { fields, selection }
}

/** Each line the selection is on in a field that can be formatted, and the visible characters it covers there. */
function coveredLines(schema: Schema, state: State): { line: LineAround; from: number; to: number }[] {
  const { selection, fields } = state
  if (!selection) return []
  const { from, to } = ordered(schema, selection)
  const out: { line: LineAround; from: number; to: number }[] = []
  for (const field of schema.order.slice(schema.order.indexOf(from.field), schema.order.indexOf(to.field) + 1)) {
    const text = fields[field]
    if (text === undefined || !isMarked(schema, field)) continue
    const start = Math.min(field === from.field ? from.offset : 0, text.length)
    const end = Math.min(field === to.field ? to.offset : text.length, text.length)
    let lineStart = lineStartOf(text, start)
    while (lineStart <= end && lineStart <= text.length) {
      const line = lineAround(schema, fields, { field, offset: lineStart }, true)
      const lineEnd = text.indexOf('\n', lineStart) < 0 ? text.length : text.indexOf('\n', lineStart)
      if (line) {
        const a = start > line.textStart ? stopAt(line.scanned, start - line.textStart).index : 0
        const b = end < lineEnd ? stopAt(line.scanned, Math.max(0, end - line.textStart)).index : line.scanned.chars.length
        out.push({ line, from: a, to: b })
      }
      lineStart = lineEnd + 1
    }
  }
  return out
}

/**
 * The formats of what's selected (those every selected character has),
 * or, at a collapsed caret, of what typing there gives; null when the
 * selection isn't in a field that can be formatted.
 */
export function activeMarks(schema: Schema, state: State): number | null {
  const { selection } = state
  if (!selection || !isMarked(schema, selection.focus.field)) return null
  if (isCollapsed(selection)) {
    return lineAround(schema, state.fields, selection.focus, true)?.stop.style.marks ?? null
  }
  let marks = -1
  let any = false
  for (const { line, from, to } of coveredLines(schema, state)) {
    for (const c of line.scanned.chars.slice(from, to)) {
      marks &= c.style.marks
      any = true
    }
  }
  return any ? marks : 0
}

/**
 * Turn `format` (a format's number) on or off: over the selected text,
 * on unless every selected character has it, keeping the selection on
 * the same text; at a collapsed caret, for what's typed there next.
 */
export function toggleMark(schema: Schema, state: State, format: number): State {
  const { selection } = state
  if (!selection) return state
  const bit = 1 << format
  if (isCollapsed(selection)) {
    const line = lineAround(schema, state.fields, selection.focus, true)
    if (!line) return state
    const style = { marks: line.stop.style.marks ^ bit, link: line.stop.style.link }
    return restyleLine(schema, state, line, (runs) => runs, { index: line.stop.index, style })
  }
  const covered = coveredLines(schema, state)
  const chars = covered.flatMap(({ line, from, to }) => line.scanned.chars.slice(from, to))
  if (chars.length === 0) return state
  const on = !chars.every((c) => c.style.marks & bit)
  return restyleCovered(schema, state, covered, (style) => ({ ...style, marks: on ? style.marks | bit : style.marks & ~bit }))
}

/**
 * Give the visible characters `covered` (lines of the selection, see
 * `coveredLines`) the style `restyle` makes of theirs. The selection
 * stays on the same characters, its ends inside their styles.
 */
function restyleCovered(
  schema: Schema,
  state: State,
  covered: { line: LineAround; from: number; to: number }[],
  restyle: (style: Style) => Style,
): State {
  const fields: Record<string, string | undefined> = { ...state.fields }
  const twoSided = schema.marks?.twoSided ?? 0
  const backwards = compare(schema, state.selection!.anchor, state.selection!.focus) > 0
  let { from: fromAt, to: toAt } = ordered(schema, state.selection!)
  // How far each field's later lines have moved.
  const shifts = new Map<string, number>()
  covered.forEach(({ line, from: a, to: b }, i) => {
    const field = line.at(0).field
    const shift = shifts.get(field) ?? 0
    const text = fields[field] ?? ''
    const start = line.textStart + shift
    const chars = line.scanned.chars.map((c, j) => ({ text: c.char, ...(j >= a && j < b ? restyle(c.style) : c.style) }))
    const written = serialize(chars).text
    fields[field] = text.slice(0, start) + written + text.slice(start + line.length)
    shifts.set(field, shift + written.length - line.length)
    const scanned = scan(written)
    if (i === 0) fromAt = { field, offset: start + bestStop(scanned, a, chars[a] ?? chars[a - 1] ?? PLAIN, twoSided).offset }
    if (i === covered.length - 1) {
      toAt = { field, offset: start + bestStop(scanned, b, chars[b - 1] ?? chars[b] ?? PLAIN, twoSided).offset }
    }
  })
  const selection = backwards ? { anchor: toAt, focus: fromAt } : { anchor: fromAt, focus: toAt }
  return select(schema, { fields, selection: state.selection }, selection)
}

/** `state` with the caret's line rewritten from its runs as `edit` changes them, and the caret at `spot`. */
function restyleLine(
  schema: Schema,
  state: State,
  line: LineAround,
  edit: (runs: { text: string; marks: number; link: string | null }[]) => { text: string; marks: number; link: string | null }[],
  spot: { index: number; style: Style },
): State {
  const field = line.at(0).field
  const text = state.fields[field] ?? ''
  const runs = edit(line.scanned.chars.map((c) => ({ text: c.char, ...c.style })))
  const written = serialize(runs, spot)
  const fields = { ...state.fields, [field]: text.slice(0, line.textStart) + written.text + text.slice(line.textStart + line.length) }
  return select(schema, { fields, selection: state.selection }, caret(field, line.textStart + written.caret))
}

/** A link: its address, and the positions of its first visible character and just after its last. */
export interface LinkSpan {
  readonly url: string
  readonly from: Position
  readonly to: Position
}

/**
 * The link the selection is in: the one around a collapsed caret (or
 * just before or after it), or the one the whole selection is in.
 */
export function linkAround(schema: Schema, state: State): LinkSpan | null {
  const { selection } = state
  if (!selection) return null
  const { from, to } = ordered(schema, selection)
  const line = lineAround(schema, state.fields, from)
  if (!line || to.field !== from.field || to.offset > line.textStart + line.length) return null
  const { chars } = line.scanned
  const index = line.stop.index
  if (isCollapsed(selection)) {
    return linkSpanAt(line, chars[index - 1]?.style.link != null ? index - 1 : index)?.link ?? null
  }
  const span = linkSpanAt(line, index)
  const end = stopAt(line.scanned, to.offset - line.textStart).index
  return span && end <= span.end ? span.link : null
}

/** The link at the position of its first visible character, `from`. */
export function linkAt(schema: Schema, fields: Fields, from: Position): LinkSpan | null {
  const line = lineAround(schema, fields, from)
  if (!line) return null
  return linkSpanAt(line, line.stop.index)?.link ?? null
}

function linkSpanAt(line: LineAround, index: number): { link: LinkSpan; end: number } | null {
  const { chars } = line.scanned
  const url = chars[index]?.style.link
  if (url == null) return null
  let start = index
  let end = index + 1
  while (start > 0 && chars[start - 1].style.link === url) start--
  while (end < chars.length && chars[end].style.link === url) end++
  return {
    link: { url, from: line.at(chars[start].offset), to: line.at(chars[end - 1].offset + 1) },
    end,
  }
}

/**
 * Make the text from `from` to `to` (in one line) a link to `url`, or no
 * link when `url` is null. With `text`, the text is replaced by it first
 * (in the style of its first character, or the caret's when it's empty).
 * The caret goes after the link.
 */
export function setLink(
  schema: Schema,
  state: State,
  target: { from: Position; to: Position },
  url: string | null,
  text?: string,
): State {
  const line = lineAround(schema, state.fields, target.from, true)
  if (!line || target.to.field !== target.from.field) return state
  const a = stopAt(line.scanned, target.from.offset - line.textStart).index
  const b = stopAt(line.scanned, Math.max(0, target.to.offset - line.textStart)).index
  const chars = line.scanned.chars.map((c) => ({ text: c.char, ...c.style }))
  const covered = chars.slice(a, b)
  const replaced = text !== undefined && text !== covered.map((c) => c.text).join('')
  const base = covered[0] ?? line.stop.style
  const inserted = replaced
    ? [...(text ?? '')].map((char) => ({ text: char, marks: base.marks, link: url }))
    : covered.map((c) => ({ ...c, link: url }))
  const runs = [...chars.slice(0, a), ...inserted, ...chars.slice(b)]
  const end = a + inserted.length
  const after = { marks: (inserted[inserted.length - 1] ?? base).marks, link: null }
  return restyleLine(schema, state, line, () => runs, { index: end, style: after })
}

/**
 * Left or Right (`direction` -1 or 1) at a collapsed caret, in a line
 * with marks: at the edge of a format with two caret positions, from the
 * one to the other (the caret doesn't move on screen); else over a
 * grapheme, arriving on the side it came from. Null when it's the
 * browser's to do: at the line's end that way, or outside such a line.
 */
export function stepCaret(schema: Schema, state: State, direction: -1 | 1): State | null {
  const { selection } = state
  if (!selection || !isCollapsed(selection)) return null
  const line = lineAround(schema, state.fields, selection.focus)
  if (!line) return null
  const twoSided = schema.marks?.twoSided ?? 0
  const { scanned, stop } = line
  const forward = direction > 0
  const field = selection.focus.field
  // At an edge: change sides first.
  const side = naturalStyle(scanned, stop.index, twoSided, forward ? 'after' : 'before')
  if ((stop.style.marks & twoSided) !== (side.marks & twoSided)) {
    const other = bestStop(scanned, stop.index, side, twoSided)
    if (other.offset !== stop.offset) return select(schema, state, caret(field, line.at(other.offset).offset))
  }
  const visible = scanned.chars.map((c) => c.char).join('')
  if (forward ? stop.index >= visible.length : stop.index === 0) return null
  const index = stop.index + (forward ? graphemeAfter(visible, stop.index) : -graphemeBefore(visible, stop.index))
  const style = naturalStyle(scanned, index, twoSided, forward ? 'before' : 'after')
  return select(schema, state, caret(field, line.at(bestStop(scanned, index, style, twoSided).offset).offset))
}

/**
 * The selection read from the page (`read`), in terms of the model's:
 * an end at the same place as the model's (no visible character between
 * them) is the model's, which knows which side of a format's edge it's
 * on; a collapsed caret anywhere else takes the style of the character
 * before it (see `naturalStyle`), inside code at its end: what side of
 * an edge the page has it on depends on the browser.
 */
export function fromPage(schema: Schema, state: State, read: Selection | null): Selection | null {
  const was = state.selection
  if (!read) return read
  const same = (p: Position, q: Position | undefined) => {
    if (!q || p.field !== q.field) return false
    if (p.offset === q.offset) return true
    const line = lineAround(schema, state.fields, q)
    if (!line) return false
    const lineEnd = line.textStart + line.length
    if (p.offset < line.textStart - line.prefix.length || p.offset > lineEnd) return false
    return stopAt(line.scanned, Math.max(0, p.offset - line.textStart)).index === line.stop.index
  }
  const anchor = same(read.anchor, was?.anchor) ? was!.anchor : read.anchor
  const focus = same(read.focus, was?.focus) ? was!.focus : read.focus
  if (anchor !== focus && !(anchor.field === focus.field && anchor.offset === focus.offset)) return { anchor, focus }
  if (focus === was?.focus) return { anchor: focus, focus }
  const line = lineAround(schema, state.fields, focus)
  if (!line) return { anchor, focus }
  const twoSided = schema.marks?.twoSided ?? 0
  const style = naturalStyle(line.scanned, line.stop.index, twoSided, 'before')
  const at = line.at(bestStop(line.scanned, line.stop.index, style, twoSided).offset)
  return { anchor: at, focus: at }
}
