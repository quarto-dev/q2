import { describe, expect, it } from 'vitest'
import { undoGroup } from './undoGroup.ts'

describe('undoGroup', () => {
  it('groups typing with typing, and deleting with deleting', () => {
    expect(undoGroup('title', 'insertText')).toBe(undoGroup('title', 'insertText'))
    expect(undoGroup('title', 'deleteContentBackward')).toBe(undoGroup('title', 'deleteWordBackward'))
  })

  it('separates typing from deleting', () => {
    expect(undoGroup('title', 'insertText')).not.toBe(undoGroup('title', 'deleteContentBackward'))
  })

  it('separates fields', () => {
    expect(undoGroup('title', 'insertText')).not.toBe(undoGroup('subtitle', 'insertText'))
  })

  it("doesn't group edits other than typing and deleting", () => {
    expect(undoGroup('title', 'insertFromPaste')).toBeUndefined()
    expect(undoGroup('title', 'deleteByCut')).toBeUndefined()
  })
})
