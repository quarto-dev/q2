/**
 * The editing commands on formatted text (see `marks.ts`): what each does
 * to a line's marks and to the caret.
 */

import { describe, expect, it } from 'vitest'
import {
  activeMarks,
  deleteBackward,
  deleteForward,
  enter,
  fromPage,
  insertText,
  linkAround,
  select,
  setLink,
  stepCaret,
  toggleMark,
} from './commands.ts'
import { closeMark, LINK_CLOSE, LINK_OPEN, LINK_TEXT, openMark, withoutMarks } from './marks.ts'
import { caret, contentChanged, range, selectedText, type Schema, type State } from './model.ts'

const B = 0
const I = 1
const C = 5
const schema: Schema = {
  order: ['body'],
  multiline: ['body'],
  marks: { fields: ['body'], twoSided: 1 << C, leaf: 1 << C },
  continued: (p) => p === '﷘',
}

const bold = (text: string) => openMark(B) + text + closeMark(B)
const italic = (text: string) => openMark(I) + text + closeMark(I)
const code = (text: string) => openMark(C) + text + closeMark(C)
const link = (url: string, text: string) => LINK_OPEN + url + LINK_TEXT + text + LINK_CLOSE

const at = (body: string, offset: number): State => ({ fields: { body }, selection: caret('body', offset) })
const type = (state: State, text: string) => insertText(schema, state, text)

describe('typing', () => {
  it('at the end of bold carries bold on', () => {
    const text = `a ${bold('bold')} b`
    // The caret inside bold's end, where a click or arrow puts it.
    const next = type(at(text, 7), 'er')
    expect(next.fields.body).toBe(`a ${bold('bolder')} b`)
  })

  it('outside code, after it, is plain', () => {
    const text = `${code('x')}y`
    expect(type(at(text, 3), 'z').fields.body).toBe(`${code('x')}zy`)
    expect(type(at(text, 2), 'z').fields.body).toBe(`${code('xz')}y`)
  })

  it('over a selection takes the style of its first character', () => {
    const text = `a${bold('bc')}d`
    const next = insertText(schema, { fields: { body: text }, selection: range('body', 1, 'body', 3) }, 'X')
    expect(next.fields.body).toBe(`a${bold('Xc')}d`)
  })
})

describe('deleting', () => {
  it('keeps the formatting of what follows a deleted selection', () => {
    const text = `a${bold('bc')}d`
    const next = insertText(schema, { fields: { body: text }, selection: range('body', 0, 'body', 3) }, '')
    expect(next.fields.body).toBe(`${bold('c')}d`)
  })

  it("deletes the visible character before the caret, never a mark", () => {
    const text = `${bold('a')}b`
    const next = deleteBackward(schema, at(text, 4))
    expect(next.fields.body).toBe(bold('a'))
    // Backspace into bold types bold.
    expect(type(next, 'x').fields.body).toBe(bold('ax'))
  })

  it('leaves the format on after deleting its last character', () => {
    const next = deleteBackward(schema, at(`a${bold('b')}`, 3))
    expect(withoutMarks(next.fields.body!)).toBe('a')
    expect(type(next, 'x').fields.body).toBe(`a${bold('x')}`)
  })

  it('forward, deletes the visible character after the caret', () => {
    expect(deleteForward(schema, at(`a${bold('bc')}`, 1)).fields.body).toBe(`a${bold('c')}`)
  })

  it('at the start of a line with marks joins it to the line before', () => {
    const next = deleteBackward(schema, at(`a\n${bold('b')}`, 3))
    expect(next.fields.body).toBe(`a${bold('b')}`)
    expect(next.selection).toEqual(caret('body', 1))
  })
})

describe('Enter', () => {
  it('carries bold on to the line it starts, but not code', () => {
    const boldLine = enter(schema, at(bold('ab'), 3))
    expect(withoutMarks(boldLine.fields.body!)).toBe('ab\n')
    expect(type(boldLine, 'x').fields.body).toBe(`${bold('ab')}\n${bold('x')}`)
    const codeLine = enter(schema, at(code('ab'), 3))
    expect(type(codeLine, 'x').fields.body).toBe(`${code('ab')}\nx`)
  })

  it('splits formatted text, keeping its style on both halves', () => {
    const next = enter(schema, at(bold('abcd'), 3))
    expect(next.fields.body).toBe(`${bold('ab')}\n${bold('cd')}`)
    expect(next.selection).toEqual(caret('body', 6))
  })
})

describe('toggleMark', () => {
  it('at a collapsed caret turns a format on for what is typed next', () => {
    const on = toggleMark(schema, at('ab', 1), B)
    expect(contentChanged(on.fields, { body: 'ab' })).toBe(false)
    expect(type(on, 'x').fields.body).toBe(`a${bold('x')}b`)
    const off = toggleMark(schema, on, B)
    expect(type(off, 'x').fields.body).toBe('axb')
  })

  it('over a selection turns it on, or off when all of it has it, keeping the selection', () => {
    const state: State = { fields: { body: 'abcd' }, selection: range('body', 1, 'body', 3) }
    const on = toggleMark(schema, state, B)
    expect(on.fields.body).toBe(`a${bold('bc')}d`)
    expect(selectedText(schema, on.fields, on.selection!)).toBe('bc')
    expect(toggleMark(schema, on, B).fields.body).toBe('abcd')
  })

  it('across lines', () => {
    const state: State = { fields: { body: 'ab\ncd' }, selection: range('body', 1, 'body', 4) }
    const on = toggleMark(schema, state, I)
    expect(on.fields.body).toBe(`a${italic('b')}\n${italic('c')}d`)
    expect(selectedText(schema, on.fields, on.selection!)).toBe('b\nc')
  })
})

