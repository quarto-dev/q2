import { describe, expect, it } from 'vitest'
import { createHistory, record, undo } from './history.ts'
import { undoGroup } from './undoGroup.ts'

const at = (n: number) => ({ start: n, end: n })

describe('undoGroup', () => {
  it('groups typing with typing, and deleting with deleting', () => {
    expect(undoGroup('title', 'insertText', at(1), at(1))).toBe(
      undoGroup('title', 'insertText', at(2), at(2)),
    )
    expect(undoGroup('title', 'deleteContentBackward', at(4), at(4))).toBe(
      undoGroup('title', 'deleteContentBackward', at(3), at(3)),
    )
  })

  it('separates typing from deleting', () => {
    expect(undoGroup('title', 'insertText', at(1), at(1))).not.toBe(
      undoGroup('title', 'deleteContentBackward', at(1), at(1)),
    )
  })

  it('separates fields', () => {
    expect(undoGroup('title', 'insertText', at(1), at(1))).not.toBe(
      undoGroup('subtitle', 'insertText', at(1), at(1)),
    )
  })

  it("doesn't group an edit away from where the last one ended", () => {
    expect(undoGroup('title', 'insertText', at(9), at(4))).toBeUndefined()
  })

  it("doesn't group edits other than typing and deleting", () => {
    expect(undoGroup('title', 'insertFromPaste', at(1), at(1))).toBeUndefined()
    expect(undoGroup('title', 'deleteByCut', at(1), at(1))).toBeUndefined()
  })

  it('makes "type mmmm, delete it, undo" bring the mmmm back', () => {
    let h = createHistory('')
    // The caret always continues from the previous edit.
    const edit = (text: string, inputType: string, now: number) => {
      const caret = at(h.present.length)
      h = record(h, text, { group: undoGroup('title', inputType, caret, caret), now })
    }
    for (const [i, text] of ['m', 'mm', 'mmm', 'mmmm'].entries()) edit(text, 'insertText', i * 50)
    for (const [i, text] of ['mmm', 'mm', 'm', ''].entries()) edit(text, 'deleteContentBackward', 300 + i * 50)
    h = undo(h)
    expect(h.present).toBe('mmmm')
    h = undo(h)
    expect(h.present).toBe('')
  })
})
