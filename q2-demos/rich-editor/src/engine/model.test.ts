import { describe, expect, it } from 'vitest'
import { lineStartOf, range, selectedText } from './model.ts'

const schema = { order: ['title', 'subtitle'] }
const fields = { title: 'Quarterly report', subtitle: 'Third quarter' }

describe('selectedText', () => {
  it('returns the text within a field', () => {
    expect(selectedText(schema, fields, range('title', 10, 'title', 16))).toBe('report')
  })

  it('puts each field on its own line', () => {
    expect(selectedText(schema, fields, range('subtitle', 5, 'title', 10))).toBe('report\nThird')
  })
})

describe('lineStartOf', () => {
  it('finds the start of the line an offset is on', () => {
    expect(lineStartOf('ab\ncd', 4)).toBe(3)
    expect(lineStartOf('ab\ncd', 3)).toBe(3)
    expect(lineStartOf('ab\ncd', 2)).toBe(0)
  })

  it('puts the start of the text on the first line, even an empty one', () => {
    expect(lineStartOf('\nab', 0)).toBe(0)
    expect(lineStartOf('\nab', 1)).toBe(1)
  })
})
