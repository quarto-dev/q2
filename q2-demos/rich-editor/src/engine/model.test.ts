import { describe, expect, it } from 'vitest'
import { range, selectedText } from './model.ts'

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
