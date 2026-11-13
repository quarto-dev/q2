/**
 * Line prefixes: a run of prefix characters at the start of a line of a
 * multi-line field gives the line a kind (a heading, in the qmd editor).
 */

import { describe, expect, it } from 'vitest'
import { applyInputRule, deleteBackward, deleteForward, enter, insertText, paste, select, selectAllStep } from './commands.ts'
import { ATOM, caret, isPrefix, linePrefix, range, selectedText, type Schema, type State } from './model.ts'
import { rebaseState } from './rebase.ts'

/** A prefix, as the qmd editor's level-2 heading. */
const P = '﷒'

const schema: Schema = {
  order: ['title', 'body'],
  addable: ['body'],
  multiline: ['body'],
  inputRule: (field, line) => (field === 'body' && line === '# ' ? P : null),
  prefixText: (prefix) => (prefix === P ? '## ' : ''),
}

const body = (text: string, selection: State['selection']): State => ({ fields: { title: 'T', body: text }, selection })

describe('prefix characters', () => {
  it('are the noncharacters U+FDD0 to U+FDEF', () => {
    expect(isPrefix('﷐')).toBe(true)
    expect(isPrefix('﷯')).toBe(true)
    expect(isPrefix('#')).toBe(false)
    expect(isPrefix(ATOM)).toBe(false)
  })
  it('make up the start of a line', () => {
    expect(linePrefix(`One\n${P}Two`, 4)).toBe(P)
    expect(linePrefix(`One\n${P}Two`, 0)).toBe('')
  })
})

describe('the caret', () => {
  it('is never put before a prefix', () => {
    expect(select(schema, body(`One\n${P}Two`, null), caret('body', 4)).selection).toEqual(caret('body', 5))
    expect(selectAllStep(schema, body(`${P}Two`, caret('body', 2))).selection).toEqual(range('body', 1, 'body', 4))
  })
  it('is put after the prefix when Enter goes from a one-line field to a heading', () => {
    expect(enter(schema, body(`${P}Two`, caret('title', 1))).selection).toEqual(caret('body', 1))
  })
})

describe('typing', () => {
  it('never puts a prefix in the text', () => {
    expect(insertText(schema, body('One', caret('body', 3)), `a${P}b`).fields.body).toBe('Oneab')
    expect(paste(schema, body('One', caret('body', 3)), `a\n${P}b`).fields.body).toBe('Onea\nb')
  })
  it('keeps the kind of a line typed in', () => {
    const next = insertText(schema, body(`${P}Two`, caret('body', 1)), 'A ')
    expect(next.fields.body).toBe(`${P}A Two`)
    expect(next.selection).toEqual(caret('body', 3))
  })
})

describe('joining lines', () => {
  it('Backspace at the start of a line\'s text removes its prefix', () => {
    const next = deleteBackward(schema, body(`One\n${P}Two`, caret('body', 5)))
    expect(next.fields.body).toBe('One\nTwo')
    expect(next.selection).toEqual(caret('body', 4))
  })
  it('Backspace with the browser\'s range from the line before removes only the prefix', () => {
    const next = deleteBackward(schema, body(`One\n${P}Two`, caret('body', 5)), {
      from: { field: 'body', offset: 3 },
      to: { field: 'body', offset: 5 },
    })
    expect(next.fields.body).toBe('One\nTwo')
  })
  it('a joined line takes the kind of the line it joins', () => {
    expect(deleteForward(schema, body(`One\n${P}Two`, caret('body', 3))).fields.body).toBe('OneTwo')
    expect(deleteBackward(schema, body(`${P}One\nTwo`, caret('body', 5))).fields.body).toBe(`${P}OneTwo`)
  })
  it('an empty line joined to the line after takes its kind', () => {
    const next = deleteForward(schema, body(`One\n\n${P}Two`, caret('body', 4)))
    expect(next.fields.body).toBe(`One\n${P}Two`)
    expect(next.selection).toEqual(caret('body', 5))
  })
  it('deleting a selection across lines keeps the first line\'s kind', () => {
    expect(insertText(schema, body(`One\n${P}Two`, range('body', 1, 'body', 6)), '').fields.body).toBe('Owo')
    expect(insertText(schema, body(`${P}One\nTwo`, range('body', 2, 'body', 6)), '').fields.body).toBe(`${P}Owo`)
  })
})

