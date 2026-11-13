import { describe, expect, it } from 'vitest'
import {
  addField,
  applyInputRule,
  deleteBackward,
  deleteForward,
  enter,
  insertText,
  paste,
  select,
  selectAllStep,
  setLinePrefixes,
} from './commands.ts'
import { ATOM, VOID, caret, range, selectedText, type Fields, type Schema, type State } from './model.ts'

const schema: Schema = { order: ['title', 'subtitle'], addable: ['subtitle'] }

const state = (fields: Fields, selection: State['selection']): State => ({ fields, selection })
const both = { title: 'Quarterly report', subtitle: 'Third quarter' }

describe('insertText', () => {
  it('replaces the selection within a field', () => {
    const next = insertText(schema, state(both, range('title', 0, 'title', 9)), 'Annual')
    expect(next.fields.title).toBe('Annual report')
    expect(next.selection).toEqual(caret('title', 6))
  })

  it('puts text typed over a cross-field selection into the first field, trimming the rest', () => {
    const next = insertText(schema, state(both, range('title', 10, 'subtitle', 6)), 'X')
    expect(next.fields).toEqual({ title: 'Quarterly X', subtitle: 'quarter' })
    expect(next.selection).toEqual(caret('title', 11))
  })

  it('handles a backwards selection', () => {
    const next = insertText(schema, state(both, range('subtitle', 6, 'title', 10)), '')
    expect(next.fields).toEqual({ title: 'Quarterly ', subtitle: 'quarter' })
  })
})

describe('deleteBackward', () => {
  it('deletes the character before the caret', () => {
    const next = deleteBackward(schema, state(both, caret('title', 16)))
    expect(next.fields.title).toBe('Quarterly repor')
    expect(next.selection).toEqual(caret('title', 15))
  })

  it('deletes a whole grapheme', () => {
    const next = deleteBackward(schema, state({ title: 'a👍🏽' }, caret('title', 5)))
    expect(next.fields.title).toBe('a')
  })

  it('deletes the range the browser asks for, clipped to the field', () => {
    const next = deleteBackward(schema, state(both, caret('subtitle', 5)), {
      from: { field: 'title', offset: 3 },
      to: { field: 'subtitle', offset: 5 },
    })
    expect(next.fields).toEqual({ title: 'Quarterly report', subtitle: ' quarter' })
  })

  it('at the start of a field moves to the end of the previous one without deleting', () => {
    const next = deleteBackward(schema, state(both, caret('subtitle', 0)))
    expect(next.fields).toEqual(both)
    expect(next.selection).toEqual(caret('title', 16))
  })

  it('at the start of a field that Backspace stops at stays put', () => {
    const stopping = { ...schema, backspaceStops: ['subtitle'] }
    const start = state(both, caret('subtitle', 0))
    expect(deleteBackward(stopping, start)).toBe(start)
  })

  it('in an empty field that Backspace stops at moves to the end of the previous one', () => {
    const stopping = { ...schema, backspaceStops: ['subtitle'] }
    const next = deleteBackward(stopping, state({ title: 'T', subtitle: '' }, caret('subtitle', 0)))
    expect(next.fields).toEqual({ title: 'T' })
    expect(next.selection).toEqual(caret('title', 1))
  })

  it('in an empty field removes it, moving to the end of the previous one', () => {
    const next = deleteBackward(schema, state({ title: 'T', subtitle: '' }, caret('subtitle', 0)))
    expect(next.fields).toEqual({ title: 'T' })
    expect(next.selection).toEqual(caret('title', 1))
  })

  it('at the start of the first field does nothing', () => {
    const before = state(both, caret('title', 0))
    expect(deleteBackward(schema, before)).toBe(before)
  })

  it('deletes a selection across fields without joining them', () => {
    const next = deleteBackward(schema, state(both, range('title', 9, 'subtitle', 13)))
    expect(next.fields).toEqual({ title: 'Quarterly' })
    expect(next.selection).toEqual(caret('title', 9))
  })
})

describe('deleteForward', () => {
  it('deletes the character after the caret', () => {
    expect(deleteForward(schema, state(both, caret('title', 0))).fields.title).toBe('uarterly report')
  })

  it('at the end of a field moves to the start of the next one without deleting', () => {
    const next = deleteForward(schema, state(both, caret('title', 16)))
    expect(next.fields).toEqual(both)
    expect(next.selection).toEqual(caret('subtitle', 0))
  })
})

