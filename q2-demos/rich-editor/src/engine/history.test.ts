import { describe, expect, it } from 'vitest'
import { createHistory, record, redo, replacePresent, undo, endGroup } from './history.ts'

describe('history', () => {
  it('undoes and redoes single steps', () => {
    let h = createHistory('a')
    h = record(h, 'b')
    h = record(h, 'c')
    h = undo(h)
    expect(h.present).toBe('b')
    h = undo(h)
    expect(h.present).toBe('a')
    expect(undo(h)).toBe(h)
    h = redo(h)
    expect(h.present).toBe('b')
  })

  it('drops the redo stack on a new edit', () => {
    let h = record(createHistory('a'), 'b')
    h = record(undo(h), 'x')
    expect(redo(h)).toBe(h)
    expect(h.present).toBe('x')
  })

  it('coalesces consecutive edits in the same group into one step', () => {
    let h = createHistory('')
    h = record(h, 'H', { group: 'title', now: 0 })
    h = record(h, 'Hi', { group: 'title', now: 100 })
    h = record(h, 'Hi!', { group: 'title', now: 200 })
    expect(undo(h).present).toBe('')
  })

  it('starts a new step after a pause', () => {
    let h = createHistory('')
    h = record(h, 'H', { group: 'title', now: 0 })
    h = record(h, 'Hi', { group: 'title', now: 5000 })
    expect(undo(h).present).toBe('H')
  })

  it('starts a new step in a different group or after endGroup', () => {
    let h = createHistory('')
    h = record(h, 'a', { group: 'title', now: 0 })
    h = record(h, 'ab', { group: 'subtitle', now: 10 })
    expect(undo(h).present).toBe('a')

    h = endGroup(h)
    h = record(h, 'abc', { group: 'subtitle', now: 20 })
    expect(undo(h).present).toBe('ab')
  })

  it('records the state before the step with a replaced value', () => {
    // E.g. the caret position just before typing started.
    let h = createHistory({ text: '', caret: 0 })
    h = record(h, { text: 'x', caret: 1 }, { before: { text: '', caret: 0 } })
    h = record(h, { text: 'xy', caret: 2 }, { before: { text: 'x', caret: 0 } })
    expect(undo(h).present).toEqual({ text: 'x', caret: 0 })
  })

  it('never coalesces across an undo', () => {
    let h = createHistory('')
    h = record(h, 'a', { group: 'title', now: 0 })
    h = record(h, 'ab', { group: 'title', now: 10 })
    h = undo(h)
    h = record(h, 'x', { group: 'title', now: 20 })
    expect(undo(h).present).toBe('')
  })

  it('replaces the present state without adding an undo step', () => {
    let h = record(createHistory('a'), 'b')
    h = replacePresent(h, 'b2')
    expect(h.present).toBe('b2')
    expect(undo(h).present).toBe('a')
  })

  it('keeps the open step open across a replaced present', () => {
    let h = createHistory('')
    h = record(h, 'a', { group: 'title', now: 0 })
    h = replacePresent(h, 'a2')
    h = record(h, 'a2b', { group: 'title', now: 10 })
    expect(undo(h).present).toBe('')
  })
})
