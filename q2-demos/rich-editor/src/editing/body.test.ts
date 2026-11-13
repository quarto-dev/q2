import { describe, expect, it } from 'vitest'
import { ATOM, VOID } from '../engine/model.ts'
import {
  bodyInputRule,
  bodyText,
  editBody,
  headingLevel,
  headingPrefix,
  isQuote,
  plainParagraph,
  prefixMarkdown,
  QUOTE,
  readOnlyBlocks,
  words,
  type Node,
} from './body.ts'

const para = (text: string): Node => ({ t: 'Para', c: words(text) })
// A heading with formatting, which can't be edited.
const heading: Node = { t: 'Header', c: [2, ['', [], []], [{ t: 'Emph', c: words('Results') }]] }
const formatted: Node = { t: 'Para', c: [{ t: 'Emph', c: words('really') }] }
const empty: Node = { t: 'Para', c: [{ t: 'Str', c: '\u00a0' }] }
const blocks = [para('One.'), heading, para('Two.'), formatted, para('Three.')]

describe('plainParagraph', () => {
  it('is the text of a paragraph of words, soft breaks and quotes', () => {
    expect(plainParagraph(para('Plain text.'))).toBe('Plain text.')
    expect(plainParagraph({ t: 'Para', c: [{ t: 'Str', c: 'a' }, { t: 'SoftBreak' }, { t: 'Str', c: 'b' }] })).toBe('a b')
    expect(
      plainParagraph({ t: 'Para', c: [{ t: 'Quoted', c: [{ t: 'DoubleQuote' }, words('so called')] }] }),
    ).toBe('“so called”')
  })
  it('is empty for a paragraph of one non-breaking space', () => {
    expect(plainParagraph(empty)).toBe('')
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
  it('has an empty line for an empty paragraph', () => {
    expect(bodyText([para('One.'), empty, empty, para('Two.')])).toBe('One.\n\n\nTwo.')
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
  it('makes a paragraph of each new line, and an empty paragraph of an empty one', () => {
    const edited = editBody(blocks, `One.\n\nNew.\n${ATOM}\nTwo.\n${ATOM}\nThree.`)!
    expect(edited).toEqual([blocks[0], empty, para('New.'), heading, blocks[2], formatted, blocks[4]])
    expect(edited[0]).toBe(blocks[0])
  })
  it('keeps the empty paragraphs that are still there', () => {
    const before = [para('One.'), empty, para('Two.')]
    const edited = editBody(before, 'One.\n\n\nTwo!')!
    expect(edited).toEqual([before[0], empty, empty, para('Two!')])
    expect(edited[1]).toBe(before[1])
    expect(editBody(before, 'One.\nTwo.')).toEqual([before[0], before[2]])
  })
  it('makes no blocks of an empty text', () => {
    expect(editBody([para('One.')], '')).toEqual([])
  })
  it('drops the spaces qmd doesn\'t keep', () => {
    expect(editBody([para('One.')], ' One  more. ')).toEqual([para('One more.')])
    const one = para('One.')
    expect(editBody([one], 'One. ')![0]).toBe(one)
  })
  it('keeps the line breaks of a wrapped paragraph around what changed', () => {
    const wrapped: Node = {
      t: 'Para',
      c: [...words('One two'), { t: 'SoftBreak' }, ...words('three four'), { t: 'SoftBreak' }, ...words('five.')],
    }
    const breaks = (block: Node) => (block.c as Node[]).filter((inline) => inline.t !== 'Str').map((inline) => inline.t)
    // Typing in the middle line.
    expect(breaks(editBody([wrapped], 'One two three more four five.')![0])).toEqual([
      'Space', 'SoftBreak', 'Space', 'Space', 'SoftBreak',
    ])
    // Removing a line break's space removes the line break.
    expect(breaks(editBody([wrapped], 'One twothree four five.')![0])).toEqual(['Space', 'Space', 'SoftBreak'])
    // Splitting: each half keeps its own.
    const split = editBody([para('Top.'), wrapped], 'Top.\nOne two three\nfour five.')!
    expect(split.slice(1).map(breaks)).toEqual([['Space', 'SoftBreak'], ['SoftBreak']])
    // Joining.
    const joined = editBody([wrapped, wrapped], 'One two three four five.One two three four five.')!
    expect(breaks(joined[0])).toEqual(['Space', 'SoftBreak', 'Space', 'SoftBreak', 'Space', 'SoftBreak', 'Space', 'SoftBreak'])
  })
  it('refuses a text without the read-only blocks', () => {
    expect(editBody(blocks, 'One.')).toBeNull()
  })
})

describe('headings', () => {
  const H2 = headingPrefix(2)
  const h = (level: number, text: string, id = '', classes: string[] = []): Node => ({
    t: 'Header',
    c: [level, [id, classes, []], text === '' ? [] : words(text)],
  })
  // An id the reader made from the text, not written in the source.
  const auto: Node = { ...h(2, 'Results', 'results'), a: { classes: [], id: null, kvs: [] } } as Node
  const explicit: Node = { ...h(1, 'One', 'first', ['unnumbered']), a: { classes: [3], id: 4, kvs: [] } } as Node

  it('a heading of plain text is a line with its level\'s prefix', () => {
    expect(headingPrefix(1)).toBe('﷑')
    expect(headingPrefix(6)).toBe('﷖')
    expect(bodyText([para('One.'), auto, h(6, 'Six')])).toBe(`One.\n${H2}Results\n${headingPrefix(6)}Six`)
    expect(headingLevel(`${H2}Results`)).toBe(2)
    expect(headingLevel('Results')).toBe(0)
  })
  it('an empty heading is a line of only its prefix', () => {
    expect(bodyText([h(2, '')])).toBe(H2)
  })
  it('a heading with formatting can\'t be edited', () => {
    const formattedHeading: Node = { t: 'Header', c: [2, ['', [], []], [{ t: 'Emph', c: words('Results') }]] }
    expect(bodyText([formattedHeading])).toBe(ATOM)
    expect(readOnlyBlocks([formattedHeading, auto])).toEqual([formattedHeading])
  })
  it('text holding a prefix character can\'t be edited', () => {
    expect(bodyText([para(`a${H2}b`)])).toBe(ATOM)
  })

  it('writes a line with a prefix as a heading', () => {
    expect(editBody([para('One.')], `One.\n${H2}New`)).toEqual([para('One.'), h(2, 'New')])
    expect(editBody([para('One.')], `${headingPrefix(3)}One.`)).toEqual([h(3, 'One.')])
    expect(editBody([para('One.')], `${H2}`)).toEqual([h(2, '')])
  })
  it('drops the spaces qmd doesn\'t keep, after the prefix too', () => {
    expect(editBody([para('One.')], `${H2}  New  heading `)).toEqual([h(2, 'New heading')])
  })
  it('keeps an unchanged heading as it was', () => {
    const edited = editBody([auto, para('One.')], `${H2}Results\nOne!`)!
    expect(edited[0]).toBe(auto)
  })
  it('keeps an edited heading\'s written attributes and source', () => {
    const [edited] = editBody([explicit], `${headingPrefix(1)}Two`)!
    expect(edited).toEqual({ ...explicit, c: [1, ['first', ['unnumbered'], []], words('Two')] })
  })
  it('drops an edited heading\'s id when the reader made it up', () => {
    const [edited] = editBody([auto], `${H2}Findings`)!
    expect(edited).toEqual({ ...auto, c: [2, ['', [], []], words('Findings')] })
  })
  it('keeps the attributes when the level changes', () => {
    const [edited] = editBody([explicit], `${headingPrefix(3)}One`)!
    expect((edited.c as unknown[])[0]).toBe(3)
    expect((edited.c as unknown[])[1]).toEqual(['first', ['unnumbered'], []])
  })
  it('writes a heading whose prefix was removed as a paragraph', () => {
    expect(editBody([para('One.'), auto], 'One.\nResults')).toEqual([para('One.'), para('Results')])
  })
  it('a new heading among others doesn\'t take a neighbour\'s attributes', () => {
    const edited = editBody([explicit, para('Two.')], `${headingPrefix(1)}One\n${H2}New\nTwo.`)!
    expect(edited[0]).toBe(explicit)
    expect(edited[1]).toEqual(h(2, 'New'))
  })
})

describe('quotes', () => {
  const quote = (...blocks: Node[]): Node => ({ t: 'BlockQuote', c: blocks })

  it('a quote of one paragraph of plain text is a line with the quote prefix', () => {
    expect(bodyText([para('One.'), quote(para('A quote.'))])).toBe(`One.\n${QUOTE}A quote.`)
    expect(isQuote(`${QUOTE}A quote.`)).toBe(true)
    expect(isQuote('A quote.')).toBe(false)
    expect(headingLevel(`${QUOTE}A quote.`)).toBe(0)
  })
  it('a quote of an empty paragraph is a line of only its prefix', () => {
    expect(bodyText([quote(empty)])).toBe(QUOTE)
  })
  it('any other quote can\'t be edited', () => {
    const others = [quote(formatted), quote(para('One.'), para('Two.')), quote(), quote(heading)]
    expect(bodyText(others)).toBe(others.map(() => ATOM).join('\n'))
    expect(readOnlyBlocks(others)).toEqual(others)
  })

  it('writes a line with the quote prefix as a quote, and an empty one as a quote of an empty paragraph', () => {
    expect(editBody([para('One.')], `One.\n${QUOTE}New`)).toEqual([para('One.'), quote(para('New'))])
    expect(editBody([para('One.')], `${QUOTE}  New  quote `)).toEqual([quote(para('New quote'))])
    expect(editBody([para('One.')], QUOTE)).toEqual([quote(empty)])
  })
  it('keeps an unchanged quote as it was', () => {
    const was = quote(para('A quote.'))
    expect(editBody([was, para('One.')], `${QUOTE}A quote.\nOne!`)![0]).toBe(was)
  })
  it('keeps the line breaks of a wrapped quote around what changed', () => {
    const wrapped = quote({ t: 'Para', c: [...words('One two'), { t: 'SoftBreak' }, ...words('three.')] })
    const [edited] = editBody([wrapped], `${QUOTE}One two, three.`)!
    expect(edited).toEqual(quote({ t: 'Para', c: [...words('One two,'), { t: 'SoftBreak' }, ...words('three.')] }))
  })
  it('writes a quote whose prefix was removed as a paragraph', () => {
    expect(editBody([quote(para('A quote.'))], 'A quote.')).toEqual([para('A quote.')])
  })

  it('> and a space at the start of a line make a quote', () => {
    expect(bodyInputRule('> ')).toBe(QUOTE)
    expect(bodyInputRule('>> ')).toBeNull()
    expect(bodyInputRule(`${QUOTE}> `)).toBeNull()
  })
  it('a quote is copied with its >', () => {
    expect(prefixMarkdown(QUOTE)).toBe('> ')
  })
})

describe('horizontal rules', () => {
  const rule: Node = { t: 'HorizontalRule' }

  it('a rule is a line that can be removed but not edited', () => {
    expect(bodyText([para('One.'), rule, para('Two.')])).toBe(`One.\n${VOID}\nTwo.`)
    expect(readOnlyBlocks([para('One.'), rule])).toEqual([])
  })
  it('text holding the character that stands for one can\'t be edited', () => {
    expect(plainParagraph(para(`One${VOID}`))).toBeUndefined()
  })

  it('writes a line of it as a rule, and keeps an unchanged rule as it was', () => {
    expect(editBody([para('One.')], `One.\n${VOID}\nTwo.`)).toEqual([para('One.'), rule, para('Two.')])
    const was: Node = { t: 'HorizontalRule', s: 3 } as Node
    expect(editBody([was, para('One.')], `${VOID}\nOne!`)![0]).toBe(was)
  })
  it('drops a rule whose line was removed', () => {
    expect(editBody([para('One.'), rule, para('Two.')], 'One.\nTwo.')).toEqual([para('One.'), para('Two.')])
  })

  it('---, *** or ___ at the start of a line make a rule, and the rest of the line goes after it', () => {
    for (const typed of ['---', '***', '___']) expect(bodyInputRule(typed)).toBe(`${VOID}\n`)
    expect(bodyInputRule('--')).toBeNull()
    expect(bodyInputRule('-*-')).toBeNull()
    expect(bodyInputRule(`${QUOTE}---`)).toBeNull()
    expect(bodyInputRule(`${headingPrefix(1)}---`)).toBeNull()
  })
})