describe('enter', () => {
  it('moves to the start of the next field without splitting', () => {
    const next = enter(schema, state(both, caret('title', 5)))
    expect(next.fields).toEqual(both)
    expect(next.selection).toEqual(caret('subtitle', 0))
  })

  it('adds a missing next field, empty', () => {
    const next = enter(schema, state({ title: 'T' }, caret('title', 1)))
    expect(next.fields).toEqual({ title: 'T', subtitle: '' })
    expect(next.selection).toEqual(caret('subtitle', 0))
  })

  it('does nothing in the last field', () => {
    const before = state(both, caret('subtitle', 3))
    expect(enter(schema, before)).toBe(before)
  })

  it('deletes a selection first', () => {
    const next = enter(schema, state(both, range('title', 9, 'title', 16)))
    expect(next.fields).toEqual({ title: 'Quarterly', subtitle: 'Third quarter' })
    expect(next.selection).toEqual(caret('subtitle', 0))
  })
})

describe('select', () => {
  it('removes an empty field once the selection leaves it', () => {
    const next = select(schema, state({ title: 'T', subtitle: '' }, caret('subtitle', 0)), caret('title', 0))
    expect(next.fields).toEqual({ title: 'T' })
  })

  it('removes every empty field when nothing is selected', () => {
    const next = select(schema, state({ title: '', subtitle: '' }, caret('title', 0)), null)
    expect(next.fields).toEqual({})
  })

  it('keeps an empty field that holds the selection', () => {
    const next = select(schema, state({ title: 'T', subtitle: '' }, caret('title', 0)), caret('subtitle', 0))
    expect(next.fields).toEqual({ title: 'T', subtitle: '' })
  })
})

describe('fields that stay when empty (keepEmpty)', () => {
  const kept: Schema = { order: ['title', 'subtitle', 'body'], keepEmpty: ['title', 'subtitle'] }

  it('stay once the selection leaves them', () => {
    const next = select(kept, state({ title: 'T', subtitle: '' }, caret('subtitle', 0)), caret('title', 0))
    expect(next.fields).toEqual({ title: 'T', subtitle: '' })
  })

  it('stay when nothing is selected', () => {
    const next = select(kept, state({ title: '', body: '' }, caret('title', 0)), null)
    expect(next.fields).toEqual({ title: '' })
  })

  it('Backspace in one that is empty removes it, moving to the end of the previous field', () => {
    const next = deleteBackward(kept, state({ title: 'T', subtitle: '' }, caret('subtitle', 0)))
    expect(next.fields).toEqual({ title: 'T' })
    expect(next.selection).toEqual(caret('title', 1))
  })

  it('Backspace in the first field, empty, removes it and leaves nothing selected', () => {
    const next = deleteBackward(kept, state({ title: '', body: 'B' }, caret('title', 0)))
    expect(next.fields).toEqual({ body: 'B' })
    expect(next.selection).toBeNull()
  })

  it('Backspace at the start of a field with text still only moves', () => {
    const next = deleteBackward(kept, state({ title: 'T', subtitle: 'S' }, caret('subtitle', 0)))
    expect(next.fields).toEqual({ title: 'T', subtitle: 'S' })
    expect(next.selection).toEqual(caret('title', 1))
  })
})

describe('a repeated Backspace (the key held down)', () => {
  it('deletes within the field as usual', () => {
    const next = deleteBackward(schema, state(both, caret('subtitle', 5)), undefined, { repeat: true })
    expect(next.fields.subtitle).toBe('Thir quarter')
  })

  it('stops at the start of the field: it neither moves on nor removes the field', () => {
    const atStart = state(both, caret('subtitle', 0))
    expect(deleteBackward(schema, atStart, undefined, { repeat: true })).toBe(atStart)
    const empty = state({ title: 'T', subtitle: '' }, caret('subtitle', 0))
    expect(deleteBackward(schema, empty, undefined, { repeat: true })).toBe(empty)
  })
})

describe('addField', () => {
  it('adds an absent field, empty, with the caret in it', () => {
    const next = addField(schema, state({ title: 'T' }, null), 'subtitle')
    expect(next.fields).toEqual({ title: 'T', subtitle: '' })
    expect(next.selection).toEqual(caret('subtitle', 0))
  })

  it('puts the caret at the start of a field that is already there', () => {
    const next = addField(schema, state(both, caret('title', 3)), 'subtitle')
    expect(next.fields).toEqual(both)
    expect(next.selection).toEqual(caret('subtitle', 0))
  })
})

