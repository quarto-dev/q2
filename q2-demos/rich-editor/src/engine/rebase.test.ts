import { describe, expect, it } from 'vitest'
import { createHistory, record, undo } from './history.ts'
import { caret, range, type Schema, type State } from './model.ts'
import { diff, mapOffset, rebaseHistory, rebaseState, rebaseText } from './rebase.ts'

const schema: Schema = { order: ['title', 'subtitle'] }

describe('diff', () => {
  it('finds the replaced range', () => {
    expect(diff('Hello world', 'Hello big world')).toEqual({ start: 6, removed: 0, inserted: 'big ' })
    expect(diff('Hello world', 'Help')).toEqual({ start: 3, removed: 8, inserted: 'p' })
    expect(diff('same', 'same')).toBeNull()
  })
})

describe('mapOffset', () => {
  const insert = { start: 3, removed: 0, inserted: 'XY' }
  it('shifts offsets after the change and keeps those before', () => {
    expect(mapOffset(1, insert)).toBe(1)
    expect(mapOffset(5, insert)).toBe(7)
  })
  it('keeps a caret before text inserted at it, unless told otherwise', () => {
    expect(mapOffset(3, insert)).toBe(3)
    expect(mapOffset(3, insert, 'after')).toBe(5)
  })
  it('moves an offset inside replaced text to the end of the replacement', () => {
    expect(mapOffset(4, { start: 2, removed: 4, inserted: 'Z' })).toBe(3)
    expect(mapOffset(2, { start: 2, removed: 4, inserted: 'Z' })).toBe(2)
  })
})

describe('rebaseText', () => {
  it('makes the change as it is when the variant is the base', () => {
    expect(rebaseText('Report', 'Report', { start: 0, removed: 0, inserted: 'The ' }).text).toBe('The Report')
  })
  it('keeps what the variant added: a trailing space the document dropped', () => {
    expect(rebaseText('Report ', 'Report', { start: 0, removed: 0, inserted: 'The ' }).text).toBe('The Report ')
  })
  it('makes a change after the variant\'s own change in the right place', () => {
    // The variant put "Big " at the start; the change appends "!".
    expect(rebaseText('Big Report', 'Report', { start: 6, removed: 0, inserted: '!' }).text).toBe('Big Report!')
  })
  it('puts text inserted into what the variant replaced after the replacement', () => {
    // An older state in the undo history: before " world" was typed.
    expect(rebaseText('Hello', 'Hello world', { start: 9, removed: 0, inserted: '!' }).text).toBe('Hello!')
  })
  it('ignores a deletion inside what the variant replaced', () => {
    expect(rebaseText('Hello', 'Hello world', { start: 7, removed: 2, inserted: '' }).text).toBe('Hello')
  })
})

describe('rebaseState', () => {
  const state = (title: string, selection: State['selection'], subtitle?: string): State => ({
    fields: { title, subtitle },
    selection,
  })

  it('returns the same state when nothing changed', () => {
    const s = state('Report', caret('title', 3))
    expect(rebaseState(schema, s, { title: 'Report' }, { title: 'Report' })).toBe(s)
  })

  it('moves the caret with the text after an insertion before it', () => {
    const s = state('Report', caret('title', 3))
    const next = rebaseState(schema, s, { title: 'Report' }, { title: 'The Report' })
    expect(next.fields.title).toBe('The Report')
    expect(next.selection).toEqual(caret('title', 7))
  })

  it('leaves the caret alone when the change is after it or in another field', () => {
    const s = state('Report', caret('title', 3), 'Sub')
    expect(rebaseState(schema, s, { title: 'Report', subtitle: 'Sub' }, { title: 'Reports', subtitle: 'Subs' }).selection).toEqual(
      caret('title', 3),
    )
  })

  it('keeps a selection on the same text', () => {
    const s = state('Quarterly report', range('title', 10, 'title', 16))
    const next = rebaseState(schema, s, { title: 'Quarterly report' }, { title: 'The Quarterly report' })
    expect(next.selection).toEqual(range('title', 14, 'title', 20))
  })

  it('does not grow a selection with text inserted at its ends', () => {
    const s = state('a report b', range('title', 2, 'title', 8))
    const next = rebaseState(schema, s, { title: 'a report b' }, { title: 'a new report b' })
    expect(next.selection).toEqual(range('title', 6, 'title', 12))
  })

  it('keeps a field the change emptied while the caret is in it, and drops it otherwise', () => {
    const base = { title: 'Report', subtitle: 'Sub' }
    const source = { title: 'Report' }
    const held = rebaseState(schema, state('Report', caret('subtitle', 2), 'Sub'), base, source)
    expect(held.fields.subtitle).toBe('')
    expect(held.selection).toEqual(caret('subtitle', 0))
    const free = rebaseState(schema, state('Report', caret('title', 2), 'Sub'), base, source)
    expect('subtitle' in free.fields).toBe(false)
  })

  it('adds a field the change added', () => {
    const next = rebaseState(schema, state('Report', null), { title: 'Report' }, { title: 'Report', subtitle: 'New' })
    expect(next.fields.subtitle).toBe('New')
  })
})

describe('rebaseHistory', () => {
  it('lets undo remove only what this editor typed', () => {
    const before = { fields: { title: 'Hello' }, selection: caret('title', 5) }
    const after = { fields: { title: 'Hello world' }, selection: caret('title', 11) }
    let h = record(createHistory<State>(before), after)
    // A collaborator puts "Oh, " at the start.
    h = rebaseHistory(schema, h, { title: 'Hello world' }, { title: 'Oh, Hello world' })
    expect(h.present.fields.title).toBe('Oh, Hello world')
    expect(undo(h).present.fields.title).toBe('Oh, Hello')
    expect(undo(h).present.selection).toEqual(caret('title', 9))
  })
})
