import { describe, expect, it } from 'vitest'
import { ATOM } from '../engine/model.ts'
import { bodyText, editBody, plainParagraph, words, type Node } from './body.ts'

const para = (text: string): Node => ({ t: 'Para', c: words(text) })
const heading: Node = { t: 'Header', c: [2, ['', [], []], words('Results')] }
const formatted: Node = { t: 'Para', c: [{ t: 'Emph', c: words('really') }] }
const blocks = [para('One.'), heading, para('Two.'), formatted, para('Three.')]

describe('plainParagraph', () => {
  it('is the text of a paragraph of words, soft breaks and quotes', () => {
    expect(plainParagraph(para('Plain text.'))).toBe('Plain text.')
    expect(plainParagraph({ t: 'Para', c: [{ t: 'Str', c: 'a' }, { t: 'SoftBreak' }, { t: 'Str', c: 'b' }] })).toBe('a b')
    expect(
      plainParagraph({ t: 'Para', c: [{ t: 'Quoted', c: [{ t: 'DoubleQuote' }, words('so called')] }] }),
    ).toBe('“so called”')
  })
  it('is undefined for anything else', () => {
    expect(plainParagraph(heading)).toBeUndefined()
    expect(plainParagraph(formatted)).toBeUndefined()
  })
})

describe('bodyText', () => {
  it('has a line per block, and an atom for each block that can\'t be edited', () => {
    expect(bodyText(blocks)).toBe(`One.\n${ATOM}\nTwo.\n${ATOM}\nThree.`)
    expect(bodyText([])).toBe('')
  })
})

describe('editBody', () => {
  it('keeps the blocks of the lines that read the same', () => {
    const edited = editBody(blocks, `One.\n${ATOM}\nTwo!\n${ATOM}\nThree.`)!
    expect(edited[0]).toBe(blocks[0])
    expect(edited[1]).toBe(heading)
    expect(edited[2]).toEqual(para('Two!'))
    expect(edited[3]).toBe(formatted)
    expect(edited[4]).toBe(blocks[4])
  })
  it('makes a paragraph of each new line, and none of an empty one', () => {
    const edited = editBody(blocks, `One.\n\nNew.\n${ATOM}\nTwo.\n${ATOM}\nThree.`)!
    expect(edited).toEqual([blocks[0], para('New.'), heading, blocks[2], formatted, blocks[4]])
    expect(edited[0]).toBe(blocks[0])
  })
  it('drops the spaces qmd doesn\'t keep', () => {
    expect(editBody([para('One.')], ' One  more. ')).toEqual([para('One more.')])
    const one = para('One.')
    expect(editBody([one], 'One. ')![0]).toBe(one)
  })
  it('refuses a text without the read-only blocks', () => {
    expect(editBody(blocks, 'One.')).toBeNull()
  })
})