describe('selectAllStep', () => {
  it('selects the field first, then every field', () => {
    let s = selectAllStep(schema, state(both, caret('subtitle', 3)))
    expect(s.selection).toEqual(range('subtitle', 0, 'subtitle', 13))
    s = selectAllStep(schema, s)
    expect(s.selection).toEqual(range('title', 0, 'subtitle', 13))
  })
})

describe('multi-line fields', () => {
  const withAbstract: Schema = { order: ['title', 'subtitle', 'abstract'], addable: ['subtitle'], multiline: ['abstract'] }
  const fields = { title: 'T', subtitle: 'S', abstract: 'One.' }

  it('Enter in a multi-line field inserts a line break', () => {
    const next = enter(withAbstract, state(fields, caret('abstract', 4)))
    expect(next.fields.abstract).toBe('One.\n')
    expect(next.selection).toEqual(caret('abstract', 5))
  })

  it('Enter moves on to the next present field, skipping absent ones that are not addable', () => {
    const next = enter(withAbstract, state({ title: 'T', abstract: 'A' }, caret('title', 1)))
    expect(next.fields).toEqual({ title: 'T', subtitle: '', abstract: 'A' })
    expect(enter(withAbstract, state(fields, caret('subtitle', 1))).selection).toEqual(caret('abstract', 0))
    const last = state({ title: 'T', subtitle: 'S' }, caret('subtitle', 1))
    expect(enter(withAbstract, last)).toBe(last)
  })

  it('pasting keeps line breaks in a multi-line field and flattens them elsewhere', () => {
    expect(paste(withAbstract, state(fields, caret('abstract', 4)), ' Two\r\nThree').fields.abstract).toBe(
      'One. Two\nThree',
    )
    expect(paste(withAbstract, state(fields, caret('title', 1)), ' a\nb ').fields.title).toBe('T a b ')
  })
})

describe('lines that can\'t be edited', () => {
  const lines: Schema = { order: ['title', 'body'], multiline: ['body'] }
  const text = `First\n${ATOM}\nSecond`
  const body = (selection: State['selection'], value = text): State => state({ title: 'T', body: value }, selection)

  it('Enter splits a line and Backspace at a line\'s start joins it to the one before', () => {
    const split = enter(lines, body(caret('body', 2), 'First'))
    expect(split.fields.body).toBe('Fi\nrst')
    expect(split.selection).toEqual(caret('body', 3))
    const joined = deleteBackward(lines, split)
    expect(joined.fields.body).toBe('First')
    expect(joined.selection).toEqual(caret('body', 2))
  })

  it('Backspace at the start of the line after one changes nothing', () => {
    const next = deleteBackward(lines, body(caret('body', 8)))
    expect(next.fields.body).toBe(text)
    expect(next.selection).toEqual(caret('body', 8))
  })

  it('Delete at the end of the line before one changes nothing', () => {
    const next = deleteForward(lines, body(caret('body', 5)))
    expect(next.fields.body).toBe(text)
    expect(next.selection).toEqual(caret('body', 5))
  })

  it('deleting a selection across one keeps it, on a line of its own', () => {
    const next = deleteBackward(lines, body(range('body', 2, 'body', 11)))
    expect(next.fields.body).toBe(`Fi\n${ATOM}\nond`)
    expect(next.selection).toEqual(caret('body', 2))
  })

  it('typing over a selection across one puts the text before it', () => {
    const next = insertText(lines, body(range('body', 2, 'body', 11)), 'X')
    expect(next.fields.body).toBe(`FiX\n${ATOM}\nond`)
    expect(next.selection).toEqual(caret('body', 3))
  })

  it('keeps them when a selection from another field covers them', () => {
    const next = deleteBackward(lines, body(range('title', 0, 'body', 11)))
    expect(next.fields).toEqual({ title: '', body: `${ATOM}\nond` })
  })

  it('can\'t be typed or pasted', () => {
    const next = paste(lines, body(caret('body', 5)), `a${ATOM}b`)
    expect(next.fields.body).toBe(`Firstab\n${ATOM}\nSecond`)
  })
})