describe('Enter', () => {
  it('at the end of a line with a prefix starts a line without one', () => {
    const next = enter(schema, body(`${P}Two`, caret('body', 4)))
    expect(next.fields.body).toBe(`${P}Two\n`)
    expect(next.selection).toEqual(caret('body', 5))
  })
  it('in the middle keeps the kind on both halves', () => {
    const next = enter(schema, body(`${P}Two`, caret('body', 2)))
    expect(next.fields.body).toBe(`${P}T\n${P}wo`)
    expect(next.selection).toEqual(caret('body', 4))
  })
  it('at the start of the text adds a line above and keeps the caret on the text', () => {
    const next = enter(schema, body(`One\n${P}Two`, caret('body', 5)))
    expect(next.fields.body).toBe(`One\n\n${P}Two`)
    expect(next.selection).toEqual(caret('body', 6))
  })
  it('on an empty line with a prefix adds a line above it', () => {
    const next = enter(schema, body(`One\n${P}`, caret('body', 5)))
    expect(next.fields.body).toBe(`One\n\n${P}`)
    expect(next.selection).toEqual(caret('body', 6))
  })
})

describe('Enter with a prefix it carries on (a list item\'s)', () => {
  /** A prefix Enter carries on, as the qmd editor's list item. */
  const L = '﷘'
  const lists: Schema = { ...schema, continued: (prefix) => prefix === L }

  it('at the end of a line starts a line with the same prefix', () => {
    const next = enter(lists, body(`${L}One`, caret('body', 4)))
    expect(next.fields.body).toBe(`${L}One\n${L}`)
    expect(next.selection).toEqual(caret('body', 6))
  })
  it('in the middle keeps the prefix on both halves', () => {
    const next = enter(lists, body(`${L}One`, caret('body', 2)))
    expect(next.fields.body).toBe(`${L}O\n${L}ne`)
    expect(next.selection).toEqual(caret('body', 4))
  })
  it('at the start of the text puts a line with the prefix above, and keeps the caret on the text', () => {
    const next = enter(lists, body(`Intro\n${L}One`, caret('body', 7)))
    expect(next.fields.body).toBe(`Intro\n${L}\n${L}One`)
    expect(next.selection).toEqual(caret('body', 9))
  })
  it('on an empty line takes the prefix off', () => {
    const next = enter(lists, body(`${L}One\n${L}\n${L}Two`, caret('body', 6)))
    expect(next.fields.body).toBe(`${L}One\n\n${L}Two`)
    expect(next.selection).toEqual(caret('body', 5))
  })
  it('leaves prefixes it doesn\'t carry on as they were', () => {
    const next = enter(lists, body(`${P}Two`, caret('body', 4)))
    expect(next.fields.body).toBe(`${P}Two\n`)
  })
})

describe('applyInputRule', () => {
  it('replaces the text before the caret with what the rule gives', () => {
    const next = applyInputRule(schema, body('One\n# Two', caret('body', 6)))
    expect(next.fields.body).toBe(`One\n${P}Two`)
    expect(next.selection).toEqual(caret('body', 5))
  })
  it('does nothing when the rule gives nothing', () => {
    const state = body('One\n## Two', caret('body', 7))
    expect(applyInputRule(schema, state)).toBe(state)
    const selected = body('One\n# Two', range('body', 4, 'body', 6))
    expect(applyInputRule(schema, selected)).toBe(selected)
  })
  it('sees the line\'s prefix, so a rule can leave a line with one alone', () => {
    const state = body(`${P}# Two`, caret('body', 3))
    expect(applyInputRule(schema, state)).toBe(state)
  })
})

describe('selectedText', () => {
  it('writes a selected prefix as the schema says', () => {
    const fields = { title: 'T', body: `One\n${P}Two` }
    expect(selectedText(schema, fields, range('body', 0, 'body', 8))).toBe('One\n## Two')
    expect(selectedText(schema, fields, range('body', 6, 'body', 8))).toBe('wo')
    // From the start of a heading's text: the caret can't be before its prefix.
    expect(selectedText(schema, fields, range('body', 5, 'body', 8))).toBe('## Two')
  })
})

describe('a collaborator\'s change', () => {
  it('that makes the caret\'s line a heading keeps the caret on its text', () => {
    const before = { title: 'T', body: 'One\nTwo' }
    const after = { title: 'T', body: `One\n${P}Two` }
    expect(rebaseState(schema, body('One\nTwo', caret('body', 4)), before, after).selection).toEqual(caret('body', 5))
    expect(rebaseState(schema, body('One\nTwo', caret('body', 6)), before, after).selection).toEqual(caret('body', 7))
  })
  it('that makes the caret\'s heading a paragraph keeps the caret on its text', () => {
    const before = { title: 'T', body: `One\n${P}Two` }
    const after = { title: 'T', body: 'One\nTwo' }
    expect(rebaseState(schema, body(before.body, caret('body', 5)), before, after).selection).toEqual(caret('body', 4))
  })
})
