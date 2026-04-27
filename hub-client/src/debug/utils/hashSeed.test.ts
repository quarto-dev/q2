import { describe, it, expect } from 'vitest'
import { parseDebugHashSeed } from './hashSeed'

describe('parseDebugHashSeed', () => {
  it('returns null for an empty hash', () => {
    expect(parseDebugHashSeed('')).toBeNull()
    expect(parseDebugHashSeed('#')).toBeNull()
  })

  it('returns null when the doc= param is missing', () => {
    expect(parseDebugHashSeed('#something=else')).toBeNull()
    expect(parseDebugHashSeed('#foo')).toBeNull()
  })

  it('extracts a bare automerge URL from doc=', () => {
    expect(parseDebugHashSeed('#doc=automerge:abc123')).toBe('automerge:abc123')
  })

  it('tolerates a leading # or not', () => {
    expect(parseDebugHashSeed('doc=automerge:abc')).toBe('automerge:abc')
    expect(parseDebugHashSeed('#doc=automerge:abc')).toBe('automerge:abc')
  })

  it('URL-decodes the value', () => {
    const encoded = encodeURIComponent('automerge:abc+def')
    expect(parseDebugHashSeed(`#doc=${encoded}`)).toBe('automerge:abc+def')
  })

  it('picks doc= when other params are present', () => {
    expect(parseDebugHashSeed('#server=wss://h&doc=automerge:xyz')).toBe(
      'automerge:xyz',
    )
  })

  it('returns null for an empty doc= value', () => {
    expect(parseDebugHashSeed('#doc=')).toBeNull()
  })
})
