import { describe, expect, it } from 'vitest'
import {
  bestStop,
  canonical,
  closeMark,
  inEmptyRun,
  LINK_CLOSE,
  LINK_OPEN,
  LINK_TEXT,
  marksIn,
  naturalStyle,
  openMark,
  PLAIN,
  runsOf,
  scan,
  serialize,
  tree,
  withoutMarks,
} from './marks.ts'

const B = 0
const I = 1
const C = 5
const bold = (text: string) => openMark(B) + text + closeMark(B)
const code = (text: string) => openMark(C) + text + closeMark(C)
const link = (url: string, text: string) => LINK_OPEN + url + LINK_TEXT + text + LINK_CLOSE

describe('scan', () => {
  it('gives each visible character its style, and a stop between every two marks', () => {
    const text = `a${bold('b')}`
    const { chars, stops } = scan(text)
    expect(chars.map((c) => [c.char, c.style.marks])).toEqual([
      ['a', 0],
      ['b', 1],
    ])
    expect(stops.map((s) => [s.offset, s.index, s.style.marks])).toEqual([
      [0, 0, 0],
      [1, 1, 0],
      [2, 1, 1],
      [3, 2, 1],
      [4, 2, 0],
    ])
  })

  it("never stops in a link's address", () => {
    const { stops, chars } = scan(link('https://q.org', 'q'))
    expect(chars.map((c) => [c.char, c.style.link])).toEqual([['q', 'https://q.org']])
    expect(stops.map((s) => s.offset)).toEqual([0, 15, 16, 17])
  })
})

describe('serialize', () => {
  it('writes runs with marks only where the style changes', () => {
    const runs = [
      { text: 'a', marks: 0, link: null },
      { text: 'b', marks: 1, link: null },
      { text: 'c', marks: 1, link: null },
      { text: '', marks: 2, link: null },
    ]
    expect(serialize(runs).text).toBe(`a${bold('bc')}`)
  })

  it('closes the format with two caret positions first, and opens it last', () => {
    const runs = [
      { text: 'a', marks: (1 << B) | (1 << C), link: null },
      { text: 'b', marks: 0, link: null },
    ]
    expect(serialize(runs).text).toBe(`${openMark(B)}${openMark(C)}a${closeMark(C)}${closeMark(B)}b`)
  })

  it('puts the caret in a run of its style, or else in an empty run of its own', () => {
    const runs = [
      { text: 'ab', marks: 1, link: null },
      { text: 'c', marks: 0, link: null },
    ]
    expect(serialize(runs, { index: 2, style: { marks: 1, link: null } })).toEqual({ text: `${bold('ab')}c`, caret: 3 })
    expect(serialize(runs, { index: 2, style: PLAIN })).toEqual({ text: `${bold('ab')}c`, caret: 4 })
    expect(serialize(runs, { index: 1, style: PLAIN })).toEqual({ text: `${bold('a')}${bold('b')}c`, caret: 3 })
    expect(serialize(runs, { index: 3, style: { marks: 2, link: null } }).text).toBe(
      `${bold('ab')}c${openMark(I)}${closeMark(I)}`,
    )
  })

  it('writes a link with its address in its opening mark', () => {
    expect(serialize([{ text: 'q', marks: 0, link: 'u' }]).text).toBe(link('u', 'q'))
  })
})

describe('canonical', () => {
  it('drops empty runs and joins runs of one style', () => {
    expect(canonical(`a${openMark(B)}${closeMark(B)}b`)).toBe('ab')
    expect(canonical(`${bold('a')}${bold('b')}`)).toBe(bold('ab'))
    expect(canonical(`${openMark(B)}ab`)).toBe(bold('ab'))
  })
})

describe('withoutMarks and marksIn', () => {
  it("are what's shown of the text, and what's hidden", () => {
    const text = `a${bold('b')} ${link('u', 'c')}`
    expect(withoutMarks(text)).toBe('ab c')
    expect(marksIn(text)).toBe(`${openMark(B)}${closeMark(B)}${LINK_OPEN}u${LINK_TEXT}${LINK_CLOSE}`)
  })
})

describe('naturalStyle and bestStop', () => {
  const twoSided = 1 << C
  it('carries bold on from the character before, and code only on its side', () => {
    const scanned = scan(`${bold('a')}${code('b')}c`)
    expect(naturalStyle(scanned, 1, twoSided, 'before')).toEqual({ marks: 1, link: null })
    expect(naturalStyle(scanned, 2, twoSided, 'before')).toEqual({ marks: 1 << C, link: null })
    expect(naturalStyle(scanned, 2, twoSided, 'after')).toEqual(PLAIN)
  })

  it("takes the start of a line's style from its first character", () => {
    expect(naturalStyle(scan(bold('a')), 0, twoSided, 'before')).toEqual({ marks: 1, link: null })
  })

  it("doesn't carry a link on past its end", () => {
    expect(naturalStyle(scan(`${link('u', 'a')}b`), 1, twoSided, 'before').link).toBeNull()
    expect(naturalStyle(scan(link('u', 'ab')), 1, twoSided, 'before').link).toBe('u')
  })

  it('finds the stop of a style', () => {
    const scanned = scan(`${code('a')}b`)
    expect(bestStop(scanned, 1, PLAIN, twoSided).offset).toBe(3)
    expect(bestStop(scanned, 1, { marks: 1 << C, link: null }, twoSided).offset).toBe(2)
  })
})

describe('inEmptyRun', () => {
  it('is whether the caret has a style neither neighbour has', () => {
    const text = `a${openMark(B)}${closeMark(B)}b`
    expect(inEmptyRun(scan(text), 2)).toBe(true)
    expect(inEmptyRun(scan(text), 1)).toBe(false)
    expect(inEmptyRun(scan(''), 0)).toBe(false)
  })
})

describe('tree', () => {
  it('puts the format that runs furthest outermost, and code innermost', () => {
    const runs = runsOf(`${openMark(B)}a${openMark(I)}b${closeMark(I)}${closeMark(B)}`)
    expect(tree(runs, 1 << C)).toEqual([
      {
        kind: 'mark',
        format: B,
        children: [
          { kind: 'text', text: 'a', start: 1 },
          { kind: 'mark', format: I, children: [{ kind: 'text', text: 'b', start: 3 }] },
        ],
      },
    ])
    const coded = runsOf(`${openMark(B)}${openMark(C)}x${closeMark(C)}${closeMark(B)}`)
    expect(tree(coded, 1 << C)).toEqual([
      { kind: 'mark', format: B, children: [{ kind: 'mark', format: C, children: [{ kind: 'text', text: 'x', start: 2 }] }] },
    ])
  })

  it('puts a link around what is in it', () => {
    const runs = runsOf(`${LINK_OPEN}u${LINK_TEXT}a${bold('b')}${LINK_CLOSE}`)
    expect(tree(runs)).toEqual([
      {
        kind: 'link',
        url: 'u',
        children: [
          { kind: 'text', text: 'a', start: 3 },
          { kind: 'mark', format: B, children: [{ kind: 'text', text: 'b', start: 5 }] },
        ],
      },
    ])
  })
})
