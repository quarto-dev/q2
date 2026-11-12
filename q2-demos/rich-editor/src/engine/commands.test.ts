import { describe, expect, it } from 'vitest'
import {
  deleteBackward,
  deleteForward,
  enter,
  insertText,
  select,
  selectAllStep,
} from './commands.ts'
import { caret, range, type Fields, type Schema, type State } from './model.ts'

const schema: Schema = { order: ['title', 'subtitle'] }

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

describe('selectAllStep', () => {
  it('selects the field first, then every field', () => {
    let s = selectAllStep(schema, state(both, caret('subtitle', 3)))
    expect(s.selection).toEqual(range('subtitle', 0, 'subtitle', 13))
    s = selectAllStep(schema, s)
    expect(s.selection).toEqual(range('title', 0, 'subtitle', 13))
  })
})