describe('activeMarks', () => {
  it('is the formats of what typing gives, or of all of the selection', () => {
    expect(activeMarks(schema, at(bold('ab'), 2))).toBe(1 << B)
    const state: State = { fields: { body: `${bold('a')}b` }, selection: range('body', 0, 'body', 4) }
    expect(activeMarks(schema, state)).toBe(0)
  })
})

describe('links', () => {
  const text = `see ${link('https://q.org', 'Quarto')} now`
  const start = 4 + 1 + 'https://q.org'.length + 1

  it('the link around the caret, at either end too', () => {
    expect(linkAround(schema, at(text, start + 2))?.url).toBe('https://q.org')
    expect(linkAround(schema, at(text, start))?.url).toBe('https://q.org')
    expect(linkAround(schema, at(text, 1))).toBeNull()
  })

  it("typing at a link's end isn't in the link", () => {
    const end = start + 'Quarto'.length + 1
    expect(type(at(text, end), 's').fields.body).toBe(`see ${link('https://q.org', 'Quarto')}s now`)
  })

  it('makes text a link, and no link', () => {
    const made = setLink(schema, at('see Quarto', 4), { from: { field: 'body', offset: 4 }, to: { field: 'body', offset: 10 } }, 'u')
    expect(made.fields.body).toBe(`see ${link('u', 'Quarto')}`)
    const span = linkAround(schema, select(schema, made, caret('body', 8)))!
    expect(setLink(schema, made, span, null).fields.body).toBe('see Quarto')
  })

  it('replaces the text of a link', () => {
    const span = linkAround(schema, at(text, start + 1))!
    expect(setLink(schema, at(text, start + 1), span, 'https://q.org', 'Q2').fields.body).toBe(
      `see ${link('https://q.org', 'Q2')} now`,
    )
  })
})

describe('stepCaret', () => {
  it('at the edge of code moves inside or outside it before moving on', () => {
    const text = `a${code('b')}c`
    // Before the code, outside it.
    const outside = at(text, 1)
    const inside = stepCaret(schema, outside, 1)!
    expect(inside.selection).toEqual(caret('body', 2))
    const past = stepCaret(schema, inside, 1)!
    expect(past.selection).toEqual(caret('body', 3))
    const out = stepCaret(schema, past, 1)!
    expect(out.selection).toEqual(caret('body', 4))
    expect(stepCaret(schema, out, -1)!.selection).toEqual(caret('body', 3))
  })

  it("is the browser's to do at the end of the line", () => {
    expect(stepCaret(schema, at(`${code('b')}`, 3), 1)).toBeNull()
    expect(stepCaret(schema, at('plain', 2), 1)).toBeNull()
  })
})

describe('fromPage', () => {
  it("keeps the model's side of an edge when the page has the caret at the same place", () => {
    const state = at(`${code('b')}c`, 3)
    expect(fromPage(schema, state, caret('body', 2))).toEqual(caret('body', 3))
  })

  it('carries bold on at a caret the page moved', () => {
    const state = at(`x ${bold('b')}c`, 0)
    expect(fromPage(schema, state, caret('body', 5))).toEqual(caret('body', 4))
  })
})

describe('outdenting a line (Schema.outdent)', () => {
  const ITEM = '﷘'
  const IN = '﷛'
  const nested: Schema = { ...schema, continued: (p) => p.startsWith(ITEM), outdent: (p) => (p.length > 1 ? p.slice(0, -1) : '') }

  it('Enter on an empty line with a prefix carried on gives it what outdent says', () => {
    const next = enter(nested, at(`${ITEM}a\n${ITEM}${IN}${IN}`, 6))
    expect(next.fields.body).toBe(`${ITEM}a\n${ITEM}${IN}`)
    expect(next.selection).toEqual(caret('body', 5))
    expect(enter(nested, at(`${ITEM}a\n${ITEM}`, 4)).fields.body).toBe(`${ITEM}a\n`)
  })

  it('Backspace at the start of a line\'s text gives its prefix what outdent says', () => {
    const next = deleteBackward(nested, at(`${ITEM}a\n${ITEM}${IN}${bold('b')}`, 6))
    expect(next.fields.body).toBe(`${ITEM}a\n${ITEM}${bold('b')}`)
    expect(next.selection).toEqual(caret('body', 5))
    expect(deleteBackward(nested, at(`${ITEM}a\n${ITEM}b`, 4)).fields.body).toBe(`${ITEM}a\nb`)
  })
})
