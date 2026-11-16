import { describe, expect, it } from 'vitest'
import { canonical, LINK_CLOSE, LINK_OPEN, LINK_TEXT, withoutMarks } from '../engine/marks.ts'
import { CODE, EMPH, formatted, HIGHLIGHT, inlineRule, inlinesOf, inlineText, keptText, STRONG, UNDERLINE, words, type Node } from './inlines.ts'

const str = (c: string): Node => ({ t: 'Str', c })
const space: Node = { t: 'Space' }
const link = (url: string, text: string) => LINK_OPEN + url + LINK_TEXT + text + LINK_CLOSE

describe('inlineText', () => {
  it('reads each kind of formatting the editor edits', () => {
    const inlines: Node[] = [
      { t: 'Strong', c: [str('b')] },
      space,
      { t: 'Emph', c: [str('i')] },
      space,
      { t: 'Underline', c: [str('u')] },
      space,
      { t: 'Span', c: [['', ['mark'], []], [str('h')]] },
      space,
      { t: 'Code', c: [['', [], []], 'c d'] },
      space,
      { t: 'Link', c: [['', [], []], [str('l')], ['https://q.org', '']] },
    ]
    expect(inlineText(inlines)).toBe(
      `${formatted(STRONG, 'b')} ${formatted(EMPH, 'i')} ${formatted(UNDERLINE, 'u')} ${formatted(HIGHLIGHT, 'h')} ${formatted(CODE, 'c d')} ${link('https://q.org', 'l')}`,
    )
  })

  it('is undefined for anything else', () => {
    expect(inlineText([{ t: 'Math', c: [{ t: 'InlineMath' }, 'x'] }])).toBeUndefined()
    expect(inlineText([{ t: 'Span', c: [['', ['smallcaps'], []], [str('s')]] }])).toBeUndefined()
    expect(inlineText([{ t: 'Link', c: [['', [], []], [str('l')], ['u', 'a title']] }])).toBeUndefined()
    expect(inlineText([{ t: 'Code', c: [['', ['python'], []], 'x'] }])).toBeUndefined()
  })
})

describe('inlinesOf', () => {
  it('writes formatting as nested inlines, the longest outermost', () => {
    const text = `${formatted(STRONG, `a ${formatted(EMPH, 'b')}`)} c`
    expect(inlinesOf(text)).toEqual([
      { t: 'Strong', c: [str('a'), space, { t: 'Emph', c: [str('b')] }] },
      space,
      str('c'),
    ])
  })

  it('reads back as it was written', () => {
    const text = `x ${formatted(HIGHLIGHT, 'h')} ${formatted(CODE, 'a  b')} ${link('u', `l ${formatted(STRONG, 'b')}`)}`
    expect(inlineText(inlinesOf(text))).toBe(canonical(text))
  })

  it('puts spaces at the ends of formatting outside it', () => {
    expect(inlinesOf(`a${formatted(STRONG, ' b ')}c`)).toEqual([str('a'), space, { t: 'Strong', c: [str('b')] }, space, str('c')])
  })

  it('writes plain text as words', () => {
    expect(inlinesOf('a b')).toEqual(words('a b'))
  })
})

describe('keptText', () => {
  it('drops spaces as qmd does, except in code', () => {
    expect(keptText('  a   b ')).toBe('a b')
    expect(withoutMarks(keptText(` a ${formatted(CODE, 'x  y')}  `))).toBe('a x  y')
  })

  it('drops formatting with nothing in it', () => {
    expect(keptText(`a${formatted(STRONG, '')}`)).toBe('a')
  })
})

describe('inlineRule', () => {
  const rule = (typed: string) => inlineRule(typed, typed.length)

  it('makes closed markdown formatting', () => {
    expect(rule('a **bold**')).toEqual({ line: `a ${formatted(STRONG, 'bold')}`, caret: 'a bold'.length + 2 })
    expect(rule('a *it*')?.line).toBe(`a ${formatted(EMPH, 'it')}`)
    expect(rule('a __b__')?.line).toBe(`a ${formatted(STRONG, 'b')}`)
    expect(rule('a _it_')?.line).toBe(`a ${formatted(EMPH, 'it')}`)
    expect(rule('`x`')?.line).toBe(formatted(CODE, 'x'))
    expect(rule('[Q](https://q.org)')?.line).toBe(link('https://q.org', 'Q'))
  })

  it('puts the caret after the formatting, outside it', () => {
    const { line, caret } = rule('**b**')!
    expect(line.slice(caret)).toBe('')
    expect(line.slice(0, caret)).toBe(formatted(STRONG, 'b'))
  })

  it("leaves what isn't closed markdown", () => {
    expect(rule('a **b')).toBeNull()
    expect(rule('snake_case_')).toBeNull()
    expect(rule('a * b *')).toBeNull()
    expect(rule('![a](b.png)')).toBeNull()
  })

  it('leaves what is typed in code', () => {
    const line = formatted(CODE, '**b**')
    expect(inlineRule(line, line.length - 1)).toBeNull()
  })
})