describe('lines that can be removed but not edited', () => {
  const lines: Schema = {
    order: ['title', 'body'],
    multiline: ['body'],
    inputRule: (_, line) => (line === '---' ? `${VOID}\n` : null),
    voidText: '---',
  }
  const text = `First\n${VOID}\nSecond`
  const body = (selection: State['selection'], value = text): State => state({ title: 'T', body: value }, selection)

  it('Backspace at the start of the line after one removes it, and the caret stays on its line', () => {
    const next = deleteBackward(lines, body(caret('body', 8)))
    expect(next.fields.body).toBe('First\nSecond')
    expect(next.selection).toEqual(caret('body', 6))
  })

  it('Backspace at the start of a line after one with a prefix takes the prefix first', () => {
    const next = deleteBackward(lines, body(caret('body', 9), `First\n${VOID}\n\ufdd2Second`))
    expect(next.fields.body).toBe(text)
  })

  it('Delete at the end of the line before one removes it', () => {
    const next = deleteForward(lines, body(caret('body', 5)))
    expect(next.fields.body).toBe('First\nSecond')
    expect(next.selection).toEqual(caret('body', 5))
    expect(deleteForward(lines, body(caret('body', 5), `First\n${VOID}`)).fields.body).toBe('First')
  })

  it('deleting a selection across one removes it', () => {
    const next = deleteBackward(lines, body(range('body', 2, 'body', 11)))
    expect(next.fields.body).toBe('Fiond')
    expect(next.selection).toEqual(caret('body', 2))
  })

  it('a deletion that would join one to a line keeps it on a line of its own', () => {
    const next = deleteBackward(lines, body(range('body', 2, 'body', 6)))
    expect(next.fields.body).toBe(`Fi\n${VOID}\nSecond`)
    expect(next.selection).toEqual(caret('body', 2))
  })

  it('can\'t be typed or pasted', () => {
    const next = paste(lines, body(caret('body', 5)), `a${VOID}b`)
    expect(next.fields.body).toBe(`Firstab\n${VOID}\nSecond`)
  })

  it('an input rule can make one', () => {
    const next = applyInputRule(lines, body(caret('body', 3), '---Next'))
    expect(next.fields.body).toBe(`${VOID}\nNext`)
    expect(next.selection).toEqual(caret('body', 2))
  })

  it('is copied as the schema says', () => {
    expect(selectedText(lines, { body: text }, range('body', 0, 'body', 14))).toBe('First\n---\nSecond')
    expect(selectedText({ ...lines, voidText: undefined }, { body: text }, range('body', 0, 'body', 14))).toBe(
      'First\n\nSecond',
    )
  })
})

describe('setLinePrefixes', () => {
  const H = '\ufdd1'
  const Q = '\ufdd7'
  const doc: Schema = { order: ['title', 'body'], multiline: ['body'] }
  const text = `One\n${H}Two\n${ATOM}\nThree`
  const at = (selection: State['selection']) => state({ title: 'T', body: text }, selection)

  it('gives the caret\'s line the prefix it returns, keeping the caret on the same text', () => {
    const next = setLinePrefixes(doc, at(caret('body', 2)), () => Q)
    expect(next.fields.body).toBe(`${Q}One\n${H}Two\n${ATOM}\nThree`)
    expect(next.selection).toEqual(caret('body', 3))
  })

  it('is given the line\'s prefix, and can remove it', () => {
    const next = setLinePrefixes(doc, at(caret('body', 6)), (prefix) => (prefix === H ? '' : Q))
    expect(next.fields.body).toBe(`One\nTwo\n${ATOM}\nThree`)
    expect(next.selection).toEqual(caret('body', 5))
  })

  it('changes every line a selection is on, but not lines that can\'t be edited', () => {
    const next = setLinePrefixes(doc, at(range('body', 1, 'body', text.length - 1)), () => Q)
    expect(next.fields.body).toBe(`${Q}One\n${Q}Two\n${ATOM}\n${Q}Three`)
    expect(next.selection).toEqual(range('body', 2, 'body', next.fields.body!.length - 1))
  })

  it('leaves fields that aren\'t multi-line, and returns the same state when nothing changes', () => {
    const s = at(range('title', 0, 'body', 2))
    expect(setLinePrefixes(doc, s, (prefix) => prefix)).toBe(s)
    const next = setLinePrefixes(doc, s, () => Q)
    expect(next.fields.title).toBe('T')
    expect(next.fields.body).toBe(`${Q}One\n${H}Two\n${ATOM}\nThree`
    )
  })

  it('does nothing without a selection', () => {
    const s = at(null)
    expect(setLinePrefixes(doc, s, () => Q)).toBe(s)
  })
})
